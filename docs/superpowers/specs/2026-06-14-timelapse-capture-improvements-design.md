# Timelapse capture improvements — design

Date: 2026-06-14
Status: approved (brainstorming), pending implementation plan

## Problem

The current timelapse/screenshot capture does a poor job of showing both the
changes the AI makes and how traffic evolves over a run:

1. There is no view of the city-wide traffic change between the start and end of
   a run.
2. Screenshots use the default map render — the red/green Traffic info view is
   never enabled, so congestion is invisible in the frames.
3. A batched edit (`apply_plan`) captures only one combined before/after pair,
   so individual road changes inside the plan are not shown.
4. The overview shot sits too far up and is oriented so the city does not fill
   the wide (16:9) frame.

This builds on the existing system (spec `2026-06-11-real-screenshot-timelapse`):
two capture streams — `overview` (top-down, every sim step) and `actions`
(before/after per mutating tool call) — assembled post-run into `timelapse.mp4`
by the `skylinebench timelapse` CLI, with a 40px HUD strip burned into each
frame.

## Goals

- City-wide begin/end flyby that follows the main highways and shows traffic
  flow.
- Traffic info-view (red/green) on the big-picture shots.
- Per-road before/after for every op inside a plan.
- A tighter, rotated overview that fills the wide frame.

## Non-goals

- Real-time smooth camera animation. Flybys are assembled from a sequence of
  still captures sampled along a path.
- Changing the scoring, run lifecycle, or the agent-facing tool contract.
- Back-compat with mods predating this change — broker and mod are versioned
  together.

## Shared infrastructure: richer camera control

Every feature needs more camera control than the current
`{x, z, size, top_down: bool}` screenshot payload allows, so generalize it once.

- **`CameraShot`** (`broker/src/service.rs`) and the `/screenshot` request body
  gain:
  - `yaw: f32` — camera heading in degrees.
  - `pitch: f32` — camera tilt in degrees.
  - `info_view: InfoView` — `"traffic"` or `"none"`.

  These replace the `top_down: bool`. Mapping of the old behaviour: top-down =
  `pitch 90, yaw 0`; close-up = `pitch 45, yaw 0`.

- **`Capture.cs`** (`mod/src/bridge/Capture.cs`): `CaptureRequest` gains `Yaw`,
  `Pitch`, and an `InfoView` flag. The camera angle becomes
  `new Vector2(Yaw, Pitch)`. When `InfoView == Traffic`, the coroutine records
  the current `InfoManager` mode, calls
  `InfoManager.instance.SetCurrentMode(InfoManager.InfoMode.Traffic, InfoManager.SubInfoMode.Default)`
  on the main thread, waits ~0.5s for the info-view colour fade, captures, then
  restores the prior mode in the `finally` block alongside the existing
  free-camera restore.

- **`bridge_client.rs`** `screenshot(...)` and the **mock bridge** are updated
  to carry the new fields.

**Risk to validate during implementation:** that the Traffic info view renders
into a free-camera screenshot, and that ~0.5s is enough for the colour fade.
Cannot be confirmed without the running game; verify on the first real run and
adjust the wait.

## Feature A — Traffic layer on overview + flyby

- `overview_shot` and all flyby shots set `info_view: Traffic`.
- `closeup_shot` (per-road action close-ups) keeps `info_view: None` so road
  geometry stays clean and readable — the desaturation the info view applies
  would obscure the exact change.

## Feature B — Tighter, rotated overview

Replace the fixed top-down framing in `service.rs::overview_shot` with a
frame-filling orientation choice:

- From the trimmed bounds, compute x-extent `Wx` and z-extent `Wz`. Let screen
  aspect `a ≈ 16/9`. Required camera `size` (vertical half-extent in metres) for
  an orientation is `max(vertical_extent, horizontal_extent / a) * margin / 2`.
- Evaluate for yaw 0 (north-up: vertical=z, horizontal=x) and yaw 90 (swapped);
  choose the yaw with the smaller required `size` — that orientation fills the
  wide frame best (usually the rotation when the city is wider than tall).
- Zoom in: `OVERVIEW_MIN_SIZE_M` 1200 → **600**; `OVERVIEW_MARGIN` 1.15 →
  **1.08**. Both tunable against the first real run.
