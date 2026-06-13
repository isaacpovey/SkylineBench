# Learnings section — draft copy

> Edit this file, then ask Claude to push it into the HTML.

---

## Eyebrow
Learnings

## Section title
AI is crafty... and lazy

## Lead
Pretty much every design decision in the prompt, the scoring, and the sandbox came from something the agent broke first.

---

## 01 — It read the answer key

The first run had no sandbox. The agent noticed it was running in the same directory as the repository, found the harness code, read the scoring function, and sidesteped the benchmark. Its solution, delete everything: No city, no traffic. **A perfect congestion score.** It took about five minutes to find the loophole I hadn't thought to close. This is why the sandbox exists.

---

## 02 — When you close a loophole, it finds the margin.

The population floor was the first version of this fix. So I added a floor that the population couldn't fall bellow whcih was supplied in the prompt. The agent found the floor and **parked exactly on it.** It reduced the minimum viable population and held it there, treating the floor as a target rather than a guardrail. Since it figured this was easier than fixing the actual structural problems. The lesson was that a hard limit just tells the agent where the limit is. The fix was to make the penalty a gradient, not a cliff.

---

## 03 — Without pressure, it took the easy road.

Early runs showed a consistent pattern: the agent only widened roads. It would find a bottleneck, upgrade the segment, and call it done. The problem is that widening a road doesn't fix congestion, **it moves it.** Cars that couldn't get through one junction pile up at the next. The agent knew this, described it in its own reasoning, and did it anyway, because upgrading an existing road is reversible and cheap. Risk aversion looks like competence until you measure outcomes. The change-count penalty exists to force a commitment. This lead to changing the scoring function to look at blocked junctions rather than overall flow rate or total meters of conggestion.

---
