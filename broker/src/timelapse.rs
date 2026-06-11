//! Post-run timelapse assembly: merge frame indexes, burn a HUD strip into
//! each frame, and drive ffmpeg to produce an mp4.

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const HUD_HEIGHT_PX: u32 = 40;
/// Action close-ups are duplicated this many times so they hold on screen.
const ACTION_HOLD: u32 = 3;

static FONT_BYTES: &[u8] = include_bytes!("../assets/DejaVuSans.ttf");

#[derive(Debug, Clone)]
pub struct Frame {
    pub path: PathBuf,
    pub tick: u64,
    pub changes: u64,
    pub flow: Option<f64>,
    pub congested: Option<f64>,
    pub caption: Option<String>,
    pub hold: u32,
}

#[derive(Deserialize)]
struct IndexEntry {
    file: String,
    tick: u64,
    changes: u64,
    flow: Option<f64>,
    congested: Option<f64>,
    #[serde(default)]
    caption: Option<String>,
}

pub fn parse_index(dir: &Path, hold: u32) -> Vec<Frame> {
    let Ok(raw) = std::fs::read_to_string(dir.join("index.jsonl")) else {
        return vec![];
    };
    raw.lines()
        .filter_map(|line| match serde_json::from_str::<IndexEntry>(line) {
            Ok(e) => Some(Frame {
                path: dir.join(&e.file),
                tick: e.tick,
                changes: e.changes,
                flow: e.flow,
                congested: e.congested,
                caption: e.caption,
                hold,
            }),
            Err(err) => {
                eprintln!("timelapse: skipping malformed index line ({err})");
                None
            }
        })
        .collect()
}

/// Chronological merge; at equal ticks the overview (hold 1) precedes the
/// action close-ups taken while paused at that tick.
pub fn merge_frames(overview: Vec<Frame>, actions: Vec<Frame>) -> Vec<Frame> {
    let mut all: Vec<Frame> = overview.into_iter().chain(actions).collect();
    all.sort_by_key(|f| (f.tick, f.hold));
    all
}

fn hud_line(f: &Frame) -> String {
    let flow = f.flow.map(|v| format!("{v:.1}%")).unwrap_or_else(|| "—".into());
    let congested = f.congested.map(|v| format!("{v:.0}m")).unwrap_or_else(|| "—".into());
    let caption = f.caption.as_deref().unwrap_or("");
    format!("tick {}  flow {}  congested {}  changes {}  {}", f.tick, flow, congested, f.changes, caption)
}

fn draw_text(pixmap: &mut tiny_skia::Pixmap, x: f32, baseline_y: f32, px: f32, text: &str) {
    use ab_glyph::{Font, FontRef, ScaleFont};
    let font = FontRef::try_from_slice(FONT_BYTES).expect("embedded font parses");
    let scaled = font.as_scaled(px);
    let width = pixmap.width();
    let height = pixmap.height();
    text.chars().fold(x, |caret, ch| {
        let mut glyph = scaled.scaled_glyph(ch);
        glyph.position = ab_glyph::point(caret, baseline_y);
        let advance = scaled.h_advance(glyph.id);
        if let Some(outline) = scaled.outline_glyph(glyph) {
            let bb = outline.px_bounds();
            outline.draw(|gx, gy, c| {
                let (px_x, px_y) = (bb.min.x as i64 + gx as i64, bb.min.y as i64 + gy as i64);
                if px_x >= 0 && px_y >= 0 && (px_x as u32) < width && (px_y as u32) < height {
                    let idx = (px_y as usize * width as usize + px_x as usize) * 4;
                    let a = (c * 255.0) as u8;
                    let data = pixmap.data_mut();
                    data[idx] = data[idx].max(a);
                    data[idx + 1] = data[idx + 1].max(a);
                    data[idx + 2] = data[idx + 2].max(a);
                    data[idx + 3] = 255;
                }
            });
        }
        caret + advance
    });
}

/// Decode a frame PNG, extend the canvas with a black HUD strip at the bottom,
/// draw the metadata line, re-encode.
pub fn annotate(png: &[u8], frame: &Frame) -> Result<Vec<u8>, anyhow::Error> {
    let src = tiny_skia::Pixmap::decode_png(png)
        .map_err(|e| anyhow::anyhow!("frame decode failed: {e}"))?;
    let mut out = tiny_skia::Pixmap::new(src.width(), src.height() + HUD_HEIGHT_PX)
        .ok_or_else(|| anyhow::anyhow!("zero-sized frame"))?;
    out.fill(tiny_skia::Color::BLACK);
    out.draw_pixmap(
        0,
        0,
        src.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        tiny_skia::Transform::identity(),
        None,
    );
    let baseline = src.height() as f32 + HUD_HEIGHT_PX as f32 * 0.7;
    draw_text(&mut out, 8.0, baseline, HUD_HEIGHT_PX as f32 * 0.55, &hud_line(frame));
    out.encode_png().map_err(|e| anyhow::anyhow!("frame encode failed: {e}"))
}

/// Select the frames to assemble for `run_dir`. Prefers real screenshots when
/// they exist and are non-empty; falls back to synthetic renders otherwise.
/// Calling `parse_index` on a missing directory is safe — it returns `vec![]`.
pub fn select_frames(run_dir: &Path) -> Vec<Frame> {
    let shots = run_dir.join("screenshots");
    let screenshot_frames = merge_frames(
        parse_index(&shots.join("overview"), 1),
        parse_index(&shots.join("actions"), ACTION_HOLD),
    );
    if screenshot_frames.is_empty() {
        parse_index(&run_dir.join("renders"), 1)
    } else {
        screenshot_frames
    }
}

