import type { Metadata } from "next";
import { Nav } from "@/components/layout/nav";
import { Footer } from "@/components/layout/footer";
import { Card } from "@/components/ui/card";
import { updates } from "@/content/updates";

export const metadata: Metadata = { title: "Updates · SkylineBench" };

const UpdatesPage = () => (
  <>
    <Nav variant="run" />
    <header className="run-hero">
      <div className="wrap-narrow">
        <p className="eyebrow">Updates</p>
        <h1 className="display">What changed, and what we learned.</h1>
      </div>
    </header>
    {updates.map((entry) => (
      <section className="section" key={entry.title}>
        <div className="wrap">
          <div className="section-head reveal">
            <p className="eyebrow">{entry.date}</p>
            <h2 className="section-title">{entry.title}</h2>
            {entry.intro && <p className="lead">{entry.intro}</p>}
          </div>
          <div className="choices" style={{ gridTemplateColumns: "1fr" }}>
            {entry.cards.map((card, i) => (
              <Card asChild className="choice reveal" key={card.title}>
                <article>
                  <span className="num">{String(i + 1).padStart(2, "0")}</span>
                  <h3>{card.title}</h3>
                  <p>{card.body}</p>
                </article>
              </Card>
            ))}
          </div>
        </div>
      </section>
    ))}
    <Footer links={false} />
  </>
);

export default UpdatesPage;
