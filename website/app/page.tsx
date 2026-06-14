import { Nav } from "@/components/layout/nav";
import { Footer } from "@/components/layout/footer";
import { Hero } from "@/components/sections/hero";
import { Thesis } from "@/components/sections/thesis";
import { HowItWorks } from "@/components/sections/how-it-works";
import { Scoring } from "@/components/sections/scoring";
import { Architecture } from "@/components/sections/architecture";

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
    <Footer />
  </>
);

export default Home;
