# Real In-Game Screenshot Timelapse — Design

**Date:** 2026-06-11
**Status:** Approved

## Goal

Let an operator watch back a benchmark run as a timelapse video built from **real in-game screenshots**, showing the changes the agent made over the run. Today the only visual artifacts are synthetic network renders (`broker/src/render.rs`) — flat line drawings of the road graph. Those stay (the agent depends on them), but a parallel stream of real game captures is added, plus a one-command video assembler.

## Decisions

- **Two frame streams:** a fixed top-down overview of the city after every sim step, and a close-up at the location of every mutating agent action.
- **Camera control is acceptable:** the mod moves the real game camera for each capture. The benchmark runs unattended; the window visibly jumping around is fine.
- **Human-only telemetry for now:** the agent does not see screenshots. The capture function lives in the broker's service layer so exposing it as an MCP tool later is a thin wrapper, but no tool is added in v1.
- **Output is a video with overlays:** a `broker timelapse` subcommand produces an mp4 with a HUD strip (tick, flow %, congested meters, changes count, action captions) burned into each frame.

## Architecture

```
Claude Code agent ──MCP──► broker ──HTTP──► CS1 mod (in-game)
                              │                  │
                              │ POST /screenshot │ CaptureBehaviour (Unity main thread)
                              │ ◄── PNG bytes ───┘ camera move + ReadPixels
                              ▼
                  runs/<ts>/screenshots/{overview,actions}/ + index.jsonl
                              ▼
                  broker timelapse <run-dir>  →  timelapse.mp4 (ffmpeg)
```

## Component 1: Mod — `POST /screenshot`

**Request body:** `{ "x": float, "z": float, "size": float, "top_down": bool }`
- `x`/`z`: world-space camera target.
- `size`: zoom extent, maps to `CameraController.m_targetSize`.
- `top_down`: straight-down angle when true, default game tilt when false.

**Response:** raw `image/png` bytes; HTTP 500 with a message on any capture failure.

**`CaptureBehaviour : MonoBehaviour`** — created on level load. Camera moves and screen reads must run on Unity's main/render thread; the existing `SimThread` queue runs on the sim thread and cannot be reused. The behaviour drains a request queue in `Update()`. The HTTP handler enqueues a request and blocks with a ~5 s timeout until the capture completes.

Capture sequence per request:
1. Set `ToolsModifierControl.cameraController` **target and current** position/size/angle — setting both skips the smoothing animation (instant cut, no easing frames).
2. Enable free-camera mode (hides game UI chrome), restore the previous value after capture.
3. `yield return new WaitForEndOfFrame()` ×2 so the moved camera actually renders.
4. `Texture2D.ReadPixels` over the full screen → `EncodeToPNG()` → hand bytes to the waiting HTTP thread.

Capture resolution is the game window resolution; no offscreen supersampling in v1. Capture failures never touch the sim or other endpoints. The broker only calls this between paused steps, so camera movement never races agent actions.

## Component 2: Broker — orchestration and persistence

**Bridge client** (`broker/src/bridge_client.rs`): `screenshot(params) -> Result<Vec<u8>>`.

**Service layer** (`broker/src/service.rs`): `capture_screenshot` alongside `render_map`. This placement is what keeps the future agent-facing option cheap (add a wrapper in `tools.rs` later; none in v1).

**Benchmark server hooks** (`broker/src/benchmark/server.rs`, mirroring existing render persistence):
- After every successful `control_time("step")`: overview screenshot. Camera target/size derived from the same network bounds the full-map synthetic render uses; `top_down: true`. → `screenshots/overview/`.
- After every successful mutating action (`build_road`, `bulldoze`, `upgrade_road`, `set_zoning`): close-up at the action location; `top_down: false`, tighter `size`. Location derivation: node positions for builds, segment midpoint for bulldoze/upgrade, zone position for zoning. → `screenshots/actions/`.

**Persistence format:** filenames keep the `NNNNN-tick<T>.png` convention. Each stream has its own `index.jsonl` in the same shape as the renders index (`seq`, `file`, `tick`, `trigger`, `changes`, `flow`, `congested`), with two extra fields on action frames: `action` (e.g. `"build_road"`) and `caption` (short human-readable description).

**Failure isolation:** a screenshot failure logs a warning and the run continues — telemetry must never fail a benchmark. A `--screenshots off` flag disables capture; it is automatically off in `mock` mode (no game). Synthetic renders are untouched.

## Component 3: `broker timelapse` subcommand

`broker timelapse <run-dir> [--fps 4] [--out timelapse.mp4]`

1. Read both screenshot `index.jsonl` files; merge frames chronologically by tick (pure function).
2. Composite a HUD strip onto each frame: tick, flow %, congested meters, cumulative changes; action frames additionally show the action caption. Text rendered with an embedded font via `ab_glyph` (or equivalent) — `tiny_skia` has no text support.
3. Hold action close-ups longer by duplicating each ~3× in the output sequence.
4. Write annotated frames to a temp dir; shell out to `ffmpeg` (fail with a clear message if not installed) to produce the mp4.

**Fallback:** if no screenshots exist (old runs, `--screenshots off`), build the timelapse from `renders/` instead — immediately useful for existing runs.

## Error handling summary

| Failure | Behavior |
| --- | --- |
| Mod capture exception | HTTP 500; broker logs warning, run continues |
| Capture timeout (>5 s) | HTTP handler returns error; same broker behavior |
| Screenshot disabled / mock mode | No capture calls made; renders unaffected |
| ffmpeg missing | `broker timelapse` exits with a clear install message |
| Missing/partial index.jsonl | Skip malformed lines with a warning; assemble what's valid |

## Testing

- **Broker unit tests** for the pure parts: index merge ordering, HUD layout/compositing (golden-image or geometry assertions), camera-bounds math.
- **Mock-mode integration:** existing tests stay green with screenshots auto-disabled.
- **Mod:** manual in-game verification per the `mod/DISCOVERY.md` workflow — `curl` the endpoint, confirm framing, UI hidden, PNG valid.
- **End-to-end:** one short real run, then `broker timelapse` over its artifacts.

## Out of scope (v1)

- Agent-facing screenshot MCP tool (designed-for, not built).
- Offscreen/secondary-camera capture, supersampling, custom resolutions.
- Live dashboards or in-run viewing; this is post-run watch-back only.
