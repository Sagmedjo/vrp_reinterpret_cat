//! Provides a feature to minimize nearest distance violations for job tasks.
//!
//! Jobs with a `target_nearest_distance` threshold are penalized if their distance
//! to the nearest appointment on the same route exceeds the threshold.

#[cfg(test)]
#[path = "../../../tests/unit/construction/features/nearest_distance_test.rs"]
mod nearest_distance_test;

use super::*;

custom_solution_state!(NearestDistancePenalty typeof Cost);
custom_tour_state!(NearestDistanceRouteData typeof RouteNearestDistanceData);

/// A function type to extract target nearest distance from a job.
pub type JobTargetNearestDistanceFn = Arc<dyn Fn(&Job) -> Option<Float> + Send + Sync>;

/// Route-level cached data for nearest distance calculations.
#[derive(Clone, Default)]
pub struct RouteNearestDistanceData {
    /// Penalty contribution from this route.
    pub penalty: Cost,
}

/// Provides a way to build a feature to minimize nearest distance violations.
pub struct NearestDistanceFeatureBuilder {
    name: String,
    transport: Option<Arc<dyn TransportCost + Send + Sync>>,
    job_target_fn: Option<JobTargetNearestDistanceFn>,
}

impl NearestDistanceFeatureBuilder {
    /// Creates a new instance of `NearestDistanceFeatureBuilder`.
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string(), transport: None, job_target_fn: None }
    }

    /// Sets the transport cost model.
    pub fn set_transport(mut self, transport: Arc<dyn TransportCost + Send + Sync>) -> Self {
        self.transport = Some(transport);
        self
    }

    /// Sets the function to extract target nearest distance from a job.
    pub fn set_job_target_fn<F>(mut self, func: F) -> Self
    where
        F: Fn(&Job) -> Option<Float> + Send + Sync + 'static,
    {
        self.job_target_fn = Some(Arc::new(func));
        self
    }

    /// Builds the feature.
    pub fn build(mut self) -> GenericResult<Feature> {
        let transport = self
            .transport
            .take()
            .ok_or_else(|| GenericError::from("transport must be set for nearest_distance feature"))?;

        let job_target_fn = self
            .job_target_fn
            .take()
            .ok_or_else(|| GenericError::from("job_target_fn must be set for nearest_distance feature"))?;

        let objective = NearestDistanceObjective { transport: transport.clone(), job_target_fn: job_target_fn.clone() };
        let state = NearestDistanceState { transport, job_target_fn };

        FeatureBuilder::default().with_name(self.name.as_str()).with_objective(objective).with_state(state).build()
    }
}

struct NearestDistanceObjective {
    transport: Arc<dyn TransportCost + Send + Sync>,
    job_target_fn: JobTargetNearestDistanceFn,
}

impl NearestDistanceObjective {
    /// Computes the penalty for a single route.
    fn compute_route_penalty(&self, route_ctx: &RouteContext) -> Cost {
        let route = route_ctx.route();
        let profile = &route.actor.vehicle.profile;

        // Collect all job activities with their locations and single jobs
        let activities: Vec<(Location, Arc<Single>)> = route
            .tour
            .all_activities()
            .filter_map(|a| a.job.as_ref().map(|j| (a.place.location, j.clone())))
            .collect();

        let n = activities.len();
        if n <= 1 {
            return 0.0;
        }

        let locations: Vec<Location> = activities.iter().map(|(loc, _)| *loc).collect();

        // For each job WITH a target_nearest_distance, compute penalty
        let mut total_penalty = 0.0;
        for (i, (loc_i, single)) in activities.iter().enumerate() {
            // Wrap Single in Job::Single to call job_target_fn
            let job = Job::Single(single.clone());
            // Skip jobs without target_nearest_distance threshold
            let Some(target) = (self.job_target_fn)(&job) else { continue };

            let min_dist: Float = locations
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, &loc_j)| self.transport.distance_approx(profile, *loc_i, loc_j))
                .min_by(|a, b| a.total_cmp(b))
                .unwrap_or(0.0);

            if min_dist > target {
                total_penalty += min_dist - target;
            }
        }

        total_penalty
    }
}

