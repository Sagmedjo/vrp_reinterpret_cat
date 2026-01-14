use crate::construction::features::NearestDistanceFeatureBuilder;
use crate::construction::heuristics::MoveContext;
use crate::helpers::construction::heuristics::TestInsertionContextBuilder;
use crate::helpers::models::problem::TestSingleBuilder;
use crate::helpers::models::problem::TestTransportCost;
use crate::helpers::models::solution::{ActivityBuilder, RouteBuilder, RouteContextBuilder};
use crate::models::problem::Job;
use rosomaxa::prelude::Float;

/// A test-specific dimension key for target nearest distance.
struct TestTargetNearestDistance;

/// Extracts target_nearest_distance from a job using the test dimension key.
fn get_target_nearest_distance(job: &Job) -> Option<Float> {
    match job {
        Job::Single(single) => single.dimens.get_value::<TestTargetNearestDistance, Float>().copied(),
        Job::Multi(multi) => multi
            .jobs
            .iter()
            .filter_map(|s| s.dimens.get_value::<TestTargetNearestDistance, Float>().copied())
            .min_by(|a, b| a.total_cmp(b)),
    }
}

fn create_test_feature() -> crate::models::Feature {
    NearestDistanceFeatureBuilder::new("test_nearest_distance")
        .set_transport(TestTransportCost::new_shared())
        .set_job_target_fn(get_target_nearest_distance)
        .build()
        .unwrap()
}

// ============================================================================
// Builder Tests
// ============================================================================

#[test]
fn can_create_feature_with_all_required_parameters() {
    let transport = TestTransportCost::new_shared();
    let result = NearestDistanceFeatureBuilder::new("test")
        .set_transport(transport)
        .set_job_target_fn(get_target_nearest_distance)
        .build();

    assert!(result.is_ok());
    let feature = result.unwrap();
    assert!(feature.objective.is_some());
    assert!(feature.state.is_some());
}

#[test]
fn can_return_error_when_transport_not_set() {
    let result = NearestDistanceFeatureBuilder::new("test").set_job_target_fn(|_| None).build();

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("transport"));
}

#[test]
fn can_return_error_when_job_target_fn_not_set() {
    let transport = TestTransportCost::new_shared();
    let result = NearestDistanceFeatureBuilder::new("test").set_transport(transport).build();

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("job_target_fn"));
}

// ============================================================================
// Fitness Tests - verify penalty calculations
// ============================================================================

#[test]
fn can_return_zero_fitness_for_empty_route() {
    let feature = create_test_feature();
    let objective = feature.objective.unwrap();
    let route_ctx = RouteContextBuilder::default().build();
    let insertion_ctx = TestInsertionContextBuilder::default().with_routes(vec![route_ctx]).build();

    let fitness = objective.fitness(&insertion_ctx);

    assert_eq!(fitness, 0.0);
}

#[test]
fn can_return_zero_fitness_for_single_job_route() {
    // Single job on route - no other jobs to compute nearest distance with
    let feature = create_test_feature();
    let objective = feature.objective.unwrap();
    // Create a job WITH target_nearest_distance at location 10
    let job =
        TestSingleBuilder::default().location(Some(10)).property::<TestTargetNearestDistance, Float>(5.0).build_shared();
    let route_ctx = RouteContextBuilder::default()
        .with_route(
            RouteBuilder::default().add_activity(ActivityBuilder::with_location(10).job(Some(job)).build()).build(),
        )
        .build();
    let insertion_ctx = TestInsertionContextBuilder::default().with_routes(vec![route_ctx]).build();

    let fitness = objective.fitness(&insertion_ctx);

    assert_eq!(fitness, 0.0);
}

#[test]
fn can_return_zero_fitness_when_jobs_have_no_target() {
    // Jobs without target_nearest_distance should not contribute penalty
    let feature = create_test_feature();
    let objective = feature.objective.unwrap();
    // Create two jobs WITHOUT target_nearest_distance at distant locations
    let job1 = TestSingleBuilder::default().location(Some(0)).build_shared();
    let job2 = TestSingleBuilder::default().location(Some(100)).build_shared();
    let route_ctx = RouteContextBuilder::default()
        .with_route(
            RouteBuilder::default()
                .add_activity(ActivityBuilder::with_location(0).job(Some(job1)).build())
                .add_activity(ActivityBuilder::with_location(100).job(Some(job2)).build())
                .build(),
        )
        .build();
    let insertion_ctx = TestInsertionContextBuilder::default().with_routes(vec![route_ctx]).build();

    let fitness = objective.fitness(&insertion_ctx);

    // No penalty because neither job has target_nearest_distance
    assert_eq!(fitness, 0.0);
}

