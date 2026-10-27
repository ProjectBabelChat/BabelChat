//! Keeps a standby copy of a service's store on another machine.
//!
//! ```sh
//! # on the standby machine, with the service's own certificate
//! babel-standby --cluster cluster.toml --name message-cell-0 --data /var/lib/babel/message-cell-0
//! ```
//!
//! It starts from a checkpoint of the primary, then replays its writes as
//! they happen. To fail over: make sure the primary is down, stop the
//! standby, start the service on the standby's data directory, and point the
//! service's address at this machine. See docs/deployment.md.

use std::path::PathBuf;

use babel_net::{Node, NodeArgs, replication};
use clap::Parser;

#[derive(Parser)]
#[command(about = "Keeps a standby copy of a BabelChat service's store")]
struct Args {
    #[command(flatten)]
    node: NodeArgs,
    /// Where the copy lives [default: data/<name>-standby]
    #[arg(long)]
    data: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    babel_net::init_tracing();
    let args = Args::parse();
    let node = Node::load_any(&args.node)?;
    let data = args
        .data
        .unwrap_or_else(|| replication::default_data(&node.identity.name));
    if let Some(parent) = data.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // The primary checks that this is the service itself.
    let client = node.http_client(&node.identity)?;
    tracing::info!(service = %node.identity, primary = %node.service.url, data = %data.display(), "standby");
    replication::follow(&node.service.url, &client, &data).await
}
