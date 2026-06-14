# SkylineBench Website Next.js Refactor — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the hand-written static `website/` with a statically generated, componentised Next.js app that reproduces the current design (port + light polish) and drives run detail pages from typed, validated content.

**Architecture:** Next.js 16 App Router + React 19 + TypeScript 6, statically generated on Vercel. shadcn/ui + Tailwind v4 themed with the site's existing oklch tokens. Run pages render from zod-validated TypeScript content modules; charts are reusable SVG components fed by pure geometry helpers.

**Tech Stack (pinned to latest as of 2026-06-14):**
- `next@16.2.9`, `react@19.2.7`, `react-dom@19.2.7`
- `typescript@6.0.3`, `@types/react@19.2.17`, `@types/react-dom@19.2.3`, `@types/node@25.9.3`
- `tailwindcss@4.3.1`, `@tailwindcss/postcss@4.3.1`, `tw-animate-css@1.4.0`
- `zod@4.4.3`
- `lucide-react@1.18.0`
- `@vercel/analytics@2.0.1`
- shadcn deps: `class-variance-authority@0.7.1`, `clsx@2.1.1`, `tailwind-merge@3.6.0`, `@radix-ui/react-slot@1.2.5`
- shadcn CLI: `shadcn@4.11.0` (invoked via `npx shadcn@latest`)

**Reference sources (the design being ported — keep open while implementing):**
- `website/index.html` — landing page markup (713 lines)
- `website/runs/fable-5.html` — canonical run page markup
- `website/runs-src/*.toml` — run narrative (verdict + beats)
- `website/styles.css` (426 lines), `website/runs.css` (41), `website/colors_and_type.css` (158) — all CSS
- `benchmark/runs/<runDir>/` (local, gitignored) — `score.json`, `run-record.json`, `end-state.json` for chart series

**Code conventions (from user CLAUDE.md — apply throughout):**
- Functional style: no mutation; `map`/`reduce`/`filter` over loop-and-push.
- Functions as `(dependencies) => (arguments)`; multiple args via a destructured object, not positional.
- No `as` casts or `any` outside test code.
- Comment only non-obvious complexity; code self-documents.

**Design quality bar (frontend-design skill):** preserve the existing distinctive aesthetic exactly (Geist type, blue-on-near-black, skyline mark, bespoke charts, hero atmosphere). Polish is additive and optional, always behind parity — see Task 18.

---

## File structure

```
website/
  package.json  tsconfig.json  next.config.ts  postcss.config.mjs  components.json
  .gitignore  .eslintrc / eslint.config.mjs
  app/
    layout.tsx          root: fonts, <html class="ds dark">, Analytics
    globals.css         Tailwind import + ported tokens + bespoke CSS
    page.tsx            landing page (composes sections)
    runs/[slug]/page.tsx run detail (generateStaticParams/Metadata)
  components/
    ui/                 shadcn primitives (button, card, badge, separator)
    layout/nav.tsx footer.tsx
    sections/           hero, thesis, how-it-works, scoring, architecture,
                        learnings, roadmap, results, findings, cta-band
    charts/             before-after, settling, spend, actions
    video-player.tsx
    icons/              brand-mark, github, linkedin, mail
    reveal.tsx          (polish) staggered reveal wrapper
  content/runs/         fable-5.ts sonnet-4-5.ts opus-4-8.ts haiku-4-5.ts index.ts
  lib/
    run.ts              zod schema + Run type
    chart.ts            pure geometry helpers
    utils.ts            cn() (shadcn)
  public/
    favicon.svg  runs/*.mp4
  __tests__/            chart.test.ts content.test.ts
```

Old files removed in Task 17: `index.html`, `runs/`, `runs-src/`, `styles.css`, `runs.css`, `colors_and_type.css`, `assets/`.

---

## Task 1: Scaffold the Next.js app in place

**Files:**
- Create: `website/package.json`, `website/tsconfig.json`, `website/next.config.ts`, `website/postcss.config.mjs`, `website/eslint.config.mjs`, `website/.gitignore`
- Create: `website/app/layout.tsx`, `website/app/globals.css`, `website/app/page.tsx`

The existing static files coexist for now (different names); they are deleted in Task 17.

- [ ] **Step 1: Create `website/package.json`**

```json
{
  "name": "skylinebench-website",
  "version": "0.1.0",
  "private": true,
  "scripts": {
    "dev": "next dev",
    "build": "next build",
    "start": "next start",
    "lint": "next lint",
    "test": "vitest run"
  },
  "dependencies": {
    "next": "16.2.9",
    "react": "19.2.7",
    "react-dom": "19.2.7",
    "zod": "4.4.3",
    "lucide-react": "1.18.0",
    "@vercel/analytics": "2.0.1",
    "class-variance-authority": "0.7.1",
    "clsx": "2.1.1",
    "tailwind-merge": "3.6.0",
    "@radix-ui/react-slot": "1.2.5"
  },
  "devDependencies": {
    "typescript": "6.0.3",
    "@types/react": "19.2.17",
    "@types/react-dom": "19.2.3",
    "@types/node": "25.9.3",
    "tailwindcss": "4.3.1",
    "@tailwindcss/postcss": "4.3.1",
    "tw-animate-css": "1.4.0",
    "vitest": "latest"
  }
}
```

- [ ] **Step 2: Install**

Run: `cd website && npm install`
Expected: lockfile created, no peer-dependency errors. If npm reports a newer patch for any pin, accept it (the directive is "latest").

