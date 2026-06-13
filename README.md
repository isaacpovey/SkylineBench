# SkylineBench

A benchmark to evaluate how an Agent can run and manage a city using Cities Skylines
as a simulation.

## Why I built this

Most agent benchmarks have a right answer. This one doesn't.

I have a theory: agents are bad at the second-order consequences of their own
actions. I keep running into the same failure in my own engineering work.
The moment an agent believes it has a solution, it stops thinking. It ships the fix
and never asks what else the fix touched. This is increasingly becoming a problem
in engineering tasks built on old codebases with a lot of tech debt, but I think
this issue is applicable to all areas of potential agent use.

A city is about the cruelest test of that I could think of, because in a city
*everything* is connected. Widen a road and it carries more cars, which makes
more noise, which makes the people living beside it unhappy, which makes them
leave, which empties the buildings, which kills the shops that depended on them.
Now you have less traffic, sure, but only because nobody wants to live
there anymore. The agent may even think, I have done a good job because I have
reduced the traffic and end the benchmark there.

That cascade is the whole point. The benchmark isn't really asking whether an
agent can read a congestion number and bring it down. It's asking whether the
agent can plan and anticipate problems before they happen and critically fix
them when they arise unexpectadly.

## How it works

The agent plays the game through tools as close to a human player has. It
looks at the map, inspects the traffic on any road, traces where cars are
actually going, then bulldozes, builds, upgrades roads, and rezones. It can
pause time, make a batch of changes, and step the simulation forward to watch
what they do. It gets a few hours of wall-clock time, then submits and walks
away.

A handful of deliberate choices decide what it's really being tested on:

**It never sees the score.** The agent is told, in plain language, to make
traffic flow better while keeping the city somewhere people want to live. It is
never shown the formula, the weights, or the thresholds behind that. It gets given
all the metrics about the city and has to make decisions about how to maintain or
improve them.

**It can't win by bulldozing the city.** Congestion has a trivial solution:
demolish everything until there's no one left to drive. So the congestion score
is multiplied by a health factor tied to population. Let the city hollow out and
your gains evaporate with the residents. The two pressures pull against each
other on purpose.

**It has to slow down.** Traffic doesn't re-route the instant you change a road,
it gets worse for a while as cars find the new layout, then settles. So a good
change and a bad change look identical for the first few steps, and the agent
has to tell a settling transient apart from real damage instead of reacting to
the first number it sees. Patience is part of the test.

**It can't read the answer key.** The agent runs inside a sandbox that blocks it
from reading this repository, so it can't go and inspect the scoring code. It
can only play the game through the tools. (An early run did exactly this, which
is why the sandbox exists.)

## Where this is going

Right now the agent inherits a city and repairs it. The version I actually want
is harder: hand it empty land and have it build a working city from scratch.
Repairing someone else's mistakes is the warm-up.

## How it's built

Three pieces:

- **`mod/`** — a C# mod for Cities Skylines that runs inside the game and
  exposes the simulation's state and controls over a localhost HTTP API.
- **`broker/`** — a Rust MCP server. It turns the game into a set of agent tools
  and runs the harness: measure a baseline, run the agent, let the sim settle,
  score it, and write out the artifacts.
- **`benchmark/`** — the prompt the agent sees, the run script, and the maps.

## Running a benchmark

You need Cities: Skylines (Steam, macOS), [Rust](https://rustup.rs), and Mono
(`brew install mono`) to build the mod.

1. **Install and enable the mod**, then load the benchmark save from the game's
   main menu — see [`mod/README.md`](mod/README.md). Confirm it's up:
   `curl -s http://127.0.0.1:8787/health` should report `"city_loaded":true`.
2. **Build the broker:** `cargo build --release --manifest-path broker/Cargo.toml`
3. **Run:** `./benchmark/run.sh --map gridlock-v1` (add `--watch` to watch the
   session live instead of headless).
4. **Read the results** in `benchmark/runs/<timestamp>/`: `score.json` for the
   breakdown, `transcript.md` for everything the agent did, `renders/` and
   `screenshots/` for the visuals, and `skylinebench timelapse <run-dir>` for an
   annotated video of the city changing.

Full details — scoring, artifacts, the mod API — live in the component READMEs:
[`benchmark/`](benchmark/README.md), [`broker/`](broker/README.md),
[`mod/`](mod/README.md).

## License

GPLv3 — see [LICENSE](LICENSE).
