use crate::contract::{Bounds, NetNode, Position};

/// Half the side length of the playable area, in metres (≈ 9×9 grid of 1920 m
/// tiles). Working assumption from spec §5; confirm against the live API in the
/// mod plan.
pub const PLAYABLE_HALF_EXTENT_M: f32 = 8640.0;

/// Snap radius for reusing an existing node instead of creating a new one
/// (one CS1 zone cell). Spec §5.
pub const SNAP_TOLERANCE_M: f32 = 8.0;

/// Maximum length of a single straight road segment, in metres. Working value;
/// the real game limit is pinned during mod integration.
pub const MAX_SEGMENT_LENGTH_M: f32 = 200.0;

pub fn playable_bounds() -> Bounds {
    Bounds {
        min_x: -PLAYABLE_HALF_EXTENT_M,
        min_z: -PLAYABLE_HALF_EXTENT_M,
        max_x: PLAYABLE_HALF_EXTENT_M,
        max_z: PLAYABLE_HALF_EXTENT_M,
    }
}

/// Horizontal (X/Z) distance between two positions; Y (elevation) is ignored
/// because roads connect in the horizontal plane.
pub fn horizontal_distance(a: Position, b: Position) -> f32 {
    let dx = a.x - b.x;
    let dz = a.z - b.z;
    (dx * dx + dz * dz).sqrt()
}

pub fn in_bounds(p: Position, bounds: Bounds) -> bool {
    p.x >= bounds.min_x && p.x <= bounds.max_x && p.z >= bounds.min_z && p.z <= bounds.max_z
}

/// Axis-aligned intersection of two rectangles. Degenerate (inverted) input
/// yields an inverted result; callers should prefer well-formed bounds.
pub fn intersect_bounds(a: Bounds, b: Bounds) -> Bounds {
    Bounds {
        min_x: a.min_x.max(b.min_x),
        max_x: a.max_x.min(b.max_x),
        min_z: a.min_z.max(b.min_z),
        max_z: a.max_z.min(b.max_z),
    }
}

/// Shrink `bounds` by `inset` metres on each side. The inset is capped at half
/// the span so a small city collapses to its centre instead of inverting.
pub fn inset_bounds(bounds: Bounds, inset: f32) -> Bounds {
    let dx = (bounds.max_x - bounds.min_x).max(0.0);
    let dz = (bounds.max_z - bounds.min_z).max(0.0);
    let ix = inset.max(0.0).min(dx * 0.5);
    let iz = inset.max(0.0).min(dz * 0.5);
    Bounds {
        min_x: bounds.min_x + ix,
        max_x: bounds.max_x - ix,
        min_z: bounds.min_z + iz,
        max_z: bounds.max_z - iz,
    }
}

pub fn clamp_to_bounds(x: f32, z: f32, bounds: Bounds) -> (f32, f32) {
    (
        x.clamp(bounds.min_x, bounds.max_x),
        z.clamp(bounds.min_z, bounds.max_z),
    )
}

/// Returns the id of the nearest node within `SNAP_TOLERANCE_M` of `p`, if any.
pub fn nearest_node_within_tolerance(p: Position, nodes: &[NetNode]) -> Option<u32> {
    nodes
        .iter()
        .map(|n| {
            (
                n.id,
                horizontal_distance(
                    p,
                    Position {
                        x: n.x,
                        y: n.y,
                        z: n.z,
                    },
                ),
            )
        })
        .filter(|(_, d)| *d <= SNAP_TOLERANCE_M)
        .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .map(|(id, _)| id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(x: f32, z: f32) -> Position {
        Position { x, y: 0.0, z }
    }

    #[test]
    fn distance_is_horizontal() {
        assert_eq!(horizontal_distance(pos(0.0, 0.0), pos(3.0, 4.0)), 5.0);
    }

    #[test]
    fn distance_ignores_elevation() {
        let a = Position {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        };
        let b = Position {
            x: 3.0,
            y: 100.0,
            z: 4.0,
        };
        assert_eq!(horizontal_distance(a, b), 5.0);
    }

    #[test]
    fn in_bounds_accepts_inside_and_rejects_outside() {
        let b = playable_bounds();
        assert!(in_bounds(pos(0.0, 0.0), b));
        assert!(!in_bounds(pos(PLAYABLE_HALF_EXTENT_M + 1.0, 0.0), b));
    }

    #[test]
    fn inset_bounds_shrinks_each_side_and_will_not_invert() {
        let b = Bounds {
            min_x: -1000.0,
            max_x: 1000.0,
            min_z: -400.0,
            max_z: 400.0,
        };
        let inset = inset_bounds(b, 200.0);
        assert_eq!(
            inset,
            Bounds {
                min_x: -800.0,
                max_x: 800.0,
                min_z: -200.0,
                max_z: 200.0,
            }
        );
        let collapsed = inset_bounds(b, 10_000.0);
        assert_eq!(collapsed.min_x, collapsed.max_x);
        assert_eq!(collapsed.min_z, collapsed.max_z);
        assert_eq!(collapsed.min_x, 0.0);
        assert_eq!(collapsed.min_z, 0.0);
    }

    #[test]
    fn clamp_to_bounds_pins_each_axis() {
        let b = Bounds {
            min_x: -10.0,
            max_x: 10.0,
            min_z: -5.0,
            max_z: 5.0,
        };
        assert_eq!(clamp_to_bounds(0.0, 0.0, b), (0.0, 0.0));
        assert_eq!(clamp_to_bounds(50.0, -50.0, b), (10.0, -5.0));
    }

    #[test]
    fn snap_finds_nearest_within_tolerance() {
        let nodes = vec![
            NetNode {
                id: 1,
                x: 5.0,
                y: 0.0,
                z: 0.0,
            },
            NetNode {
                id: 2,
                x: 2.0,
                y: 0.0,
                z: 0.0,
            },
        ];
        assert_eq!(
            nearest_node_within_tolerance(pos(0.0, 0.0), &nodes),
            Some(2)
        );
    }

    #[test]
    fn snap_returns_none_when_all_too_far() {
        let nodes = vec![NetNode {
            id: 1,
            x: 50.0,
            y: 0.0,
            z: 0.0,
        }];
        assert_eq!(nearest_node_within_tolerance(pos(0.0, 0.0), &nodes), None);
    }
}
