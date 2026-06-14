import { Nav } from "@/components/layout/nav";
import { Footer } from "@/components/layout/footer";
import { Hero } from "@/components/sections/hero";
import { Thesis } from "@/components/sections/thesis";

const Home = () => (
  <>
    <Nav />
    <Hero />
    <hr className="divider" />
    <Thesis />
    <Footer />
  </>
);

export default Home;