/// Assemble `<run_dir>` frames into an mp4. Prefers real screenshots; falls
/// back to synthetic renders for runs captured before screenshots existed.
pub fn assemble(run_dir: &Path, fps: u32, out: &Path) -> Result<(), anyhow::Error> {
    let frames = select_frames(run_dir);
    anyhow::ensure!(!frames.is_empty(), "no frames found under {}", run_dir.display());

    let staging = run_dir.join("timelapse-frames");
    std::fs::create_dir_all(&staging)?;
    let total: u32 = frames.iter().try_fold(0u32, |n, f| {
        let png = std::fs::read(&f.path)?;
        let annotated = annotate(&png, f)?;
        (0..f.hold).try_fold(n, |n, _| {
            std::fs::write(staging.join(format!("{:06}.png", n + 1)), &annotated)?;
            Ok::<u32, anyhow::Error>(n + 1)
        })
    })?;
    eprintln!("timelapse: {total} frames staged; running ffmpeg…");

    let status = std::process::Command::new("ffmpeg")
        .args(["-y", "-framerate", &fps.to_string(), "-i"])
        .arg(staging.join("%06d.png"))
        .args(["-pix_fmt", "yuv420p"])
        .arg(out)
        .status()
        .map_err(|e| anyhow::anyhow!("could not run ffmpeg ({e}) — install it with `brew install ffmpeg`"))?;
    anyhow::ensure!(status.success(), "ffmpeg exited with {status}");
    std::fs::remove_dir_all(&staging).ok();
    eprintln!("timelapse: wrote {}", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(tick: u64, hold: u32) -> Frame {
        Frame {
            path: std::path::PathBuf::from(format!("f{tick}.png")),
            tick,
            changes: 0,
            flow: Some(50.0),
            congested: Some(1000.0),
            caption: None,
            hold,
        }
    }

    #[test]
    fn merge_orders_by_tick_with_overview_before_actions() {
        let overview = vec![frame(100, 1), frame(200, 1)];
        let actions = vec![frame(100, 3)];
        let merged = merge_frames(overview, actions);
        let key: Vec<(u64, u32)> = merged.iter().map(|f| (f.tick, f.hold)).collect();
        assert_eq!(key, vec![(100, 1), (100, 3), (200, 1)]);
    }

    #[test]
    fn parse_index_skips_malformed_lines() {
        let dir = std::env::temp_dir().join(format!("sb-tl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("index.jsonl"),
            "{\"seq\":1,\"file\":\"a.png\",\"tick\":5,\"trigger\":\"step\",\"changes\":0,\"flow\":50.0,\"congested\":null}\nnot json\n",
        )
        .unwrap();
        let frames = parse_index(&dir, 1);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].tick, 5);
        assert_eq!(frames[0].path, dir.join("a.png"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn select_frames_falls_back_to_renders_when_screenshots_dir_is_empty() {
        let run_dir = std::env::temp_dir().join(format!("sb-tl-sel-{}", std::process::id()));
        // screenshots/ exists but has no index files
        std::fs::create_dir_all(run_dir.join("screenshots")).unwrap();
        // renders/ has one valid frame
        let renders = run_dir.join("renders");
        std::fs::create_dir_all(&renders).unwrap();
        std::fs::write(
            renders.join("index.jsonl"),
            "{\"seq\":1,\"file\":\"r1.png\",\"tick\":10,\"trigger\":\"step\",\"changes\":0,\"flow\":null,\"congested\":null}\n",
        )
        .unwrap();
        let frames = select_frames(&run_dir);
        assert_eq!(frames.len(), 1, "should fall back to renders when screenshots are empty");
        assert_eq!(frames[0].tick, 10);
        assert_eq!(frames[0].path, renders.join("r1.png"));
        std::fs::remove_dir_all(&run_dir).ok();
    }

    #[test]
    fn select_frames_prefers_screenshots_when_present() {
        let run_dir = std::env::temp_dir().join(format!("sb-tl-sel2-{}", std::process::id()));
        // screenshots/overview has one valid frame
        let overview = run_dir.join("screenshots/overview");
        std::fs::create_dir_all(&overview).unwrap();
        std::fs::write(
            overview.join("index.jsonl"),
            "{\"seq\":1,\"file\":\"s1.png\",\"tick\":5,\"trigger\":\"step\",\"changes\":0,\"flow\":null,\"congested\":null}\n",
        )
        .unwrap();
        // renders/ also has a frame — should be ignored
        let renders = run_dir.join("renders");
        std::fs::create_dir_all(&renders).unwrap();
        std::fs::write(
            renders.join("index.jsonl"),
            "{\"seq\":1,\"file\":\"r1.png\",\"tick\":10,\"trigger\":\"step\",\"changes\":0,\"flow\":null,\"congested\":null}\n",
        )
        .unwrap();
        let frames = select_frames(&run_dir);
        assert_eq!(frames.len(), 1, "should use screenshots when they have frames");
        assert_eq!(frames[0].tick, 5, "should be the screenshot frame, not the render");
        std::fs::remove_dir_all(&run_dir).ok();
    }

    #[test]
    fn annotate_adds_a_hud_strip() {
        let net = crate::contract::Network { nodes: vec![], segments: vec![] };
        let opts = crate::render::RenderOptions {
            bounds: crate::geometry::playable_bounds(),
            width_px: 320,
            height_px: 200,
            grid_spacing_m: 0.0,
        };
        let png = crate::render::render_network(&net, &std::collections::HashMap::new(), &opts);
        let out = annotate(&png, &frame(123, 1)).unwrap();
        let decoded = tiny_skia::Pixmap::decode_png(&out).unwrap();
        assert_eq!(decoded.width(), 320);
        assert_eq!(decoded.height(), 200 + HUD_HEIGHT_PX);
    }
}
