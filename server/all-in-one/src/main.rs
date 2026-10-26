//! Runs every BabelChat service in one process.
//!
//! ```sh
//! cargo run -p babel-server -- --data ./data --plan paid3
//! ```

use std::{path::PathBuf, time::Duration};

use babel_api_translation_cell::types::Plan;
use babel_server::{EngineChoice, ServerConfig, start};
use clap::{Parser, ValueEnum};

#[derive(Clone, Copy, ValueEnum)]
enum PlanArg {
    Free,
    Paid1,
    Paid2,
    Paid3,
}

#[derive(Parser)]
#[command(about = "BabelChat Phase 0: all services in one process")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:7000")]
    listen: String,
    #[arg(long, default_value = "data")]
    data: PathBuf,
    #[arg(long, default_value_t = 2)]
    message_cells: usize,
    #[arg(long, default_value_t = 2)]
    translation_cells: usize,
    /// Plan given to new guilds until billing exists.
    #[arg(long, value_enum, default_value = "free")]
    plan: PlanArg,
    /// LLM-translated messages per guild per day.
    #[arg(long)]
    daily_budget: Option<i64>,
    /// Use a separate Engine API server (e.g. `babel-engine`) instead of the
    /// in-process mock.
    #[arg(long)]
    engine_url: Option<String>,
    /// Latency of the in-process mock engine.
    #[arg(long, default_value_t = 300)]
    mock_latency_ms: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    let config = ServerConfig {
        data_dir: args.data,
        message_cells: args.message_cells,
        translation_cells: args.translation_cells,
        engine: match args.engine_url {
            Some(url) => EngineChoice::Remote { url },
            None => EngineChoice::Mock {
                latency: Duration::from_millis(args.mock_latency_ms),
            },
        },
        default_plan: match args.plan {
            PlanArg::Free => Plan::Free,
            PlanArg::Paid1 => Plan::Paid1,
            PlanArg::Paid2 => Plan::Paid2,
            PlanArg::Paid3 => Plan::Paid3,
        },
        daily_budget: args.daily_budget,
    };

    let (address, server) = start(&config, &args.listen).await?;
    tracing::info!(%address, data = %config.data_dir.display(), "BabelChat is listening");
    // A plain line for scripts and tests that start the server on port 0.
    println!("listening on http://{address}");
    server.await??;
    Ok(())
}
