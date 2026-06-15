import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import fs from "node:fs";
import path from "node:path";
import Anthropic from "@anthropic-ai/sdk";
import type { RunRecord, ScoreFile } from "@/scripts/lib/build-run";
import { mapRecordToRun } from "@/scripts/lib/build-run";
import { camelVarName, serializeRun, addRunToIndex } from "@/scripts/lib/emit";
import { summarizeRun, anthropicCreateSummary } from "@/scripts/lib/summarize";
import { CURRENT_HARNESS_VERSION } from "@/lib/version";

type Args = {
  runDir: string;
  slug: string;
  modelName: string;
  harnessVersion: string;
  repoRoot: string;
  skipTimelapse: boolean;
};

const parseArgs = (argv: string[]): Args => {
  const get = (flag: string): string | undefined => {
    const i = argv.indexOf(flag);
    return i >= 0 ? argv[i + 1] : undefined;
  };
  const scriptDir = path.dirname(fileURLToPath(import.meta.url)); // website/scripts
  const runDir = get("--run-dir");
  const slug = get("--slug");
  const modelName = get("--model-name");
  if (!runDir || !slug || !modelName) {
    throw new Error("usage: generate-run --run-dir <dir> --slug <slug> --model-name <name> [--harness-version v0.1] [--skip-timelapse] [--repo-root <path>]");
  }
  return {
    runDir,
    slug,
    modelName,
    harnessVersion: get("--harness-version") ?? CURRENT_HARNESS_VERSION,
    repoRoot: get("--repo-root") ?? path.resolve(scriptDir, "../.."),
    skipTimelapse: argv.includes("--skip-timelapse"),
  };
};

const readJson = <T>(file: string): T => JSON.parse(fs.readFileSync(file, "utf8")) as T;

const buildTimelapse = (repoRoot: string, runDir: string): string => {
  const binary = path.join(repoRoot, "broker/target/release/skylinebench");
  if (!fs.existsSync(binary)) {
    console.error("building broker (release)…");
    execFileSync("cargo", ["build", "--release", "--manifest-path", path.join(repoRoot, "broker/Cargo.toml")], { stdio: "inherit" });
  }
  console.error(`generating timelapse for ${runDir}…`);
  execFileSync(binary, ["timelapse", runDir], { stdio: "inherit" });
  return path.join(runDir, "timelapse.mp4");
};

const main = async () => {
  const args = parseArgs(process.argv.slice(2));
  const scriptDir = path.dirname(fileURLToPath(import.meta.url));
  const websiteRoot = path.resolve(scriptDir, "..");

  const record = readJson<RunRecord>(path.join(args.runDir, "run-record.json"));
  const score = readJson<ScoreFile>(path.join(args.runDir, "score.json"));
  const transcript = fs.readFileSync(path.join(args.runDir, "transcript.md"), "utf8");

  console.error("summarizing run via Anthropic API…");
  const client = new Anthropic();
  const summary = await summarizeRun({ createSummary: anthropicCreateSummary(client) })({
    transcript,
    modelName: args.modelName,
    metrics: { score: score.score, changes: record.tally.num_changes, spend: record.tally.money_spent },
  });

  const run = mapRecordToRun({
    record, score, slug: args.slug, modelName: args.modelName,
    harnessVersion: args.harnessVersion, runDir: args.runDir,
    verdict: summary.verdict, beats: summary.beats,
  });

  const runFile = path.join(websiteRoot, "content/runs", `${args.slug}.ts`);
  if (fs.existsSync(runFile)) console.error(`note: overwriting existing ${runFile}`);
  fs.writeFileSync(runFile, serializeRun(run));

  const indexFile = path.join(websiteRoot, "content/runs/index.ts");
  fs.writeFileSync(indexFile, addRunToIndex(fs.readFileSync(indexFile, "utf8"), { slug: args.slug, varName: camelVarName(args.slug) }));

  const publicMp4 = path.join(websiteRoot, "public/runs", `${args.slug}.mp4`);
  const timelapse = args.skipTimelapse
    ? path.join(args.runDir, "timelapse.mp4")
    : buildTimelapse(args.repoRoot, args.runDir);
  if (fs.existsSync(timelapse)) {
    if (fs.existsSync(publicMp4)) console.error(`note: overwriting existing ${publicMp4}`);
    fs.copyFileSync(timelapse, publicMp4);
  } else {
    console.error(`warning: no timelapse at ${timelapse}; skipping mp4 copy`);
  }

  console.error(`done. Review content/runs/${args.slug}.ts before committing.`);
};

main().catch((err) => {
  console.error(err instanceof Error ? err.message : err);
  process.exitCode = 1;
});
