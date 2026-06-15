import type { Metadata } from "next";
import { Nav } from "@/components/layout/nav";
import { Footer } from "@/components/layout/footer";
import { leaderboards } from "@/lib/leaderboards";
import { LeaderboardTable } from "@/components/leaderboard-table";

export const metadata: Metadata = { title: "Results · SkylineBench" };

const ResultsPage = () => (
  <>
    <Nav variant="run" />
    <header className="run-hero">
      <div className="wrap-narrow">
        <p className="eyebrow">Results</p>
        <h1 className="display">Leaderboards.</h1>
        <p className="lead">
          Each scenario and harness version is scored independently. Open a run to see how it got there.
        </p>
      </div>
    </header>
    {leaderboards.map((board) => (
      <section className="section" key={board.label}>
        <div className="wrap">
          <div className="section-head reveal">
            <p className="eyebrow">Leaderboard</p>
            <h2 className="section-title">
              {board.map} <span className="mono">· {board.harnessVersion}</span>
            </h2>
          </div>
          <LeaderboardTable board={board} />
        </div>
      </section>
    ))}
    <Footer links={false} />
  </>
);

export default ResultsPage;
