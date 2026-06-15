import { z } from "zod";
import Anthropic from "@anthropic-ai/sdk";
import { zodOutputFormat } from "@anthropic-ai/sdk/helpers/zod";

export const summaryShape = z.object({
  verdict: z.string(),
  beats: z.array(z.object({ title: z.string(), body: z.string() })),
});

export type Summary = z.infer<typeof summaryShape>;

export type SummarizeInput = {
  transcript: string;
  modelName: string;
  metrics: Record<string, number>;
};

export type CreateSummary = (args: { prompt: string }) => Promise<Summary>;

export const buildPrompt = ({ transcript, modelName, metrics }: SummarizeInput): string =>
  `You are writing the post-run writeup for a SkylineBench benchmark run by ${modelName}.
The benchmark drops an AI agent into a congested Cities: Skylines II city and scores how well it reduces congestion without harming the city.

Write in a precise, factual, lightly narrative voice grounded ONLY in the transcript and metrics below — no speculation.

Final metrics (composite score out of 1.00 and tallies):
${JSON.stringify(metrics, null, 2)}

Produce:
- verdict: one paragraph (2-4 sentences) summarizing what the agent did and why it scored as it did.
- beats: a chronological list of titled sections describing what the agent did, in order. Each beat has a short title and a body of one or more paragraphs (separate paragraphs with a blank line).

Transcript:
${transcript}`;

export const summarizeRun =
  (deps: { createSummary: CreateSummary }) =>
  async (input: SummarizeInput): Promise<Summary> => {
    const out = await deps.createSummary({ prompt: buildPrompt(input) });
    return summaryShape.parse(out);
  };

export const anthropicCreateSummary =
  (client: Anthropic): CreateSummary =>
  async ({ prompt }) => {
    const response = await client.messages.parse({
      model: "claude-opus-4-8",
      max_tokens: 16000,
      thinking: { type: "adaptive" },
      messages: [
        {
          role: "user",
          content: [{ type: "text", text: prompt, cache_control: { type: "ephemeral" } }],
        },
      ],
      output_config: { format: zodOutputFormat(summaryShape) },
    });
    if (!response.parsed_output) {
      throw new Error(`model returned no structured summary (stop_reason: ${response.stop_reason})`);
    }
    return response.parsed_output;
  };
