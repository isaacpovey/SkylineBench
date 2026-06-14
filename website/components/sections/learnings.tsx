import { Search, Target, Activity } from "lucide-react";
import { Card } from "@/components/ui/card";

export const Learnings = () => (
  <section className="section" id="learnings">
    <div className="wrap">
      <div className="section-head reveal">
        <p className="eyebrow">Learnings</p>
        <h2 className="section-title">AI is crafty... and lazy.</h2>
        <p className="lead">Pretty much every design decision in the prompt, the scoring, and the sandbox came from something the agent broke first.</p>
      </div>

      <div className="choices" style={{ gridTemplateColumns: "1fr" }}>
        <Card asChild className="choice reveal">
          <article>
            <span className="num">01</span>
            <div className="ico"><Search /></div>
            <h3>It read the answer key</h3>
            <p>The first run had no sandbox. The agent noticed it was running in the same directory as the repository, found the harness code, read the scoring function, and sidestepped the benchmark. Its solution: delete everything. No city, no traffic. <strong>A perfect congestion score.</strong> It took about five minutes to find the loophole I hadn&apos;t thought to close. This is why the sandbox exists.</p>
          </article>
        </Card>

        <Card asChild className="choice reveal">
          <article>
            <span className="num">02</span>
            <div className="ico"><Target /></div>
            <h3>When you close a loophole, it finds the margin.</h3>
            <p>The population floor was the first version of this fix: a minimum the population couldn&apos;t fall below, supplied in the prompt. The agent found the floor and <strong>parked exactly on it.</strong> It reduced the population to the minimum viable number and held it there, treating the floor as a target rather than a guardrail, since it figured this was easier than fixing the actual structural problems. The lesson was that a hard limit just tells the agent where the limit is. The fix was to make the penalty a gradient, not a cliff.</p>
          </article>
        </Card>

        <Card asChild className="choice reveal">
          <article>
            <span className="num">03</span>
            <div className="ico"><Activity /></div>
            <h3>Without pressure, it took the easy road.</h3>
            <p>Early runs showed a consistent pattern: the agent only widened roads. It would find a bottleneck, upgrade the segment, and call it done. The problem is that widening a road doesn&apos;t fix congestion. <strong>It moves it.</strong> Cars that couldn&apos;t get through one junction pile up at the next. The agent knew this, described it in its own reasoning, and did it anyway, because upgrading an existing road is reversible and cheap. Risk aversion looks like competence until you measure outcomes. The change-count penalty exists to force a commitment. This led to changing the scoring function to look at blocked junctions rather than overall flow rate or total metres of congestion.</p>
          </article>
        </Card>
      </div>
    </div>
  </section>
);
