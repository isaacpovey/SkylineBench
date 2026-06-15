import type { Metadata } from "next";
import { Nav } from "@/components/layout/nav";
import { Footer } from "@/components/layout/footer";
import { Card } from "@/components/ui/card";
import { changelog } from "@/content/changelog";

export const metadata: Metadata = { title: "Changelog · SkylineBench" };

const ChangelogPage = () => (
  <>
    <Nav variant="run" />
    <header className="run-hero">
      <div className="wrap-narrow">
        <p className="eyebrow">Changelog</p>
        <h1 className="display">What changed between versions.</h1>
      </div>
    </header>
    <section className="section">
      <div className="wrap-narrow">
        <ol className="timeline">
          {changelog.map((entry) => (
            <Card asChild className="beat" key={entry.version}>
              <li>
                <h3>{entry.version} <span className="mono">· {entry.date}</span></h3>
                <p>{entry.summary}</p>
                <ul>
                  {entry.changes.map((change) => (
                    <li key={change}>{change}</li>
                  ))}
                </ul>
              </li>
            </Card>
          ))}
        </ol>
      </div>
    </section>
    <Footer links={false} />
  </>
);

export default ChangelogPage;