#[test]
fn can_return_zero_fitness_when_within_threshold() {
    // Two jobs close together, both with target_nearest_distance = 20
    // Distance between them = |10 - 15| = 5, which is < 20
    let feature = create_test_feature();
    let objective = feature.objective.unwrap();
    let job1 =
        TestSingleBuilder::default().location(Some(10)).property::<TestTargetNearestDistance, Float>(20.0).build_shared();
    let job2 =
        TestSingleBuilder::default().location(Some(15)).property::<TestTargetNearestDistance, Float>(20.0).build_shared();
    let route_ctx = RouteContextBuilder::default()
        .with_route(
            RouteBuilder::default()
                .add_activity(ActivityBuilder::with_location(10).job(Some(job1)).build())
                .add_activity(ActivityBuilder::with_location(15).job(Some(job2)).build())
                .build(),
        )
        .build();
    let insertion_ctx = TestInsertionContextBuilder::default().with_routes(vec![route_ctx]).build();

    let fitness = objective.fitness(&insertion_ctx);

    // Min distance = 5 for each job (to the other), threshold = 20, no penalty
    assert_eq!(fitness, 0.0);
}

#[test]
fn can_return_penalty_when_exceeding_threshold() {
    // Two jobs far apart, both with target_nearest_distance = 5
    // Distance between them = |0 - 100| = 100, which is > 5
    // Each job's min distance to others = 100 (only one other job)
    // Penalty per job = 100 - 5 = 95
    // Total penalty = 95 + 95 = 190
    let feature = create_test_feature();
    let objective = feature.objective.unwrap();
    let job1 =
        TestSingleBuilder::default().location(Some(0)).property::<TestTargetNearestDistance, Float>(5.0).build_shared();
    let job2 =
        TestSingleBuilder::default().location(Some(100)).property::<TestTargetNearestDistance, Float>(5.0).build_shared();
    let route_ctx = RouteContextBuilder::default()
        .with_route(
            RouteBuilder::default()
                .add_activity(ActivityBuilder::with_location(0).job(Some(job1)).build())
                .add_activity(ActivityBuilder::with_location(100).job(Some(job2)).build())
                .build(),
        )
        .build();
    let insertion_ctx = TestInsertionContextBuilder::default().with_routes(vec![route_ctx]).build();

    let fitness = objective.fitness(&insertion_ctx);

    // Each job has min_dist = 100, target = 5, penalty = 95 each
    assert_eq!(fitness, 190.0);
}

#[test]
fn can_penalize_only_jobs_with_target() {
    // Three jobs: job1 (with target=5), job2 (no target), job3 (no target)
    // Locations: 0, 50, 100
    // Job1's min distance = min(50, 100) = 50, penalty = 50 - 5 = 45
    // Job2 and job3 have no target, so no penalty
    let feature = create_test_feature();
    let objective = feature.objective.unwrap();
    let job1 =
        TestSingleBuilder::default().location(Some(0)).property::<TestTargetNearestDistance, Float>(5.0).build_shared();
    let job2 = TestSingleBuilder::default().location(Some(50)).build_shared(); // no target
    let job3 = TestSingleBuilder::default().location(Some(100)).build_shared(); // no target
    let route_ctx = RouteContextBuilder::default()
        .with_route(
            RouteBuilder::default()
                .add_activity(ActivityBuilder::with_location(0).job(Some(job1)).build())
                .add_activity(ActivityBuilder::with_location(50).job(Some(job2)).build())
                .add_activity(ActivityBuilder::with_location(100).job(Some(job3)).build())
                .build(),
        )
        .build();
    let insertion_ctx = TestInsertionContextBuilder::default().with_routes(vec![route_ctx]).build();

    let fitness = objective.fitness(&insertion_ctx);

    // Only job1 contributes: min_dist = min(50, 100) = 50, penalty = 50 - 5 = 45
    assert_eq!(fitness, 45.0);
}

