//! Pure batch-plan logic for the `apply_plan` tool: op expansion (polylines →
//! straight chunks under the segment-length cap) and pre-validation against a
//! city snapshot. No game I/O here — the server wires it to the bridge.

use std::collections::{HashMap, HashSet};

use serde::Deserialize;

use crate::contract::{ActionError, Bounds, Position, RoadType};
use crate::geometry::horizontal_distance;
use crate::validate::validate_build_road;

/// Spans are split into chunks at most this long — comfortably under the game's
/// 200 m segment cap so endpoint snapping can't push a chunk over it.
pub const POLYLINE_CHUNK_M: f32 = 180.0;
/// Limits keeping one tool call's work (and wall-clock) bounded.
pub const MAX_OPS: usize = 50;
pub const MAX_EXPANDED_OPS: usize = 120;

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PlanOp {
    /// Straight link; spans longer than the segment cap are auto-split.
    BuildRoad {
        from: Position,
        to: Position,
        road_type: String,
        #[serde(default = "default_true")]
        snap: bool,
        /// Metres above terrain at `from` (0 = ground).
        #[serde(default)]
        from_elevation: f32,
        /// Metres above terrain at `to` (0 = ground); differ from `from_elevation` for a ramp.
        #[serde(default)]
        to_elevation: f32,
    },
    /// Poly-link through `points` in order; each leg is auto-split.
    BuildPolyline {
        points: Vec<Position>,
        road_type: String,
        #[serde(default = "default_true")]
        snap: bool,
        /// Metres above terrain per point (parallel to `points`). Omitted or
        /// short → missing entries default to 0 (ground).
        #[serde(default)]
        elevations: Vec<f32>,
    },
    UpgradeRoad {
        segment: u32,
        road_type: String,
    },
    Bulldoze {
        target_type: String,
        id: u32,
    },
    SetZoning {
        area: Bounds,
        zone_type: String,
    },
}

fn default_true() -> bool {
    true
}

/// A primitive, directly executable op (post-expansion).
#[derive(Debug, Clone, PartialEq)]
pub enum ExecOp {
    Build {
        from: Position,
        to: Position,
        road_type: String,
        snap: bool,
        from_elevation: f32,
        to_elevation: f32,
    },
    Upgrade {
        segment: u32,
        road_type: String,
    },
    Bulldoze {
        target_type: String,
        id: u32,
    },
    Zone {
        area: Bounds,
        zone_type: String,
    },
    /// Placeholder for a source op that cannot expand (e.g. a 1-point
    /// polyline); always fails validation with INVALID_ARGS.
    Invalid,
}

/// City snapshot the plan is validated against.
pub struct ExecCtx {
    pub road_types: Vec<RoadType>,
    pub zone_types: Vec<String>,
    pub segment_ids: HashSet<u32>,
    pub node_ids: HashSet<u32>,
    pub building_ids: HashSet<u32>,
    pub segment_lengths: HashMap<u32, f32>,
}

fn lerp_pos(a: Position, b: Position, t: f32) -> Position {
    Position {
        x: a.x + (b.x - a.x) * t,
        y: a.y + (b.y - a.y) * t,
        z: a.z + (b.z - a.z) * t,
    }
}