impl FeatureObjective for NearestDistanceObjective {
    fn fitness(&self, solution: &InsertionContext) -> Cost {
        // Use cached value from accept_solution_state() if available
        solution.solution.state.get_nearest_distance_penalty().copied().unwrap_or_else(|| {
            // Fallback: compute directly
            solution.solution.routes.iter().map(|route_ctx| self.compute_route_penalty(route_ctx)).sum()
        })
    }

    fn estimate(&self, move_ctx: &MoveContext<'_>) -> Cost {
        match move_ctx {
            MoveContext::Route { route_ctx, job, .. } => {
                // Skip if job has no target_nearest_distance
                let Some(target) = (self.job_target_fn)(job) else {
                    return Cost::default();
                };

                let route = route_ctx.route();
                let profile = &route.actor.vehicle.profile;

                // Get primary location of the job being inserted
                let job_loc = match job {
                    Job::Single(single) => single.places.first().and_then(|p| p.location),
                    Job::Multi(multi) => multi.jobs.first().and_then(|s| s.places.first().and_then(|p| p.location)),
                };

                let Some(job_loc) = job_loc else {
                    return Cost::default();
                };

                // Compute minimum distance from this job to existing route jobs
                let existing_locs: Vec<Location> =
                    route.tour.all_activities().filter(|a| a.job.is_some()).map(|a| a.place.location).collect();

                if existing_locs.is_empty() {
                    return Cost::default();
                }

                let min_dist: Float = existing_locs
                    .iter()
                    .map(|&loc| self.transport.distance_approx(profile, job_loc, loc))
                    .min_by(|a, b| a.total_cmp(b))
                    .unwrap_or(0.0);

                // Return estimated penalty contribution
                if min_dist > target {
                    min_dist - target
                } else {
                    Cost::default()
                }
            }
            MoveContext::Activity { .. } => Cost::default(),
        }
    }
}

struct NearestDistanceState {
    transport: Arc<dyn TransportCost + Send + Sync>,
    job_target_fn: JobTargetNearestDistanceFn,
}

impl NearestDistanceState {
    /// Computes the penalty for a single route.
    fn compute_route_penalty(&self, route_ctx: &RouteContext) -> Cost {
        let route = route_ctx.route();
        let profile = &route.actor.vehicle.profile;

        // Collect all job activities with their locations and single jobs
        let activities: Vec<(Location, Arc<Single>)> = route
            .tour
            .all_activities()
            .filter_map(|a| a.job.as_ref().map(|j| (a.place.location, j.clone())))
            .collect();

        let n = activities.len();
        if n <= 1 {
            return 0.0;
        }

        let locations: Vec<Location> = activities.iter().map(|(loc, _)| *loc).collect();

        // For each job WITH a target_nearest_distance, compute penalty
        let mut total_penalty = 0.0;
        for (i, (loc_i, single)) in activities.iter().enumerate() {
            // Wrap Single in Job::Single to call job_target_fn
            let job = Job::Single(single.clone());
            // Skip jobs without target_nearest_distance threshold
            let Some(target) = (self.job_target_fn)(&job) else { continue };

            let min_dist: Float = locations
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != i)
                .map(|(_, &loc_j)| self.transport.distance_approx(profile, *loc_i, loc_j))
                .min_by(|a, b| a.total_cmp(b))
                .unwrap_or(0.0);

            if min_dist > target {
                total_penalty += min_dist - target;
            }
        }

        total_penalty
    }
}

impl FeatureState for NearestDistanceState {
    fn accept_insertion(&self, _: &mut SolutionContext, _: usize, _: &Job) {
        // Route will be marked stale, recomputed in accept_solution_state
    }

    fn accept_route_state(&self, route_ctx: &mut RouteContext) {
        let penalty = self.compute_route_penalty(route_ctx);
        route_ctx.state_mut().set_nearest_distance_route_data(RouteNearestDistanceData { penalty });
    }

    fn accept_solution_state(&self, solution_ctx: &mut SolutionContext) {
        // Update stale routes
        solution_ctx
            .routes
            .iter_mut()
            .filter(|rc| rc.is_stale())
            .for_each(|rc| self.accept_route_state(rc));

        // Compute total fitness from cached route data
        let total: Cost = solution_ctx
            .routes
            .iter()
            .map(|rc| rc.state().get_nearest_distance_route_data().map(|data| data.penalty).unwrap_or(0.0))
            .sum();

        solution_ctx.state.set_nearest_distance_penalty(total);
    }
}
