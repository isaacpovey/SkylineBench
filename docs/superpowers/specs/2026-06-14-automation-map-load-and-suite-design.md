# Benchmark automation: reliable map load + sequential suite runs

Date: 2026-06-14
Status: Approved (design)

## Problem

Two gaps block hands-off benchmarking:

1. **Map load is unreliable and opaque.** A `--map <id>` is only a metadata
   label — nothing binds it to a real in-game save. `SaveLoader.Load` matches a
   save name loosely (`asset.name` → `cityName` → `fullName` fallback) and
   returns `{ok:true, city_loaded:true}` *at LoadLevel kick-off*, before the
   async load finishes. When the game then rejects what was resolved, the
   operator sees an in-game "invalid file" modal while the tool call reported
   success. Working hypothesis (operator): the save name/identifier we send
   resolves to the wrong or an invalid asset. We cannot confirm this today
   because the loader never reports *what it resolved*.

2. **No way to run a list of models/harnesses in order.** `run.sh` drives a
   single (harness, model) run. Benchmarking several requires manual repetition,
   with a manual map reset between each.

These are linked: sequential runs need each run to start from an identical city,
so a dependable load is the prerequisite for the suite.

## Scope / non-goals

- **In scope:** correctness + observability of the existing load path; binding
  `--map id` to a real save name; a sequential suite runner reading a manifest.
- **Out of scope:** menu-transition reload machinery, per-run game relaunch, or
  any re-architecture of CS1's load path. The operator's assessment is that the
  identifier we send is wrong, so Part 1 stays a focused correctness fix.

---

## Part 1 — Reliable, observable map load

### 1.1 Make the loader report what it resolved

`LoadResultDto` and the `/load-save` response gain the resolved asset identity so
callers (and operators) can verify the right save was chosen:

- On match: `ok:true`, plus `name`, `city_name`, `full_name` of the resolved
  asset.
- On no match: `ok:false` plus `available` — the list of save names the game
  actually exposes (`asset.name` for each `SaveGameMetaData` asset). This is what
  turns "invalid file, no idea why" into "we sent X, the game has [Y, Z]".

Touch points:
- `mod/src/dto/Dtos.cs` — extend `LoadResultDto`.
- `mod/src/json/Serialize.cs` — serialize the new fields.
- `mod/src/bridge/SaveLoader.cs` — populate resolved identity; return available
  names on miss.
- `broker/src/bridge_client.rs` / `broker/src/service.rs` — surface the richer
  result through `reset_scenario`.

### 1.2 Add a read-only saves enumeration

A `/saves` GET endpoint (mod) + a broker accessor that lists available save
identities (`name`, `city_name`, `full_name`). Purpose: pick the *exact* string
to pin in the map manifest, and support operator debugging. Read-only; no sim
thread mutation.

Touch points: `mod/src/http/Router.cs`, `mod/src/http/Handlers.cs`,
`mod/src/bridge/SaveLoader.cs` (reuse the existing `FilterAssets` enumeration),
`broker/src/bridge_client.rs`.

### 1.3 Confirm completion by polling, not kick-off

Because `LoadLevel` is async and `OnLevelUnloading` briefly stops the HTTP
bridge, the **caller** confirms a load by polling `/health` until
`city_loaded:true`, with a timeout. Timeout ⇒ a surfaced failure (the
invalid-file case), never a false success.

This polling lives in the orchestration layer (run.sh / run-suite.sh), reusing
the existing `/health` preflight pattern already in `run.sh`. The mod's
kick-off-returns-immediately contract is unchanged; we stop *trusting* it as a
completion signal.

### 1.4 Bind `--map id` → real save name

Extend the `benchmark/maps` manifest so each map id records the exact in-game
save name (the `name`/`full_name` from `/saves`) the loader must use, alongside
the existing source/version metadata. A small mapping file
(`benchmark/maps/maps.tsv` or similar: `id<TAB>save_name<TAB>source<TAB>version`)
is the source of truth.

