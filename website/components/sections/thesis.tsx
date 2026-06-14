import { ArrowRight } from "lucide-react";

export const Thesis = () => (
  <section className="section" id="thesis">
    <div className="wrap-narrow">
      <div className="section-head reveal">
        <p className="eyebrow">Why I built this</p>
        <h2 className="section-title">Most agent benchmarks have a right answer. This one doesn&apos;t.</h2>
      </div>
      <div className="prose reveal" style={{ marginTop: 32 }}>
        <p>
          I have a theory:{" "}
          <strong>agents are bad at the second-order consequences of their own actions.</strong>{" "}
          I keep running into the same failure in my own engineering work. The moment an agent
          believes it has a solution, it stops thinking. It ships the fix and never asks what else
          the fix touched.
        </p>
        <p>
          A city is about the cruelest test of that I could think of, because in a city{" "}
          <em>everything</em> is connected.
        </p>
      </div>

      <div className="cascade reveal">
        <span className="step">Widen a road</span>
        <span className="arrow"><ArrowRight /></span>
        <span className="step">more cars</span>
        <span className="arrow"><ArrowRight /></span>
        <span className="step">more noise</span>
        <span className="arrow"><ArrowRight /></span>
        <span className="step">residents leave</span>
        <span className="arrow"><ArrowRight /></span>
        <span className="step">shops close</span>
        <span className="arrow"><ArrowRight /></span>
        <span className="step bad">no traffic, no city</span>
      </div>

      <div className="prose reveal" style={{ marginTop: 36 }}>
        <p>
          The agent that widened the road got exactly what it asked for and lost the city doing it.
          That cascade is the whole point.
        </p>
      </div>

      <blockquote className="thesis-quote reveal">
        <p>
          The benchmark isn&apos;t really asking whether an agent can read a congestion number and
          bring it down. It&apos;s asking whether the agent keeps reasoning after it thinks
          it&apos;s done.
        </p>
      </blockquote>
    </div>
  </section>
);
