export type NavSection = { href: string; label: string; route?: boolean };

export const navSections: NavSection[] = [
  { href: "#thesis", label: "Thesis" },
  { href: "#how", label: "How it works" },
  { href: "#scoring", label: "Scoring" },
  { href: "#built", label: "Architecture" },
  { href: "#future", label: "Roadmap" },
  { href: "#results", label: "Results" },
  { href: "/updates", label: "Updates", route: true },
  { href: "/changelog", label: "Changelog", route: true },
];
