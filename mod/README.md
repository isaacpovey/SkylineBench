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

## Logs
The mod logs via the game's debug log. On macOS the player log is at
`~/Library/Logs/Unity/Player.log` (search for `[SkylineBench]`).

## Run the pure tests (no game needed)

    cd mod/test && xbuild Tests.csproj && mono bin/Debug/Tests.exe
