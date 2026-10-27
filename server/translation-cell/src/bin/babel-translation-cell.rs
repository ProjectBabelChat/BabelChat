//! A Translation Cell as its own service.
//!
//! ```sh
//! babel-translation-cell --cluster cluster.toml --name translation-cell-0 --plan paid3
//! ```
//!
//! BabelChat's Edges and Message Cells call it, and so may other platforms
//! with a tenant certificate; each tenant only sees its own spaces. It
//! translates with every engine of the cluster file (their models together
//! are what spaces choose from), or an in-process mock when it lists none.

use std::{sync::Arc, time::Duration};

use babel_api_engine::types::Engine;
use babel_api_translation_cell::types::Plan;
use babel_common::{Identity, Role};
use babel_engine::{RemoteEngine, mock::MockConfig};
use babel_net::{Node, NodeArgs, StoreArgs, policies};
use babel_translation_cell::{Engines, TABLES, TranslationCell, TranslationCellConfig};
use clap::{Parser, ValueEnum};

#[derive(Clone, Copy, ValueEnum)]
enum PlanArg {
    Free,
    Paid1,
    Paid2,
    Paid3,
}

#[derive(Parser)]
#[command(about = "BabelChat Translation Cell: translations, context and language policy")]
struct Args {
    #[command(flatten)]
    node: NodeArgs,
    #[command(flatten)]
    store: StoreArgs,
    /// Plan given to new spaces until billing exists.
    #[arg(long, value_enum, default_value = "free")]
    plan: PlanArg,
    /// LLM-translated messages per space per day.
    #[arg(long)]
    daily_budget: Option<i64>,
    /// Latency of the in-process mock engine, used when the cluster lists
    /// no engine.
    #[arg(long, default_value_t = 300)]
    mock_latency_ms: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    babel_net::init_tracing();
    let args = Args::parse();
    let node = Node::load(&args.node, Role::TranslationCell)?;

    let mut engines: Vec<Arc<dyn Engine>> = Vec::new();
    for service in &node.cluster.engine {
        let peer = Identity {
            role: Role::Engine,
            name: service.name.clone(),
        };
        tracing::info!(engine = %peer, "translating with");
        engines.push(Arc::new(RemoteEngine::with_client(
            service.url.clone(),
            node.http_client(&peer)?,
        )));
    }
    if engines.is_empty() {
        engines.push(Arc::new(babel_engine::Engine::mock(MockConfig {
            latency: Duration::from_millis(args.mock_latency_ms),
            ..MockConfig::default()
        })));
    }

    let store = node.open_store(&args.store, TABLES)?;
    let cell = TranslationCell::open(
        store.clone(),
        Engines::new(engines),
        TranslationCellConfig {
            default_plan: match args.plan {
                PlanArg::Free => Plan::Free,
                PlanArg::Paid1 => Plan::Paid1,
                PlanArg::Paid2 => Plan::Paid2,
                PlanArg::Paid3 => Plan::Paid3,
            },
            daily_budget: args.daily_budget,
            ..TranslationCellConfig::default()
        },
    )?;
    node.serve_replicated(
        babel_api_translation_cell::router(cell),
        policies::translation_cell(),
        store,
    )
    .await
}
