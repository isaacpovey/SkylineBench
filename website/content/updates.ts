export type UpdateCard = { title: string; body: string };

export type UpdateEntry = {
  title: string;
  date: string;
  intro?: string;
  cards: UpdateCard[];
};

export const updates: UpdateEntry[] = [
  {
    title: "Learnings from the first version",
    date: "v0.1",
    intro: "Pretty much every design decision in the prompt, the scoring, and the sandbox came from something the agent broke first.",
    cards: [
      {
        title: "It read the answer key",
        body: "The first run had no sandbox. The agent noticed it was running in the same directory as the repository, found the harness code, read the scoring function, and sidestepped the benchmark. Its solution: delete everything. No city, no traffic. A perfect congestion score. It took about five minutes to find the loophole I hadn't thought to close. This is why the sandbox exists.",
      },
      {
        title: "When you close a loophole, it finds the margin.",
        body: "The population floor was the first version of this fix: a minimum the population couldn't fall below, supplied in the prompt. The agent found the floor and parked exactly on it. It reduced the population to the minimum viable number and held it there, treating the floor as a target rather than a guardrail, since it figured this was easier than fixing the actual structural problems. The lesson was that a hard limit just tells the agent where the limit is. The fix was to make the penalty a gradient, not a cliff.",
      },
      {
        title: "Without pressure, it took the easy road.",
        body: "Early runs showed a consistent pattern: the agent only widened roads. It would find a bottleneck, upgrade the segment, and call it done. The problem is that widening a road doesn't fix congestion. It moves it. Cars that couldn't get through one junction pile up at the next. The agent knew this, described it in its own reasoning, and did it anyway, because upgrading an existing road is reversible and cheap. Risk aversion looks like competence until you measure outcomes. The change-count penalty exists to force a commitment. This led to changing the scoring function to look at blocked junctions rather than overall flow rate or total metres of congestion.",
      },
    ],
  },
];
