import type { Run } from "@/lib/run";
import { fable5 } from "./fable-5";
import { sonnet45 } from "./sonnet-4-5";
import { opus48 } from "./opus-4-8";
import { haiku45 } from "./haiku-4-5";
import { gpt55 } from "./gpt-5-5";
import { gpt54mini } from "./gpt-5-4-mini";

export const runs: Run[] = [fable5, sonnet45, opus48, haiku45, gpt55, gpt54mini].sort((a, b) => b.score - a.score);

export const getRun = (slug: string): Run | undefined => runs.find((r) => r.slug === slug);
