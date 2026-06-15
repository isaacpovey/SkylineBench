"use client";

import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { BrandMark } from "@/components/icons/brand-mark";
import { GitHub } from "@/components/icons/github";
import { LinkedIn } from "@/components/icons/linkedin";
import { Mail } from "@/components/icons/mail";
import { navSections } from "@/lib/nav-sections";

type NavProps = {
  variant?: "landing" | "run";
};

export const Nav = ({ variant = "landing" }: NavProps) => {
  const [scrolled, setScrolled] = useState(false);

  useEffect(() => {
    const update = () => setScrolled(window.scrollY > 8);
    update();
    window.addEventListener("scroll", update, { passive: true });
    return () => window.removeEventListener("scroll", update);
  }, []);

  const brandHref = variant === "run" ? "/#top" : "#top";

  return (
    <nav className="nav" data-scrolled={scrolled}>
      <div className="wrap">
        <a className="brand" href={brandHref}>
          <BrandMark className="mark" />
          <span>
            <b>Skyline</b>
            <span className="slash">Bench</span>
          </span>
        </a>
        <div className="nav-links">
          {variant === "landing" ? (
            <>
              {navSections.map((section) => (
                <a key={section.href} className="nav-link hide-sm" href={section.href}>
                  {section.label}
                </a>
              ))}
              <span className="nav-sep hide-sm" />
              <Button asChild variant="outline" size="icon">
                <a
                  href="mailto:skylinebench@isaacpovey.dev"
                  aria-label="Email me"
                  title="Email me"
                >
                  <Mail />
                </a>
              </Button>
              <Button asChild variant="outline" size="icon">
                <a
                  href="https://www.linkedin.com/in/isaacpovey/"
                  target="_blank"
                  rel="noopener"
                  aria-label="Connect on LinkedIn"
                  title="Connect on LinkedIn"
                >
                  <LinkedIn />
                </a>
              </Button>
            </>
          ) : (
            <>
              <a className="nav-link" href="/#results">← Back to results</a>
              <a className="nav-link hide-sm" href="/updates">Updates</a>
              <a className="nav-link hide-sm" href="/changelog">Changelog</a>
            </>
          )}
          <Button asChild variant="outline" size="sm">
            <a
              href="https://github.com/isaacpovey/SkylineBench"
              target="_blank"
              rel="noopener"
            >
              <GitHub />
              GitHub
            </a>
          </Button>
        </div>
      </div>
    </nav>
  );
};