/// Fraction (0..1) of the way from `from` to `to` for each chunk boundary.
fn chunk_fractions(from: Position, to: Position) -> Vec<f32> {
    let len = horizontal_distance(from, to);
    let n = (len / POLYLINE_CHUNK_M).ceil().max(1.0) as usize;
    (0..=n).map(|i| i as f32 / n as f32).collect()
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Split `from..to` into elevation-aware Build ops; endpoint elevations
/// interpolate linearly between `from_elev` and `to_elev`.
fn build_chunks(
    from: Position,
    to: Position,
    road_type: &str,
    snap: bool,
    from_elev: f32,
    to_elev: f32,
) -> Vec<ExecOp> {
    let fr = chunk_fractions(from, to);
    fr.windows(2)
        .map(|w| ExecOp::Build {
            from: lerp_pos(from, to, w[0]),
            to: lerp_pos(from, to, w[1]),
            road_type: road_type.to_string(),
            snap,
            from_elevation: lerp(from_elev, to_elev, w[0]),
            to_elevation: lerp(from_elev, to_elev, w[1]),
        })
        .collect()
}

/// Expand source ops into primitive ops, each tagged with its source index.
pub fn expand(ops: &[PlanOp]) -> Vec<(usize, ExecOp)> {
    ops.iter()
        .enumerate()
        .flat_map(|(i, op)| -> Vec<(usize, ExecOp)> {
            match op {
                PlanOp::BuildRoad {
                    from,
                    to,
                    road_type,
                    snap,
                    from_elevation,
                    to_elevation,
                } => build_chunks(*from, *to, road_type, *snap, *from_elevation, *to_elevation)
                    .into_iter()
                    .map(|op| (i, op))
                    .collect(),
                PlanOp::BuildPolyline {
                    points,
                    road_type,
                    snap,
                    elevations,
                } => {
                    if points.len() < 2 {
                        return vec![(i, ExecOp::Invalid)];
                    }
                    points
                        .windows(2)
                        .enumerate()
                        .flat_map(|(leg, w)| {
                            let e0 = elevations.get(leg).copied().unwrap_or(0.0);
                            let e1 = elevations.get(leg + 1).copied().unwrap_or(0.0);
                            build_chunks(w[0], w[1], road_type, *snap, e0, e1)
                        })
                        .map(|op| (i, op))
                        .collect()
                }
                PlanOp::UpgradeRoad { segment, road_type } => {
                    vec![(
                        i,
                        ExecOp::Upgrade {
                            segment: *segment,
                            road_type: road_type.clone(),
                        },
                    )]
                }
                PlanOp::Bulldoze { target_type, id } => {
                    vec![(
                        i,
                        ExecOp::Bulldoze {
                            target_type: target_type.clone(),
                            id: *id,
                        },
                    )]
                }
                PlanOp::SetZoning { area, zone_type } => {
                    vec![(
                        i,
                        ExecOp::Zone {
                            area: *area,
                            zone_type: zone_type.clone(),
                        },
                    )]
                }
            }
        })
        .collect()
}

/// Structural pre-validation against the snapshot. The game can still reject
/// an op at execution time (OBJECT_COLLISION) — only it knows.
pub fn validate(op: &ExecOp, ctx: &ExecCtx) -> Result<(), ActionError> {
    match op {
        ExecOp::Build {
            from,
            to,
            road_type,
            ..
        } => validate_build_road(*from, *to, road_type, &ctx.road_types),
        ExecOp::Upgrade { segment, road_type } => {
            if !ctx.road_types.iter().any(|t| t.name == *road_type) {
                return Err(ActionError::InvalidPrefab);
            }
            ctx.segment_ids
                .contains(segment)
                .then_some(())
                .ok_or(ActionError::InvalidArgs)
        }
        ExecOp::Bulldoze { target_type, id } => {
            let known = match target_type.as_str() {
                "segment" => ctx.segment_ids.contains(id),
                "node" => ctx.node_ids.contains(id),
                "building" => ctx.building_ids.contains(id),
                _ => false,
            };
            known.then_some(()).ok_or(ActionError::InvalidArgs)
        }
        ExecOp::Zone { zone_type, .. } => ctx
            .zone_types
            .contains(zone_type)
            .then_some(())
            .ok_or(ActionError::InvalidArgs),
        ExecOp::Invalid => Err(ActionError::InvalidArgs),
    }
}

/// Tool name an exec op is recorded under (matches single-op accounting).
pub fn tool_name(op: &ExecOp) -> &'static str {
    match op {
        ExecOp::Build { .. } => "build_road",
        ExecOp::Upgrade { .. } => "upgrade_road",
        ExecOp::Bulldoze { .. } => "bulldoze",
        ExecOp::Zone { .. } => "set_zoning",
        ExecOp::Invalid => "invalid",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{ActionError, Position, RoadType};
    use std::collections::{HashMap, HashSet};

    fn pos(x: f32, z: f32) -> Position {
        Position { x, y: 0.0, z }
    }

    fn ctx() -> ExecCtx {
        ExecCtx {
            road_types: vec![RoadType {
                name: "road".into(),
                construction_cost: 1000,
                ..Default::default()
            }],
            zone_types: vec!["residential".into()],
            segment_ids: HashSet::from([10]),
            node_ids: HashSet::from([1, 2]),
            building_ids: HashSet::new(),
            segment_lengths: HashMap::from([(10, 64.0)]),
        }
    }

    #[test]
    fn build_road_carries_elevation_into_exec() {
        let ops = vec![PlanOp::BuildRoad {
            from: pos(0.0, 0.0),
            to: pos(50.0, 0.0),
            road_type: "road".into(),
            snap: true,
            from_elevation: 0.0,
            to_elevation: 12.0,
        }];
        let exec = expand(&ops);
        assert_eq!(exec.len(), 1);
        match &exec[0].1 {
            ExecOp::Build {
                from_elevation,
                to_elevation,
                ..
            } => {
                assert_eq!(*from_elevation, 0.0);
                assert_eq!(*to_elevation, 12.0);
            }
            other => panic!("expected Build, got {other:?}"),
        }
    }

    #[test]
    fn polyline_interpolates_elevation_per_chunk() {
        // 360 m line split at 180 m => 2 chunks; elevations 0 -> 12 over the line.
        let ops = vec![PlanOp::BuildPolyline {
            points: vec![pos(0.0, 0.0), pos(360.0, 0.0)],
            road_type: "road".into(),
            snap: true,
            elevations: vec![0.0, 12.0],
        }];
        let exec = expand(&ops);
        assert_eq!(exec.len(), 2);
        let elevs: Vec<(f32, f32)> = exec
            .iter()
            .map(|(_, op)| match op {
                ExecOp::Build {
                    from_elevation,
                    to_elevation,
                    ..
                } => (*from_elevation, *to_elevation),
                _ => panic!(),
            })
            .collect();
        assert_eq!(elevs, vec![(0.0, 6.0), (6.0, 12.0)]);
    }

    fn build_endpoints(ops: &[ExecOp]) -> Vec<(Position, Position)> {
        ops.iter()
            .map(|op| match op {
                ExecOp::Build { from, to, .. } => (*from, *to),
                other => panic!("expected Build, got {other:?}"),
            })
            .collect()
    }

    #[test]
    fn build_chunks_respects_chunk_length() {
        let chunks = build_chunks(pos(0.0, 0.0), pos(500.0, 0.0), "road", true, 0.0, 0.0);
        let ends = build_endpoints(&chunks);
        assert_eq!(ends.len(), 3);
        assert!((ends[0].1.x - 166.66667).abs() < 0.01);
        assert_eq!(ends[0].0.x, 0.0);
        assert_eq!(ends[2].1.x, 500.0);
        let max = ends
            .iter()
            .map(|(a, b)| crate::geometry::horizontal_distance(*a, *b))
            .fold(0.0_f32, f32::max);
        assert!(max <= POLYLINE_CHUNK_M + 0.01, "max chunk {max}");
    }

    #[test]
    fn short_span_is_one_chunk() {
        assert_eq!(
            build_chunks(pos(0.0, 0.0), pos(50.0, 0.0), "road", true, 0.0, 0.0).len(),
            1
        );
    }

    #[test]
    fn expand_polyline_chains_points_and_splits() {
        let ops = vec![PlanOp::BuildPolyline {
            points: vec![pos(0.0, 0.0), pos(250.0, 0.0), pos(250.0, 100.0)],
            road_type: "road".into(),
            snap: true,
            elevations: vec![],
        }];
        let exec = expand(&ops);
        // 250m span → 2 chunks, 100m span → 1 chunk.
        assert_eq!(exec.len(), 3);
        assert!(exec.iter().all(|(source, _)| *source == 0));
        match &exec[2].1 {
            ExecOp::Build { from, to, .. } => {
                assert_eq!(from.x, 250.0);
                assert_eq!(to.z, 100.0);
            }
            other => panic!("expected Build, got {other:?}"),
        }
    }

    #[test]
    fn expand_splits_long_build_road_too() {
        let ops = vec![PlanOp::BuildRoad {
            from: pos(0.0, 0.0),
            to: pos(400.0, 0.0),
            road_type: "road".into(),
            snap: true,
            from_elevation: 0.0,
            to_elevation: 0.0,
        }];
        assert_eq!(expand(&ops).len(), 3);
    }

    #[test]
    fn validate_catches_each_failure_mode() {
        let c = ctx();
        let bad_prefab = ExecOp::Build {
            from: pos(0.0, 0.0),
            to: pos(50.0, 0.0),
            road_type: "monorail".into(),
            snap: true,
            from_elevation: 0.0,
            to_elevation: 0.0,
        };
        assert_eq!(validate(&bad_prefab, &c), Err(ActionError::InvalidPrefab));

        let missing_segment = ExecOp::Upgrade {
            segment: 99,
            road_type: "road".into(),
        };
        assert_eq!(
            validate(&missing_segment, &c),
            Err(ActionError::InvalidArgs)
        );

        let missing_bulldoze = ExecOp::Bulldoze {
            target_type: "segment".into(),
            id: 99,
        };
        assert_eq!(
            validate(&missing_bulldoze, &c),
            Err(ActionError::InvalidArgs)
        );

        let bad_zone = ExecOp::Zone {
            area: crate::contract::Bounds {
                min_x: 0.0,
                min_z: 0.0,
                max_x: 8.0,
                max_z: 8.0,
            },
            zone_type: "spaceport".into(),
        };
        assert_eq!(validate(&bad_zone, &c), Err(ActionError::InvalidArgs));

        let good = ExecOp::Upgrade {
            segment: 10,
            road_type: "road".into(),
        };
        assert_eq!(validate(&good, &c), Ok(()));
    }

    #[test]
    fn degenerate_polyline_is_invalid() {
        let ops = vec![PlanOp::BuildPolyline {
            points: vec![pos(0.0, 0.0)],
            road_type: "road".into(),
            snap: true,
            elevations: vec![],
        }];
        let exec = expand(&ops);
        assert_eq!(exec.len(), 1);
        assert_eq!(validate(&exec[0].1, &ctx()), Err(ActionError::InvalidArgs));
    }
}
