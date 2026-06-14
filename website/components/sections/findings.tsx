import { LineChart, Users, EyeOff, Clock } from "lucide-react";
import { Card } from "@/components/ui/card";

export const Findings = () => (
  <section className="section" id="findings">
    <div className="wrap">
      <div className="section-head reveal">
        <p className="eyebrow">Findings</p>
        <h2 className="section-title">What the scores actually tell us.</h2>
        <p className="lead">Four models ran the same city under the same hidden scoring. The results didn&apos;t line up the way I expected, and the surprises all come back to the same thing.</p>
      </div>

      <div className="choices" style={{ gridTemplateColumns: "1fr" }}>
        <Card asChild className="choice reveal">
          <article>
            <span className="num">01</span>
            <div className="ico"><LineChart /></div>
            <h3>Model size didn&apos;t decide it</h3>
            <p>I expected the biggest models to come out on top. They didn&apos;t. Haiku, the smallest, did finish last. But Opus 4.8 is a flagship and it landed below Sonnet, which sits a tier under it. The thing that decided the order wasn&apos;t how clever the model was. It was <strong>whether it noticed the damage it was doing</strong> while it worked on the traffic.</p>
          </article>
        </Card>

        <Card asChild className="choice reveal">
          <article>
            <span className="num">02</span>
            <div className="ico"><Users /></div>
            <h3>Nobody lost on traffic. They lost on the city</h3>
            <p>Every model could move cars around. None of them could do it <strong>without emptying the place out</strong>. The population side of the score did almost all of the work. Fable left the city intact and scored 0.63. The other three lost residents, 9% and 15% and then a brutal 57%, and once the people were gone it stopped mattering what they had done to the traffic.</p>
          </article>
        </Card>

        <Card asChild className="choice reveal">
          <article>
            <span className="num">03</span>
            <div className="ico"><EyeOff /></div>
            <h3>The fix was what broke the city</h3>
            <p>This is the exact failure the benchmark was built to catch. Opus widened a road to a Large Road without checking that the wider road has a bigger footprint, and it flattened about 60 homes. Haiku bulldozed five highway ramps to push traffic somewhere else, then couldn&apos;t rebuild three of them, and cut an interchange in half. 1,238 buildings emptied out in a single step. Both of them were staring at the traffic and <strong>never asked what else the change was touching</strong>.</p>
          </article>
        </Card>

        <Card asChild className="choice reveal">
          <article>
            <span className="num">04</span>
            <div className="ico"><Clock /></div>
            <h3>Doing less was safer than doing harm</h3>
            <p>Sonnet barely touched the city. It made nine changes and still beat two models that went in hard and broke things. But doing less isn&apos;t the answer either. Fable made more changes than anyone, 197 of them, and it won. The difference was that it <strong>stepped the simulation forward and watched each batch settle</strong> before it moved on. The point was never to do nothing. It was to know what you had actually done.</p>
          </article>
        </Card>
      </div>
    </div>
  </section>
);
