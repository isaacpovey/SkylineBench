use clap::{Parser, Subcommand};

use skylinebench::mock;
use skylinebench::tools::Skyline;

#[derive(Parser)]
#[command(
    name = "skylinebench",
    about = "Cities: Skylines 1 MCP harness (broker)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the MCP server over stdio, talking to the mod at --mod-url.
    Serve {
        #[arg(long, default_value = "http://127.0.0.1:8787")]
        mod_url: String,
    },
    /// Run the in-memory mock mod (for development/testing) on --addr.
    Mock {
        #[arg(long, default_value = "127.0.0.1:8787")]
        addr: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Mock { addr } => {
            let (bound, server) = mock::bind(addr.parse()?).await;
            eprintln!("mock mod listening on http://{bound}");
            server.await;
        }
        Command::Serve { mod_url } => {
            use rmcp::ServiceExt;
            let server = Skyline::new(mod_url)
                .serve((tokio::io::stdin(), tokio::io::stdout()))
                .await?;
            server.waiting().await?;
        }
    }
    Ok(())
}
