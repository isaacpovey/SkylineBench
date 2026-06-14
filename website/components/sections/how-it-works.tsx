import { EyeOff, Building2, Clock, Lock } from "lucide-react";

export const HowItWorks = () => (
  <section className="section section-soft" id="how">
    <div className="wrap">
      <div className="section-head reveal">
        <p className="eyebrow">How it works</p>
        <h2 className="section-title">The agent plays the game through tools, the same moves a human player has.</h2>
        <p className="lead">It looks at the map, inspects the traffic on any road, traces where cars are actually going, then bulldozes, builds, upgrades roads, and rezones. It can pause time, make a batch of changes, and step the simulation forward to watch what they do. It gets a few hours of wall-clock time, then submits and walks away.</p>
      </div>

      <div className="tools reveal">
        <div className="tool-group">
          <h4>Observe</h4>
          <ul>
            <li>get_city_overview</li>
            <li>observe_area</li>
            <li>render_map</li>
            <li>get_metrics</li>
          </ul>
        </div>
        <div className="tool-group">
          <h4>Act</h4>
          <ul>
            <li>build_road</li>
            <li>bulldoze</li>
            <li>upgrade_road</li>
            <li>set_zoning</li>
          </ul>
        </div>
        <div className="tool-group">
          <h4>Reference</h4>
          <ul>
            <li>list_road_types</li>
            <li>list_zone_types</li>
          </ul>
        </div>
        <div className="tool-group">
          <h4>Control</h4>
          <ul>
            <li>control_time</li>
            <li>reset_scenario</li>
          </ul>
        </div>
      </div>

      <p className="lead reveal" style={{ marginTop: "clamp(56px,8vw,80px)", maxWidth: "640px" }}>A handful of deliberate choices decide what it&apos;s <em>really</em> being tested on.</p>

      <div className="choices">
        <article className="choice reveal">
          <span className="num">01</span>
          <div className="ico"><EyeOff /></div>
          <h3>It never sees the score</h3>
          <p>The agent is told, in plain language, to make traffic flow better while keeping the city somewhere people want to live. It is <strong>never shown the formula, the weights, or the thresholds.</strong> There&apos;s no scoreboard to play to. The only way to score well is to leave the city better than it found it.</p>
        </article>

        <article className="choice reveal">
          <span className="num">02</span>
          <div className="ico"><Building2 /></div>
          <h3>It can&apos;t win by bulldozing the city</h3>
          <p>Congestion has a trivial solution: demolish everything until there&apos;s no one left to drive. So the congestion score is <strong>multiplied by a health factor tied to population.</strong> Let the city hollow out and your gains evaporate with the residents. The two pressures pull against each other on purpose.</p>
        </article>

        <article className="choice reveal">
          <span className="num">03</span>
          <div className="ico"><Clock /></div>
          <h3>It has to slow down</h3>
          <p>Traffic doesn&apos;t re-route the instant you change a road. It gets worse for a while as cars find the new layout, then settles. A good change and a bad change <strong>look identical for the first few steps</strong>, so the agent has to tell a settling transient apart from real damage instead of reacting to the first number it sees. Patience is part of the test.</p>
        </article>

        <article className="choice reveal">
          <span className="num">04</span>
          <div className="ico"><Lock /></div>
          <h3>It can&apos;t read the answer key</h3>
          <p>The agent runs inside a sandbox that blocks it from reading this repository, so it can&apos;t inspect the scoring code. It can only play the game through the tools. <strong>An early run did exactly this</strong>, which is why the sandbox exists.</p>
        </article>
      </div>
    </div>
  </section>
);
