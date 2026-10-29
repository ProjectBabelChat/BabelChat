//! A Media service as its own service.
//!
//! ```sh
//! babel-media --cluster cluster.toml --name media-0 --data data/media-0
//! ```
//!
//! Edges pass files to it; Message Cells tell it which files messages took
//! in and which went.

use babel_common::Role;
use babel_media::{BLOB_TABLES, Media, TABLES};
use babel_net::{Node, NodeArgs, StoreArgs, policies};
use clap::Parser;

#[derive(Parser)]
#[command(about = "BabelChat Media service: the files people upload")]
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
    let node = Node::load(&args.node, Role::Media)?;
    let store = node.open_store_with_blobs(&args.store, TABLES, BLOB_TABLES)?;
    let media = Media::open(node.identity.name.clone(), store.clone());
    node.serve_replicated(babel_media::router(media), policies::media(), store)
        .await
}