- [ ] **Step 3: Create `website/tsconfig.json`**

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "lib": ["dom", "dom.iterable", "esnext"],
    "allowJs": false,
    "skipLibCheck": true,
    "strict": true,
    "noEmit": true,
    "esModuleInterop": true,
    "module": "esnext",
    "moduleResolution": "bundler",
    "resolveJsonModule": true,
    "isolatedModules": true,
    "jsx": "preserve",
    "incremental": true,
    "plugins": [{ "name": "next" }],
    "paths": { "@/*": ["./*"] }
  },
  "include": ["next-env.d.ts", "**/*.ts", "**/*.tsx", ".next/types/**/*.ts"],
  "exclude": ["node_modules"]
}
```

- [ ] **Step 4: Create `website/next.config.ts`**

```ts
import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  reactStrictMode: true,
};

export default nextConfig;
```

- [ ] **Step 5: Create `website/postcss.config.mjs`**

```js
const config = {
  plugins: ["@tailwindcss/postcss"],
};

export default config;
```

- [ ] **Step 6: Create `website/eslint.config.mjs`**

```js
import { dirname } from "path";
import { fileURLToPath } from "url";
import { FlatCompat } from "@eslint/eslintrc";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const compat = new FlatCompat({ baseDirectory: __dirname });

export default [...compat.extends("next/core-web-vitals", "next/typescript")];
```

(If `@eslint/eslintrc` is missing, `npm i -D @eslint/eslintrc`. Linting is non-blocking for this plan.)

- [ ] **Step 7: Create `website/.gitignore`**

```
/node_modules
/.next
/out
next-env.d.ts
*.tsbuildinfo
.vercel
.DS_Store
```

- [ ] **Step 8: Create minimal `website/app/globals.css`** (tokens filled in Task 2)

```css
@import "tailwindcss";
@import "tw-animate-css";
```

- [ ] **Step 9: Create `website/app/layout.tsx`**

```tsx
import type { Metadata } from "next";
import { Geist, Geist_Mono, Inter_Tight } from "next/font/google";
import { Analytics } from "@vercel/analytics/next";
import "./globals.css";

const geist = Geist({ subsets: ["latin"], variable: "--font-geist" });
const geistMono = Geist_Mono({ subsets: ["latin"], variable: "--font-geist-mono" });
const interTight = Inter_Tight({ subsets: ["latin"], variable: "--font-inter-tight" });

export const metadata: Metadata = {
  title: "SkylineBench: an AI agent benchmark",
  description:
    "A benchmark that evaluates how an agent can run and manage a city in Cities: Skylines. It has to improve the traffic without ever being told how it's being judged.",
  icons: { icon: "/favicon.svg" },
};

const RootLayout = ({ children }: { children: React.ReactNode }) => (
  <html lang="en" className={`ds dark ${geist.variable} ${geistMono.variable} ${interTight.variable}`}>
    <body className="ds dark">
      {children}
      <Analytics />
    </body>
  </html>
);

export default RootLayout;
```

- [ ] **Step 10: Create placeholder `website/app/page.tsx`**

```tsx
const Home = () => <main className="wrap"><h1 className="display">SkylineBench</h1></main>;

