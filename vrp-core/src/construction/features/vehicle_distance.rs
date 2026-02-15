//! Provides a feature to minimize vehicle distance penalties.
//!
//! For each job on a route, the penalty is the excess distance from the job to its
//! assigned vehicle's start location compared to the nearest compatible vehicle's start.
//! penalty = max(0, dist(job, assigned_vehicle) - dist(job, nearest_compatible_vehicle))

#[cfg(test)]
#[path = "../../../tests/unit/construction/features/vehicle_distance_test.rs"]
mod vehicle_distance_test;

use super::*;

custom_solution_state!(VehicleDistancePenalty typeof Cost);
custom_tour_state!(VehicleDistanceRouteData typeof RouteVehicleDistanceData);

/// A function type that checks whether a given actor is compatible with a given job.
pub type ActorJobCompatibilityFn = Arc<dyn Fn(&Job, &Actor) -> bool + Send + Sync>;

/// Route-level cached data for vehicle distance calculations.
#[derive(Clone, Default)]
pub struct RouteVehicleDistanceData {
    /// Penalty contribution from this route.
    pub penalty: Cost,
}

/// Provides a way to build a feature to minimize vehicle distance penalties.
pub struct VehicleDistanceFeatureBuilder {
    name: String,
    transport: Option<Arc<dyn TransportCost + Send + Sync>>,
    actors: Option<Vec<Arc<Actor>>>,
    compatibility_fn: Option<ActorJobCompatibilityFn>,
}

impl VehicleDistanceFeatureBuilder {
    /// Creates a new instance of `VehicleDistanceFeatureBuilder`.
    pub fn new(name: &str) -> Self {
        Self { name: name.to_string(), transport: None, actors: None, compatibility_fn: None }
    }

    /// Sets the transport cost model.
    pub fn set_transport(mut self, transport: Arc<dyn TransportCost + Send + Sync>) -> Self {
        self.transport = Some(transport);
        self
    }

    /// Sets the fleet actors to consider when finding the nearest compatible vehicle.
    pub fn set_actors(mut self, actors: Vec<Arc<Actor>>) -> Self {
        self.actors = Some(actors);
        self
    }

    /// Sets the compatibility function that checks if an actor can serve a job.
    pub fn set_compatibility_fn<F>(mut self, func: F) -> Self
    where
        F: Fn(&Job, &Actor) -> bool + Send + Sync + 'static,
    {
        self.compatibility_fn = Some(Arc::new(func));
        self
    }

    /// Builds the feature.
    pub fn build(mut self) -> GenericResult<Feature> {
        let transport = self
            .transport
            .take()
            .ok_or_else(|| GenericError::from("transport must be set for vehicle_distance feature"))?;

        let actors =
            self.actors.take().ok_or_else(|| GenericError::from("actors must be set for vehicle_distance feature"))?;

        let compatibility_fn = self
            .compatibility_fn
            .take()
            .ok_or_else(|| GenericError::from("compatibility_fn must be set for vehicle_distance feature"))?;

        let objective = VehicleDistanceObjective {
            transport: transport.clone(),
            actors: actors.clone(),
            compatibility_fn: compatibility_fn.clone(),
        };
        let state = VehicleDistanceState { transport, actors, compatibility_fn };

        FeatureBuilder::default().with_name(self.name.as_str()).with_objective(objective).with_state(state).build()
    }
}

/// Gets the primary location of a job.
fn get_job_location(job: &Job) -> Option<Location> {
    match job {
        Job::Single(single) => single.places.first().and_then(|p| p.location),
        Job::Multi(multi) => multi.jobs.first().and_then(|s| s.places.first().and_then(|p| p.location)),
    }
}

/// Finds the minimum distance from a job location to the start of any compatible vehicle.
fn find_nearest_compatible_vehicle_dist(
    job_loc: Location,
    job: &Job,
    actors: &[Arc<Actor>],
    compatibility_fn: &ActorJobCompatibilityFn,
    transport: &(dyn TransportCost + Send + Sync),
) -> Option<Float> {
    actors
        .iter()
        .filter(|actor| compatibility_fn(job, actor))
        .filter_map(|actor| actor.detail.start.as_ref().map(|s| s.location))
        .map(|start_loc| transport.distance_approx(&actors[0].vehicle.profile, job_loc, start_loc))
        .min_by(|a, b| a.total_cmp(b))
}

struct VehicleDistanceObjective {
    transport: Arc<dyn TransportCost + Send + Sync>,
    actors: Vec<Arc<Actor>>,
    compatibility_fn: ActorJobCompatibilityFn,
}

