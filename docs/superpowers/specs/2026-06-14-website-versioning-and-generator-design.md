# Website Versioning, Run Generator, Updates & Changelog — Design

Date: 2026-06-14
Status: Approved (pending spec review)

## Problem

The SkylineBench website currently shows a single, hand-authored leaderboard for
one scenario (`gridlock-v1`) under one (implicit) benchmark version. Three gaps:

1. **No versioning.** As the scenario and the mod/harness/scoring evolve, results
   from different versions are not comparable and there is no way to view a
   leaderboard for a specific `(scenario, version)` combination. Run files do not
   record which benchmark version produced them.
2. **Manual authoring.** Each run's TS file in `content/runs/` is written entirely
   by hand — metrics copied from `run-record.json`/`score.json`, the verdict and
   narrative beats written from the transcript, and the timelapse produced and
   copied separately. This is slow and error-prone.
3. **Site structure.** The homepage carries a `Learnings` section that should
   become an ongoing **Updates** page (first entry: "Learnings from the first
   version"), and there is no **Changelog** showing what changed between versions.

## Goals

- Tag every run with the benchmark version that produced it; let the leaderboard
  be viewed per `(scenario, version)`.
- A repeatable generator script that turns a benchmark run directory into a
  reviewable website run file — metrics, AI-written verdict + beats, and the
  copied timelapse.
- Move `Learnings` off the homepage into a data-driven `/updates` page.
- Add a data-driven `/changelog` page.

## Non-goals

- No automated publishing/commit of generated runs — output is reviewed and
  committed by hand.
- No git-derived changelog — entries are hand-authored.
- No backfill of multiple historical versions — only the current `v0.1` exists.

## Existing context

- **Website:** Next.js SSG. Runs are TS modules in `website/content/runs/<slug>.ts`
  built with `defineRun(...)` (validated by the Zod `runSchema` in
  `website/lib/run.ts`). `content/runs/index.ts` statically imports each run and
  exports the sorted `runs` array + `getRun(slug)`. Homepage (`app/page.tsx`) is a
  single page of anchor sections including `<Learnings/>`. Run detail at
  `app/runs/[slug]/page.tsx`. Nav anchors live in `lib/nav-sections.ts`;
  `components/layout/nav.tsx` renders them and supports a `run` variant.
- **Benchmark:** `benchmark/run.sh` writes a run dir containing `run-record.json`
  (schema v3: `map.{id,source,game_version}`, `baseline`/`final` `WindowStats`,
  `flow_samples.{baseline,final}`, `tally.{num_changes,money_spent}`, `actions[]`),
  `score.json` (`score`, `health`, norms…), `transcript.md`, `harness.txt`,
  `model.txt`, `renders/`. The Rust `skylinebench timelapse <run-dir>` builds
  `<run-dir>/timelapse.mp4` (requires `ffmpeg`).
- A pre-Next.js Rust generator (`broker/src/page.rs`) reads a narrative TOML +
  `run-record.json` + `score.json` and emits static HTML. It is superseded by the
  Next.js site and establishes the `verdict`/`beats` content shape and the
  timelapse-copy step; it is **not** modified by this work.

## Design

### 1. Versioning data model

A **leaderboard** is the unique pair `(map, harnessVersion)`. `map` already encodes
scenario + scenario version (`"gridlock-v1"`), so it serves as the scenario
identifier; only the benchmark/mod version is added.

- `website/lib/version.ts` — single source of truth:
  ```ts
  export const CURRENT_HARNESS_VERSION = "v0.1";
  ```
- `website/lib/run.ts` — add one field to `runSchema`:
  ```ts
  harnessVersion: z.string(), // e.g. "v0.1"
  ```
- Backfill `harnessVersion: "v0.1"` into all 6 existing run files.
- `website/lib/leaderboards.ts` — pure grouping:
  ```ts
  export type Leaderboard = {
    map: string;
    harnessVersion: string;
    label: string;        // e.g. "gridlock-v1 · v0.1"
    runs: Run[];          // sorted by score desc
  };
  export const leaderboards: Leaderboard[];   // grouped from runs
  export const currentLeaderboard: Leaderboard;
  ```
  Grouping is functional (reduce over `runs`); "current" is the newest version of
  the most-populated/most-recent scenario — concretely, the leaderboard containing
  `CURRENT_HARNESS_VERSION`, falling back to the first group.

`slug` remains globally unique and the `/runs/[slug]` route is unchanged. Future
re-runs of the same model on a new version get a version-suffixed slug
(e.g. `opus-4-8-v0-2`); the generator warns on slug collision.

### 2. Results leaderboard selector

- `components/sections/results.tsx` becomes a client component that renders from
  `leaderboards` with a selector (scenario × version) defaulting to
  `currentLeaderboard`. It maps the selected leaderboard's `runs` exactly as today
  (rank, score, junctions/population/spend deltas, link to `/runs/<slug>`).
- With a single leaderboard the selector renders one inert option/label; multiple
  combos appear automatically. Selector state is local (`useState`); no routing
  change.

### 3. Generator script — `website/scripts/generate-run.ts`

Run with `tsx` (added as a dev dependency). CLI:

```
tsx scripts/generate-run.ts \
  --run-dir <path-to-benchmark-run-dir> \
  --slug <slug> \
  --model-name "<display name>" \
  [--harness-version v0.1] \
  [--model claude-opus-4-8] \
  [--skip-timelapse] \
  [--repo-root <path>]
```

Steps, each a small dependency-injected function `(deps) => (args) => ...`:

1. **Read run data.** Parse `run-record.json` + `score.json` from `--run-dir`.
   Derive the website `Run` fields:
   - `map` = `record.map.id`
   - `score` = `score.score`
   - `metrics.flow` = `{ from: round(baseline.flow_mean), to: round(final.flow_mean) }`
   - `metrics.congestedMetres` = `{ from: baseline.congested_meters, to: final.congested_meters }` (rounded)
   - `metrics.jammedJunctions` = `{ from: baseline.congested_junctions, to: final.congested_junctions }`
   - `metrics.population` = `{ from: baseline.population, to: final.population }`
   - `metrics.activeVehicles` = `{ from: round(baseline.active_vehicles_mean), to: round(final.active_vehicles_mean) }`
   - `metrics.changes` = `tally.num_changes`; `metrics.spend` = `tally.money_spent`
   - `flowSettling` = `{ base: flow_samples.baseline, final: flow_samples.final }`
   - `spendSeries` = cumulative sum over `actions[].cost` (prefixed with 0, matching existing files)
   - `actions` = group `actions[]` by `tool` in first-seen order → `{ type, count, cost }`
     (mirrors `group_actions` in `page.rs`)
   - `runDir` = the `--run-dir` value (kept for provenance, as today)
   - `harnessVersion` = `--harness-version` or `CURRENT_HARNESS_VERSION`
   - `modelName` = `--model-name`; `slug` = `--slug`
2. **AI summary.** Read `transcript.md` and pass it plus the computed metrics to
   the Anthropic API (`@anthropic-ai/sdk`, model `claude-opus-4-8`, `thinking:
   {type:"adaptive"}`, **streaming** via `messages.stream` + `finalMessage()`,
   `cache_control` on the transcript block). Use a Zod structured output
   (`output_config.format` via `zodOutputFormat`) returning:
   ```ts
   { verdict: string; beats: { title: string; body: string }[] }
   ```
   The system prompt instructs it to write in the established voice (one-paragraph
   verdict; chronological beats of titled sections describing what the agent did),
   grounded only in the transcript + metrics. Requires `ANTHROPIC_API_KEY`.
3. **Timelapse.** Unless `--skip-timelapse`, run
   `skylinebench timelapse <run-dir>` (resolve the release binary under the repo,
   build if needed) to produce `<run-dir>/timelapse.mp4`, then copy it to
   `website/public/runs/<slug>.mp4`. If `--skip-timelapse`, copy an existing
   `timelapse.mp4` if present, else warn.
4. **Emit.** Write `website/content/runs/<slug>.ts` as a `defineRun({...})` module
   (formatted to match existing files). Idempotently insert the
   `import { <camelSlug> } from "./<slug>";` line and the array entry into
   `content/runs/index.ts` (skip if already present). Warn if `<slug>.ts` or the
   public mp4 already exists (overwrite the run file; the AI summary is expected to
   be reviewed/edited before commit).

The script is a developer tool; failures (missing files, schema mismatch, missing
API key, ffmpeg absent) exit non-zero with a clear message.

### 4. Updates page — `/updates`

- `website/content/updates.ts`:
  ```ts
  export type UpdateCard = { title: string; body: string };
  export type UpdateEntry = {
    title: string;       // e.g. "Learnings from the first version"
    date: string;
    intro?: string;      // optional lead paragraph
    cards: UpdateCard[]; // numbered cards rendered by the page
  };
  export const updates: UpdateEntry[]; // newest-first
  ```
  Seeded with one entry **"Learnings from the first version"** whose `cards` carry
  the three learning items currently in `components/sections/learnings.tsx` (text
  moved verbatim). The card text is plain strings; the page renders the numbering,
  icons, and emphasis styling.
- `website/app/updates/page.tsx` — renders `updates` newest-first using the
  existing card/section styles. Includes `<Nav variant="run">`-style nav + footer.
- Remove `<Learnings/>` from `app/page.tsx` and delete/repurpose
  `components/sections/learnings.tsx` (its content now lives in `updates.ts` +
  the updates page).

### 5. Changelog page — `/changelog`

- `website/content/changelog.ts`:
  ```ts
  export type ChangelogEntry = {
    version: string;   // "v0.1"
    date: string;
    summary: string;
    changes: string[];
  };
  export const changelog: ChangelogEntry[]; // newest-first
  ```
  Seeded with the **v0.1** baseline entry.
- `website/app/changelog/page.tsx` — renders `changelog` newest-first.

### 6. Navigation

- `lib/nav-sections.ts`: replace the `#learnings` anchor with a route link to
  `/updates`, and add a `/changelog` route link. Distinguish anchor links (landing
  page) from route links so `nav.tsx` renders both correctly (anchors only resolve
  on the landing page; routes are absolute `<a href="/updates">`).
- `components/layout/nav.tsx`: render route links unconditionally; render anchor
  links as today on the landing variant.

## Testing

- **Unit (`website/__tests__`):**
  - `leaderboards` grouping: multiple `(map, harnessVersion)` runs group and sort
    correctly; `currentLeaderboard` selection.
  - Generator field mapping: given a fixture `run-record.json` + `score.json`,
    `buildRun(...)` produces a `Run` that passes `runSchema` with the expected
    metrics, `spendSeries` (cumulative), and grouped `actions`.
  - `content.test.ts` continues to validate all run files (now incl.
    `harnessVersion`).
- **Manual:** run the generator against an existing run dir (e.g. the opus-4-8
  run), confirm the emitted TS validates, the page renders, the selector switches
  leaderboards, and `/updates` + `/changelog` render.

## Risks / notes

- Transcripts can be large; the generator streams and prompt-caches the transcript
  prefix. Opus 4.8's 1M context handles full transcripts; if one ever exceeds it,
  the script errors rather than silently truncating.
- The generator edits `content/runs/index.ts` textually — kept idempotent and
  guarded; if the file's shape changes substantially the insertion logic must be
  updated.
- AI-generated verdict/beats are a starting point, reviewed before commit (matches
  the prior hand-authored workflow).
