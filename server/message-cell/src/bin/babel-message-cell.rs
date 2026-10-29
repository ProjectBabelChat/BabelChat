//! A Message Cell as its own service.
//!
//! ```sh
//! babel-message-cell --cluster cluster.toml --name message-cell-0 --data data/message-cell-0
//! ```
//!
//! Edges call it on users' behalf; the Directory creates guilds on it. It
//! tells Media services which files messages took in and which went.

use std::sync::Arc;

use babel_api_media::types::Media;
use babel_common::Role;
use babel_message_cell::{MessageCell, TABLES};
use babel_net::{Node, NodeArgs, StoreArgs, policies};
use clap::Parser;

#[derive(Parser)]
#[command(about = "BabelChat Message Cell: guilds, channels, members and messages")]
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
    let node = Node::load(&args.node, Role::MessageCell)?;
    let store = node.open_store(&args.store, TABLES)?;
    let media = babel_media::connect(&node)?
        .into_iter()
        .map(|(name, media)| (name, media as Arc<dyn Media>))
        .collect();
    let cell = MessageCell::open(store.clone(), node.translation_cells()?, media)?;
    cell.connect_directory(node.directory()?);
    node.serve_replicated(
        babel_api_message_cell::router(cell),
        policies::message_cell(),
        store,
    )
    .await
}
