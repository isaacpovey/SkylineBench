import { Nav } from "@/components/layout/nav";
import { Footer } from "@/components/layout/footer";
import { Hero } from "@/components/sections/hero";
import { Thesis } from "@/components/sections/thesis";
import { HowItWorks } from "@/components/sections/how-it-works";
import { Scoring } from "@/components/sections/scoring";
import { Architecture } from "@/components/sections/architecture";
import { Learnings } from "@/components/sections/learnings";
import { Roadmap } from "@/components/sections/roadmap";
import { Findings } from "@/components/sections/findings";

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
    <hr className="divider" />
    <Findings />
    <Footer />
  </>
);

export default Home;
