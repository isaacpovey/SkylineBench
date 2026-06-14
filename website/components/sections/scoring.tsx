import { EyeOff, LineChart, Shuffle, Users } from "lucide-react";
import { Card } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";

export const Scoring = () => (
  <section className="section" id="scoring">
    <div className="wrap">
      <div className="section-head reveal">
        <p className="eyebrow">Scoring</p>
        <h2 className="section-title">A formula the operator can see, and the agent never can.</h2>
        <p className="lead">The prompt frames the task as &quot;optimise this city&apos;s traffic simulation&quot; and states its objectives qualitatively. It is deliberately <strong>not</strong> told the formula, the weights, the caps, or the population thresholds, so it optimises the city, not the scoreboard.</p>
      </div>

      <div className="score-layout">
        <Card asChild className="formula-card reveal">
          <div>
            <div className="fc-head">
              <span className="t">score.json · composite</span>
              <Badge asChild className="hidden-badge">
                <span><EyeOff /> hidden from agent</span>
              </Badge>
            </div>
            <div className="formula-body">
              <div className="formula">
                <span className="term">score</span> =<br />
                (<span className="w">0.60</span>·congestion_reward<br />
                + <span className="w">0.20</span>·(1−norm(money))<br />
                + <span className="w">0.20</span>·(1−norm(changes)))<br />
                · <span className="health">health</span>
              </div>
              <div className="formula-legend">
                <div className="row"><span className="k">congestion_reward</span><span className="v">blend of metres-reduced and congested-junctions-reduced (0.5 / 0.5).</span></div>
                <div className="row"><span className="k">congested</span><span className="v">road density ≥ 0.7; a junction of degree ≥ 3 with ≥ 2 congested segments.</span></div>
                <div className="row"><span className="k health">health</span><span className="v">graded population factor: 1.0 at ≥ 95% of baseline, 0.0 at ≤ 75%, linear between.</span></div>
                <div className="row"><span className="k">norm</span><span className="v">money against a $10M budget; changes against a 300-change cap.</span></div>
              </div>
            </div>
          </div>
        </Card>

        <div className="score-notes">
          <Card asChild className="score-note reveal">
            <div>
              <div className="ico"><LineChart /></div>
              <div>
                <h4>Congestion is 60% of the weight</h4>
                <p>Reward comes from cutting the total length of jammed road and the number of jammed junctions versus a measured baseline, never from an absolute number the agent could chase.</p>
              </div>
            </div>
          </Card>
          <Card asChild className="score-note reveal">
            <div>
              <div className="ico"><Shuffle /></div>
              <div>
                <h4>Cost and restraint matter</h4>
                <p>Money spent and number of changes each carry 20%. A surgical fix beats a sprawling rebuild that happens to land the same congestion number.</p>
              </div>
            </div>
          </Card>
          <Card asChild className="score-note reveal">
            <div>
              <div className="ico"><Users /></div>
              <div>
                <h4>The population multiplier governs everything</h4>
                <p>Health multiplies the whole score, so depopulating the city drags it down smoothly rather than off a cliff. A run is invalid <span className="mono">(score 0)</span> only when the baseline has no congestion to fix.</p>
              </div>
            </div>
          </Card>
        </div>
      </div>
    </div>
  </section>
);
