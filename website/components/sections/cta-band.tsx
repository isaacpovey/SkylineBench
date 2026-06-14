import { Button } from "@/components/ui/button";
import { GitHub } from "@/components/icons/github";
import { LinkedIn } from "@/components/icons/linkedin";
import { Mail } from "@/components/icons/mail";

export const CtaBand = () => (
  <section className="section cta-band">
    <div className="wrap-narrow">
      <p className="eyebrow reveal" style={{ justifyContent: "center" }}>Get involved</p>
      <h2 className="section-title reveal" style={{ marginTop: 18 }}>It&apos;s open source. Drop an agent into a city and watch what it breaks.</h2>
      <p className="lead reveal">You&apos;ll need Cities: Skylines 1, Rust, and Mono to build the mod. The full scoring, artifacts, and mod API live in the component READMEs.</p>
      <p className="lead reveal">Have an idea for a harness improvement, a new tool, a great CS map to test on, or a model you want benchmarked? Email me or reach out on LinkedIn. Contributions on GitHub are always welcome.</p>
      <div className="row reveal">
        <Button asChild variant="primary">
          <a href="mailto:skylinebench@isaacpovey.dev">
            <Mail />
            Email me
          </a>
        </Button>
        <Button asChild variant="outline">
          <a href="https://www.linkedin.com/in/isaacpovey/" target="_blank" rel="noopener">
            <LinkedIn />
            Connect on LinkedIn
          </a>
        </Button>
        <Button asChild variant="outline">
          <a href="https://github.com/isaacpovey/SkylineBench" target="_blank" rel="noopener">
            <GitHub />
            Contribute on GitHub
          </a>
        </Button>
      </div>
    </div>
  </section>
);
