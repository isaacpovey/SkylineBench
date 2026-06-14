import { Layers, Clock } from "lucide-react";
import { runs } from "@/content/runs";
import { formatDelta, percentChange } from "@/lib/format";
import { Card } from "@/components/ui/card";

export const Results = () => (
  <section className="section section-soft" id="results">
    <div className="wrap">
      <div className="results-head reveal">
        <div className="section-head" style={{ margin: 0 }}>
          <p className="eyebrow">Results</p>
          <h2 className="section-title">How the models did.</h2>
          <p className="lead">Every model runs the same <span className="mono">gridlock-v1</span> scenario under identical scoring, ranked by composite score. Open a run to see how it got there.</p>
        </div>
      </div>

      <div className="results-grid">
        {runs.map((run, index) => {
          const flowGood = percentChange(run.metrics.flow) > 0;
          const popGood = percentChange(run.metrics.population) >= 0;
          return (
            <Card asChild className="result-card reveal" key={run.slug}>
              <a href={`/runs/${run.slug}`}>
                <div className="result-body">
                  <div className="result-top">
                    <div className="result-model">
                      <span className="result-rank">{index + 1}</span>
                      <span className="mico"><Layers /></span>
                      <span className="name">{run.modelName}<small>{run.map}</small></span>
                    </div>
                    <span className="status-pill scored">view run &#x2192;</span>
                  </div>
                  <div className="result-score">
                    <span className="val scored">{run.score.toFixed(2)}</span>
                    <span className="of">/ 1.00</span>
                    <span className="result-metrics">
                      <span className="metric"><span className={`m-val ${flowGood ? "good" : "bad"}`}>{formatDelta(run.metrics.flow)}</span><span className="m-lbl">flow</span></span>
                      <span className="metric"><span className={`m-val ${popGood ? "good" : "bad"}`}>{formatDelta(run.metrics.population)}</span><span className="m-lbl">population</span></span>
                    </span>
                  </div>
                </div>
              </a>
            </Card>
          );
        })}
      </div>

      <Card asChild className="coming-soon reveal">
        <div>
          <span className="cs-ico"><Clock /></span>
          <div>
            <h4>Non-Anthropic models, coming soon</h4>
            <p>Other frontier models will run the same gridlock-v1 scenario under identical scoring. Their results land here as the runs complete.</p>
          </div>
        </div>
      </Card>
    </div>
  </section>
);