export default Home;
```

- [ ] **Step 11: Verify dev server boots**

Run: `cd website && npm run dev` then `curl -s localhost:3000 | grep -o "SkylineBench" | head -1` in another shell; stop the server.
Expected: prints `SkylineBench`. No build errors in the dev log.

- [ ] **Step 12: Verify production build**

Run: `cd website && npm run build`
Expected: build succeeds; output lists `/` as a static (prerendered) route.

- [ ] **Step 13: Commit**

```bash
git add website/package.json website/package-lock.json website/tsconfig.json website/next.config.ts website/postcss.config.mjs website/eslint.config.mjs website/.gitignore website/app
git commit -m "feat(website): scaffold Next.js app"
```

---

## Task 2: Port design tokens and bespoke CSS into globals.css

**Files:**
- Modify: `website/app/globals.css`
- Reference: `website/colors_and_type.css`, `website/styles.css`, `website/runs.css`

Tailwind v4 reads theme tokens from CSS. We keep the site's existing class-based CSS (it already uses the shadcn token names) and expose the tokens to Tailwind via `@theme inline` so shadcn primitives resolve `bg-card`, `text-muted-foreground`, etc.

- [ ] **Step 1: Build `website/app/globals.css`** in this order:

1. `@import "tailwindcss";` and `@import "tw-animate-css";` (already present).
2. `@custom-variant dark (&:is(.dark *));` — so `dark:` utilities follow the `.dark` class (the site is class-themed, not media-themed).
3. Paste the `:root`/`.ds`/`.dark` token blocks from `website/colors_and_type.css` **verbatim** (the oklch tokens), and the `--skl-*` brand vars from the top of `website/styles.css` (lines 3-8).
4. Map fonts to the next/font CSS variables: set `--font-sans: var(--font-geist)`, `--font-mono: var(--font-geist-mono)`, `--font-heading: var(--font-geist), var(--font-inter-tight), var(--font-sans)`. Remove the Google Fonts `@import url(...)` line (next/font handles loading).
5. Add an `@theme inline { ... }` block bridging tokens to Tailwind utilities:

```css
@theme inline {
  --color-background: var(--background);
  --color-foreground: var(--foreground);
  --color-card: var(--card);
  --color-card-foreground: var(--card-foreground);
  --color-popover: var(--popover);
  --color-popover-foreground: var(--popover-foreground);
  --color-primary: var(--primary);
  --color-primary-foreground: var(--primary-foreground);
  --color-secondary: var(--secondary);
  --color-secondary-foreground: var(--secondary-foreground);
  --color-muted: var(--muted);
  --color-muted-foreground: var(--muted-foreground);
  --color-accent: var(--accent);
  --color-accent-foreground: var(--accent-foreground);
  --color-destructive: var(--destructive);
  --color-border: var(--border);
  --color-input: var(--input);
  --color-ring: var(--ring);
  --color-skl-blue: var(--skl-blue);
  --color-skl-blue-bright: var(--skl-blue-bright);
  --radius-lg: var(--radius);
  --radius-md: calc(var(--radius) - 2px);
  --radius-sm: calc(var(--radius) - 4px);
  --font-sans: var(--font-geist);
  --font-mono: var(--font-geist-mono);
}
```

6. Paste the remainder of `website/styles.css` (everything after the brand vars — all the `.nav`, `.hero`, `.section`, `.choice`, `.arch`, `.roadmap`, `.results`, `.footer`, `.btn` rules, etc.) **verbatim**.
7. Paste all of `website/runs.css` **verbatim** (the `.run-hero`, `.chips`, `.chart-card`, `.c-*`, `.timeline`, `.beat` rules).

- [ ] **Step 2: Apply tokens to the body base** — confirm `.ds` base typography rules from `colors_and_type.css` are included so `body.ds` gets background/foreground/font.

- [ ] **Step 3: Verify build + visual smoke**

Run: `cd website && npm run dev`, open `localhost:3000`.
Expected: the placeholder `<h1 class="display">SkylineBench</h1>` renders in Geist on the near-black background with correct heading style (confirms tokens + fonts + bespoke CSS all load).

- [ ] **Step 4: Commit**

```bash
git add website/app/globals.css
git commit -m "feat(website): port design tokens and bespoke CSS to globals"
```

---

## Task 3: Initialise shadcn and add primitives

**Files:**
- Create: `website/components.json`, `website/lib/utils.ts`, `website/components/ui/{button,card,badge,separator}.tsx`
- Modify: `website/components/ui/button.tsx` (brand variants)

- [ ] **Step 1: Create `website/lib/utils.ts`**

```ts
import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export const cn = (...inputs: ClassValue[]) => twMerge(clsx(inputs));
```

- [ ] **Step 2: Create `website/components.json`**

```json
{
  "$schema": "https://ui.shadcn.com/schema.json",
  "style": "new-york",
  "rsc": true,
  "tsx": true,
  "tailwind": {
    "config": "",
    "css": "app/globals.css",
    "baseColor": "neutral",
    "cssVariables": true,
    "prefix": ""
  },
  "aliases": {
    "components": "@/components",
    "utils": "@/lib/utils",
    "ui": "@/components/ui",
    "lib": "@/lib",
    "hooks": "@/hooks"
  },
  "iconLibrary": "lucide"
}
```

- [ ] **Step 3: Add primitives via CLI**

Run: `cd website && npx shadcn@latest add button card badge separator --yes`
Expected: files created under `components/ui/`. If the CLI prompts about overwriting `globals.css`, decline (we manage it manually); the components themselves use the token utilities already defined in Task 2.

- [ ] **Step 4: Extend `button.tsx` with the site's brand variants**

In the `buttonVariants` `cva` call, ensure these variants/sizes exist (matching `website/styles.css` `.btn*` rules). Add to the `variants` map:

```ts
variant: {
  // keep shadcn defaults, then:
  primary: "bg-primary text-primary-foreground hover:bg-primary/90",
  outline: "bg-transparent text-foreground border border-border hover:bg-foreground/5 hover:border-foreground/20",
  ghost: "bg-transparent text-muted-foreground hover:text-foreground",
},
size: {
  // keep defaults, then:
  sm: "h-9 px-[13px] text-sm",
  icon: "h-9 w-9 p-0 [&_svg]:size-4",
},
```

Keep the base classes consistent with `.btn` (height 40px default, inline-flex, gap, radius `--radius`, font-weight 500). The default `<Button asChild>` pattern wraps `<a>` for links.

- [ ] **Step 5: Verify build**

Run: `cd website && npm run build`
Expected: build succeeds.

- [ ] **Step 6: Commit**

```bash
git add website/components.json website/lib/utils.ts website/components/ui
git commit -m "feat(website): add shadcn primitives with brand variants"
```

---

## Task 4: Icon components

**Files:**
- Create: `website/components/icons/{brand-mark,github,linkedin,mail}.tsx`

Generic stroke icons (clock, users, eye-off, activity, search, lock, target, refresh, trending-up, building) come from `lucide-react` directly at call sites. Only the brand mark and social logos (which are custom paths, repeated ~5x in the current HTML) become components.

- [ ] **Step 1: Create `website/components/icons/brand-mark.tsx`** — copy the skyline `<svg class="mark">` from `website/index.html:23-29`:

```tsx
export const BrandMark = ({ className }: { className?: string }) => (
  <svg className={className} viewBox="0 0 28 28" fill="none" aria-hidden="true">
    <rect x="2" y="14" width="4" height="10" rx="1" fill="currentColor" opacity="0.55" />
    <rect x="8" y="9" width="4" height="15" rx="1" fill="currentColor" opacity="0.8" />
    <rect x="14" y="4" width="4" height="20" rx="1" fill="var(--skl-blue-bright)" />
    <rect x="20" y="11" width="4" height="13" rx="1" fill="currentColor" opacity="0.65" />
    <line x1="1" y1="25.5" x2="27" y2="25.5" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" opacity="0.4" />
  </svg>
);
```

- [ ] **Step 2: Create `github.tsx`, `linkedin.tsx`, `mail.tsx`** — same pattern, copying the respective `<svg>` path data from `website/index.html` (GitHub at line 48, LinkedIn at line 45, Mail at line 42). Each: `({ className }: { className?: string }) => (<svg className={className} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">…</svg>)`. Convert `fill-rule`→`fillRule` etc. where present.

- [ ] **Step 3: Typecheck**

Run: `cd website && npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add website/components/icons
git commit -m "feat(website): add brand and social icon components"
```

---

## Task 5: Run content schema and type

**Files:**
- Create: `website/lib/run.ts`, `website/__tests__/content.test.ts`
- Create test fixture inline.

- [ ] **Step 1: Write failing schema-validation test** `website/__tests__/content.test.ts`

```ts
import { describe, it, expect } from "vitest";
import { runSchema } from "@/lib/run";

const valid = {
  slug: "demo", modelName: "Demo", map: "gridlock-v1", runDir: "benchmark/runs/x",
  score: 0.5, verdict: "ok",
  metrics: {
    flow: { from: 57, to: 71 }, congestedMetres: { from: 5122, to: 1854 },
    jammedJunctions: { from: 35, to: 12 }, population: { from: 31640, to: 31174 },
    activeVehicles: { from: 2112, to: 1709 }, changes: 197, spend: 1240000,
  },
  flowSettling: { base: [57, 56], final: [57, 71] },
  spendSeries: [0, 1000, 1240000],
  actions: [{ type: "upgrade_road", count: 180, cost: 1180000 }],
  beats: [{ title: "Survey", body: "read the map" }],
};

describe("runSchema", () => {
  it("accepts a valid run", () => {
    expect(() => runSchema.parse(valid)).not.toThrow();
  });
  it("rejects a score above 1", () => {
    expect(() => runSchema.parse({ ...valid, score: 1.5 })).toThrow();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd website && npx vitest run __tests__/content.test.ts`
Expected: FAIL — cannot resolve `@/lib/run`.

- [ ] **Step 3: Create `website/lib/run.ts`**

```ts
import { z } from "zod";

const pair = z.object({ from: z.number(), to: z.number() });

export const runSchema = z.object({
  slug: z.string(),
  modelName: z.string(),
  map: z.string(),
  runDir: z.string(),
  score: z.number().min(0).max(1),
  verdict: z.string(),
  metrics: z.object({
    flow: pair,
    congestedMetres: pair,
    jammedJunctions: pair,
    population: pair,
    activeVehicles: pair,
    changes: z.number(),
    spend: z.number(),
  }),
  flowSettling: z.object({ base: z.array(z.number()), final: z.array(z.number()) }),
  spendSeries: z.array(z.number()),
  actions: z.array(z.object({ type: z.string(), count: z.number(), cost: z.number() })),
  beats: z.array(z.object({ title: z.string(), body: z.string() })),
});

export type Run = z.infer<typeof runSchema>;

export const defineRun = (run: Run): Run => runSchema.parse(run);
```

- [ ] **Step 4: Add vitest config** `website/vitest.config.ts`

```ts
import { defineConfig } from "vitest/config";
import tsconfigPaths from "vite-tsconfig-paths";

export default defineConfig({ plugins: [tsconfigPaths()] });
```

Run: `cd website && npm i -D vite-tsconfig-paths`

- [ ] **Step 5: Run test to verify it passes**

Run: `cd website && npx vitest run __tests__/content.test.ts`
Expected: PASS (2 tests).

- [ ] **Step 6: Commit**

```bash
git add website/lib/run.ts website/__tests__/content.test.ts website/vitest.config.ts website/package.json website/package-lock.json
git commit -m "feat(website): add run content schema with validation"
```

---

## Task 6: Pure chart geometry helpers

**Files:**
- Create: `website/lib/chart.ts`, `website/__tests__/chart.test.ts`
- Reference: the computed SVG geometry in `website/runs/fable-5.html:44` (to match output scaling)

Charts are SVGs built from data. The math is pure and unit-tested. Helpers follow `(deps) => (args)` with object args.

- [ ] **Step 1: Write failing tests** `website/__tests__/chart.test.ts`

```ts
import { describe, it, expect } from "vitest";
import { scaleBars, polyline, normaliseSeries } from "@/lib/chart";

describe("scaleBars", () => {
  it("maps the largest value to full width", () => {
    const bars = scaleBars({ maxWidth: 200 })({ values: [50, 100] });
    expect(bars).toEqual([100, 200]);
  });
});

describe("normaliseSeries", () => {
  it("maps min to 0 and max to 1", () => {
    expect(normaliseSeries([10, 20])).toEqual([0, 1]);
  });
  it("returns zeros for a flat series", () => {
    expect(normaliseSeries([5, 5])).toEqual([0, 0]);
  });
});

describe("polyline", () => {
  it("spreads points across the width and inverts y", () => {
    const pts = polyline({ width: 100, height: 100 })({ values: [0, 1] });
    expect(pts).toBe("0,100 100,0");
  });
});
```

- [ ] **Step 2: Run to verify fail**

Run: `cd website && npx vitest run __tests__/chart.test.ts`
Expected: FAIL — cannot resolve `@/lib/chart`.

- [ ] **Step 3: Create `website/lib/chart.ts`**

```ts
export const normaliseSeries = (values: number[]): number[] => {
  const min = Math.min(...values);
  const max = Math.max(...values);
  const span = max - min;
  return values.map((v) => (span === 0 ? 0 : (v - min) / span));
};

export const scaleBars =
  ({ maxWidth }: { maxWidth: number }) =>
  ({ values }: { values: number[] }): number[] => {
    const max = Math.max(...values);
    return values.map((v) => (max === 0 ? 0 : (v / max) * maxWidth));
  };

export const polyline =
  ({ width, height }: { width: number; height: number }) =>
  ({ values }: { values: number[] }): string => {
    const step = values.length > 1 ? width / (values.length - 1) : 0;
    return values
      .map((v, i) => `${+(i * step).toFixed(2)},${+((1 - v) * height).toFixed(2)}`)
      .join(" ");
  };
```

- [ ] **Step 4: Run to verify pass**

Run: `cd website && npx vitest run __tests__/chart.test.ts`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add website/lib/chart.ts website/__tests__/chart.test.ts
git commit -m "feat(website): add pure chart geometry helpers"
```

---

## Task 7: Chart components

**Files:**
- Create: `website/components/charts/{before-after-chart,settling-chart,spend-chart,actions-chart}.tsx`
- Reference: `website/runs/fable-5.html:44` (target SVG structure), `website/runs.css:27-34` (`.c-*` classes)

Each is a server component taking typed props, rendering an SVG using `lib/chart.ts` and the existing `.chart-svg` / `.c-*` classes (already in globals.css). No client JS.

- [ ] **Step 1: Create `before-after-chart.tsx`** — horizontal paired bars (base vs final) per metric row. Props: `{ rows: { label: string; base: number; final: number; format?: (n: number) => string }[] }`. Layout mirrors the "Before → after" SVG: label text at `x=0`, base bar `.c-base` and final bar `.c-final` scaled by `scaleBars({ maxWidth: 200 })` against the row's larger value, value labels via `.c-val`/`.c-val-final`. Render inside `<figure class="chart-card"><figcaption>Before → after</figcaption><svg class="chart-svg" viewBox=...>`.

- [ ] **Step 2: Create `settling-chart.tsx`** — two polylines (base, final) from `flowSettling`. Props `{ base: number[]; final: number[] }`. Normalise both series **together** (shared min/max so curves are comparable): compute `normaliseSeries([...base, ...final])` then split. Use `polyline({ width, height })` for points, classes `.c-line-base` / `.c-line-final`, min/max axis labels via `.c-axis`. `<figcaption>Flow settling</figcaption>`.

- [ ] **Step 3: Create `spend-chart.tsx`** — single cumulative polyline from `spendSeries` via `normaliseSeries` + `polyline`, class `.c-line-final`, plus a `.c-val-final` label `"$X.XXM · N changes"`. Props `{ series: number[]; total: number; changes: number }`. `<figcaption>Cumulative spend</figcaption>`.

- [ ] **Step 4: Create `actions-chart.tsx`** — horizontal bars per action type. Props `{ actions: { type: string; count: number; cost: number }[] }`. Bar widths via `scaleBars({ maxWidth: 150 })` on `count`; row label = type, value label = `"{count} · ${cost}"` formatted. Class `.c-final`. `<figcaption>Actions by type</figcaption>`.

- [ ] **Step 5: Typecheck**

Run: `cd website && npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add website/components/charts
git commit -m "feat(website): add reusable SVG chart components"
```

---

## Task 8: Author run content

**Files:**
- Create: `website/content/runs/{fable-5,sonnet-4-5,opus-4-8,haiku-4-5}.ts`, `website/content/runs/index.ts`
- Modify: `website/__tests__/content.test.ts` (validate all real runs)
- Source: `website/runs-src/*.toml` (verdict + beats), `website/runs/*.html` (headline numbers), `benchmark/runs/<runDir>/{score.json,run-record.json,end-state.json}` (series)

- [ ] **Step 1: Author `fable-5.ts`** using `defineRun`. Copy `verdict` and the 8 `beats` (title + body) verbatim from `website/runs-src/fable-5.toml`. Fill `metrics` from the chips in `website/runs/fable-5.html:43` (flow 57→71, congestedMetres 5122→1854, jammedJunctions 35→12, population 31640→31174, activeVehicles 2112→1709, changes 197, spend 1240000). Derive `flowSettling`, `spendSeries`, `actions` from `benchmark/runs/20260612-121219/run-record.json`:
  - `actions`: from `run-record.json` — counts and summed cost per action type (bulldoze 6/$0, build_road 11/$57800, upgrade_road 180/$1180000 per the current chart).
  - `spendSeries`: cumulative cost after each action (array of running totals). If `run-record.json` lists per-action cost, `reduce` to a cumulative array.
  - `flowSettling`: the flow readings over sim steps the agent observed (base = baseline-hold reference, final = the climbing curve). Use the values narrated in the beats / present in `run-record.json` metrics snapshots.

```ts
import { defineRun } from "@/lib/run";

export const fable5 = defineRun({
  slug: "fable-5",
  modelName: "Claude Fable 5",
  map: "gridlock-v1",
  runDir: "benchmark/runs/20260612-121219",
  score: 0.63,
  verdict: "Fable 5 diagnosed gridlock-v1 as a queue-spillback problem …", // verbatim from toml
  metrics: { /* … as above … */ },
  flowSettling: { base: [/* … */], final: [/* … */] },
  spendSeries: [/* cumulative … */],
  actions: [
    { type: "bulldoze", count: 6, cost: 0 },
    { type: "build_road", count: 11, cost: 57800 },
    { type: "upgrade_road", count: 180, cost: 1180000 },
  ],
  beats: [ /* 8 beats verbatim from toml */ ],
});
```

- [ ] **Step 2: Author `sonnet-4-5.ts`, `opus-4-8.ts`, `haiku-4-5.ts`** the same way, from their `runs-src/*.toml` + `runs/*.html` chips + their `runDir` artifacts. Headline numbers visible on the landing results grid: Sonnet score 0.31 (+6% flow, -9% pop), Opus 0.21 (+9% flow, -15% pop), Haiku 0.00 (-17% flow, -57% pop). **If a run's `benchmark/runs/<dir>` artifacts are missing locally, set `spendSeries`/`flowSettling` from the values transcribed in that run's current HTML charts, and note the gap in the commit message.** (See spec "Sourcing the chart data".)

- [ ] **Step 3: Create `website/content/runs/index.ts`**

```ts
import type { Run } from "@/lib/run";
import { fable5 } from "./fable-5";
import { sonnet45 } from "./sonnet-4-5";
import { opus48 } from "./opus-4-8";
import { haiku45 } from "./haiku-4-5";

export const runs: Run[] = [fable5, sonnet45, opus48, haiku45].sort((a, b) => b.score - a.score);

export const getRun = (slug: string): Run | undefined => runs.find((r) => r.slug === slug);
```

- [ ] **Step 4: Extend content test** to validate every authored run:

```ts
import { runs } from "@/content/runs";
it("all authored runs are valid and ranked", () => {
  expect(runs.length).toBe(4);
  expect(runs[0].slug).toBe("fable-5");
  runs.forEach((r) => expect(() => runSchema.parse(r)).not.toThrow());
});
```

- [ ] **Step 5: Run tests**

Run: `cd website && npx vitest run`
Expected: PASS. (`defineRun` would already throw at import if any run were malformed.)

- [ ] **Step 6: Commit**

```bash
git add website/content website/__tests__/content.test.ts
git commit -m "feat(website): author typed run content for the four models"
```

---

## Task 9: Layout components (Nav, Footer)

**Files:**
- Create: `website/components/layout/nav.tsx` (client), `website/components/layout/footer.tsx`
- Reference: `website/index.html:20-53` (nav), `:616-665` (footer)

- [ ] **Step 1: Create `nav.tsx`** as a client component (`"use client"`). Port the nav markup; replace the scroll script with a `useEffect` that toggles a `data-scrolled` attribute on `window.scrollY > 8`. Use `Button` (`asChild`) for the GitHub/email/LinkedIn buttons, `BrandMark` + icon components for logos. Props: `{ variant?: "landing" | "run" }` — `landing` shows the section anchor links (`#thesis` … `#results`), `run` shows only "← Back to results" + GitHub (matches `website/runs/fable-5.html:17-23`).

```tsx
"use client";
import { useEffect, useState } from "react";
// … imports …
export const Nav = ({ variant = "landing" }: { variant?: "landing" | "run" }) => {
  const [scrolled, setScrolled] = useState(false);
  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 8);
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
    return () => window.removeEventListener("scroll", onScroll);
  }, []);
  return (
    <nav className="nav" data-scrolled={scrolled}>
      {/* ported markup; anchors gated on variant */}
    </nav>
  );
};
```

- [ ] **Step 2: Create `footer.tsx`** (server component) — port `website/index.html:616-665`, using `BrandMark` and icon components, section anchor links. Accept `{ links?: boolean }` so run pages can render the slim footer (`website/runs/fable-5.html:53-56`).

- [ ] **Step 3: Typecheck + commit**

Run: `cd website && npx tsc --noEmit` (expect no errors)
```bash
git add website/components/layout
git commit -m "feat(website): add nav and footer components"
```

---

## Task 10: VideoPlayer client component

**Files:**
- Create: `website/components/video-player.tsx`
- Reference: `website/index.html:680-709` and `website/runs/fable-5.html:58-71`

- [ ] **Step 1: Create `video-player.tsx`** (`"use client"`). Props: `{ src: string; autoplayOnView?: boolean; className?: string; children?: React.ReactNode }` (children = placeholder content). Use a `ref` + `IntersectionObserver` to play/pause when `autoplayOnView`, else play/pause on hover. Hide the placeholder on `loadeddata`. Drop the HEAD-check (assets are known build-time files in `public/`).

```tsx
"use client";
import { useEffect, useRef } from "react";

