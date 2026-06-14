import { notFound } from "next/navigation";
import type { Metadata } from "next";
import { runs, getRun } from "@/content/runs";
import { Nav } from "@/components/layout/nav";
import { Footer } from "@/components/layout/footer";
import { VideoPlayer } from "@/components/video-player";
import { BeforeAfterChart } from "@/components/charts/before-after-chart";
import { SettlingChart } from "@/components/charts/settling-chart";
import { SpendChart } from "@/components/charts/spend-chart";
import { ActionsChart } from "@/components/charts/actions-chart";
import { formatMillions } from "@/lib/format";

export const generateStaticParams = () => runs.map((r) => ({ slug: r.slug }));

export const generateMetadata = async ({
  params,
}: {
  params: Promise<{ slug: string }>;
}): Promise<Metadata> => {
  const { slug } = await params;
  const run = getRun(slug);
  return { title: run ? `${run.modelName} · SkylineBench run` : "SkylineBench run" };
};

const RunPage = async ({ params }: { params: Promise<{ slug: string }> }) => {
  const { slug } = await params;
  const run = getRun(slug);
  if (!run) notFound();

  const { metrics } = run;

  const congestedPct =
    metrics.congestedMetres.from === 0
      ? 0
      : Math.round(
          ((metrics.congestedMetres.to - metrics.congestedMetres.from) / metrics.congestedMetres.from) * 100,
        );
  const congestedSign = congestedPct > 0 ? "+" : "";

  return (
    <>
      <Nav variant="run" />

      <header className="run-hero">
        <div className="wrap-narrow">
          <p className="eyebrow">
            Run detail · <span className="mono">{run.map}</span>
          </p>
          <h1 className="display">{run.modelName}</h1>
          <div className="run-score">
            <span className="rs-val">{run.score.toFixed(2)}</span>
            <span className="rs-of">/ 1.00 composite</span>
          </div>
          <p className="lead">{run.verdict}</p>
        </div>
        <div className="wrap">
          <figure className="media-frame run-media">
            <VideoPlayer src={`/runs/${run.slug}.mp4`} autoplayOnView>
              <div className="media-placeholder">
                <div className="ph-title">timelapse</div>
              </div>
            </VideoPlayer>
          </figure>
        </div>
      </header>

      <section className="section">
        <div className="wrap">
          <div className="chips">
            <div className="chip">
              <span className="chip-v">{metrics.flow.from} → {metrics.flow.to}</span>
              <span className="chip-l">flow</span>
            </div>
            <div className="chip">
              <span className="chip-v">{congestedSign}{congestedPct}%</span>
              <span className="chip-l">congested metres</span>
            </div>
            <div className="chip">
              <span className="chip-v">{metrics.jammedJunctions.from} → {metrics.jammedJunctions.to}</span>
              <span className="chip-l">jammed junctions</span>
            </div>
            <div className="chip">
              <span className="chip-v">
                {metrics.population.from.toLocaleString("en-US")} → {metrics.population.to.toLocaleString("en-US")}
              </span>
              <span className="chip-l">population</span>
            </div>
            <div className="chip">
              <span className="chip-v">{metrics.changes}</span>
              <span className="chip-l">changes</span>
            </div>
            <div className="chip">
              <span className="chip-v">{formatMillions(metrics.spend)}</span>
              <span className="chip-l">spent</span>
            </div>
          </div>

          <div className="chart-grid">
            <BeforeAfterChart
              rows={[
                { label: "Flow", base: metrics.flow.from, final: metrics.flow.to },
                {
                  label: "Congested m",
                  base: metrics.congestedMetres.from,
                  final: metrics.congestedMetres.to,
                  format: (n) => n.toLocaleString("en-US"),
                },
                {
                  label: "Jammed junctions",
                  base: metrics.jammedJunctions.from,
                  final: metrics.jammedJunctions.to,
                },
                {
                  label: "Active vehicles",
                  base: metrics.activeVehicles.from,
                  final: metrics.activeVehicles.to,
                  format: (n) => n.toLocaleString("en-US"),
                },
                {
                  label: "Population",
                  base: metrics.population.from,
                  final: metrics.population.to,
                  format: (n) => n.toLocaleString("en-US"),
                },
              ]}
            />
            <SettlingChart base={run.flowSettling.base} final={run.flowSettling.final} />
            <SpendChart
              series={run.spendSeries}
              total={metrics.spend}
              changes={metrics.changes}
            />
            <ActionsChart actions={run.actions} />
          </div>
        </div>
      </section>

      <section className="section section-soft">
        <div className="wrap-narrow">
          <div className="section-head">
            <p className="eyebrow">What the agent did</p>
            <h2 className="section-title">Step by step.</h2>
          </div>
          <ol className="timeline">
            {run.beats.map((beat) => (
              <li key={beat.title} className="beat">
                <h3>{beat.title}</h3>
                {beat.body.split(/\n\s*\n/).map((paragraph, i) => (
                  <p key={i}>{paragraph.trim()}</p>
                ))}
              </li>
            ))}
          </ol>
        </div>
      </section>

      <Footer links={false} />
    </>
  );
};

export default RunPage;
