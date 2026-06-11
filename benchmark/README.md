# SkylineBench benchmark

Score a Claude Code agent on improving traffic in a bad-traffic city.

## Per-run steps (spec §2, §3)
1. Launch Cities: Skylines and load the benchmark save from the **main menu**
   (never reload mid-session — it crashes). Confirm the city is loaded:
   `curl -s http://127.0.0.1:8787/health` shows `"city_loaded":true`.
2. Build the broker once: `cargo build --release --manifest-path broker/Cargo.toml`.
3. Run: `./benchmark/run.sh --map gridlock-v1`
   - The broker measures a baseline and the agent works inside a Seatbelt
     sandbox that blocks reading this repo. On any run-end condition
     (submit / congestion target / 3h) the run state is snapshotted to
     `end-state.json`; after the agent session exits, run.sh runs
     `skylinebench benchmark-finalize`, which settles the sim, measures the
     final window, scores, and writes the artifacts.
   - Use `--watch` to observe an interactive session instead of headless.
   - Runs are serialized by a lock at `${TMPDIR:-/tmp}/skylinebench.lock`; never start
     two runs against one game instance. `run.sh` keeps the machine awake
     (`caffeinate`) for the whole session.
4. Read the results in `benchmark/runs/<timestamp>/`:
   - `score.json` — the composite score and per-term breakdown.
   - `run-record.json` — baseline/final stats, tally, per-action cost log.
   - `transcript.md` / `transcript.jsonl` — what the agent did *(headless runs only)*, for diagnosing a poor score (harness issue vs prompt issue).
   - `renders/` — one PNG per agent `render_map` call plus an automatic
     full-map frame after every sim step, with `index.jsonl` (tick, changes,
     flow, congested per frame). Timelapse:
     `ffmpeg -framerate 4 -pattern_type glob -i 'benchmark/runs/<ts>/renders/*.png' -pix_fmt yuv420p timelapse.mp4`

## Scoring (spec §4)
`score = 0.60·congestion_reduction + 0.20·(1−norm(money)) + 0.20·(1−norm(changes))`,
zeroed if final population drops below 80% of baseline.
`congestion_reduction = max(0, baseline_congested_meters − final_congested_meters) / baseline_congested_meters`,
where `congested_meters` is the total length of road segments with traffic density ≥ 0.7.
The run ends early when the windowed congested meters fall to 5% of the baseline
(`congestion_end_ratio`). Money is normalised against a $10,000,000 budget;
changes against a 300-change cap. Constants live in
`broker/src/benchmark/config.rs` and are tuned against the map. The agent
prompt now states these constants explicitly — keep prompt.md in sync when
retuning them.
