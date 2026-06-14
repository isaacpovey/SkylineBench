# SkylineBench Website — Next.js Refactor

**Date:** 2026-06-14
**Status:** Design — pending review

## Summary

Rebuild the SkylineBench marketing/benchmark website as a statically generated
Next.js (App Router) application, replacing the current hand-written HTML/CSS in
`website/`. The goal is a modular, componentised codebase with no change in
hosting model: still static, still on Vercel. Visual target is **port + light
polish** — reproduce the current design, allowing small consistency
improvements that fall out of componentisation, not a redesign.

The current site is already designed against shadcn/ui's design tokens
(`website/colors_and_type.css` is shadcn's token file copied verbatim), so the
move to real shadcn/ui + Tailwind is a natural fit rather than a rewrite of the
visual language.

## Goals

- Componentised, modular React codebase replacing two monolithic HTML documents
  (`index.html` ~713 lines, four near-duplicate `runs/*.html`).
- Run detail pages driven entirely by structured, typed content — narrative,
  metrics, and chart inputs in one place — rather than hand-baked HTML.
- Charts become reusable components that render from semantic data, not
  hand-computed SVG geometry.
- Eliminate duplicated inline SVG (the GitHub logo appears ~5 times, etc.).
- Static generation; deployable on Vercel with no server runtime needed.

## Non-Goals

- Visual redesign. Parity with light polish only.
- A data pipeline that auto-generates run content from `benchmark/runs/`. Run
  content is authored/derived once into committed data files. An automated
  generator is possible future work, out of scope here.
- Changing the benchmark, broker, or mod.

## Decisions (from brainstorming)

| Decision | Choice |
|---|---|
| Framework | Next.js, App Router, TypeScript |
| Static output | Vercel native SSG (not `output: export`) |
| Styling | shadcn/ui + Tailwind v4, themed with existing oklch tokens |
| shadcn depth | Maximise shadcn primitive usage; bespoke sections stay custom only where no primitive fits |
| Visual goal | Port + light polish |
| Run content | TypeScript data modules validated by zod |
| App location | Replace `website/` in place |

## Architecture

### Routes

- `app/layout.tsx` — root layout. Sets `<html class="ds dark">`, loads fonts
  (Geist, Geist Mono, Inter Tight via `next/font`), renders Vercel
  `<Analytics />`, applies `globals.css`.
- `app/page.tsx` — landing page, composed from section components in order:
  Hero, Thesis, HowItWorks, Scoring, Architecture, Learnings, Roadmap, Results,
  Findings, CtaBand, Footer.
- `app/runs/[slug]/page.tsx` — per-run detail. `generateStaticParams()`
  enumerates run slugs from `content/runs`. Each run is statically generated to
  its own HTML file at build time. `generateMetadata()` sets per-run title.

### Content model

`lib/run.ts` defines a zod schema and inferred `Run` type. `content/runs/<slug>.ts`
exports one validated `Run` per model. `content/runs/index.ts` collects them,
sorted by composite score for the results grid.

The `Run` shape (informed by current `runs-src/*.toml` plus the numbers
currently baked into `runs/*.html`):

```ts
type Run = {
  slug: string
  modelName: string
  map: string                 // e.g. "gridlock-v1"
  runDir: string              // provenance pointer into benchmark/runs
  score: number               // composite, 0..1
  verdict: string
  // headline chips
  metrics: {
    flow: { from: number; to: number }
    congestedMetres: { from: number; to: number }
    jammedJunctions: { from: number; to: number }
    population: { from: number; to: number }
    activeVehicles: { from: number; to: number }
    changes: number
    spend: number             // dollars
  }
  // chart series
  flowSettling: { base: number[]; final: number[] }   // % over steps
  spendSeries: number[]                                // cumulative $ over actions
  actions: { type: string; count: number; cost: number }[]
  // narrative
  beats: { title: string; body: string }[]
}
```

The landing page's results grid and hero "best run" stats derive from this same
content (single source of truth — no second copy of the headline numbers).

### Sourcing the chart data

The semantic numbers for the four existing runs are recovered as follows, in
priority order:

1. From the local `benchmark/runs/<runDir>/` artifacts — `score.json`,
   `run-record.json`, `end-state.json` — which exist on the author's machine
   (gitignored, not in CI). This is the source of truth for series data
   (flow-over-time, cumulative spend, action breakdown).
2. Where a value only survives as pixel geometry in the current SVGs and cannot
   be re-derived, transcribe the headline number from the existing HTML.

Once authored into `content/runs/*.ts`, the data is committed and the build no
longer depends on `benchmark/runs/`.

### Components

```
components/
  ui/                  shadcn primitives (themed)
    button.tsx         variants: primary, outline, sm, icon
    card.tsx
    badge.tsx
    separator.tsx      replaces .divider
  layout/
    nav.tsx            "use client" — scroll-border state
    footer.tsx
  sections/
    hero.tsx
    thesis.tsx
    how-it-works.tsx
    scoring.tsx
    architecture.tsx
    learnings.tsx
    roadmap.tsx
    results.tsx        renders result cards from content
    findings.tsx
    cta-band.tsx
  charts/
    before-after-chart.tsx
    settling-chart.tsx
    spend-chart.tsx
    actions-chart.tsx
  video-player.tsx     "use client" — IntersectionObserver autoplay
  icons/
    brand-mark.tsx     the skyline logo
    github.tsx linkedin.tsx mail.tsx   social marks
lib/
  run.ts               zod schema + Run type
  chart.ts             pure chart-geometry helpers
content/
  runs/
    fable-5.ts sonnet-4-5.ts opus-4-8.ts haiku-4-5.ts
    index.ts
```

Per the user's CLAUDE.md conventions, functions use
`(dependencies) => (arguments)` currying, object-destructured arguments over
positional, and a functional style (no mutation; `reduce`/`map` over
loop-and-push). Chart helpers in `lib/chart.ts` are pure.

#### shadcn usage (maximise)

- `Button` — every `.btn*` (nav, hero CTAs, CTA band). Variants map existing
  classes: `primary`, `outline`, sizes `default`/`sm`, an `icon` size.
- `Card` — result cards, chart cards, choice/learning/finding cards, score
  notes, the formula card, the architecture nodes, the roadmap goal card.
- `Badge` — chips (run metrics), status pills, the "hidden from agent" badge,
  the roadmap "destination" badge, hero meta tags.
- `Separator` — the `.divider` rules between sections.
- Generic stroke icons → `lucide-react` (already shadcn's icon set): e.g.
  `EyeOff`, `Clock`, `Users`, `Activity`, `Search`, `Lock`, `Target`,
  `RefreshCw`, `TrendingUp`, `Building2`. Brand mark and social logos stay
  custom in `components/icons/`.

Bespoke visuals with no primitive equivalent stay as custom components styled in
`globals.css`: hero grid/glow background, the thesis cascade, the architecture
flow connectors, the timeline beats (`runs.css`).

### Styling

- Tailwind v4 with CSS-first `@theme`. `globals.css` imports Tailwind and
  carries: the shadcn oklch tokens (light + `.dark`) from
  `colors_and_type.css`, the brand `--skl-*` variables from `styles.css`, base
  typography, and the bespoke section CSS ported largely verbatim initially.
- shadcn primitives are re-themed to the brand: primary actions and chart
  accents use `--skl-blue-bright`.
- Class-name-driven bespoke CSS migrates to Tailwind/component scope
  opportunistically, not in a big-bang pass. Parity first.

### Interactivity

Two client behaviours from the current inline scripts:

- **Nav scroll border** — `Nav` is a client component tracking `window.scrollY`
  to toggle `data-scrolled`.
- **Video autoplay-on-view** — `VideoPlayer` client component encapsulating the
  IntersectionObserver play/pause and the placeholder fallback. Reused by the
  hero and run pages. The HEAD-check-for-missing-file logic is dropped: in
  Next.js the video paths are build-time assets in `public/`, so existence is
  known.

The current "reveal" animation is already a no-op (content always visible); it
is not reintroduced as hidden-first.

### Assets

`website/assets/runs/*.mp4` and `favicon.svg` move to `public/`. References
update to root-absolute paths (`/runs/fable-5.mp4`, `/favicon.svg`).

### Analytics

Replace the manual `window.va` snippet + `/_vercel/insights/script.js` with the
`@vercel/analytics/next` `<Analytics />` component in the root layout.

## Verification

- **Type safety:** TypeScript `strict`. zod validates every run content module;
  a malformed run fails the build.
- **Build:** `next build` succeeds and emits one static page per run slug plus
  the landing page.
- **Visual parity:** manual side-by-side review of the built site against the
  current `website/` for each section and each run page.
- **Optional:** a Playwright smoke test asserting `/` and each `/runs/<slug>`
  render with their key headings. Nice-to-have, not a gate.

## Deployment

- Vercel auto-detects Next.js. Project root set to `website/`.
- No `output: export`; Vercel performs SSG. All pages are static (no runtime
  data fetching), so output is effectively a static site with no server
  functions.
- `package.json` added under `website/` (Next, React, Tailwind v4, shadcn deps,
  zod, lucide-react, @vercel/analytics). Note: this introduces the repo's first
  node toolchain; `.config/wt.toml`'s "website/ — no build step" comment is
  updated accordingly.

## Risks / Open considerations

- **Chart series fidelity:** re-deriving flow/spend series depends on local
  `benchmark/runs` artifacts. If a run's artifacts are missing, that run's
  series charts fall back to transcribed headline numbers only (the before/after
  and actions charts still render; the settling/spend curves may be approximated
  or omitted for that run). Flag any such gap during implementation.
- **shadcn re-theming vs. parity:** "maximise shadcn" raises the chance of small
  visual drift from the current pixel layout. Acceptable under "port + light
  polish," but each primitive swap is checked against the original.
- **First node toolchain in the monorepo:** adds `node_modules`/lockfile under
  `website/`. Worktree config and `.gitignore` updated.
