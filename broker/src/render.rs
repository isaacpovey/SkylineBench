use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Stroke, Transform};

use crate::contract::{Bounds, Network, Position};

pub struct RenderOptions {
    pub bounds: Bounds,
    pub width_px: u32,
    pub height_px: u32,
}

/// Map a world position to pixel coordinates within `bounds`. World +Z is drawn
/// upward, so the Z axis is flipped (screen Y grows downward).
fn to_pixel(p: Position, bounds: Bounds, w: u32, h: u32) -> (f32, f32) {
    let span_x = (bounds.max_x - bounds.min_x).max(f32::EPSILON);
    let span_z = (bounds.max_z - bounds.min_z).max(f32::EPSILON);
    let px = (p.x - bounds.min_x) / span_x * w as f32;
    let py = (1.0 - (p.z - bounds.min_z) / span_z) * h as f32;
    (px, py)
}

/// Render the network to PNG bytes.
pub fn render_network(network: &Network, opts: &RenderOptions) -> Vec<u8> {
    // Clamp to at least 1px: dimensions arrive from MCP tool args, and
    // Pixmap::new returns None (→ panic) on a zero dimension.
    let w = opts.width_px.max(1);
    let h = opts.height_px.max(1);
    let mut pixmap = Pixmap::new(w, h).expect("dimensions are clamped to at least 1");
    pixmap.fill(Color::from_rgba8(20, 20, 28, 255));

    let node_pos = network
        .nodes
        .iter()
        .map(|n| (n.id, Position { x: n.x, y: n.y, z: n.z }))
        .collect::<std::collections::HashMap<_, _>>();

    let mut road_paint = Paint::default();
    road_paint.set_color(Color::from_rgba8(230, 230, 120, 255));
    road_paint.anti_alias = true;
    let stroke = Stroke { width: 2.0, ..Stroke::default() };

    for seg in &network.segments {
        if let (Some(a), Some(b)) = (node_pos.get(&seg.start_node), node_pos.get(&seg.end_node)) {
            let (ax, ay) = to_pixel(*a, opts.bounds, w, h);
            let (bx, by) = to_pixel(*b, opts.bounds, w, h);
            let mut pb = PathBuilder::new();
            pb.move_to(ax, ay);
            pb.line_to(bx, by);
            if let Some(path) = pb.finish() {
                pixmap.stroke_path(&path, &road_paint, &stroke, Transform::identity(), None);
            }
        }
    }

    pixmap.encode_png().expect("PNG encoding never fails for a valid pixmap")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{NetNode, NetSegment};

    fn sample_network() -> Network {
        Network {
            nodes: vec![
                NetNode { id: 1, x: -50.0, y: 0.0, z: -50.0 },
                NetNode { id: 2, x: 50.0, y: 0.0, z: -50.0 },
                NetNode { id: 3, x: 50.0, y: 0.0, z: 50.0 },
            ],
            segments: vec![
                NetSegment { id: 10, start_node: 1, end_node: 2, prefab: "road".into(), lanes: 2, length: 100.0 },
                NetSegment { id: 11, start_node: 2, end_node: 3, prefab: "road".into(), lanes: 2, length: 100.0 },
            ],
        }
    }

    fn opts() -> RenderOptions {
        RenderOptions {
            bounds: Bounds { min_x: -100.0, min_z: -100.0, max_x: 100.0, max_z: 100.0 },
            width_px: 128,
            height_px: 128,
        }
    }

    #[test]
    fn render_is_deterministic() {
        let a = render_network(&sample_network(), &opts());
        let b = render_network(&sample_network(), &opts());
        assert_eq!(a, b, "rendering must be a pure function of its inputs");
    }

    #[test]
    fn render_matches_golden() {
        let produced = render_network(&sample_network(), &opts());
        let golden = include_bytes!("../fixtures/golden_map.png");
        assert_eq!(
            produced, golden,
            "render output changed; if intentional, regenerate fixtures/golden_map.png (see Task 5 Step 4)"
        );
    }
}
