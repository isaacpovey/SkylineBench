import { BrandMark } from "@/components/icons/brand-mark";
import { GitHub } from "@/components/icons/github";
import { LinkedIn } from "@/components/icons/linkedin";
import { Mail } from "@/components/icons/mail";
import { navSections } from "@/lib/nav-sections";

type FooterProps = {
  links?: boolean;
};

export const Footer = ({ links = true }: FooterProps) => (
  <footer className="footer">
    {links && (
      <div className="wrap">
        <div className="f-left">
          <a className="brand" href="#top">
            <BrandMark className="mark" />
            <span>
              <b>Skyline</b>
              <span className="slash">Bench</span>
            </span>
          </a>
          <p className="f-tag">
            A benchmark that asks whether an agent keeps reasoning after it thinks it&apos;s done.
          </p>
        </div>
        <div className="f-links">
          <div className="f-col">
            <h5>Project</h5>
            {navSections.map((section) => (
              <a key={section.href} href={section.href}>
                {section.label}
              </a>
            ))}
          </div>
          <div className="f-col">
            <h5>Links</h5>
            <a href="mailto:skylinebench@isaacpovey.dev">
              <Mail />
              Email
            </a>
            <a
              href="https://github.com/isaacpovey/SkylineBench"
              target="_blank"
              rel="noopener"
            >
              <GitHub />
              GitHub repository
            </a>
            <a
              href="https://www.linkedin.com/in/isaacpovey/"
              target="_blank"
              rel="noopener"
            >
              <LinkedIn />
              LinkedIn
            </a>
          </div>
        </div>
      </div>
    )}
    <div className="wrap">
      <div className="footer-base">
        <span>
          Built by{" "}
          <a
            href="https://www.linkedin.com/in/isaacpovey/"
            target="_blank"
            rel="noopener"
            style={{ textDecoration: "underline", textUnderlineOffset: "3px" }}
          >
            Isaac Povey
          </a>
        </span>
        <span className="mono">
          GPLv3 · Cities: Skylines is a trademark of its respective owners
        </span>
      </div>
    </div>
  </footer>
);