- Stays top-down (`pitch 90`).

## Feature C — Per-op before/after in plans

In `apply_plan` (`broker/src/benchmark/server.rs`), replace the single combined
`region_shot` before/after with per-logical-op pairs:

- Group the expanded exec ops back by their **source op index** (the user's
  logical op). A long road that expanded into chunks is one logical op, framed
  on its full span via `region_shot` over its chunk positions.
- **Before** the execution loop, grab a before-frame for every source op (keyed
  by source index), held in memory — mirrors the existing `grab_before`.
- After the loop, for each source op with **≥1 successful** exec op, capture an
  after-frame and persist the before+after pair to the Action stream, clean
  render, captioned `apply_plan op k/N: <tool>`.
- No cap on the number of pairs.
- Single-tool calls (`build_road`, `upgrade_road`, `bulldoze`, `set_zoning`)
  are unchanged — they already pair correctly.

This drops the existing combined-region before/after for plans.

## Feature D — Begin/end highway flyby

Assembled from still captures sampled along the main highways.

**Path** — new `service.rs::highway_flyby_path(net) -> { ns: Vec<CameraShot>, we: Vec<CameraShot> }`:

- Filter segments whose `prefab` contains "highway" (case-insensitive). If none
  exist, fall back to all segments so a flyby always renders.
- Collect node positions on those segments and build two passes:
  - **N/S pass:** bucket waypoints into ~24 bins along z (south→north); within
    each bin take the **median x** → a smoothed centerline that glides along the
    corridor instead of jittering between parallel highways. Camera `yaw 0`.
  - **W/E pass:** bucket along x (west→east), median z, `yaw 90`.
- Each waypoint → `CameraShot { x, z, size ≈ 500m, pitch ≈ 32°, info_view: Traffic }`.

**Capture & triggers:**

- New `Stream::FlybyStart` / `Stream::FlybyEnd` (subdirs `flyby_start/`,
  `flyby_end/`), each holding N/S frames then W/E frames in order, each with its
  own `index.jsonl` (`trigger: "flyby_start_ns"`, `"flyby_start_we"`, etc.).
- **Begin** flyby fires in `ensure_baseline()` (run start, city untouched).
- **End** flyby fires in `finalize()` (`broker/src/benchmark/measure.rs`), which
  already runs at end of run with the live bridge client connected, before the
  game shuts down. The `ScreenshotSink` and output dir are threaded into
  `finalize`.
- Both reuse `ScreenshotSink.grab`/`persist`; flyby capture failures disable the
  sink like any other capture and never fail the run.

**Assembly (`broker/src/timelapse.rs`):**

- `select_frames` prepends `flyby_start` and appends `flyby_end` around the
  merged overview+action sequence, so `timelapse.mp4` reads intro → run → outro.
- **Also** assemble standalone `flyby_start.mp4` and `flyby_end.mp4` from those
  streams.
- Flyby frames get a minimal HUD (caption + flow%), hold = 1.

**Tunable constants** (single source of truth, adjusted after the first real
run): flyby frames per pass (24), tilt (32°), zoom (500m).

**Risks to validate:** angled-tilt shots near the map edge may catch
skybox/empty terrain; highway `prefab` naming; `finalize()` correctly threading
the sink + output dir.

## Testing

- `service.rs`: unit tests for `highway_flyby_path` over a synthetic network
  (N/S vs W/E ordering, median smoothing, highway filter + fallback);
  `overview_shot` orientation/size selection; per-source-op grouping in
  `apply_plan`.
- `screenshots.rs`: new `FlybyStart`/`FlybyEnd` streams persist frames + index.
- Mock bridge: `/screenshot` accepts and echoes `yaw`/`pitch`/`info_view`.
- `timelapse.rs`: `select_frames` includes flyby streams as intro/outro;
  standalone flyby files are produced.
- Existing capture tests updated for the `CameraShot` field change.

## Open knobs (defaults chosen, tune on first real run)

- Overview zoom: `OVERVIEW_MIN_SIZE_M = 600`, `OVERVIEW_MARGIN = 1.08`.
- Flyby: 24 frames/pass, `pitch 32°`, `size 500m`.
- Traffic info-view fade wait: ~0.5s.
