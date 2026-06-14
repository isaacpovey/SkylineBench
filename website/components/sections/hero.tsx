import { Play } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { GitHub } from "@/components/icons/github";
import { VideoPlayer } from "@/components/video-player";
import { runs } from "@/content/runs";
import { formatDelta, formatMillions } from "@/lib/format";

export const Hero = () => {
  const best = runs[0];

  return (
    <header className="hero">
      <div className="hero-grid-bg" />
      <div className="hero-glow" />
      <div className="wrap">
        <div className="hero-head">
          <p className="eyebrow reveal">A benchmark for AI agents</p>
          <h1 className="display reveal">Fix the traffic.<br />Don&apos;t kill the city.</h1>
          <p className="lead reveal">
            SkylineBench drops an AI agent into a congested <strong>Cities: Skylines</strong> city
            and asks it to improve the traffic,{" "}
            <strong>without ever telling it how it&apos;s being judged.</strong>
          </p>
          <div className="hero-cta reveal">
            <Button asChild variant="primary">
              <a
                href="https://github.com/isaacpovey/SkylineBench"
                target="_blank"
                rel="noopener"
              >
                <GitHub />
                View on GitHub
              </a>
            </Button>
            <Button asChild variant="outline">
              <a href="#thesis">Read the thesis</a>
            </Button>
          </div>
          <div className="hero-meta reveal">
            <span>Cities: Skylines 1</span>
            <span className="dot" />
            <span>Rust MCP harness</span>
            <span className="dot" />
            <span className="mono">no right answer</span>
          </div>
        </div>

        <Card asChild className="media-frame reveal" style={{ marginLeft: 0, marginRight: 0 }}>
          <figure>
            <div className="media-bar">
              <span className="media-dot" />
              <span className="media-dot" />
              <span className="media-dot" />
              <span className="file">skylinebench timelapse · gridlock-v1</span>
              <span className="right"><span className="live" /> annotated run</span>
            </div>
            <VideoPlayer src={`/runs/${best.slug}.mp4`} autoplayOnView>
              <div className="media-placeholder">
                <div className="play"><Play /></div>
                <div className="ph-title">{best.modelName} · {best.map}</div>
                <div className="ph-sub">the best run so far, annotated</div>
              </div>
            </VideoPlayer>
            <div className="media-foot">
              <a className="mf-label" href={`/runs/${best.slug}`}>
                Best run so far · {best.modelName} &#x2192;
              </a>
              <span className="mf-stats">
                <span><b>{best.score.toFixed(2)}</b> score</span>
                <span><b className="good">{formatDelta(best.metrics.congestedMetres)}</b> congestion</span>
                <span><b className="good">{formatDelta(best.metrics.jammedJunctions)}</b> junctions</span>
                <span><b className="bad">{formatDelta(best.metrics.population)}</b> population</span>
                <span><b>{formatMillions(best.metrics.spend)}</b> spent</span>
              </span>
            </div>
          </figure>
        </Card>
      </div>
    </header>
  );
};
