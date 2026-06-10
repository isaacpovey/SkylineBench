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
     (submit / flow target / 3h) the run state is snapshotted to
     `end-state.json`; after the agent session exits, run.sh runs
     `skylinebench benchmark-finalize`, which settles the sim, measures the
     final window, scores, and writes the artifacts.
   - Use `--watch` to observe an interactive session instead of headless.
   - Runs are serialized by a lock at `$TMPDIR/skylinebench.lock`; never start
     two runs against one game instance. `run.sh` keeps the machine awake
     (`caffeinate`) for the whole session.
4. Read the results in `benchmark/runs/<timestamp>/`:
   - `score.json` — the composite score and per-term breakdown.
   - `run-record.json` — baseline/final stats, tally, per-action cost log.
   - `transcript.md` / `transcript.jsonl` — what the agent did *(headless runs only)*, for diagnosing a poor score (harness issue vs prompt issue).

## Scoring (spec §4)
`score = 0.60·norm(Δflow) + 0.20·(1−norm(money)) + 0.20·(1−norm(changes))`,
zeroed if final active vehicles drop below 90% of baseline. Constants live in
`broker/src/benchmark/config.rs` and are tuned against the map.
