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
    BuildRoad { from: Position, to: Position, road_type: String, #[serde(default = "default_true")] snap: bool },
    /// Poly-link through `points` in order; each leg is auto-split.
    BuildPolyline { points: Vec<Position>, road_type: String, #[serde(default = "default_true")] snap: bool },
    UpgradeRoad { segment: u32, road_type: String },
    Bulldoze { target_type: String, id: u32 },
    SetZoning { area: Bounds, zone_type: String },
}

fn default_true() -> bool {
    true
}

/// A primitive, directly executable op (post-expansion).
#[derive(Debug, Clone, PartialEq)]
pub enum ExecOp {
    Build { from: Position, to: Position, road_type: String, snap: bool },
    Upgrade { segment: u32, road_type: String },
    Bulldoze { target_type: String, id: u32 },
    Zone { area: Bounds, zone_type: String },
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

/// Split one straight span into equal chunks no longer than POLYLINE_CHUNK_M.
pub fn split_span(from: Position, to: Position) -> Vec<(Position, Position)> {
    let len = horizontal_distance(from, to);
    let n = (len / POLYLINE_CHUNK_M).ceil().max(1.0) as usize;
    (0..n)
        .map(|i| {
            (
                lerp_pos(from, to, i as f32 / n as f32),
                lerp_pos(from, to, (i + 1) as f32 / n as f32),
            )
        })
        .collect()
}

/// Expand source ops into primitive ops, each tagged with its source index.
pub fn expand(ops: &[PlanOp]) -> Vec<(usize, ExecOp)> {
    ops.iter()
        .enumerate()
        .flat_map(|(i, op)| -> Vec<(usize, ExecOp)> {
            match op {
                PlanOp::BuildRoad { from, to, road_type, snap } => split_span(*from, *to)
                    .into_iter()
                    .map(|(a, b)| (i, ExecOp::Build { from: a, to: b, road_type: road_type.clone(), snap: *snap }))
                    .collect(),
                PlanOp::BuildPolyline { points, road_type, snap } => {
                    if points.len() < 2 {
                        return vec![(i, ExecOp::Invalid)];
                    }
                    points
                        .windows(2)
                        .flat_map(|w| split_span(w[0], w[1]))
                        .map(|(a, b)| (i, ExecOp::Build { from: a, to: b, road_type: road_type.clone(), snap: *snap }))
                        .collect()
                }
                PlanOp::UpgradeRoad { segment, road_type } => {
                    vec![(i, ExecOp::Upgrade { segment: *segment, road_type: road_type.clone() })]
                }
                PlanOp::Bulldoze { target_type, id } => {
                    vec![(i, ExecOp::Bulldoze { target_type: target_type.clone(), id: *id })]
                }
                PlanOp::SetZoning { area, zone_type } => {
                    vec![(i, ExecOp::Zone { area: *area, zone_type: zone_type.clone() })]
                }
            }
        })
        .collect()
}

/// Structural pre-validation against the snapshot. The game can still reject
/// an op at execution time (COLLISION, INSUFFICIENT_FUNDS) — only it knows.
pub fn validate(op: &ExecOp, ctx: &ExecCtx) -> Result<(), ActionError> {
    match op {
        ExecOp::Build { from, to, road_type, .. } => {
            validate_build_road(*from, *to, road_type, &ctx.road_types)
        }
        ExecOp::Upgrade { segment, road_type } => {
            if !ctx.road_types.iter().any(|t| t.name == *road_type) {
                return Err(ActionError::InvalidPrefab);
            }
            ctx.segment_ids.contains(segment).then_some(()).ok_or(ActionError::InvalidArgs)
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
            road_types: vec![RoadType { name: "road".into(), construction_cost: 1000 }],
            zone_types: vec!["residential".into()],
            segment_ids: HashSet::from([10]),
            node_ids: HashSet::from([1, 2]),
            building_ids: HashSet::new(),
            segment_lengths: HashMap::from([(10, 64.0)]),
        }
    }

    #[test]
    fn split_span_respects_chunk_length() {
        let chunks = split_span(pos(0.0, 0.0), pos(500.0, 0.0));
        assert_eq!(chunks.len(), 3);
        assert!((chunks[0].1.x - 166.66667).abs() < 0.01);
        assert_eq!(chunks[0].0.x, 0.0);
        assert_eq!(chunks[2].1.x, 500.0);
        let max = chunks
            .iter()
            .map(|(a, b)| crate::geometry::horizontal_distance(*a, *b))
            .fold(0.0_f32, f32::max);
        assert!(max <= POLYLINE_CHUNK_M + 0.01, "max chunk {max}");
    }

    #[test]
    fn short_span_is_one_chunk() {
        assert_eq!(split_span(pos(0.0, 0.0), pos(50.0, 0.0)).len(), 1);
    }

    #[test]
    fn expand_polyline_chains_points_and_splits() {
        let ops = vec![PlanOp::BuildPolyline {
            points: vec![pos(0.0, 0.0), pos(250.0, 0.0), pos(250.0, 100.0)],
            road_type: "road".into(),
            snap: true,
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
        }];
        assert_eq!(expand(&ops).len(), 3);
    }

    #[test]
    fn validate_catches_each_failure_mode() {
        let c = ctx();
        let bad_prefab = ExecOp::Build { from: pos(0.0, 0.0), to: pos(50.0, 0.0), road_type: "monorail".into(), snap: true };
        assert_eq!(validate(&bad_prefab, &c), Err(ActionError::InvalidPrefab));

        let missing_segment = ExecOp::Upgrade { segment: 99, road_type: "road".into() };
        assert_eq!(validate(&missing_segment, &c), Err(ActionError::InvalidArgs));

        let missing_bulldoze = ExecOp::Bulldoze { target_type: "segment".into(), id: 99 };
        assert_eq!(validate(&missing_bulldoze, &c), Err(ActionError::InvalidArgs));

        let bad_zone = ExecOp::Zone {
            area: crate::contract::Bounds { min_x: 0.0, min_z: 0.0, max_x: 8.0, max_z: 8.0 },
            zone_type: "spaceport".into(),
        };
        assert_eq!(validate(&bad_zone, &c), Err(ActionError::InvalidArgs));

        let good = ExecOp::Upgrade { segment: 10, road_type: "road".into() };
        assert_eq!(validate(&good, &c), Ok(()));
    }

    #[test]
    fn degenerate_polyline_is_invalid() {
        let ops = vec![PlanOp::BuildPolyline { points: vec![pos(0.0, 0.0)], road_type: "road".into(), snap: true }];
        let exec = expand(&ops);
        assert_eq!(exec.len(), 1);
        assert_eq!(validate(&exec[0].1, &ctx()), Err(ActionError::InvalidArgs));
    }
}
