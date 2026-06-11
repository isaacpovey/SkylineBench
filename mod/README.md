# SkylineBench Mod (Cities: Skylines 1 bridge)

In-game C# mod exposing a localhost HTTP API for the SkylineBench broker.

## Prerequisites
- Cities: Skylines 1 installed (Steam, macOS).
- Mono: `brew install mono` (provides `xbuild`/`msbuild` for net35).

## Build & install (macOS)

    cd mod
    ./build.sh
    # If your game is elsewhere:
    # MANAGED_DLL_PATH="/path/to/Cities.app/Contents/Resources/Data/Managed" ./build.sh

This compiles `SkylineBenchMod.dll` and copies it to
`~/Library/Application Support/Colossal Order/Cities_Skylines/Addons/Mods/SkylineBench/`.

## Enable in-game (one-time, manual)
1. Launch Cities: Skylines.
2. Content Manager > Mods > enable **SkylineBench Bridge**.
3. Load (or start) a city. The HTTP server starts on `http://127.0.0.1:8787` when the city finishes loading.

## Verify
- `curl -s http://127.0.0.1:8787/health` -> JSON with `"city_loaded":true`.
- `curl -s http://127.0.0.1:8787/probe`  -> the discovery dump (also written to the game log).

## HTTP endpoints (selected)

### `POST /screenshot`
Capture the current game framebuffer as a PNG with the UI chrome hidden (free-camera mode). Runs on Unity's main thread via a queued request.

**Request body (JSON):**

| Field | Type | Default | Description |
|---|---|---|---|
| `x` | f32 | required | World X coordinate (metres) to centre the camera on |
| `z` | f32 | required | World Z coordinate (metres) to centre the camera on |
| `size` | f32 | 1000 | Camera `m_targetSize` — vertical view extent in metres; larger = more zoomed out |
| `top_down` | bool | false | `true` = straight-down (90°) view; `false` = angled (45°) view |

**Responses:**

- `200 image/png` — raw PNG bytes at the game window's resolution.
- `500` `{"error":"capture_failed","message":"<detail>"}` — capture did not complete within ~5 seconds (5000 ms), or another failure occurred.

## Logs
The mod logs via the game's debug log. On macOS the player log is at
`~/Library/Logs/Unity/Player.log` (search for `[SkylineBench]`).

## Run the pure tests (no game needed)

    cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe
