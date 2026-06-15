export type NavSection = { href: string; label: string; route?: boolean };

export const navSections: NavSection[] = [
  { href: "#thesis", label: "Why" },
  { href: "#how", label: "Methodology" },
  { href: "#scoring", label: "Scoring" },
  { href: "#built", label: "Architecture" },
  { href: "#future", label: "Roadmap" },
  { href: "/results", label: "Results", route: true },
  { href: "/updates", label: "Updates", route: true },
  { href: "/changelog", label: "Changelog", route: true },
];
