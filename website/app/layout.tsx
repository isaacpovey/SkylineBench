import type { Metadata } from "next";
import { Geist, Geist_Mono, Inter_Tight } from "next/font/google";
import { Analytics } from "@vercel/analytics/next";
import { Reveal } from "@/components/reveal";
import "./globals.css";

const geist = Geist({ subsets: ["latin"], variable: "--font-geist" });
const geistMono = Geist_Mono({ subsets: ["latin"], variable: "--font-geist-mono" });
const interTight = Inter_Tight({ subsets: ["latin"], variable: "--font-inter-tight" });

export const metadata: Metadata = {
  title: "SkylineBench: an AI agent benchmark",
  description:
    "A benchmark that evaluates how an agent can run and manage a city in Cities: Skylines. It has to improve the traffic without ever being told how it's being judged.",
  icons: { icon: "/favicon.svg" },
};

const RootLayout = ({ children }: { children: React.ReactNode }) => (
  <html lang="en" className={`ds dark ${geist.variable} ${geistMono.variable} ${interTight.variable}`}>
    <body className="ds dark">
      {children}
      <Analytics />
      <Reveal />
    </body>
  </html>
);

export default RootLayout;
