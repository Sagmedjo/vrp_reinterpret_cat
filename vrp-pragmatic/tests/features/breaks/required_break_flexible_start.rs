use crate::format::problem::*;
use crate::format::solution::{Solution, Stop, Tour};
use crate::format_time;
use crate::helpers::*;
use crate::parse_time;

/// Tests that OffsetTime required breaks work correctly with flexible start times
/// (shift.start.latest is None), verifying that departure rescheduling
/// produces feasible solutions with breaks placed at the correct offset from the anchor.

/// Collects all activity intervals (start, end, type, job_id) from a tour, flattened across stops.
fn collect_activity_intervals(tour: &Tour) -> Vec<(f64, f64, String, String)> {
    let mut intervals = Vec::new();
    for stop in &tour.stops {
        let schedule = stop.schedule();
        let stop_arrival = parse_time(&schedule.arrival);
        let stop_departure = parse_time(&schedule.departure);
        let activities = stop.activities();

        if activities.len() == 1 {
            let a = &activities[0];
            if let Some(time) = &a.time {
                intervals.push((
                    parse_time(&time.start),
                    parse_time(&time.end),
                    a.activity_type.clone(),
                    a.job_id.clone(),
                ));
            } else {
                intervals.push((stop_arrival, stop_departure, a.activity_type.clone(), a.job_id.clone()));
            }
        } else {
            for a in activities {
                if let Some(time) = &a.time {
                    intervals.push((
                        parse_time(&time.start),
                        parse_time(&time.end),
                        a.activity_type.clone(),
                        a.job_id.clone(),
                    ));
                } else {
                    intervals.push((stop_arrival, stop_departure, a.activity_type.clone(), a.job_id.clone()));
                }
            }
        }
    }
    intervals
}

/// Comprehensive validation of break placement and schedule consistency for a single tour.
/// Checks:
///  1. Correct number of breaks with correct duration
///  2. Breaks don't overlap with job activities (cross-stop)
///  3. Stop schedule consistency (departure >= arrival, monotonic)
///  4. Activities within each stop are time-ordered and within stop bounds
///  5. Break time is within tour time bounds
///  6. Break doesn't have a location (required breaks are locationless)
fn validate_tour_breaks_and_schedule(tour: &Tour, expected_break_count: usize, expected_break_duration: f64) {
    let intervals = collect_activity_intervals(tour);

    // 1. Break count and duration
    let break_intervals: Vec<_> = intervals.iter().filter(|(_, _, typ, _)| typ == "break").collect();
    assert_eq!(
        break_intervals.len(),
        expected_break_count,
        "expected {expected_break_count} break(s), got {}\ntour stops: {}",
        break_intervals.len(),
        format_tour_debug(tour)
    );

    for (start, end, _, _) in &break_intervals {
        let duration = end - start;
        assert!(
            (duration - expected_break_duration).abs() < 1.0,
            "break duration mismatch: expected {expected_break_duration}, got {duration}\ntour: {}",
            format_tour_debug(tour)
        );
    }

    // 2. Breaks don't overlap with job activities at DIFFERENT stops
    let non_break_job_intervals: Vec<_> =
        intervals.iter().filter(|(_, _, typ, _)| typ != "break" && typ != "departure" && typ != "arrival").collect();

    for (b_start, b_end, _, _) in &break_intervals {
        for (a_start, a_end, a_type, a_id) in &non_break_job_intervals {
            let same_stop = tour.stops.iter().any(|stop| {
                let acts = stop.activities();
                acts.iter().any(|a| a.activity_type == "break") && acts.iter().any(|a| a.job_id == **a_id)
            });
            if !same_stop {
                let overlaps = b_start < a_end && a_start < b_end;
                assert!(
                    !overlaps,
                    "break [{b_start}..{b_end}] overlaps with {a_type} '{a_id}' [{a_start}..{a_end}] at different stop\ntour: {}",
                    format_tour_debug(tour)
                );
            }
        }
    }

    // 3. Stop schedule consistency
    let mut prev_departure: Option<f64> = None;
    for (i, stop) in tour.stops.iter().enumerate() {
        let arr = parse_time(&stop.schedule().arrival);
        let dep = parse_time(&stop.schedule().departure);
        assert!(dep >= arr - 0.001, "stop {i}: departure ({dep}) < arrival ({arr})\ntour: {}", format_tour_debug(tour));
        if let Some(prev_dep) = prev_departure {
            assert!(
                arr >= prev_dep - 0.001,
                "stop {i}: arrival ({arr}) < previous departure ({prev_dep})\ntour: {}",
                format_tour_debug(tour)
            );
        }
        prev_departure = Some(dep);
    }

    // 4. Activities within each stop are time-ordered and within bounds
    for (i, stop) in tour.stops.iter().enumerate() {
        let stop_arr = parse_time(&stop.schedule().arrival);
        let stop_dep = parse_time(&stop.schedule().departure);
        let mut prev_act_start = f64::NEG_INFINITY;

        for act in stop.activities() {
            if let Some(time) = &act.time {
                let act_start = parse_time(&time.start);
                let act_end = parse_time(&time.end);
                assert!(
                    act_end >= act_start - 0.001,
                    "stop {i}: activity '{}' ({}) has end ({act_end}) < start ({act_start})\ntour: {}",
                    act.job_id,
                    act.activity_type,
                    format_tour_debug(tour)
                );
                assert!(
                    act_start >= stop_arr - 0.001,
                    "stop {i}: activity '{}' start ({act_start}) < stop arrival ({stop_arr})\ntour: {}",
                    act.job_id,
                    format_tour_debug(tour)
                );
                assert!(
                    act_end <= stop_dep + 0.001,
                    "stop {i}: activity '{}' end ({act_end}) > stop departure ({stop_dep})\ntour: {}",
                    act.job_id,
                    format_tour_debug(tour)
                );
                assert!(
                    act_start >= prev_act_start - 0.001,
                    "stop {i}: activity '{}' start ({act_start}) < previous activity start ({prev_act_start}) — not time-ordered\ntour: {}",
                    act.job_id,
                    format_tour_debug(tour)
                );
                prev_act_start = act_start;
            }
        }
    }

    // 5. Break time within tour bounds
    let tour_start = parse_time(&tour.stops.first().unwrap().schedule().departure);
    let tour_end = parse_time(&tour.stops.last().unwrap().schedule().arrival);
    for (b_start, b_end, _, _) in &break_intervals {
        assert!(
            *b_start >= tour_start - 0.001 && *b_end <= tour_end + 0.001,
            "break [{b_start}..{b_end}] outside tour time [{tour_start}..{tour_end}]\ntour: {}",
            format_tour_debug(tour)
        );
    }

    // 6. Break activities have no location
    for stop in &tour.stops {
        for act in stop.activities() {
            if act.activity_type == "break" {
                assert!(
                    act.location.is_none(),
                    "required break should have no location, but got {:?}\ntour: {}",
                    act.location,
                    format_tour_debug(tour)
                );
            }
        }
    }
}

/// Validates all tours in a solution.
fn validate_solution_breaks(solution: &Solution, expected_break_count: usize, expected_break_duration: f64) {
    assert!(!solution.tours.is_empty(), "expected at least one tour");
    for tour in &solution.tours {
        validate_tour_breaks_and_schedule(tour, expected_break_count, expected_break_duration);
    }
}

