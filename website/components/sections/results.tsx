import { Clock } from "lucide-react";
import { currentLeaderboard } from "@/lib/leaderboards";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { LeaderboardTable } from "@/components/leaderboard-table";

export const Results = () => (
  <section className="section section-soft" id="results">
    <div className="wrap">
      <div className="results-head reveal">
        <div className="section-head" style={{ margin: 0 }}>
          <p className="eyebrow">Results</p>
          <h2 className="section-title">How the models did.</h2>
          <p className="lead">
            Every model runs the same <span className="mono">{currentLeaderboard.map}</span> scenario under
            identical scoring on harness <span className="mono">{currentLeaderboard.harnessVersion}</span>, ranked
            by composite score. Open a run to see how it got there.
          </p>
        </div>
        <Button asChild variant="outline" size="sm">
          <a href="/results">All leaderboards &#x2192;</a>
        </Button>
      </div>

      <LeaderboardTable board={currentLeaderboard} />

      <Card asChild className="coming-soon reveal">
        <div>
          <span className="cs-ico"><Clock /></span>
          <div>
            <h4>More models, coming soon</h4>
            <p>Other frontier models will run the same {currentLeaderboard.map} scenario under identical scoring. Their results land here as the runs complete.</p>
          </div>
        </div>
      </Card>
    </div>
  </section>
);