#[test]
fn can_handle_multiple_jobs_with_different_thresholds() {
    // Three jobs at locations 0, 10, 100
    // job1: loc=0, target=50 → min_dist = min(10, 100) = 10 → penalty = 0 (within threshold)
    // job2: loc=10, target=100 → min_dist = min(10, 90) = 10 → penalty = 0 (within threshold)
    // job3: loc=100, target=20 → min_dist = min(100, 90) = 90 → penalty = 90 - 20 = 70
    let feature = create_test_feature();
    let objective = feature.objective.unwrap();
    let job1 =
        TestSingleBuilder::default().location(Some(0)).property::<TestTargetNearestDistance, Float>(50.0).build_shared();
    let job2 =
        TestSingleBuilder::default().location(Some(10)).property::<TestTargetNearestDistance, Float>(100.0).build_shared();
    let job3 =
        TestSingleBuilder::default().location(Some(100)).property::<TestTargetNearestDistance, Float>(20.0).build_shared();
    let route_ctx = RouteContextBuilder::default()
        .with_route(
            RouteBuilder::default()
                .add_activity(ActivityBuilder::with_location(0).job(Some(job1)).build())
                .add_activity(ActivityBuilder::with_location(10).job(Some(job2)).build())
                .add_activity(ActivityBuilder::with_location(100).job(Some(job3)).build())
                .build(),
        )
        .build();
    let insertion_ctx = TestInsertionContextBuilder::default().with_routes(vec![route_ctx]).build();

    let fitness = objective.fitness(&insertion_ctx);

    // job1: min=10, target=50, penalty=0
    // job2: min=10, target=100, penalty=0
    // job3: min=90, target=20, penalty=70
    // Total = 0 + 0 + 70 = 70
    assert_eq!(fitness, 70.0);
}

// ============================================================================
// Estimate Tests - verify construction-time guidance
// ============================================================================

#[test]
fn can_estimate_zero_for_job_without_target() {
    // Job without target_nearest_distance should have zero estimate
    let feature = create_test_feature();
    let objective = feature.objective.unwrap();
    let job = TestSingleBuilder::default().location(Some(50)).build_as_job_ref(); // no target
    let route_ctx = RouteContextBuilder::default()
        .with_route(
            RouteBuilder::default()
                .add_activity(ActivityBuilder::with_location(0).build())
                .add_activity(ActivityBuilder::with_location(100).build())
                .build(),
        )
        .build();
    let insertion_ctx = TestInsertionContextBuilder::default().build();

    let estimate = objective.estimate(&MoveContext::route(&insertion_ctx.solution, &route_ctx, &job));

    assert_eq!(estimate, 0.0);
}

#[test]
fn can_estimate_zero_when_within_threshold() {
    // Job with target=100, inserting at location 50 into route with jobs at 40, 60
    // Min distance = min(10, 10) = 10, which is < 100
    let feature = create_test_feature();
    let objective = feature.objective.unwrap();
    let job_single =
        TestSingleBuilder::default().location(Some(50)).property::<TestTargetNearestDistance, Float>(100.0).build_shared();
    let job = Job::Single(job_single);
    let existing1 = TestSingleBuilder::default().location(Some(40)).build_shared();
    let existing2 = TestSingleBuilder::default().location(Some(60)).build_shared();
    let route_ctx = RouteContextBuilder::default()
        .with_route(
            RouteBuilder::default()
                .add_activity(ActivityBuilder::with_location(40).job(Some(existing1)).build())
                .add_activity(ActivityBuilder::with_location(60).job(Some(existing2)).build())
                .build(),
        )
        .build();
    let insertion_ctx = TestInsertionContextBuilder::default().build();

    let estimate = objective.estimate(&MoveContext::route(&insertion_ctx.solution, &route_ctx, &job));

    // Min distance from 50 to (40, 60) = min(10, 10) = 10 < 100, no penalty
    assert_eq!(estimate, 0.0);
}

#[test]
fn can_estimate_penalty_when_exceeding_threshold() {
    // Job with target=5, inserting at location 50 into route with jobs at 0, 100
    // Min distance = min(50, 50) = 50, which is > 5
    // Estimated penalty = 50 - 5 = 45
    let feature = create_test_feature();
    let objective = feature.objective.unwrap();
    let job_single =
        TestSingleBuilder::default().location(Some(50)).property::<TestTargetNearestDistance, Float>(5.0).build_shared();
    let job = Job::Single(job_single);
    let existing1 = TestSingleBuilder::default().location(Some(0)).build_shared();
    let existing2 = TestSingleBuilder::default().location(Some(100)).build_shared();
    let route_ctx = RouteContextBuilder::default()
        .with_route(
            RouteBuilder::default()
                .add_activity(ActivityBuilder::with_location(0).job(Some(existing1)).build())
                .add_activity(ActivityBuilder::with_location(100).job(Some(existing2)).build())
                .build(),
        )
        .build();
    let insertion_ctx = TestInsertionContextBuilder::default().build();

    let estimate = objective.estimate(&MoveContext::route(&insertion_ctx.solution, &route_ctx, &job));

    // Min distance from 50 to (0, 100) = min(50, 50) = 50 > 5, penalty = 45
    assert_eq!(estimate, 45.0);
}

