# Benchmark maps

Pin the bad-traffic save here (the `.crp` file) and document it below.

Requirements (spec §9):
- The save must have **Unlimited Money** and **Unlock All** enabled, so cash never
  blocks building and all tiles/features are available. Money is only a *scoring*
  penalty (the broker computes spend from each action; it never reads in-game funds).
- Record the source and the game version it was made on.

## Pinned saves

The machine-readable id → save-name binding lives in `maps.tsv` (one row per map:
`id`, `save_name`, `source`, `game_version`). `run.sh --map <id>` resolves the id
to its `save_name` and loads it. List the game's actual save identities with
`GET /saves` (or `curl http://127.0.0.1:8787/saves`) to fill in `save_name`.