impl VehicleDistanceObjective {
    /// Computes the penalty for a single route.
    fn compute_route_penalty(&self, route_ctx: &RouteContext) -> Cost {
        let route = route_ctx.route();
        let profile = &route.actor.vehicle.profile;

        let assigned_start = match route.actor.detail.start.as_ref() {
            Some(start) => start.location,
            None => return 0.0,
        };

        let mut total_penalty = 0.0;

        for activity in route.tour.all_activities() {
            let Some(single) = activity.job.as_ref() else { continue };
            let job_loc = activity.place.location;
            let job = Job::Single(single.clone());

            let dist_assigned = self.transport.distance_approx(profile, job_loc, assigned_start);

            let dist_nearest = find_nearest_compatible_vehicle_dist(
                job_loc,
                &job,
                &self.actors,
                &self.compatibility_fn,
                self.transport.as_ref(),
            )
            .unwrap_or(dist_assigned);

            let penalty = (dist_assigned - dist_nearest).max(0.0);
            total_penalty += penalty;
        }

        total_penalty
    }
}

impl FeatureObjective for VehicleDistanceObjective {
    fn fitness(&self, solution: &InsertionContext) -> Cost {
        solution.solution.state.get_vehicle_distance_penalty().copied().unwrap_or_else(|| {
            solution.solution.routes.iter().map(|route_ctx| self.compute_route_penalty(route_ctx)).sum()
        })
    }

    fn estimate(&self, move_ctx: &MoveContext<'_>) -> Cost {
        match move_ctx {
            MoveContext::Route { route_ctx, job, .. } => {
                let Some(job_loc) = get_job_location(job) else {
                    return Cost::default();
                };

                let route = route_ctx.route();
                let profile = &route.actor.vehicle.profile;

                let Some(assigned_start) = route.actor.detail.start.as_ref().map(|s| s.location) else {
                    return Cost::default();
                };

                let dist_assigned = self.transport.distance_approx(profile, job_loc, assigned_start);

                let dist_nearest = find_nearest_compatible_vehicle_dist(
                    job_loc,
                    job,
                    &self.actors,
                    &self.compatibility_fn,
                    self.transport.as_ref(),
                )
                .unwrap_or(dist_assigned);

                (dist_assigned - dist_nearest).max(0.0)
            }
            MoveContext::Activity { .. } => Cost::default(),
        }
    }
}

struct VehicleDistanceState {
    transport: Arc<dyn TransportCost + Send + Sync>,
    actors: Vec<Arc<Actor>>,
    compatibility_fn: ActorJobCompatibilityFn,
}

impl VehicleDistanceState {
    /// Computes the penalty for a single route.
    fn compute_route_penalty(&self, route_ctx: &RouteContext) -> Cost {
        let route = route_ctx.route();
        let profile = &route.actor.vehicle.profile;

        let assigned_start = match route.actor.detail.start.as_ref() {
            Some(start) => start.location,
            None => return 0.0,
        };

        let mut total_penalty = 0.0;

        for activity in route.tour.all_activities() {
            let Some(single) = activity.job.as_ref() else { continue };
            let job_loc = activity.place.location;
            let job = Job::Single(single.clone());

            let dist_assigned = self.transport.distance_approx(profile, job_loc, assigned_start);

            let dist_nearest = find_nearest_compatible_vehicle_dist(
                job_loc,
                &job,
                &self.actors,
                &self.compatibility_fn,
                self.transport.as_ref(),
            )
            .unwrap_or(dist_assigned);

            let penalty = (dist_assigned - dist_nearest).max(0.0);
            total_penalty += penalty;
        }

        total_penalty
    }
}

impl FeatureState for VehicleDistanceState {
    fn accept_insertion(&self, _: &mut SolutionContext, _: usize, _: &Job) {
        // Route will be marked stale, recomputed in accept_solution_state
    }

    fn accept_route_state(&self, route_ctx: &mut RouteContext) {
        let penalty = self.compute_route_penalty(route_ctx);
        route_ctx.state_mut().set_vehicle_distance_route_data(RouteVehicleDistanceData { penalty });
    }

    fn accept_solution_state(&self, solution_ctx: &mut SolutionContext) {
        solution_ctx.routes.iter_mut().filter(|rc| rc.is_stale()).for_each(|rc| self.accept_route_state(rc));

        let total: Cost = solution_ctx
            .routes
            .iter()
            .map(|rc| rc.state().get_vehicle_distance_route_data().map(|data| data.penalty).unwrap_or(0.0))
            .sum();

        solution_ctx.state.set_vehicle_distance_penalty(total);
    }
}