#[test]
fn can_estimate_zero_for_empty_route() {
    // Inserting into empty route should return zero (no jobs to compare with)
    let feature = create_test_feature();
    let objective = feature.objective.unwrap();
    let job_single =
        TestSingleBuilder::default().location(Some(50)).property::<TestTargetNearestDistance, Float>(5.0).build_shared();
    let job = Job::Single(job_single);
    let route_ctx = RouteContextBuilder::default().build();
    let insertion_ctx = TestInsertionContextBuilder::default().build();

    let estimate = objective.estimate(&MoveContext::route(&insertion_ctx.solution, &route_ctx, &job));

    // No existing jobs, no penalty estimate
    assert_eq!(estimate, 0.0);
}

// ============================================================================
// Compactness Tests - verify objective creates more compact routes
// ============================================================================

#[test]
fn can_prefer_compact_route_over_scattered_route() {
    // Compare two scenarios:
    // Compact route: jobs at 10, 12, 14 (all close together)
    // Scattered route: jobs at 0, 50, 100 (spread out)
    // With target_nearest_distance = 10, compact route should have lower penalty
    let feature = create_test_feature();
    let objective = feature.objective.unwrap();

    // Compact route: locations 10, 12, 14
    // Each job's min distance to others:
    // job at 10: min = min(2, 4) = 2 < 10, penalty = 0
    // job at 12: min = min(2, 2) = 2 < 10, penalty = 0
    // job at 14: min = min(4, 2) = 2 < 10, penalty = 0
    let compact_job1 =
        TestSingleBuilder::default().location(Some(10)).property::<TestTargetNearestDistance, Float>(10.0).build_shared();
    let compact_job2 =
        TestSingleBuilder::default().location(Some(12)).property::<TestTargetNearestDistance, Float>(10.0).build_shared();
    let compact_job3 =
        TestSingleBuilder::default().location(Some(14)).property::<TestTargetNearestDistance, Float>(10.0).build_shared();
    let compact_route = RouteContextBuilder::default()
        .with_route(
            RouteBuilder::default()
                .add_activity(ActivityBuilder::with_location(10).job(Some(compact_job1)).build())
                .add_activity(ActivityBuilder::with_location(12).job(Some(compact_job2)).build())
                .add_activity(ActivityBuilder::with_location(14).job(Some(compact_job3)).build())
                .build(),
        )
        .build();
    let compact_ctx = TestInsertionContextBuilder::default().with_routes(vec![compact_route]).build();
    let compact_fitness = objective.fitness(&compact_ctx);

    // Scattered route: locations 0, 50, 100
    // Each job's min distance to others:
    // job at 0: min = min(50, 100) = 50 > 10, penalty = 40
    // job at 50: min = min(50, 50) = 50 > 10, penalty = 40
    // job at 100: min = min(100, 50) = 50 > 10, penalty = 40
    let scattered_job1 =
        TestSingleBuilder::default().location(Some(0)).property::<TestTargetNearestDistance, Float>(10.0).build_shared();
    let scattered_job2 =
        TestSingleBuilder::default().location(Some(50)).property::<TestTargetNearestDistance, Float>(10.0).build_shared();
    let scattered_job3 =
        TestSingleBuilder::default().location(Some(100)).property::<TestTargetNearestDistance, Float>(10.0).build_shared();
    let scattered_route = RouteContextBuilder::default()
        .with_route(
            RouteBuilder::default()
                .add_activity(ActivityBuilder::with_location(0).job(Some(scattered_job1)).build())
                .add_activity(ActivityBuilder::with_location(50).job(Some(scattered_job2)).build())
                .add_activity(ActivityBuilder::with_location(100).job(Some(scattered_job3)).build())
                .build(),
        )
        .build();
    let scattered_ctx = TestInsertionContextBuilder::default().with_routes(vec![scattered_route]).build();
    let scattered_fitness = objective.fitness(&scattered_ctx);

    // Compact route should have significantly lower penalty than scattered route
    assert_eq!(compact_fitness, 0.0);
    assert_eq!(scattered_fitness, 120.0); // 40 + 40 + 40
    assert!(compact_fitness < scattered_fitness);
}
