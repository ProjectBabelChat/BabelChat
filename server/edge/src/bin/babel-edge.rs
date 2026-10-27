//! An Edge as its own service: the public API, in front of the cluster.
//!
//! ```sh
//! babel-edge --cluster cluster.toml --name edge-0
//! ```
//!
//! Clients connect to the `listen` address of `edge-0` (plain HTTP; put
//! TLS in front, at a load balancer or a reverse proxy). Everything behind
//! it is called over mutual TLS. Tokens are verified with `pki/token.pub`.

use babel_common::Role;
use babel_edge::{Cluster, Edge, EdgeConfig};
use babel_net::{Node, NodeArgs};
use clap::Parser;
use jsonwebtoken::DecodingKey;

#[derive(Parser)]
#[command(about = "BabelChat Edge: the public entry point")]
struct Args {
    #[command(flatten)]
    node: NodeArgs,
    /// Sustained writes (messages, guilds, channels) per user per second.
    #[arg(long, default_value_t = 5.0)]
    writes_per_second: f64,
    #[arg(long, default_value_t = 20.0)]
    write_burst: f64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    babel_net::init_tracing();
    let args = Args::parse();
    let node = Node::load(&args.node, Role::Edge)?;
    let token_key = std::fs::read(node.cluster.token_public_key_path())?;

    let edge = Edge::start(
        Cluster {
            account: node.account()?,
            directory: node.directory()?,
            message_cells: node.message_cells()?.into_iter().collect(),
            translation_cells: node.translation_cells()?,
        },
        EdgeConfig {
            token_key: DecodingKey::from_ed_pem(&token_key)?,
            writes_per_second: args.writes_per_second,
            write_burst: args.write_burst,
        },
    );

    let address = node.service.listen_address()?;
    let listener = tokio::net::TcpListener::bind(&address).await?;
    tracing::info!(edge = %node.identity, %address, url = %node.service.url, "serving clients");
    let router = babel_api_edge::router(edge.clone()).merge(edge.websocket_router());
    axum::serve(listener, router).await?;
    Ok(())
}