`run.sh` resolves `--map <id>` to its save name and loads it at run start
(replacing the manual "load from the main menu" step and the check-only
preflight with an actual load + 1.3 poll). If the id is unknown, fail fast with
the list of known ids.

---

## Part 2 — Sequential suite runs

### 2.1 Manifest format

A committable suite file at `benchmark/suites/<name>.txt`. One entry per line,
`harness[:model]`; `#` comments and blank lines ignored:

```
# default suite
claude:claude-opus-4-8
claude:claude-sonnet-4-6
codex
gemini:gemini-2.5-flash
opencode
```

`harness` with no `:model` uses the harness default (current `run.sh` behavior
when `--model` is omitted).

### 2.2 Orchestrator: `benchmark/run-suite.sh`

A thin wrapper over `run.sh`, which stays the unchanged single-run primitive.
Usage: `run-suite.sh --map <id> --suite <file> [--fail-fast]`.

Per manifest entry, strictly in order:

1. **Reset the map** — load-by-id (Part 1.4) + poll `/health` until
   `city_loaded:true` (Part 1.3), so every run starts from an identical city.
2. Invoke `run.sh --map <id> --harness <h> [--model <m>] --out <child-dir>`.
3. Capture the run's exit status.

**Failure handling:** record-and-continue by default — log the failure to
`summary.tsv` and proceed to the next entry. `--fail-fast` opts into
stop-on-first-failure.

### 2.3 Concurrency / locking

Strictly sequential. Relies on `run.sh`'s existing `LOCK_DIR` mutex (one game
instance serves one run at a time). The suite holds no extra lock; each child
`run.sh` takes and releases its own. (Reset in 2.1 step 1 happens between runs,
while no `run.sh` holds the lock.)

### 2.4 Output layout

```
benchmark/runs/suite-<timestamp>/
  suite.txt                     # copy of the manifest used (reproducibility)
  <runid>-<harness>[-<model>]/  # individual run dir (existing run.sh layout)
  summary.tsv                   # harness <TAB> model <TAB> runid <TAB> status <TAB> exit_code
```

`run.sh` already accepts `--out`, so the suite passes a child path per entry.
`summary.tsv` gives at-a-glance pass/fail across the suite.

### 2.5 Pre-suite validation (fail-fast on setup)

Before the loop, validate every distinct harness's binary on PATH and required
secrets present — reuse `harness-prepare`'s required-env output in a dry-run
pass per harness. This prevents an hour-long suite dying on entry 4 for a
missing API key. Map id is validated once (2.4 binding).

---

## Testing

- **Mod (`mod/test`):** serialization of the extended `LoadResultDto`
  (resolved identity present on match; `available` present on miss). `FindSave`
  resolution precedence unchanged (existing behavior) — add a case asserting a
  miss returns the available list.
- **Broker (`cargo test`):** `reset_scenario` surfaces resolved identity and the
  available-names miss path (extend the existing `reset_scenario_*` tests against
  the mock; add `/saves` + miss responses to `broker/src/mock.rs`).
- **Manifest parsing:** unit-test the `harness[:model]` line parser (comments,
  blanks, `harness` vs `harness:model`). Implemented in shell, tested via a
  small `DRY_RUN=1` invocation that prints the resolved plan per line.
- **Suite orchestration:** `DRY_RUN=1 run-suite.sh` prints the per-entry resolved
  `run.sh` command and the summary layout without launching harnesses or the
  game (mirrors `run.sh`'s existing `DRY_RUN` contract).

## Risks

- **Mid-session load may still be unstable** (DISCOVERY.md D1: prior crash during
  mid-session `LoadLevel`). Part 1 makes failure *observable and surfaced* rather
  than silently successful; if the name-correctness fix doesn't fully stabilize
  it, the polling timeout (1.3) turns it into a clean per-run failure recorded in
  `summary.tsv` rather than a corrupt run. The menu-transition/relaunch fallback
  remains a documented future option, not built here.
- **Bridge restart window** during reload — handled by 1.3 polling tolerating
  transient unreachability until the timeout.
