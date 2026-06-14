import { Monitor, Server, Bot, RefreshCw } from "lucide-react";

export const Architecture = () => (
  <section className="section section-soft" id="built">
    <div className="wrap">
      <div className="section-head reveal">
        <p className="eyebrow">How it&apos;s built</p>
        <h2 className="section-title">Three pieces between the game and the agent.</h2>
        <p className="lead">A C# mod exposes the live simulation. A Rust MCP server turns it into agent tools and runs the harness. The benchmark layer holds the prompt, the maps, and the run script.</p>
      </div>

      <div className="arch reveal">
        <div className="arch-flow">
          <div className="arch-node">
            <div className="ico"><Monitor /></div>
            <span className="tag">mod/ <span className="lang">· C#</span></span>
            <h3>The game</h3>
            <p>A mod for Cities: Skylines 1 that runs inside the game and exposes the simulation&apos;s state and controls over a localhost HTTP API.</p>
          </div>

          <div className="arch-conn">
            <span className="label">HTTP</span>
            <span className="line" />
            <span className="sub">:8787</span>
          </div>

          <div className="arch-node">
            <div className="ico"><Server /></div>
            <span className="tag">broker/ <span className="lang">· Rust</span></span>
            <h3>The harness</h3>
            <p>An MCP server. It turns the game into agent tools and runs the harness: measure a baseline, run the agent, let the sim settle, score it, and write out the artifacts.</p>
          </div>

          <div className="arch-conn">
            <span className="label">MCP</span>
            <span className="line" />
            <span className="sub">tools</span>
          </div>

          <div className="arch-node">
            <div className="ico"><Bot /></div>
            <span className="tag">benchmark/ <span className="lang">· agent</span></span>
            <h3>The run</h3>
            <p>The prompt the agent sees, the run script, and the maps. The agent works inside a Seatbelt sandbox that blocks it from reading the repo.</p>
          </div>
        </div>

        <div className="arch-loop">
          <span className="ico"><RefreshCw /></span>
          <p><strong>Observe → act → step the sim → re-measure.</strong> The agent loops through the tools for hours of wall-clock time, watching changes settle, until it submits a solution or the clock runs out. Then the broker settles, scores, and writes <span className="mono">score.json</span>, the transcript, renders, and the timelapse.</p>
        </div>
      </div>
    </div>
  </section>
);
