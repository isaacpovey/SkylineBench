# skylinebench (broker)

The Rust MCP server for SkylineBench. Exposes Cities: Skylines 1 to AI agents.
Until the C# mod (separate plan) is installed, run against the built-in mock mod.

## Build

    cd broker
    cargo build --release

## Try it against the mock mod

Terminal 1 — start the mock city:

    cargo run -- mock            # listens on http://127.0.0.1:8787

Terminal 2 — point an MCP client at the server (see below). The server talks to
the mod URL given by `--mod-url` (default `http://127.0.0.1:8787`, i.e. the mock).

## Register with Claude Code

    claude mcp add skylinebench -- /absolute/path/to/broker/target/release/skylinebench serve

Or in `.mcp.json` / settings:

    {
      "mcpServers": {
        "skylinebench": {
          "command": "/absolute/path/to/broker/target/release/skylinebench",
          "args": ["serve", "--mod-url", "http://127.0.0.1:8787"]
        }
      }
    }

## Register with Codex

Add to your Codex MCP config:

    [mcp_servers.skylinebench]
    command = "/absolute/path/to/broker/target/release/skylinebench"
    args = ["serve", "--mod-url", "http://127.0.0.1:8787"]

## Tools

- Observe: `get_city_overview`, `observe_area`, `render_map`, `get_metrics`
- Act: `build_road`, `bulldoze`, `upgrade_road`, `set_zoning`
- Reference: `list_road_types`, `list_zone_types`
- Control: `control_time`, `reset_scenario`

## Tests

    cargo test                       # unit + integration, no game needed
    cargo run --example gen_golden   # regenerate the renderer golden image
