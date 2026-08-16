# SkylineBench benchmark

Score a Claude Code agent on improving traffic in a bad-traffic city.

## Per-run steps (spec §2, §3)
1. Launch Cities: Skylines and load the benchmark save from the **main menu**.
   Confirm the city is loaded: `curl -s http://127.0.0.1:8787/health` shows
   `"city_loaded":true`. `run.sh` will skip `/load-save` when that health
   payload already names the bound save (`save_name` / `city_name`).
2. Build the broker once: `cargo build --release --manifest-path broker/Cargo.toml`.
3. Run: `./benchmark/run.sh --map gridlock-v1`
   - If `/load-save` still hits CS1's "file format version not supported"
     error, load the save from the main menu and re-run with `--skip-load`.
     A failed load no longer waits 180s; it prints that hint and exits.
   - Put harness secrets in a root `.env` file if you do not want to export
     them in your shell each time: `cp .env.example .env`, then fill in the
     keys you need. `.env` is ignored by git.
   - The broker measures a baseline and the agent works inside a Seatbelt
     sandbox that blocks reading this repo. On any run-end condition
     (submit / congestion target / 3h) the run state is snapshotted to
     `end-state.json`; after the agent session exits, run.sh runs
     `skylinebench benchmark-finalize`, which settles the sim, measures the
     final window, scores, and writes the artifacts.
   - Use `--harness <claude|codex|gemini|opencode>` to pick the agent harness
     (default `claude`). codex needs `CODEX_API_KEY`, gemini `GEMINI_API_KEY`,
     opencode `OPENROUTER_API_KEY`; each harness binary must be on `PATH`.
   - Use `--model <name>` to pick the model (e.g. `claude-opus-4-8`,
     `gpt-5.5`, `gemini-2.5-pro`, `openrouter/qwen/qwen-2.5-coder-32b-instruct`).
     The harness + model are recorded in the run dir as `harness.txt` / `model.txt`.
   - The deny-repo-read sandbox (macOS Seatbelt, Linux bubblewrap/firejail)
     wraps the agent; the active backend is recorded in `sandbox.txt`. On a host
     with no sandbox available the run proceeds with a loud warning and
     `sandbox.txt = none`.
   - Runs are serialized by a lock at `${TMPDIR:-/tmp}/skylinebench.lock`; never start
     two runs against one game instance.
4. Read the results in `benchmark/runs/<timestamp>/`:
   - `score.json` — the composite score and per-term breakdown.
   - `run-record.json` — baseline/final stats, tally, per-action cost log.
   - `transcript.md` / `transcript.jsonl` — what the agent did *(headless runs only)*, for diagnosing a poor score (harness issue vs prompt issue).
   - `renders/` — one PNG per agent `render_map` call plus an automatic
     full-map frame after every sim step, with `index.jsonl` (tick, changes,
     congested metres, congested junctions, population per frame).
   - `screenshots/overview/` — a top-down overview frame captured from the live
     game after every sim step.
   - `screenshots/actions/` — an angled close-up captured after every successful
     mutating action (build_road / upgrade_road / bulldoze / set_zoning /
     apply_plan). Each screenshots directory has an `index.jsonl` sidecar with
     per-frame metadata: seq, file, tick, trigger/action, changes,
     congested metres, congested junctions, population, caption.
   - Screenshot capture is best-effort telemetry. If the mod lacks the
     `/screenshot` endpoint (older mod) or a capture fails, the broker logs once
     and disables screenshots for the rest of the run — a benchmark never fails
     and no per-step latency is added retrying. Runs without screenshots simply
     have no `screenshots/` dir.
   - Timelapse: `skylinebench timelapse <run-dir>` (e.g.
     `broker/target/release/skylinebench timelapse benchmark/runs/<ts>`).
     Optional flags: `--fps <n>` (default 4), `--out <path>` (default
     `<run-dir>/timelapse.mp4`). Requires `ffmpeg` on PATH (`brew install
     ffmpeg`). The command composites a HUD strip (tick, population, congested
     metres, congested junctions, changes count, and any action caption) onto each frame and
     assembles an annotated mp4. It prefers real in-game screenshots under
     `screenshots/` and falls back to `renders/` for older runs.

