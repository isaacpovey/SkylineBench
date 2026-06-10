use std::collections::HashMap;

use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Stroke, Transform};

use crate::contract::{Bounds, Network, Position};

pub struct RenderOptions {
    pub bounds: Bounds,
    pub width_px: u32,
    pub height_px: u32,
    /// World metres between gridlines; 0.0 disables the grid.
    pub grid_spacing_m: f32,
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

/// Congestion colour ramp: green (free) → yellow (busy, 0.5) → red (saturated).
pub fn density_color(d: f32) -> Color {
    let t = d.clamp(0.0, 1.0);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let (r, g, b) = if t < 0.5 {
        let t2 = t / 0.5;
        (lerp(80.0, 230.0, t2), lerp(200.0, 230.0, t2), 80.0)
    } else {
        let t2 = (t - 0.5) / 0.5;
        (230.0, lerp(230.0, 60.0, t2), lerp(80.0, 50.0, t2))
    };
    Color::from_rgba8(r as u8, g as u8, b as u8, 255)
}

fn stroke_line(pixmap: &mut Pixmap, a: (f32, f32), b: (f32, f32), color: Color, width: f32) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    let stroke = Stroke { width, ..Stroke::default() };
    let mut pb = PathBuilder::new();
    pb.move_to(a.0, a.1);
    pb.line_to(b.0, b.1);
    if let Some(path) = pb.finish() {
        pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

fn draw_grid(pixmap: &mut Pixmap, opts: &RenderOptions, w: u32, h: u32) {
    if opts.grid_spacing_m <= 0.0 {
        return;
    }
    let b = opts.bounds;
    let spacing = opts.grid_spacing_m;
    let line = |v: f32| (v / spacing).ceil() * spacing;
    let grid = Color::from_rgba8(45, 45, 58, 255);
    let axis = Color::from_rgba8(75, 75, 95, 255);
    let mut x = line(b.min_x);
    while x <= b.max_x {
        let (px, _) = to_pixel(Position { x, y: 0.0, z: b.min_z }, b, w, h);
        let color = if x == 0.0 { axis } else { grid };
        stroke_line(pixmap, (px, 0.0), (px, h as f32), color, 1.0);
        x += spacing;
    }
    let mut z = line(b.min_z);
    while z <= b.max_z {
        let (_, py) = to_pixel(Position { x: b.min_x, y: 0.0, z }, b, w, h);
        let color = if z == 0.0 { axis } else { grid };
        stroke_line(pixmap, (0.0, py), (w as f32, py), color, 1.0);
        z += spacing;
    }
}

/// Chevron at the segment midpoint pointing from `a` to `b` (pixel space).
fn draw_arrow(pixmap: &mut Pixmap, a: (f32, f32), b: (f32, f32)) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 6.0 {
        return;
    }
    let (ux, uy) = (dx / len, dy / len);
    let (mx, my) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let tip = (mx + ux * 4.0, my + uy * 4.0);
    let left = (mx - uy * 3.0 - ux * 1.0, my + ux * 3.0 - uy * 1.0);
    let right = (mx + uy * 3.0 - ux * 1.0, my - ux * 3.0 - uy * 1.0);
    let white = Color::from_rgba8(245, 245, 245, 255);
    stroke_line(pixmap, left, tip, white, 1.2);
    stroke_line(pixmap, right, tip, white, 1.2);
}

/// Render the road network to PNG bytes. `loads` maps segment id → density
/// (0..1); segments missing from it draw neutral gray.
pub fn render_network(network: &Network, loads: &HashMap<u32, f32>, opts: &RenderOptions) -> Vec<u8> {
    // Clamp to at least 1px: dimensions arrive from MCP tool args, and
    // Pixmap::new returns None (→ panic) on a zero dimension.
    let w = opts.width_px.max(1);
    let h = opts.height_px.max(1);
    let mut pixmap = Pixmap::new(w, h).expect("dimensions are clamped to at least 1");
    pixmap.fill(Color::from_rgba8(20, 20, 28, 255));

    draw_grid(&mut pixmap, opts, w, h);

    let node_pos: HashMap<u32, Position> = network
        .nodes
        .iter()
        .map(|n| (n.id, Position { x: n.x, y: n.y, z: n.z }))
        .collect();

    for seg in &network.segments {
        if let (Some(a), Some(b)) = (node_pos.get(&seg.start_node), node_pos.get(&seg.end_node)) {
            let pa = to_pixel(*a, opts.bounds, w, h);
            let pb = to_pixel(*b, opts.bounds, w, h);
            let color = loads
                .get(&seg.id)
                .map(|d| density_color(*d))
                .unwrap_or(Color::from_rgba8(120, 120, 130, 255));
            let width = (1.0 + seg.lanes as f32 * 0.5).min(5.0);
            stroke_line(&mut pixmap, pa, pb, color, width);
            if seg.one_way {
                let (from, to) = if seg.travel_direction == "end_to_start" { (pb, pa) } else { (pa, pb) };
                draw_arrow(&mut pixmap, from, to);
            }
        }
    }

    pixmap
        .encode_png()
        .expect("PNG encoding never fails for a valid pixmap")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{NetNode, NetSegment};
    use std::collections::HashMap;

    fn sample_network() -> Network {
        Network {
            nodes: vec![
                NetNode { id: 1, x: -50.0, y: 0.0, z: -50.0 },
                NetNode { id: 2, x: 50.0, y: 0.0, z: -50.0 },
                NetNode { id: 3, x: 50.0, y: 0.0, z: 50.0 },
            ],
            segments: vec![
                NetSegment {
                    id: 10,
                    start_node: 1,
                    end_node: 2,
                    prefab: "road".into(),
                    lanes: 2,
                    length: 100.0,
                    one_way: false,
                    travel_direction: "both".into(),
                    speed_limit: 1.0,
                },
                NetSegment {
                    id: 11,
                    start_node: 2,
                    end_node: 3,
                    prefab: "oneway".into(),
                    lanes: 4,
                    length: 100.0,
                    one_way: true,
                    travel_direction: "start_to_end".into(),
                    speed_limit: 2.0,
                },
            ],
        }
    }

    fn sample_loads() -> HashMap<u32, f32> {
        HashMap::from([(10, 0.15), (11, 0.9)])
    }

    fn opts() -> RenderOptions {
        RenderOptions {
            bounds: Bounds { min_x: -100.0, min_z: -100.0, max_x: 100.0, max_z: 100.0 },
            width_px: 128,
            height_px: 128,
            grid_spacing_m: 50.0,
        }
    }

    #[test]
    fn render_is_deterministic() {
        let a = render_network(&sample_network(), &sample_loads(), &opts());
        let b = render_network(&sample_network(), &sample_loads(), &opts());
        assert_eq!(a, b, "rendering must be a pure function of its inputs");
    }

    #[test]
    fn render_matches_golden() {
        let produced = render_network(&sample_network(), &sample_loads(), &opts());
        let golden = include_bytes!("../fixtures/golden_map.png");
        assert_eq!(
            produced, golden,
            "render output changed; if intentional, regenerate via `cargo run --example gen_golden`"
        );
    }

    #[test]
    fn density_changes_the_image() {
        let hot: HashMap<u32, f32> = HashMap::from([(10, 1.0), (11, 1.0)]);
        assert_ne!(
            render_network(&sample_network(), &sample_loads(), &opts()),
            render_network(&sample_network(), &hot, &opts()),
            "congestion colouring must show up in the pixels"
        );
    }

    #[test]
    fn one_way_arrow_changes_the_image() {
        let mut both = sample_network();
        both.segments[1].one_way = false;
        both.segments[1].travel_direction = "both".into();
        assert_ne!(
            render_network(&sample_network(), &sample_loads(), &opts()),
            render_network(&both, &sample_loads(), &opts()),
            "one-way chevrons must show up in the pixels"
        );
    }

    #[test]
    fn grid_can_be_disabled() {
        let no_grid = RenderOptions { grid_spacing_m: 0.0, ..opts() };
        assert_ne!(
            render_network(&sample_network(), &sample_loads(), &opts()),
            render_network(&sample_network(), &sample_loads(), &no_grid),
        );
    }

    #[test]
    fn density_color_ramps_green_yellow_red() {
        assert_eq!(density_color(0.0), Color::from_rgba8(80, 200, 80, 255));
        assert_eq!(density_color(0.5), Color::from_rgba8(230, 230, 80, 255));
        assert_eq!(density_color(1.0), Color::from_rgba8(230, 60, 50, 255));
    }
}
