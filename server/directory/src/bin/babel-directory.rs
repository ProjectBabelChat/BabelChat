//! The Directory as its own service.
//!
//! ```sh
//! babel-directory --cluster cluster.toml --name directory --data data/directory
//! ```
//!
//! Edges ask it where guilds live; Message Cells tell it who joined and
//! left.

use babel_common::Role;
use babel_directory::{Directory, TABLES};
use babel_net::{Node, NodeArgs, StoreArgs, policies};
use clap::Parser;

#[derive(Parser)]
#[command(about = "BabelChat Directory: guild placement and membership index")]
struct Args {
    #[command(flatten)]
    node: NodeArgs,
    #[command(flatten)]
    store: StoreArgs,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    babel_net::init_tracing();
    let args = Args::parse();
    let node = Node::load(&args.node, Role::Directory)?;
    let store = node.open_store(&args.store, TABLES)?;
    let translation_cells = node
        .cluster
        .translation_cell
        .iter()
        .map(|service| service.name.clone())
        .collect();
    let directory = Directory::open(store.clone(), node.message_cells()?, translation_cells)?;
    node.serve_replicated(
        babel_api_directory::router(directory),
        policies::directory(),
        store,
    )
    .await
}
