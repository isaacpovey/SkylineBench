import { Layers } from "lucide-react";
import type { Leaderboard } from "@/lib/leaderboards";
import { formatDelta, formatMillions, percentChange } from "@/lib/format";
import { Card } from "@/components/ui/card";

export const LeaderboardTable = ({ board }: { board: Leaderboard }) => (
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
);
