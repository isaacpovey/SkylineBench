import { Building2 } from "lucide-react";

export const Roadmap = () => (
  <section className="section" id="future">
    <div className="wrap-narrow">
      <div className="section-head reveal">
        <p className="eyebrow">Where this is going</p>
        <h2 className="section-title">A roadmap toward a city built from scratch.</h2>
        <p className="lead">Right now the agent inherits a city and repairs it. Repairing someone else&apos;s mistakes is the warm-up. Each step below hands it more rope.</p>
      </div>

      <ol className="roadmap reveal">
        <li className="rm-step">
          <span className="n">1</span>
          <div className="rm-body">
            <h4>Run the benchmark on more models</h4>
            <p>Extend the run script so it drives agents beyond the Claude line, all on the same hidden scoring.</p>
          </div>
        </li>
        <li className="rm-step">
          <span className="n">2</span>
          <div className="rm-body">
            <h4>Find harder maps</h4>
            <p>Source bigger, messier, more tangled cities so a quick fix can&apos;t paper over the real problems.</p>
          </div>
        </li>
        <li className="rm-step">
          <span className="n">3</span>
          <div className="rm-body">
            <h4>Give the agent more traffic tools</h4>
            <p>Add levers beyond roads, like public transport, so it can move people without only moving cars.</p>
          </div>
        </li>
        <li className="rm-step">
          <span className="n">4</span>
          <div className="rm-body">
            <h4>Introduce the rest of the city</h4>
            <p>Open up rezoning, education, healthcare, and the other systems that decide whether a city actually works.</p>
          </div>
        </li>
        <li className="rm-step">
          <span className="n">5</span>
          <div className="rm-body">
            <h4>Add a multi-agent mode</h4>
            <p>Split the city between agents that each own a district and have to communicate, all working toward one shared goal.</p>
          </div>
        </li>
      </ol>

      <div className="rm-goal reveal">
        <span className="goal-mark"><Building2 /></span>
        <span className="badge-soon">The destination</span>
        <h3>Hand it empty land.</h3>
        <p>The version I actually want is harder: hand the agent empty land and have it build and run a whole city from scratch, balancing budgets, population growth, taxation, happiness, and the environment.</p>
      </div>
    </div>
  </section>
);
