"use client";

import { useState } from "react";
import { Layers, Clock } from "lucide-react";
import { leaderboards, currentLeaderboard } from "@/lib/leaderboards";
import { formatDelta, formatMillions, percentChange } from "@/lib/format";
import { Card } from "@/components/ui/card";

export const Results = () => {
  const [selected, setSelected] = useState(currentLeaderboard.label);
  const board = leaderboards.find((b) => b.label === selected) ?? currentLeaderboard;

  return (
    <section className="section section-soft" id="results">
      <div className="wrap">
        <div className="results-head reveal">
          <div className="section-head" style={{ margin: 0 }}>
            <p className="eyebrow">Results</p>
            <h2 className="section-title">How the models did.</h2>
            <p className="lead">
              Every model runs the same <span className="mono">{board.map}</span> scenario under
              identical scoring on harness <span className="mono">{board.harnessVersion}</span>, ranked
              by composite score. Open a run to see how it got there.
            </p>
          </div>
          {leaderboards.length > 1 && (
            <label className="leaderboard-select">
              <span className="hide-sm">Leaderboard</span>
              <select value={selected} onChange={(e) => setSelected(e.target.value)}>
                {leaderboards.map((b) => (
                  <option key={b.label} value={b.label}>{b.label}</option>
                ))}
              </select>
            </label>
          )}
        </div>

        <div className="results-grid">
          {board.runs.map((run, index) => {
            const junctionsGood = percentChange(run.metrics.jammedJunctions) < 0;
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
                        <span className="metric"><span className={`m-val ${junctionsGood ? "good" : "bad"}`}>{formatDelta(run.metrics.jammedJunctions)}</span><span className="m-lbl">junctions</span></span>
                        <span className="metric"><span className={`m-val ${popGood ? "good" : "bad"}`}>{formatDelta(run.metrics.population)}</span><span className="m-lbl">population</span></span>
                        <span className="metric"><span className="m-val">{formatMillions(run.metrics.spend)}</span><span className="m-lbl">spent</span></span>
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
              <h4>More models, coming soon</h4>
              <p>Other frontier models will run the same {board.map} scenario under identical scoring. Their results land here as the runs complete.</p>
            </div>
          </div>
        </Card>
      </div>
    </section>
  );
};