/// Debug formatter for a tour — prints all stops with activities, times, and locations.
fn format_tour_debug(tour: &Tour) -> String {
    let mut lines = vec![format!("vehicle={} shift={}", tour.vehicle_id, tour.shift_index)];
    for (i, stop) in tour.stops.iter().enumerate() {
        let s = stop.schedule();
        let loc = stop.location().map(|l| format!("{l:?}")).unwrap_or_default();
        let acts: Vec<_> = stop
            .activities()
            .iter()
            .map(|a| {
                let t = a.time.as_ref().map(|t| format!("[{}..{}]", t.start, t.end)).unwrap_or_default();
                format!("  {}({}) {}", a.job_id, a.activity_type, t)
            })
            .collect();
        let stop_type = if matches!(stop, Stop::Transit(_)) { "T" } else { "P" };
        lines.push(format!("  stop {i}{stop_type} {loc}: arr={} dep={}", s.arrival, s.departure));
        for a in acts {
            lines.push(format!("    {a}"));
        }
    }
    lines.join("\n")
}

// =============================================================================
// Basic scenarios
// =============================================================================

#[test]
fn can_assign_offset_break_with_flexible_departure() {
    let problem = Problem {
        plan: Plan {
            jobs: vec![create_delivery_job("job1", (5., 0.)), create_delivery_job("job2", (15., 0.))],
            ..create_empty_plan()
        },
        fleet: Fleet {
            vehicles: vec![VehicleType {
                shifts: vec![VehicleShift {
                    end: Some(ShiftEnd { earliest: None, latest: format_time(100.), location: (0., 0.).to_loc() }),
                    breaks: Some(vec![VehicleBreak::Required {
                        time: VehicleRequiredBreakTime::OffsetTime { earliest: 7., latest: 7. },
                        duration: 2.,
                    }]),
                    ..create_default_vehicle_shift()
                }],
                ..create_default_vehicle_type()
            }],
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic(problem, Some(vec![matrix]));

    assert!(solution.violations.is_none(), "expected no violations");
    assert!(solution.unassigned.is_none(), "expected all jobs assigned");
    validate_solution_breaks(&solution, 1, 2.0);

    let departure = parse_time(&solution.tours[0].stops[0].schedule().departure);
    let intervals = collect_activity_intervals(&solution.tours[0]);
    let (b_start, _, _, _) = intervals.iter().find(|(_, _, t, _)| t == "break").unwrap();
    let offset = b_start - departure;
    assert!((offset - 7.0).abs() < 1.0, "break offset from departure should be ~7, got {offset}");
}

#[test]
fn can_assign_offset_break_with_wide_end_window_and_late_jobs() {
    let problem = Problem {
        plan: Plan {
            jobs: vec![
                create_delivery_job_with_times("job1", (5., 0.), vec![(30, 100)], 1.),
                create_delivery_job_with_times("job2", (15., 0.), vec![(30, 100)], 1.),
            ],
            ..create_empty_plan()
        },
        fleet: Fleet {
            vehicles: vec![VehicleType {
                shifts: vec![VehicleShift {
                    end: Some(ShiftEnd { earliest: None, latest: format_time(200.), location: (0., 0.).to_loc() }),
                    breaks: Some(vec![VehicleBreak::Required {
                        time: VehicleRequiredBreakTime::OffsetTime { earliest: 7., latest: 7. },
                        duration: 2.,
                    }]),
                    ..create_default_vehicle_shift()
                }],
                ..create_default_vehicle_type()
            }],
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic_and_iterations_without_check(problem, Some(vec![matrix]), 200);

    assert!(solution.unassigned.is_none(), "expected all jobs assigned");
    let departure = parse_time(&solution.tours[0].stops[0].schedule().departure);
    assert!(departure > 0., "expected departure to be advanced past time 0, got {departure}");
    validate_solution_breaks(&solution, 1, 2.0);
}

#[test]
fn can_assign_offset_break_with_recede_departure() {
    let problem = Problem {
        plan: Plan {
            jobs: vec![create_delivery_job("job1", (5., 0.)), create_delivery_job("job2", (15., 0.))],
            ..create_empty_plan()
        },
        fleet: Fleet {
            vehicles: vec![VehicleType {
                shifts: vec![VehicleShift {
                    end: Some(ShiftEnd { earliest: None, latest: format_time(100.), location: (0., 0.).to_loc() }),
                    breaks: Some(vec![VehicleBreak::Required {
                        time: VehicleRequiredBreakTime::OffsetTime { earliest: 7., latest: 7. },
                        duration: 2.,
                    }]),
                    ..create_default_vehicle_shift()
                }],
                ..create_default_vehicle_type()
            }],
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic(problem, Some(vec![matrix]));

    assert!(solution.violations.is_none());
    assert!(solution.unassigned.is_none());
    validate_solution_breaks(&solution, 1, 2.0);
}

// =============================================================================
// Mixed break types
// =============================================================================

#[test]
fn can_handle_mixed_break_types_in_validation() {
    let problem = Problem {
        plan: Plan {
            jobs: vec![
                create_delivery_job("job1", (5., 0.)),
                create_delivery_job("job2", (15., 0.)),
                create_delivery_job("job3", (25., 0.)),
            ],
            ..create_empty_plan()
        },
        fleet: Fleet {
            vehicles: vec![VehicleType {
                shifts: vec![VehicleShift {
                    start: ShiftStart {
                        earliest: format_time(0.),
                        latest: Some(format_time(0.)),
                        location: (0., 0.).to_loc(),
                    },
                    breaks: Some(vec![
                        VehicleBreak::Required {
                            time: VehicleRequiredBreakTime::ExactTime {
                                earliest: format_time(7.),
                                latest: format_time(7.),
                            },
                            duration: 2.,
                        },
                        VehicleBreak::Required {
                            time: VehicleRequiredBreakTime::OffsetTime { earliest: 22., latest: 22. },
                            duration: 2.,
                        },
                    ]),
                    ..create_default_vehicle_shift()
                }],
                ..create_default_vehicle_type()
            }],
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic_and_iterations_without_check(problem, Some(vec![matrix]), 200);

    assert!(solution.unassigned.is_none());
    validate_solution_breaks(&solution, 2, 2.0);

    let intervals = collect_activity_intervals(&solution.tours[0]);
    let mut break_starts: Vec<f64> =
        intervals.iter().filter(|(_, _, t, _)| t == "break").map(|(s, _, _, _)| *s).collect();
    break_starts.sort_by(|a, b| a.total_cmp(b));
    assert_eq!(break_starts.len(), 2);
    assert!((break_starts[0] - 7.0).abs() < 1.0, "first break should start at ~7, got {}", break_starts[0]);
    assert!((break_starts[1] - 22.0).abs() < 1.0, "second break should start at ~22, got {}", break_starts[1]);
}

// =============================================================================
// FirstJobToLastJob cost span
// =============================================================================

#[test]
fn can_assign_offset_break_with_first_job_cost_span() {
    let problem = Problem {
        plan: Plan {
            jobs: vec![create_delivery_job("job1", (10., 0.)), create_delivery_job("job2", (25., 0.))],
            ..create_empty_plan()
        },
        fleet: Fleet {
            vehicles: vec![VehicleType {
                costs: VehicleCosts {
                    fixed: Some(10.),
                    distance: 1.,
                    time: 1.,
                    span: Some(RouteCostSpan::FirstJobToLastJob),
                },
                shifts: vec![VehicleShift {
                    start: ShiftStart {
                        earliest: format_time(0.),
                        latest: Some(format_time(0.)),
                        location: (0., 0.).to_loc(),
                    },
                    end: Some(ShiftEnd { earliest: None, latest: format_time(200.), location: (0., 0.).to_loc() }),
                    breaks: Some(vec![VehicleBreak::Required {
                        time: VehicleRequiredBreakTime::OffsetTime { earliest: 7., latest: 7. },
                        duration: 2.,
                    }]),
                    ..create_default_vehicle_shift()
                }],
                ..create_default_vehicle_type()
            }],
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic_and_iterations_without_check(problem, Some(vec![matrix]), 200);

    assert!(solution.unassigned.is_none());
    validate_solution_breaks(&solution, 1, 2.0);

    let intervals = collect_activity_intervals(&solution.tours[0]);
    let first_job = intervals.iter().find(|(_, _, _, id)| id == "job1").expect("job1 missing");
    let brk = intervals.iter().find(|(_, _, t, _)| t == "break").expect("break missing");
    let offset_from_first_job = brk.0 - first_job.0;
    assert!(
        (offset_from_first_job - 7.0).abs() < 1.0,
        "break offset from first job arrival ({}) should be ~7, got {offset_from_first_job} (break at {})",
        first_job.0,
        brk.0
    );
}

#[test]
fn can_assign_offset_break_with_first_job_span_and_range_offset() {
    let problem = Problem {
        plan: Plan {
            jobs: vec![
                create_delivery_job("job1", (10., 0.)),
                create_delivery_job("job2", (20., 0.)),
                create_delivery_job("job3", (30., 0.)),
            ],
            ..create_empty_plan()
        },
        fleet: Fleet {
            vehicles: vec![VehicleType {
                costs: VehicleCosts {
                    fixed: Some(10.),
                    distance: 1.,
                    time: 1.,
                    span: Some(RouteCostSpan::FirstJobToLastJob),
                },
                shifts: vec![VehicleShift {
                    start: ShiftStart {
                        earliest: format_time(0.),
                        latest: Some(format_time(0.)),
                        location: (0., 0.).to_loc(),
                    },
                    end: Some(ShiftEnd { earliest: None, latest: format_time(200.), location: (0., 0.).to_loc() }),
                    breaks: Some(vec![VehicleBreak::Required {
                        time: VehicleRequiredBreakTime::OffsetTime { earliest: 7., latest: 12. },
                        duration: 2.,
                    }]),
                    ..create_default_vehicle_shift()
                }],
                ..create_default_vehicle_type()
            }],
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic_and_iterations_without_check(problem, Some(vec![matrix]), 200);

    assert!(solution.unassigned.is_none());
    validate_solution_breaks(&solution, 1, 2.0);

    let intervals = collect_activity_intervals(&solution.tours[0]);
    let first_route_job = intervals
        .iter()
        .find(|(_, _, typ, _)| typ != "departure" && typ != "arrival" && typ != "break")
        .expect("no job in route");
    let brk = intervals.iter().find(|(_, _, t, _)| t == "break").unwrap();
    let offset = brk.0 - first_route_job.0;
    assert!(
        offset >= 6.0 && offset <= 14.0,
        "break offset from first job arrival ({}) should be in [7..12], got {offset} (break at {})",
        first_route_job.0,
        brk.0
    );
}

// =============================================================================
// Wide offset range — the core bug scenario
// =============================================================================

#[test]
fn can_assign_wide_range_offset_break_during_long_travel() {
    // Time windows force ordering: depot→job1→job2→depot.
    // Wide offset [4, 40]: break triggers at 40 during long travel job1→job2.
    // Previously: avoid_reserved_time_when_driving incorrectly shifted departure (6 > 4),
    // and break_writer failed to place the break due to TransitBreakMoved with no matching stop.
    let problem = Problem {
        plan: Plan {
            jobs: vec![
                create_delivery_job_with_times("job1", (5., 0.), vec![(0, 10)], 1.),
                create_delivery_job_with_times("job2", (50., 0.), vec![(40, 100)], 1.),
            ],
            ..create_empty_plan()
        },
        fleet: Fleet {
            vehicles: vec![VehicleType {
                shifts: vec![VehicleShift {
                    start: ShiftStart {
                        earliest: format_time(0.),
                        latest: Some(format_time(0.)),
                        location: (0., 0.).to_loc(),
                    },
                    end: Some(ShiftEnd { earliest: None, latest: format_time(500.), location: (0., 0.).to_loc() }),
                    breaks: Some(vec![VehicleBreak::Required {
                        time: VehicleRequiredBreakTime::OffsetTime { earliest: 4., latest: 40. },
                        duration: 2.,
                    }]),
                    ..create_default_vehicle_shift()
                }],
                ..create_default_vehicle_type()
            }],
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic_and_iterations_without_check(problem, Some(vec![matrix]), 200);

    assert!(solution.unassigned.is_none());
    validate_solution_breaks(&solution, 1, 2.0);
}

#[test]
fn can_place_wide_offset_break_on_transit_leg_with_consistent_times() {
    // Strict regression check for wide offset break placement:
    // break must be placed on transit leg job1 -> job2 with coherent timing.
    let problem = Problem {
        plan: Plan {
            jobs: vec![
                create_delivery_job_with_times("job1", (5., 0.), vec![(0, 10)], 1.),
                create_delivery_job_with_times("job2", (50., 0.), vec![(40, 100)], 1.),
            ],
            ..create_empty_plan()
        },
        fleet: Fleet {
            vehicles: vec![VehicleType {
                shifts: vec![VehicleShift {
                    start: ShiftStart {
                        earliest: format_time(0.),
                        latest: Some(format_time(0.)),
                        location: (0., 0.).to_loc(),
                    },
                    end: Some(ShiftEnd { earliest: None, latest: format_time(500.), location: (0., 0.).to_loc() }),
                    breaks: Some(vec![VehicleBreak::Required {
                        time: VehicleRequiredBreakTime::OffsetTime { earliest: 4., latest: 40. },
                        duration: 2.,
                    }]),
                    ..create_default_vehicle_shift()
                }],
                ..create_default_vehicle_type()
            }],
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic_and_iterations_without_check(problem, Some(vec![matrix]), 200);

    assert!(solution.unassigned.is_none(), "expected all jobs assigned");
    assert_eq!(solution.tours.len(), 1, "expected one tour");

    let tour = &solution.tours[0];
    let debug = format_tour_debug(tour);

    validate_tour_schedule_only(tour);
    validate_no_break_job_overlap(tour);

    let break_positions: Vec<_> = tour
        .stops
        .iter()
        .enumerate()
        .flat_map(|(stop_idx, stop)| {
            stop.activities().iter().enumerate().filter_map(move |(act_idx, activity)| {
                if activity.activity_type == "break" { Some((stop_idx, act_idx)) } else { None }
            })
        })
        .collect();

    assert_eq!(break_positions.len(), 1, "expected exactly one break\n{debug}");

    let flat_order: Vec<_> =
        tour.stops.iter().flat_map(|stop| stop.activities().iter().map(|activity| activity.job_id.clone())).collect();
    assert_eq!(
        flat_order,
        vec!["departure", "job1", "break", "job2", "arrival"],
        "unexpected flattened activity order\n{debug}"
    );

    let (break_stop_idx, break_activity_idx) = break_positions[0];
    assert!(
        break_stop_idx > 0 && break_stop_idx + 1 < tour.stops.len(),
        "break stop should have previous and next stops\n{debug}"
    );

    let break_stop = &tour.stops[break_stop_idx];
    assert!(matches!(break_stop, Stop::Transit(_)), "break should be attached to transit stop\n{debug}");
    assert_eq!(break_stop.activities().len(), 1, "transit break stop should have a single break activity\n{debug}");

    let prev_stop = &tour.stops[break_stop_idx - 1];
    let next_stop = &tour.stops[break_stop_idx + 1];
    assert!(
        prev_stop.activities().iter().any(|activity| activity.job_id == "job1"),
        "break previous stop should contain job1\n{debug}"
    );
    assert!(
        next_stop.activities().iter().any(|activity| activity.job_id == "job2"),
        "break next stop should contain job2\n{debug}"
    );

    let break_activity = &break_stop.activities()[break_activity_idx];
    let stop_arrival = parse_time(&break_stop.schedule().arrival);
    let stop_departure = parse_time(&break_stop.schedule().departure);
    let (break_start, break_end) = break_activity
        .time
        .as_ref()
        .map(|time| (parse_time(&time.start), parse_time(&time.end)))
        .unwrap_or((stop_arrival, stop_departure));

    assert!(
        (break_start - stop_arrival).abs() < 1e-3 && (break_end - stop_departure).abs() < 1e-3,
        "break activity interval should match transit stop interval\n{debug}"
    );

    let prev_departure = parse_time(&prev_stop.schedule().departure);
    let next_arrival = parse_time(&next_stop.schedule().arrival);
    assert!(
        break_start >= prev_departure - 1e-3,
        "break starts before previous stop departure: break_start={break_start}, prev_departure={prev_departure}\n{debug}"
    );
    assert!(
        break_start < break_end - 1e-3,
        "break interval is not strictly positive: [{break_start}..{break_end}]\n{debug}"
    );
    assert!(
        break_end <= next_arrival + 1e-3,
        "break ends after next stop arrival: break_end={break_end}, next_arrival={next_arrival}\n{debug}"
    );

    let departure = parse_time(&tour.stops[0].schedule().departure);
    let offset = break_start - departure;
    assert!((offset - 40.0).abs() <= 1.0, "break offset from tour departure should be near 40, got {offset}\n{debug}");
}

#[test]
fn can_keep_job_activity_duration_when_break_starts_at_activity_end_on_same_stop() {
    // Boundary regression: required break starts exactly when job1 activity ends.
    // Break should be on the same point stop as job1 without extending job1 activity time.
    let problem = Problem {
        plan: Plan {
            jobs: vec![
                create_delivery_job_with_times("job1", (4., 0.), vec![(5, 10)], 1.),
                create_delivery_job_with_times("job2", (12., 0.), vec![(20, 100)], 1.),
            ],
            ..create_empty_plan()
        },
        fleet: Fleet {
            vehicles: vec![VehicleType {
                shifts: vec![VehicleShift {
                    start: ShiftStart {
                        earliest: format_time(0.),
                        latest: Some(format_time(0.)),
                        location: (0., 0.).to_loc(),
                    },
                    end: Some(ShiftEnd { earliest: None, latest: format_time(300.), location: (0., 0.).to_loc() }),
                    breaks: Some(vec![VehicleBreak::Required {
                        time: VehicleRequiredBreakTime::OffsetTime { earliest: 5., latest: 6. },
                        duration: 2.,
                    }]),
                    ..create_default_vehicle_shift()
                }],
                ..create_default_vehicle_type()
            }],
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic_and_iterations_without_check(problem, Some(vec![matrix]), 200);

    assert!(solution.unassigned.is_none(), "expected all jobs assigned");
    assert_eq!(solution.tours.len(), 1, "expected one tour");

    let tour = &solution.tours[0];
    let debug = format_tour_debug(tour);

    let break_positions: Vec<_> = tour
        .stops
        .iter()
        .enumerate()
        .flat_map(|(stop_idx, stop)| {
            stop.activities().iter().enumerate().filter_map(move |(act_idx, activity)| {
                if activity.activity_type == "break" { Some((stop_idx, act_idx)) } else { None }
            })
        })
        .collect();

    assert_eq!(break_positions.len(), 1, "expected exactly one break\n{debug}");

    let flat_order: Vec<_> =
        tour.stops.iter().flat_map(|stop| stop.activities().iter().map(|activity| activity.job_id.clone())).collect();
    assert_eq!(
        flat_order,
        vec!["departure", "job1", "break", "job2", "arrival"],
        "unexpected flattened activity order\n{debug}"
    );

    let (break_stop_idx, break_activity_idx) = break_positions[0];
    assert!(
        break_stop_idx > 0 && break_stop_idx + 1 < tour.stops.len(),
        "break stop should have previous and next stops\n{debug}"
    );

    let break_stop = &tour.stops[break_stop_idx];
    assert!(
        matches!(break_stop, Stop::Point(_)),
        "break should be attached to point stop with job1 in this scenario\n{debug}"
    );
    assert!(
        break_stop.activities().iter().any(|activity| activity.job_id == "job1"),
        "break stop should contain job1\n{debug}"
    );

    let stop_arrival = parse_time(&break_stop.schedule().arrival);
    let stop_departure = parse_time(&break_stop.schedule().departure);

    let break_activity = &break_stop.activities()[break_activity_idx];
    let (break_start, break_end) = break_activity
        .time
        .as_ref()
        .map(|time| (parse_time(&time.start), parse_time(&time.end)))
        .unwrap_or((stop_arrival, stop_departure));

    let job1_activity = break_stop
        .activities()
        .iter()
        .find(|activity| activity.job_id == "job1")
        .expect("job1 activity should be on break stop");
    let (job1_start, job1_end) = job1_activity
        .time
        .as_ref()
        .map(|time| (parse_time(&time.start), parse_time(&time.end)))
        .unwrap_or((stop_arrival, stop_departure));

    assert!(
        ((job1_end - job1_start) - 1.0).abs() < 1e-3,
        "job1 activity duration should stay at 1, got {}\n{debug}",
        job1_end - job1_start
    );
    assert!(
        job1_end <= break_start + 1e-3,
        "job1 end should not be after break start: job1_end={job1_end}, break_start={break_start}\n{debug}"
    );
    assert!(
        break_start < break_end - 1e-3,
        "break interval is not strictly positive: [{break_start}..{break_end}]\n{debug}"
    );
    assert!(
        break_start >= stop_arrival - 1e-3 && break_end <= stop_departure + 1e-3,
        "break interval should stay within stop bounds: stop=[{stop_arrival}..{stop_departure}], break=[{break_start}..{break_end}]\n{debug}"
    );

    let prev_departure = parse_time(&tour.stops[break_stop_idx - 1].schedule().departure);
    let next_arrival = parse_time(&tour.stops[break_stop_idx + 1].schedule().arrival);
    assert!(
        prev_departure <= break_start + 1e-3,
        "break starts before previous stop departure: prev_departure={prev_departure}, break_start={break_start}\n{debug}"
    );
    assert!(
        break_end <= next_arrival + 1e-3,
        "break ends after next stop arrival: break_end={break_end}, next_arrival={next_arrival}\n{debug}"
    );

    validate_tour_schedule_only(tour);
    validate_no_break_job_overlap(tour);
}

#[test]
fn can_align_required_break_to_job_boundary_when_reserved_time_hits_mid_activity() {
    let problem = Problem {
        plan: Plan { jobs: vec![create_delivery_job_with_duration("job1", (5., 0.), 3.)], ..create_empty_plan() },
        fleet: Fleet {
            vehicles: vec![VehicleType {
                shifts: vec![VehicleShift {
                    start: ShiftStart {
                        earliest: format_time(0.),
                        latest: Some(format_time(0.)),
                        location: (0., 0.).to_loc(),
                    },
                    end: Some(ShiftEnd { earliest: None, latest: format_time(200.), location: (0., 0.).to_loc() }),
                    breaks: Some(vec![VehicleBreak::Required {
                        time: VehicleRequiredBreakTime::ExactTime {
                            earliest: format_time(7.),
                            latest: format_time(7.),
                        },
                        duration: 2.,
                    }]),
                    ..create_default_vehicle_shift()
                }],
                ..create_default_vehicle_type()
            }],
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic(problem, Some(vec![matrix]));

    assert!(solution.unassigned.is_none(), "expected all jobs assigned");
    assert_eq!(solution.tours.len(), 1, "expected one tour");

    let tour = &solution.tours[0];
    let debug = format_tour_debug(tour);

    let stop = tour
        .stops
        .iter()
        .find(|stop| {
            stop.activities().iter().any(|activity| activity.job_id == "job1")
                && stop.activities().iter().any(|activity| activity.activity_type == "break")
        })
        .expect("expected job1 and break at same stop");

    let stop_arrival = parse_time(&stop.schedule().arrival);
    let stop_departure = parse_time(&stop.schedule().departure);

    let job = stop.activities().iter().find(|activity| activity.job_id == "job1").expect("job1 activity");
    let brk = stop.activities().iter().find(|activity| activity.activity_type == "break").expect("break activity");

    let (job_start, job_end) = job
        .time
        .as_ref()
        .map(|time| (parse_time(&time.start), parse_time(&time.end)))
        .unwrap_or((stop_arrival, stop_departure));
    let (break_start, break_end) = brk
        .time
        .as_ref()
        .map(|time| (parse_time(&time.start), parse_time(&time.end)))
        .unwrap_or((stop_arrival, stop_departure));

    assert!(
        ((job_end - job_start) - 3.0).abs() < 1e-3,
        "job1 duration should stay equal to service duration, got {}\n{debug}",
        job_end - job_start
    );
    assert!(
        (break_start - job_end).abs() <= 1e-3 || (job_start - break_end).abs() <= 1e-3,
        "break should start at job end or finish at job start, got job=[{job_start}..{job_end}], break=[{break_start}..{break_end}]\n{debug}"
    );
    assert!(
        !(break_start < job_end - 1e-3 && job_start < break_end - 1e-3),
        "break should not overlap job activity at same stop: job=[{job_start}..{job_end}], break=[{break_start}..{break_end}]\n{debug}"
    );
    assert!(
        break_start >= stop_arrival - 1e-3 && break_end <= stop_departure + 1e-3,
        "break should stay within stop bounds: stop=[{stop_arrival}..{stop_departure}], break=[{break_start}..{break_end}]\n{debug}"
    );

    validate_tour_schedule_only(tour);
    validate_no_break_job_overlap(tour);
}

#[test]
fn can_skip_required_break_when_it_starts_at_tour_end_boundary() {
    let problem = Problem {
        plan: Plan { jobs: vec![create_delivery_job("job1", (5., 0.))], ..create_empty_plan() },
        fleet: Fleet {
            vehicles: vec![VehicleType {
                shifts: vec![VehicleShift {
                    start: ShiftStart {
                        earliest: format_time(0.),
                        latest: Some(format_time(0.)),
                        location: (0., 0.).to_loc(),
                    },
                    end: Some(ShiftEnd { earliest: None, latest: format_time(200.), location: (0., 0.).to_loc() }),
                    breaks: Some(vec![VehicleBreak::Required {
                        time: VehicleRequiredBreakTime::OffsetTime { earliest: 11., latest: 11. },
                        duration: 2.,
                    }]),
                    ..create_default_vehicle_shift()
                }],
                ..create_default_vehicle_type()
            }],
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic(problem, Some(vec![matrix]));

    assert!(solution.unassigned.is_none(), "expected all jobs assigned");
    assert_eq!(solution.tours.len(), 1, "expected one tour");

    let tour = &solution.tours[0];
    let debug = format_tour_debug(tour);

    let break_count = tour
        .stops
        .iter()
        .flat_map(|stop| stop.activities().iter())
        .filter(|activity| activity.activity_type == "break")
        .count();
    assert_eq!(break_count, 0, "required break should be skipped when it only touches tour end boundary\n{debug}");

    let flat_order: Vec<_> =
        tour.stops.iter().flat_map(|stop| stop.activities().iter().map(|activity| activity.job_id.clone())).collect();
    assert_eq!(flat_order, vec!["departure", "job1", "arrival"], "unexpected flattened activity order\n{debug}");

    let last_stop = tour.stops.last().expect("expected last stop");
    let last_arrival = parse_time(&last_stop.schedule().arrival);
    let last_departure = parse_time(&last_stop.schedule().departure);
    assert!(
        (last_departure - last_arrival).abs() <= 1e-3,
        "last stop should not be stretched by boundary-touching break: arrival={last_arrival}, departure={last_departure}\n{debug}"
    );

    validate_tour_schedule_only(tour);
    validate_no_break_job_overlap(tour);
}

#[test]
fn can_assign_range_offset_break_without_wrong_departure_shift() {
    let problem = Problem {
        plan: Plan {
            jobs: vec![
                create_delivery_job("job1", (5., 0.)),
                create_delivery_job("job2", (12., 0.)),
                create_delivery_job("job3", (20., 0.)),
            ],
            ..create_empty_plan()
        },
        fleet: Fleet {
            vehicles: vec![VehicleType {
                shifts: vec![VehicleShift {
                    start: ShiftStart {
                        earliest: format_time(0.),
                        latest: Some(format_time(0.)),
                        location: (0., 0.).to_loc(),
                    },
                    end: Some(ShiftEnd { earliest: None, latest: format_time(200.), location: (0., 0.).to_loc() }),
                    breaks: Some(vec![VehicleBreak::Required {
                        time: VehicleRequiredBreakTime::OffsetTime { earliest: 4., latest: 12. },
                        duration: 2.,
                    }]),
                    ..create_default_vehicle_shift()
                }],
                ..create_default_vehicle_type()
            }],
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic_and_iterations_without_check(problem, Some(vec![matrix]), 200);

    assert!(solution.unassigned.is_none());
    validate_solution_breaks(&solution, 1, 2.0);
}

// =============================================================================
// Complex realistic scenarios
// =============================================================================

#[test]
fn can_assign_break_with_many_closely_spaced_jobs_and_long_service() {
    // 6 jobs along a line with varying service durations (some long).
    // Break at offset [10, 15] with duration 3.
    // Tests that break is placed correctly between dense job stops with long service times.
    let problem = Problem {
        plan: Plan {
            jobs: vec![
                create_delivery_job_with_duration("j1", (3., 0.), 2.),
                create_delivery_job_with_duration("j2", (6., 0.), 4.),
                create_delivery_job_with_duration("j3", (9., 0.), 1.),
                create_delivery_job_with_duration("j4", (12., 0.), 3.),
                create_delivery_job_with_duration("j5", (15., 0.), 2.),
                create_delivery_job_with_duration("j6", (18., 0.), 1.),
            ],
            ..create_empty_plan()
        },
        fleet: Fleet {
            vehicles: vec![VehicleType {
                shifts: vec![VehicleShift {
                    start: ShiftStart {
                        earliest: format_time(0.),
                        latest: Some(format_time(0.)),
                        location: (0., 0.).to_loc(),
                    },
                    end: Some(ShiftEnd { earliest: None, latest: format_time(300.), location: (0., 0.).to_loc() }),
                    breaks: Some(vec![VehicleBreak::Required {
                        time: VehicleRequiredBreakTime::OffsetTime { earliest: 10., latest: 15. },
                        duration: 3.,
                    }]),
                    ..create_default_vehicle_shift()
                }],
                ..create_default_vehicle_type()
            }],
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic_and_iterations_without_check(problem, Some(vec![matrix]), 200);

    assert!(solution.unassigned.is_none(), "expected all 6 jobs assigned");
    validate_solution_breaks(&solution, 1, 3.0);

    // Verify offset is in expected range
    let tour = &solution.tours[0];
    let departure = parse_time(&tour.stops[0].schedule().departure);
    let intervals = collect_activity_intervals(tour);
    let brk = intervals.iter().find(|(_, _, t, _)| t == "break").unwrap();
    let offset = brk.0 - departure;
    assert!(
        offset >= 9.0 && offset <= 18.0,
        "break offset from departure should be in [10..15] range, got {offset}\ntour: {}",
        format_tour_debug(tour)
    );
}

#[test]
fn can_assign_break_with_pickup_delivery_jobs() {
    // Pickup-delivery pairs: pickup at one location, deliver at another.
    // Break at offset [8, 12] with duration 2.
    // Tests that break doesn't split a pickup from its delivery incorrectly,
    // and that all schedule constraints hold.
    let problem = Problem {
        plan: Plan {
            jobs: vec![
                create_pickup_delivery_job("pd1", (5., 0.), (15., 0.)),
                create_pickup_delivery_job("pd2", (8., 0.), (20., 0.)),
            ],
            ..create_empty_plan()
        },
        fleet: Fleet {
            vehicles: vec![VehicleType {
                shifts: vec![VehicleShift {
                    start: ShiftStart {
                        earliest: format_time(0.),
                        latest: Some(format_time(0.)),
                        location: (0., 0.).to_loc(),
                    },
                    end: Some(ShiftEnd { earliest: None, latest: format_time(300.), location: (0., 0.).to_loc() }),
                    breaks: Some(vec![VehicleBreak::Required {
                        time: VehicleRequiredBreakTime::OffsetTime { earliest: 8., latest: 12. },
                        duration: 2.,
                    }]),
                    ..create_default_vehicle_shift()
                }],
                ..create_default_vehicle_type()
            }],
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic_and_iterations_without_check(problem, Some(vec![matrix]), 200);

    assert!(solution.unassigned.is_none(), "expected all pickup-delivery jobs assigned");
    validate_solution_breaks(&solution, 1, 2.0);
}

#[test]
fn can_assign_break_with_tight_time_windows_and_long_break() {
    // Jobs with time windows forcing a specific schedule.
    // Break duration is relatively long (5 units).
    // Tests that long breaks don't violate time window constraints or overlap with activities.
    let problem = Problem {
        plan: Plan {
            jobs: vec![
                create_delivery_job_with_times("j1", (5., 0.), vec![(4, 12)], 1.),
                create_delivery_job_with_times("j2", (10., 0.), vec![(12, 30)], 1.),
                create_delivery_job_with_times("j3", (15., 0.), vec![(20, 45)], 1.),
            ],
            ..create_empty_plan()
        },
        fleet: Fleet {
            vehicles: vec![VehicleType {
                shifts: vec![VehicleShift {
                    start: ShiftStart {
                        earliest: format_time(0.),
                        latest: Some(format_time(0.)),
                        location: (0., 0.).to_loc(),
                    },
                    end: Some(ShiftEnd { earliest: None, latest: format_time(300.), location: (0., 0.).to_loc() }),
                    breaks: Some(vec![VehicleBreak::Required {
                        time: VehicleRequiredBreakTime::OffsetTime { earliest: 8., latest: 12. },
                        duration: 5.,
                    }]),
                    ..create_default_vehicle_shift()
                }],
                ..create_default_vehicle_type()
            }],
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic_and_iterations_without_check(problem, Some(vec![matrix]), 200);

    assert!(solution.unassigned.is_none(), "expected all jobs assigned");
    validate_solution_breaks(&solution, 1, 5.0);
}

#[test]
fn can_assign_break_with_multiple_vehicles() {
    // Two vehicles, each with their own break offset.
    // 4 jobs spread out: each vehicle takes ~2 jobs.
    // Tests that each vehicle gets its own break with correct offset.
    let problem = Problem {
        plan: Plan {
            jobs: vec![
                create_delivery_job("j1", (5., 5.)),
                create_delivery_job("j2", (10., 5.)),
                create_delivery_job("j3", (5., -5.)),
                create_delivery_job("j4", (10., -5.)),
            ],
            ..create_empty_plan()
        },
        fleet: Fleet {
            vehicles: vec![
                VehicleType {
                    type_id: "v1_type".to_string(),
                    vehicle_ids: vec!["v1".to_string()],
                    shifts: vec![VehicleShift {
                        start: ShiftStart {
                            earliest: format_time(0.),
                            latest: Some(format_time(0.)),
                            location: (0., 0.).to_loc(),
                        },
                        end: Some(ShiftEnd { earliest: None, latest: format_time(300.), location: (0., 0.).to_loc() }),
                        breaks: Some(vec![VehicleBreak::Required {
                            time: VehicleRequiredBreakTime::OffsetTime { earliest: 8., latest: 8. },
                            duration: 2.,
                        }]),
                        ..create_default_vehicle_shift()
                    }],
                    capacity: vec![10],
                    ..create_default_vehicle_type()
                },
                VehicleType {
                    type_id: "v2_type".to_string(),
                    vehicle_ids: vec!["v2".to_string()],
                    shifts: vec![VehicleShift {
                        start: ShiftStart {
                            earliest: format_time(0.),
                            latest: Some(format_time(0.)),
                            location: (0., 0.).to_loc(),
                        },
                        end: Some(ShiftEnd { earliest: None, latest: format_time(300.), location: (0., 0.).to_loc() }),
                        breaks: Some(vec![VehicleBreak::Required {
                            time: VehicleRequiredBreakTime::OffsetTime { earliest: 10., latest: 10. },
                            duration: 3.,
                        }]),
                        ..create_default_vehicle_shift()
                    }],
                    capacity: vec![10],
                    ..create_default_vehicle_type()
                },
            ],
            profiles: create_default_matrix_profiles(),
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic_and_iterations_without_check(problem, Some(vec![matrix]), 200);

    assert!(solution.unassigned.is_none(), "expected all 4 jobs assigned");
    assert!(solution.tours.len() >= 1, "expected at least 1 tour");

    // Validate each tour independently — break count/duration varies by vehicle type
    for tour in &solution.tours {
        let expected_duration = if tour.vehicle_id == "v1" { 2.0 } else { 3.0 };
        validate_tour_breaks_and_schedule(tour, 1, expected_duration);
    }
}

#[test]
fn can_assign_break_with_flexible_departure_and_many_jobs_clustered() {
    // Flexible departure (no latest). 8 jobs clustered in two groups far apart.
    // Group A at x~5..10, Group B at x~40..50. Break offset [15, 25] duration 3.
    // The break should occur during the long travel between groups, not overlap with any job.
    let problem = Problem {
        plan: Plan {
            jobs: vec![
                create_delivery_job("a1", (5., 0.)),
                create_delivery_job("a2", (7., 0.)),
                create_delivery_job("a3", (8., 1.)),
                create_delivery_job("a4", (10., 0.)),
                create_delivery_job("b1", (40., 0.)),
                create_delivery_job("b2", (43., 0.)),
                create_delivery_job("b3", (45., 1.)),
                create_delivery_job("b4", (48., 0.)),
            ],
            ..create_empty_plan()
        },
        fleet: Fleet {
            vehicles: vec![VehicleType {
                shifts: vec![VehicleShift {
                    end: Some(ShiftEnd { earliest: None, latest: format_time(500.), location: (0., 0.).to_loc() }),
                    breaks: Some(vec![VehicleBreak::Required {
                        time: VehicleRequiredBreakTime::OffsetTime { earliest: 15., latest: 25. },
                        duration: 3.,
                    }]),
                    ..create_default_vehicle_shift()
                }],
                ..create_default_vehicle_type()
            }],
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic_and_iterations_without_check(problem, Some(vec![matrix]), 200);

    assert!(solution.unassigned.is_none(), "expected all 8 jobs assigned");
    validate_solution_breaks(&solution, 1, 3.0);
}

#[test]
fn can_assign_break_with_first_job_span_flexible_departure_and_wide_offset() {
    // The full combination: FirstJobToLastJob span + flexible departure + wide offset range.
    // Departure is flexible, first job at (8,0). Anchor = first job arrival.
    // Break offset [4, 10] relative to anchor (first job arrival), duration 2.
    // 4 jobs along a line: 8, 15, 22, 30.
    let problem = Problem {
        plan: Plan {
            jobs: vec![
                create_delivery_job("j1", (8., 0.)),
                create_delivery_job("j2", (15., 0.)),
                create_delivery_job("j3", (22., 0.)),
                create_delivery_job("j4", (30., 0.)),
            ],
            ..create_empty_plan()
        },
        fleet: Fleet {
            vehicles: vec![VehicleType {
                costs: VehicleCosts {
                    fixed: Some(10.),
                    distance: 1.,
                    time: 1.,
                    span: Some(RouteCostSpan::FirstJobToLastJob),
                },
                shifts: vec![VehicleShift {
                    end: Some(ShiftEnd { earliest: None, latest: format_time(500.), location: (0., 0.).to_loc() }),
                    breaks: Some(vec![VehicleBreak::Required {
                        time: VehicleRequiredBreakTime::OffsetTime { earliest: 4., latest: 10. },
                        duration: 2.,
                    }]),
                    ..create_default_vehicle_shift()
                }],
                ..create_default_vehicle_type()
            }],
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic_and_iterations_without_check(problem, Some(vec![matrix]), 200);

    assert!(solution.unassigned.is_none(), "expected all 4 jobs assigned");
    validate_solution_breaks(&solution, 1, 2.0);

    // Verify offset is relative to first job, not departure
    let tour = &solution.tours[0];
    let intervals = collect_activity_intervals(tour);
    let first_route_job = intervals
        .iter()
        .find(|(_, _, typ, _)| typ != "departure" && typ != "arrival" && typ != "break")
        .expect("no job in route");
    let brk = intervals.iter().find(|(_, _, t, _)| t == "break").unwrap();
    let offset = brk.0 - first_route_job.0;
    assert!(
        offset >= 3.0 && offset <= 12.0,
        "break offset from first job ({}) should be in [4..10], got {offset} (break at {})\ntour: {}",
        first_route_job.0,
        brk.0,
        format_tour_debug(tour)
    );
}

#[test]
fn can_assign_break_with_first_job_span_late_time_windows_and_wide_offset() {
    // FirstJobToLastJob + late time windows + wide offset range [4, 20].
    // Jobs available only after t=20, so departure must be advanced.
    // Break should be relative to first job arrival, not departure.
    let problem = Problem {
        plan: Plan {
            jobs: vec![
                create_delivery_job_with_times("j1", (5., 0.), vec![(20, 50)], 1.),
                create_delivery_job_with_times("j2", (12., 0.), vec![(25, 60)], 1.),
                create_delivery_job_with_times("j3", (20., 0.), vec![(30, 70)], 1.),
            ],
            ..create_empty_plan()
        },
        fleet: Fleet {
            vehicles: vec![VehicleType {
                costs: VehicleCosts {
                    fixed: Some(10.),
                    distance: 1.,
                    time: 1.,
                    span: Some(RouteCostSpan::FirstJobToLastJob),
                },
                shifts: vec![VehicleShift {
                    end: Some(ShiftEnd { earliest: None, latest: format_time(500.), location: (0., 0.).to_loc() }),
                    breaks: Some(vec![VehicleBreak::Required {
                        time: VehicleRequiredBreakTime::OffsetTime { earliest: 4., latest: 20. },
                        duration: 2.,
                    }]),
                    ..create_default_vehicle_shift()
                }],
                ..create_default_vehicle_type()
            }],
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic_and_iterations_without_check(problem, Some(vec![matrix]), 200);

    assert!(solution.unassigned.is_none(), "expected all 3 jobs assigned");
    validate_solution_breaks(&solution, 1, 2.0);

    // Departure should have been advanced past 0
    let departure = parse_time(&solution.tours[0].stops[0].schedule().departure);
    assert!(departure > 0., "expected departure advanced for late time windows, got {departure}");
}

#[test]
fn can_assign_break_with_jobs_requiring_long_service_times() {
    // Jobs with long service durations (10, 15, 8 units). Break offset [20, 25] duration 3.
    // Tests that break placed during or between long-service jobs doesn't overlap.
    let problem = Problem {
        plan: Plan {
            jobs: vec![
                create_delivery_job_with_duration("j1", (5., 0.), 10.),
                create_delivery_job_with_duration("j2", (15., 0.), 15.),
                create_delivery_job_with_duration("j3", (25., 0.), 8.),
            ],
            ..create_empty_plan()
        },
        fleet: Fleet {
            vehicles: vec![VehicleType {
                shifts: vec![VehicleShift {
                    start: ShiftStart {
                        earliest: format_time(0.),
                        latest: Some(format_time(0.)),
                        location: (0., 0.).to_loc(),
                    },
                    end: Some(ShiftEnd { earliest: None, latest: format_time(500.), location: (0., 0.).to_loc() }),
                    breaks: Some(vec![VehicleBreak::Required {
                        time: VehicleRequiredBreakTime::OffsetTime { earliest: 20., latest: 25. },
                        duration: 3.,
                    }]),
                    ..create_default_vehicle_shift()
                }],
                ..create_default_vehicle_type()
            }],
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic_and_iterations_without_check(problem, Some(vec![matrix]), 200);

    assert!(solution.unassigned.is_none());
    validate_solution_breaks(&solution, 1, 3.0);
}

#[test]
fn can_assign_two_offset_breaks_with_wide_ranges() {
    // Two required breaks with wide offset ranges: [5, 15] and [25, 40].
    // 5 jobs along a long route. Tests that both breaks are placed correctly
    // without overlapping each other or any job activities.
    let problem = Problem {
        plan: Plan {
            jobs: vec![
                create_delivery_job("j1", (5., 0.)),
                create_delivery_job("j2", (15., 0.)),
                create_delivery_job("j3", (25., 0.)),
                create_delivery_job("j4", (35., 0.)),
                create_delivery_job("j5", (45., 0.)),
            ],
            ..create_empty_plan()
        },
        fleet: Fleet {
            vehicles: vec![VehicleType {
                shifts: vec![VehicleShift {
                    start: ShiftStart {
                        earliest: format_time(0.),
                        latest: Some(format_time(0.)),
                        location: (0., 0.).to_loc(),
                    },
                    end: Some(ShiftEnd { earliest: None, latest: format_time(500.), location: (0., 0.).to_loc() }),
                    breaks: Some(vec![
                        VehicleBreak::Required {
                            time: VehicleRequiredBreakTime::OffsetTime { earliest: 5., latest: 15. },
                            duration: 2.,
                        },
                        VehicleBreak::Required {
                            time: VehicleRequiredBreakTime::OffsetTime { earliest: 25., latest: 40. },
                            duration: 2.,
                        },
                    ]),
                    ..create_default_vehicle_shift()
                }],
                ..create_default_vehicle_type()
            }],
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic_and_iterations_without_check(problem, Some(vec![matrix]), 200);

    assert!(solution.unassigned.is_none(), "expected all 5 jobs assigned");
    validate_solution_breaks(&solution, 2, 2.0);

    // Verify the two breaks don't overlap each other
    let intervals = collect_activity_intervals(&solution.tours[0]);
    let breaks: Vec<_> = intervals.iter().filter(|(_, _, t, _)| t == "break").collect();
    assert_eq!(breaks.len(), 2);
    let (b1_start, b1_end, _, _) = breaks[0];
    let (b2_start, b2_end, _, _) = breaks[1];
    let overlaps = b1_start < b2_end && b2_start < b1_end;
    assert!(
        !overlaps,
        "two breaks overlap: [{b1_start}..{b1_end}] and [{b2_start}..{b2_end}]\ntour: {}",
        format_tour_debug(&solution.tours[0])
    );
}

#[test]
fn can_assign_exact_and_offset_breaks_with_many_jobs() {
    // Mixed: one ExactTime break at t=10, one OffsetTime break at offset [30, 40].
    // 6 jobs along a 60-unit route. Tests that both types coexist with many activities.
    let problem = Problem {
        plan: Plan {
            jobs: vec![
                create_delivery_job("j1", (5., 0.)),
                create_delivery_job("j2", (12., 0.)),
                create_delivery_job("j3", (20., 0.)),
                create_delivery_job("j4", (30., 0.)),
                create_delivery_job("j5", (42., 0.)),
                create_delivery_job("j6", (55., 0.)),
            ],
            ..create_empty_plan()
        },
        fleet: Fleet {
            vehicles: vec![VehicleType {
                shifts: vec![VehicleShift {
                    start: ShiftStart {
                        earliest: format_time(0.),
                        latest: Some(format_time(0.)),
                        location: (0., 0.).to_loc(),
                    },
                    end: Some(ShiftEnd { earliest: None, latest: format_time(500.), location: (0., 0.).to_loc() }),
                    breaks: Some(vec![
                        VehicleBreak::Required {
                            time: VehicleRequiredBreakTime::ExactTime {
                                earliest: format_time(10.),
                                latest: format_time(10.),
                            },
                            duration: 2.,
                        },
                        VehicleBreak::Required {
                            time: VehicleRequiredBreakTime::OffsetTime { earliest: 30., latest: 40. },
                            duration: 3.,
                        },
                    ]),
                    ..create_default_vehicle_shift()
                }],
                ..create_default_vehicle_type()
            }],
            ..create_default_fleet()
        },
        ..create_empty_problem()
    };
    let matrix = create_matrix_from_problem(&problem);
    let solution = solve_with_metaheuristic_and_iterations_without_check(problem, Some(vec![matrix]), 200);

    assert!(solution.unassigned.is_none(), "expected all 6 jobs assigned");

    let tour = &solution.tours[0];
    let intervals = collect_activity_intervals(tour);
    let breaks: Vec<_> = intervals.iter().filter(|(_, _, t, _)| t == "break").collect();
    assert_eq!(breaks.len(), 2, "expected 2 breaks\ntour: {}", format_tour_debug(tour));

    // Validate each break individually for duration
    for (b_start, b_end, _, _) in &breaks {
        let dur = b_end - b_start;
        assert!(dur >= 1.5 && dur <= 3.5, "unexpected break duration {dur}\ntour: {}", format_tour_debug(tour));
    }

    // Full validation (uses the longer break's duration for the uniform check — skip that, check manually)
    // Instead validate schedule and overlap manually
    validate_tour_schedule_only(tour);
    validate_no_break_job_overlap(tour);
}

/// Validates stop schedule consistency only (no break count/duration check).
fn validate_tour_schedule_only(tour: &Tour) {
    let mut prev_departure: Option<f64> = None;
    for (i, stop) in tour.stops.iter().enumerate() {
        let arr = parse_time(&stop.schedule().arrival);
        let dep = parse_time(&stop.schedule().departure);
        assert!(dep >= arr - 0.001, "stop {i}: dep ({dep}) < arr ({arr})\ntour: {}", format_tour_debug(tour));
        if let Some(prev_dep) = prev_departure {
            assert!(
                arr >= prev_dep - 0.001,
                "stop {i}: arr ({arr}) < prev dep ({prev_dep})\ntour: {}",
                format_tour_debug(tour)
            );
        }
        prev_departure = Some(dep);

        // Activities within stop should be time-ordered and within bounds
        for act in stop.activities() {
            if let Some(time) = &act.time {
                let a_start = parse_time(&time.start);
                let a_end = parse_time(&time.end);
                assert!(
                    a_end >= a_start - 0.001,
                    "stop {i}: activity '{}' end < start\ntour: {}",
                    act.job_id,
                    format_tour_debug(tour)
                );
                assert!(
                    a_start >= arr - 0.001,
                    "stop {i}: activity '{}' start ({a_start}) < stop arr ({arr})\ntour: {}",
                    act.job_id,
                    format_tour_debug(tour)
                );
                assert!(
                    a_end <= dep + 0.001,
                    "stop {i}: activity '{}' end ({a_end}) > stop dep ({dep})\ntour: {}",
                    act.job_id,
                    format_tour_debug(tour)
                );
            }
        }
    }
}

/// Validates no cross-stop overlap between break activities and job activities.
fn validate_no_break_job_overlap(tour: &Tour) {
    let intervals = collect_activity_intervals(tour);
    let breaks: Vec<_> = intervals.iter().filter(|(_, _, t, _)| t == "break").collect();
    let jobs: Vec<_> =
        intervals.iter().filter(|(_, _, t, _)| t != "break" && t != "departure" && t != "arrival").collect();

    for (b_start, b_end, _, _) in &breaks {
        for (a_start, a_end, a_type, a_id) in &jobs {
            let same_stop = tour.stops.iter().any(|stop| {
                let acts = stop.activities();
                acts.iter().any(|a| a.activity_type == "break") && acts.iter().any(|a| a.job_id == **a_id)
            });
            if !same_stop {
                let overlaps = b_start < a_end && a_start < b_end;
                assert!(
                    !overlaps,
                    "break [{b_start}..{b_end}] overlaps {a_type} '{a_id}' [{a_start}..{a_end}]\ntour: {}",
                    format_tour_debug(tour)
                );
            }
        }
    }
}
