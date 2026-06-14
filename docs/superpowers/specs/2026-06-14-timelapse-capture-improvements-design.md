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

- City-wide begin/end flyby that follows the main highways with the sim running,
  so moving cars are visible and the start-vs-end traffic change reads.
- Traffic info-view (red/green) on the overview stills.
- Per-road before/after for every op inside a plan.
- A tighter, rotated overview that fills the wide frame.

## Non-goals

- Engine-level video recording (Unity Recorder, RenderTexture pipelines,
  AsyncGPUReadback). The flyby is a frame-sequence recorded by an in-mod
  coroutine grabbing full-res PNGs at ~12fps while the sim runs, then assembled
  by ffmpeg — not a real-time encoder.
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

A second new endpoint, **`/flyby`**, drives the recorded flyby (Feature D); it
is detailed there.

**Risk to validate during implementation:** that the Traffic info view renders
into a free-camera screenshot, and that ~0.5s is enough for the colour fade.
Cannot be confirmed without the running game; verify on the first real run and
adjust the wait.

## Feature A — Traffic layer on overview stills

- `overview_shot` sets `info_view: Traffic`, so the per-step and begin/end
  overview stills carry the red/green congestion coloring. These stills carry
  the congestion story.
- `closeup_shot` (per-road action close-ups) keeps `info_view: None` so road
  geometry stays clean and readable — the desaturation the info view applies
  would obscure the exact change.
- The recorded flyby (Feature D) uses **normal render**, not the traffic view,
  so moving cars stay vivid. The congestion change is shown by the
  traffic-layer overview stills that bracket the run.

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

## Feature D — Begin/end highway flyby (recorded video)

A flyby is a **recorded frame sequence**, not sampled stills: the sim runs while
the camera glides along keyframes, so cars move. Normal render, full-res
(~720p) PNG at ~12fps capture, assembled at 24fps playback (≈2× speed —
shorter clip, smooth motion).

**Keyframe path** — new
`service.rs::highway_flyby_path(net) -> { ns: Vec<CameraKeyframe>, we: Vec<CameraKeyframe> }`:

- Filter segments whose `prefab` contains "highway" (case-insensitive). If none
  exist, fall back to all segments so a flyby always renders.
- Collect node positions on those segments and build two passes:
  - **N/S pass:** bucket waypoints into bins along z (south→north); within each
    bin take the **median x** → a smoothed centerline that glides along the
    corridor instead of jittering between parallel highways. Camera `yaw 0`.
  - **W/E pass:** bucket along x (west→east), median z, `yaw 90`.
- Reduce each pass to ~6–10 **keyframes** (control points), not per-frame
  samples — the in-mod coroutine interpolates between them. Each keyframe is a
  camera pose `{ x, z, yaw, pitch ≈ 32°, size ≈ 500m }`. Normal render.

**`/flyby` endpoint + in-mod recording coroutine** (`mod/src/bridge/Capture.cs`,
`mod/src/http/Handlers.cs`):

- Request body: `{ keyframes: [...], duration_s, capture_fps, out_dir, label }`.
- The coroutine:
  1. Records current `simulationSpeed` and `ForcedSimulationPaused`.
  2. Unpauses the sim at 1× so cars move at a natural pace.
  3. Enables free camera (hides UI chrome).
  4. Interpolates the camera along the keyframes (Catmull-Rom for position,
     lerp for yaw/pitch/size) over `duration_s`, grabbing a frame every
     `1/capture_fps` s (~83ms) via `ReadPixels`+`EncodeToPNG`, writing
     `NNNN.png` to `{out_dir}`.
  5. In a `finally`: restores free-camera state, `simulationSpeed`, and
     `ForcedSimulationPaused` — even on error/timeout — so the run continues
     unaffected.
- The HTTP call blocks for the full pass; the broker uses a long timeout
  (`duration_s` + margin), distinct from the 5s screenshot timeout.
- The mod writes frame PNGs directly to the shared session dir (new mod
  capability; safe because broker and game are same-machine/localhost).

**Triggers (`broker/src/benchmark/server.rs`, `measure.rs`):**

- **Begin** flyby fires in `ensure_baseline()` (run start, city untouched). Adds
  ~12–16s to the first tool call — acceptable for a benchmark.
- **End** flyby fires in `finalize()`, which already runs at end of run with the
  live bridge client connected, before the game shuts down. The output dir is
  threaded into `finalize`.
- The broker issues one `/flyby` per pass (N/S then W/E) into subdirs
  `flyby_start/` and `flyby_end/`. Flyby failures are logged and never fail the
  run, mirroring the screenshot sink's best-effort contract.

**Assembly (`broker/src/timelapse.rs`):**

- Assemble standalone `flyby_start.mp4` and `flyby_end.mp4` from their PNG
  sequences at **24fps**.
- The overview+action core timelapse is assembled as today at its own fps.
- Final `timelapse.mp4` is an ffmpeg **concat** of
  `[flyby_start, core, flyby_end]`, normalized to common codec/params/fps, so it
  reads intro → run → outro.
- Flyby frames are raw recordings (no per-frame HUD); an optional title card or
  the standalone-file name carries the "start"/"end" label.

**Tunable constants** (single source of truth, adjusted after the first real
run): keyframes per pass (6–10), pass duration, capture fps (12), playback fps
(24), tilt (32°), zoom (500m), flyby sim speed (1×).

**Risks to validate:** sustained ~12fps full-res PNG capture without unacceptable
hitching; angled-tilt shots near the map edge catching skybox/empty terrain;
highway `prefab` naming; reliable restore of sim pause/speed after the pass;
ffmpeg concat normalization across differing source fps.

## Testing

- `service.rs`: unit tests for `highway_flyby_path` over a synthetic network
  (N/S vs W/E keyframe ordering, median smoothing, keyframe reduction, highway
  filter + fallback); `overview_shot` orientation/size selection; per-source-op
  grouping in `apply_plan`.
- Mock bridge: `/screenshot` accepts and echoes `yaw`/`pitch`/`info_view`;
  `/flyby` accepts a keyframe request and writes a stub frame sequence to
  `out_dir` so broker-side assembly is testable headless.
- `timelapse.rs`: standalone `flyby_start.mp4`/`flyby_end.mp4` are produced from
  a frame sequence; the concat into `timelapse.mp4` orders intro → core →
  outro.
- Existing capture tests updated for the `CameraShot` field change.

## Open knobs (defaults chosen, tune on first real run)

- Overview zoom: `OVERVIEW_MIN_SIZE_M = 600`, `OVERVIEW_MARGIN = 1.08`.
- Flyby: 6–10 keyframes/pass, capture 12fps, playback 24fps, `pitch 32°`,
  `size 500m`, sim speed 1×, per-pass duration.
- Traffic info-view fade wait: ~0.5s.
