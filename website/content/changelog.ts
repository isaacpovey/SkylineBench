export type ChangelogEntry = {
  version: string;
  date: string;
  summary: string;
  changes: string[];
};

export const changelog: ChangelogEntry[] = [
  {
    version: "v0.1",
    date: "2026-06",
    summary: "First public version: the gridlock-v1 scenario, junction-aware congestion scoring, and the anti-cheat sandbox.",
    changes: [
      "gridlock-v1 scenario: a congested city the agent must unblock without harming population or happiness.",
      "Junction-aware composite scoring blending congested metres and jammed junctions, with a graded population-health factor.",
      "Change-count and spend penalties to discourage timid, reversible-only edits.",
      "Deny-repo-read sandbox so the agent cannot read the scoring code.",
    ],
  },
];
