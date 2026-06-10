// Regenerates fixtures/golden_map.png for the render::tests::render_matches_golden
// test. The network, loads, and RenderOptions below MUST stay identical to
// render::tests::{sample_network, sample_loads, opts} — if they diverge,
// regenerating from this example produces a golden the test no longer matches.
use std::collections::HashMap;

use skylinebench::contract::{Bounds, NetNode, NetSegment, Network};
use skylinebench::render::{render_network, RenderOptions};

fn main() {
    let network = Network {
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
    };
    let loads = HashMap::from([(10u32, 0.15f32), (11, 0.9)]);
    let opts = RenderOptions {
        bounds: Bounds { min_x: -100.0, min_z: -100.0, max_x: 100.0, max_z: 100.0 },
        width_px: 128,
        height_px: 128,
        grid_spacing_m: 50.0,
    };
    let png = render_network(&network, &loads, &opts);
    std::fs::create_dir_all("fixtures").unwrap();
    std::fs::write("fixtures/golden_map.png", png).unwrap();
    println!("wrote fixtures/golden_map.png");
}