## Scoring (operator-facing — the agent is NOT told this)

The agent prompt frames the task as "optimise this city's traffic simulation" and
states its objectives qualitatively; it is deliberately **not** told the formula,
the weights, the caps, or the population thresholds, so it optimises the city
rather than the scoreboard.

`score = (0.60·congestion_reward + 0.20·(1−norm(money)) + 0.20·(1−norm(changes))) · health`

- `congestion_reward = blend_meters·meters_reduction + blend_junctions·junction_reduction`
  (default 0.5/0.5; falls back to meters-only when the baseline has no congested junctions).
- `meters_reduction = max(0, baseline_congested_meters − final_congested_meters) / baseline_congested_meters`,
  where `congested_meters` is the total length of road segments with traffic density ≥ 0.7.
- `junction_reduction = max(0, baseline_congested_junctions − final_congested_junctions) / baseline_congested_junctions`.
  A **congested junction** is a node of degree ≥ `junction_min_degree` (3) with ≥ `junction_min_congested` (2)
  incident segments at density ≥ `congestion_threshold` (0.7), measured over the final window.
- `health` is a graded population factor (1.0 at population ≥ `health_full`·baseline (0.85),
  0.0 at ≤ `health_zero`·baseline (0.75), linear between, capped at 1.0 even if population
  grows). The 85% full-health line leaves room for normal death-wave / settle noise;
  only a drop into the 75–85% band, or a collapse below 75%, drags the score.
- A run is invalid (score 0) only when the baseline has no congestion to fix.
- Money is normalised against a $10,000,000 budget; changes against a 300-change cap.

Constants live in `broker/src/benchmark/config.rs`. The run ends on `submit_solution`
or the wall-clock cap; the old auto-stop-at-5%-of-baseline condition was removed.

## Running a suite

To benchmark several harness/model combinations back-to-back, use
`run-suite.sh` instead of invoking `run.sh` once per combination.

1. Launch Cities: Skylines with the SkylineBench mod enabled and the benchmark
   save available in your save list (the suite loads the map itself — see below —
   so you do not need to load it from the menu first). Build the broker once:
   `cargo build --release --manifest-path broker/Cargo.toml`.
2. Map binding: each `--map <id>` resolves to a real in-game save name via
   `benchmark/maps/maps.tsv` (columns: `id`, `save_name`, `source`,
   `game_version`). Fill in `save_name` with the exact identity the game reports —
   list them with `curl http://127.0.0.1:8787/saves`.
3. Suite manifest: one run per line, `harness[:model]`; `#` comments and blank
   lines are ignored; `harness` with no `:model` uses the harness default. See
   `benchmark/suites/default.txt`.
4. Run: `./benchmark/run-suite.sh --map gridlock-v1 --suite benchmark/suites/default.txt`

The suite validates every entry up front (a `DRY_RUN` `run.sh` per entry, which
fails fast on an unknown map id or an unsupported harness), then runs each entry
in order. Before each run, `run.sh` loads the map and waits for the level reload
to complete — so every run starts from the identical city. A failed load (or any
run failure) is recorded and the suite **continues** to the next entry; pass
`--fail-fast` to stop on the first failure instead.

Output lands in `benchmark/runs/suite-<timestamp>/`:
- `suite.txt` — a copy of the manifest used.
- `<runid>-<harness>[-<model>]/` — one per entry, the normal `run.sh` layout
  (`score.json`, `run-record.json`, transcript, renders, screenshots).
- `summary.tsv` — `harness`, `model`, `runid`, `status` (`ok`/`failed`),
  `exit_code`, one row per entry.

Runs are still serialized by the `${TMPDIR:-/tmp}/skylinebench.lock` lock, so a
suite cannot collide with a stray single run against the same game instance.