export const VideoPlayer = ({ src, autoplayOnView, className, children }: {
  src: string; autoplayOnView?: boolean; className?: string; children?: React.ReactNode;
}) => {
  const ref = useRef<HTMLVideoElement>(null);
  useEffect(() => {
    const v = ref.current;
    if (!v) return;
    const play = () => { void v.play().catch(() => {}); };
    if (autoplayOnView && "IntersectionObserver" in window) {
      const io = new IntersectionObserver(
        (es) => es.forEach((e) => (e.isIntersecting ? play() : v.pause())),
        { threshold: 0.3 },
      );
      io.observe(v);
      return () => io.disconnect();
    }
  }, [autoplayOnView]);
  return (
    <div className="media-stage">
      <video ref={ref} className={className} muted loop playsInline preload="none">
        <source src={src} type="video/mp4" />
      </video>
      {children}
    </div>
  );
};
```

- [ ] **Step 2: Typecheck + commit**

```bash
git add website/components/video-player.tsx
git commit -m "feat(website): add VideoPlayer client component"
```

---

## Task 11: Move assets to public

**Files:**
- Move: `website/assets/runs/*.mp4` → `website/public/runs/`, `website/assets/favicon.svg` → `website/public/favicon.svg`

- [ ] **Step 1: Move files**

Run:
```bash
cd website && mkdir -p public/runs && git mv assets/runs/*.mp4 public/runs/ && git mv assets/favicon.svg public/favicon.svg
```
Expected: files relocated, still tracked.

- [ ] **Step 2: Commit**

```bash
git commit -m "feat(website): move video and favicon assets into public/"
```

---

## Task 12: Run detail page

**Files:**
- Create: `website/app/runs/[slug]/page.tsx`
- Reference: `website/runs/fable-5.html` (full structure)

- [ ] **Step 1: Create the page**

```tsx
import type { Metadata } from "next";
import { notFound } from "next/navigation";
import { runs, getRun } from "@/content/runs";
import { Nav } from "@/components/layout/nav";
import { Footer } from "@/components/layout/footer";
import { VideoPlayer } from "@/components/video-player";
import { BeforeAfterChart } from "@/components/charts/before-after-chart";
import { SettlingChart } from "@/components/charts/settling-chart";
import { SpendChart } from "@/components/charts/spend-chart";
import { ActionsChart } from "@/components/charts/actions-chart";

export const generateStaticParams = () => runs.map((r) => ({ slug: r.slug }));

export const generateMetadata = async ({ params }: { params: Promise<{ slug: string }> }): Promise<Metadata> => {
  const { slug } = await params;
  const run = getRun(slug);
  return { title: run ? `${run.modelName} · SkylineBench run` : "SkylineBench run" };
};

const RunPage = async ({ params }: { params: Promise<{ slug: string }> }) => {
  const { slug } = await params;
  const run = getRun(slug);
  if (!run) notFound();
  // render: <Nav variant="run" />, run-hero (score, verdict, VideoPlayer),
  // chips grid from run.metrics, chart-grid with the four charts,
  // timeline <ol class="timeline"> from run.beats, <Footer links={false} />
};

export default RunPage;
```

Build the chips and charts from `run.metrics` / `run.flowSettling` / `run.spendSeries` / `run.actions`. The `BeforeAfterChart` rows = flow, congested m, jammed junctions, active vehicles, population. Timeline `<li class="beat">` per beat with `<h3>{title}</h3>` and body split on blank lines into `<p>`.

- [ ] **Step 2: Verify each run statically generates**

Run: `cd website && npm run build`
Expected: build output lists `/runs/fable-5`, `/runs/sonnet-4-5`, `/runs/opus-4-8`, `/runs/haiku-4-5` as prerendered (SSG) pages.

- [ ] **Step 3: Visual parity check**

Run: `npm run dev`; open `/runs/fable-5`. Compare against `website/runs/fable-5.html` opened in the browser side by side: hero score, chips, four charts, timeline.
Expected: visually equivalent (light-polish differences acceptable).

- [ ] **Step 4: Commit**

```bash
git add website/app/runs
git commit -m "feat(website): add statically generated run detail pages"
```

---

## Task 13: Landing sections — atmosphere + hero + thesis

**Files:**
- Create: `website/components/sections/{hero,thesis}.tsx`
- Reference: `website/index.html:57-148`

- [ ] **Step 1: Create `hero.tsx`** — port `index.html:57-110`. Hero head (eyebrow, `h1.display`, lead, CTA `Button`s, meta tags as `Badge`/spans), and the media frame using `VideoPlayer` with `src="/runs/fable-5.mp4"` `autoplayOnView` and the placeholder children. The "best run" stats footer pulls from the top-ranked run in `content/runs` (`runs[0]`) rather than hardcoding.

- [ ] **Step 2: Create `thesis.tsx`** — port `index.html:114-148` (prose, the `.cascade` chain with arrow SVGs, the blockquote). Static markup; cascade arrows use a small inline arrow SVG or a lucide `ArrowRight`.

- [ ] **Step 3: Wire into `app/page.tsx`** (progressively) and visually check the hero renders with autoplaying video.

- [ ] **Step 4: Commit**

```bash
git add website/components/sections/hero.tsx website/components/sections/thesis.tsx website/app/page.tsx
git commit -m "feat(website): add hero and thesis sections"
```

---

## Task 14: Landing sections — how-it-works, scoring, architecture

**Files:**
- Create: `website/components/sections/{how-it-works,scoring,architecture}.tsx`
- Reference: `website/index.html:150-341`

- [ ] **Step 1: `how-it-works.tsx`** — port `:150-227`: the tool inventory (`.tools`/`.tool-group`) and the four numbered `.choice` `Card`s with lucide icons (eye-off, building, clock, lock).

- [ ] **Step 2: `scoring.tsx`** — port `:231-288`: the formula `Card`, the `hidden-badge` (`Badge` + eye-off icon), the legend rows, and the three `.score-note` `Card`s with lucide icons.

- [ ] **Step 3: `architecture.tsx`** — port `:290-341`: the `.arch-flow` nodes (`Card`s with tags + lucide icons), `.arch-conn` connectors, and the `.arch-loop` summary.

- [ ] **Step 4: Wire into `app/page.tsx`; visual check; commit**

```bash
git add website/components/sections/how-it-works.tsx website/components/sections/scoring.tsx website/components/sections/architecture.tsx website/app/page.tsx
git commit -m "feat(website): add how-it-works, scoring, architecture sections"
```

---

## Task 15: Landing sections — learnings, roadmap, findings

**Files:**
- Create: `website/components/sections/{learnings,roadmap,findings}.tsx`
- Reference: `website/index.html:343-433`, `:548-587`

- [ ] **Step 1: `learnings.tsx`** — port `:343-375`: three full-width `.choice` `Card`s (search, target, activity icons).

- [ ] **Step 2: `roadmap.tsx`** — port `:379-433`: the `<ol class="roadmap">` five steps and the `.rm-goal` destination `Card` with `Badge`.

- [ ] **Step 3: `findings.tsx`** — port `:548-587`: four full-width `.choice` `Card`s (trending-up, users, eye-off, clock icons).

- [ ] **Step 4: Wire into `app/page.tsx`; visual check; commit**

```bash
git add website/components/sections/learnings.tsx website/components/sections/roadmap.tsx website/components/sections/findings.tsx website/app/page.tsx
git commit -m "feat(website): add learnings, roadmap, findings sections"
```

---

## Task 16: Landing sections — results, CTA band; assemble page

**Files:**
- Create: `website/components/sections/{results,cta-band}.tsx`
- Modify: `website/app/page.tsx` (final assembly)
- Reference: `website/index.html:435-544`, `:591-613`

- [ ] **Step 1: `results.tsx`** — port `:435-544`. Render the results grid by mapping over `runs` from `content/runs` (rank = index + 1), each an `<a class="result-card" href="/runs/{slug}">` `Card` showing rank, model name, score, and the flow/population metric deltas computed from `run.metrics` (`+/-%`). Keep the "coming soon" block.

- [ ] **Step 2: `cta-band.tsx`** — port `:591-613`: heading, two leads, three `Button`s (email/LinkedIn/GitHub).

- [ ] **Step 3: Finalise `app/page.tsx`** composing, in order: `<Nav />`, `<Hero />`, `Separator`, `<Thesis />`, `<HowItWorks />`, `Separator`, `<Scoring />`, `<Architecture />`, `<Learnings />`, `Separator`, `<Roadmap />`, `<Results />`, `Separator`, `<Findings />`, `Separator`, `<CtaBand />`, `<Footer />`. Match the `<hr class="divider">` placements in `index.html` (use `Separator` styled as `.divider`, or keep `<hr className="divider" />`).

- [ ] **Step 4: Full build + parity check**

Run: `cd website && npm run build`
Expected: succeeds; `/` prerendered. Open `/` and compare top-to-bottom against `website/index.html`.

- [ ] **Step 5: Commit**

```bash
git add website/components/sections/results.tsx website/components/sections/cta-band.tsx website/app/page.tsx
git commit -m "feat(website): add results and CTA sections, assemble landing page"
```

---

## Task 17: Remove legacy static site

**Files:**
- Delete: `website/index.html`, `website/runs/`, `website/runs-src/`, `website/styles.css`, `website/runs.css`, `website/colors_and_type.css`, `website/assets/`

- [ ] **Step 1: Confirm nothing still references them** — `grep -rn "runs-src\|colors_and_type\|styles.css" website/app website/components` returns nothing.

- [ ] **Step 2: Delete**

Run:
```bash
cd website && git rm -r index.html runs runs-src styles.css runs.css colors_and_type.css assets
```
(`runs-src` content is now preserved inside `content/runs/*.ts`; the narrative was copied verbatim in Task 8.)

- [ ] **Step 3: Build still passes**

Run: `cd website && npm run build`
Expected: succeeds.

- [ ] **Step 4: Commit**

```bash
git commit -m "chore(website): remove legacy static site files"
```

---

## Task 18: Light polish (optional, behind parity)

**Files:**
- Create: `website/components/reveal.tsx`
- Modify: section components to wrap top-level blocks

Only after full parity is confirmed. Each item is independently revertible if it drifts from the original.

- [ ] **Step 1: Create `reveal.tsx`** — a client component that adds a staggered fade/translate on mount via IntersectionObserver, replacing the currently-inert `.reveal` class with a real (motion-safe) reveal. Respect `prefers-reduced-motion` (no transform when reduced). Content is never hidden-first without JS (SSR outputs visible content; the class only enhances).

- [ ] **Step 2: Apply** to the elements that currently carry `class="reveal"`. Verify with JS disabled that all content is still visible.

- [ ] **Step 3: Hover micro-interactions** — confirm `.result-card`/`.choice`/`.chart-card` hover states from `styles.css` carried over; add subtle `transition` only if missing. No layout shift.

- [ ] **Step 4: Build, reduced-motion check, commit**

```bash
git add website/components/reveal.tsx website/components/sections website/app
git commit -m "feat(website): wire staggered reveal and hover polish"
```

---

## Task 19: Final verification and deploy config

**Files:**
- Modify: `.config/wt.toml` (update the "website/ — no build step" comment)
- Verify: Vercel project root

- [ ] **Step 1: Update `.config/wt.toml`** — change the `website/  static HTML/CSS  — no build step` line to note `website/  Next.js app  — npm install / npm run build`.

- [ ] **Step 2: Full test + build**

Run: `cd website && npx vitest run && npm run build`
Expected: all tests pass; build succeeds with `/`, `/runs/fable-5`, `/runs/sonnet-4-5`, `/runs/opus-4-8`, `/runs/haiku-4-5` all prerendered.

- [ ] **Step 3: Confirm Vercel config** — Vercel auto-detects Next.js. Ensure the Vercel project's Root Directory is set to `website/` (dashboard setting; note in commit if a `vercel.json` is preferred instead). Static output: all routes prerendered, no server functions expected.

- [ ] **Step 4: Final parity sweep** — open built `/` and every `/runs/<slug>` against the originals (recover originals from git if already deleted). Confirm: nav scroll border, hero video autoplay, all sections, run charts, timelines.

- [ ] **Step 5: Commit**

```bash
git add .config/wt.toml
git commit -m "chore: update worktrunk config for website build step"
```

---

## Self-review notes

- **Spec coverage:** routes (T1, T12, T16), tokens/styling (T2, T3), content model + zod (T5, T8), chart components from data (T6, T7), icons/dedup (T4), client behaviours (T9 nav, T10 video), assets (T11), analytics (T1 layout), legacy removal (T17), polish (T18), deploy (T19). All spec sections mapped.
- **Type consistency:** `Run` field names (`flowSettling`, `spendSeries`, `metrics.congestedMetres`, etc.) are identical across `lib/run.ts` (T5), content (T8), charts (T7), and the run page (T12). Chart helper signatures (`scaleBars`, `polyline`, `normaliseSeries`) match between T6 definitions and T7 usage.
- **Known data risk:** chart series for runs whose `benchmark/runs/<dir>` artifacts are absent locally fall back to transcribed HTML values (T8 Step 2) — surfaced, not silent.
