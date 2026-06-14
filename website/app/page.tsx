import { Nav } from "@/components/layout/nav";
import { Footer } from "@/components/layout/footer";
import { Hero } from "@/components/sections/hero";
import { Thesis } from "@/components/sections/thesis";
import { HowItWorks } from "@/components/sections/how-it-works";
import { Scoring } from "@/components/sections/scoring";
import { Architecture } from "@/components/sections/architecture";
import { Learnings } from "@/components/sections/learnings";
import { Roadmap } from "@/components/sections/roadmap";
import { Results } from "@/components/sections/results";
import { Findings } from "@/components/sections/findings";
import { CtaBand } from "@/components/sections/cta-band";

const Home = () => (
  <>
    <Nav />
    <Hero />
    <hr className="divider" />
    <Thesis />
    <HowItWorks />
    <hr className="divider" />
    <Scoring />
    <Architecture />
    <Learnings />
    <hr className="divider" />
    <Roadmap />
    <Results />
    <hr className="divider" />
    <Findings />
    <hr className="divider" />
    <CtaBand />
    <Footer />
  </>
);

export default Home;
