//! The Account stand-in as its own service, for split deployments.
//!
//! ```sh
//! babel-account --cluster cluster.toml --name account --data data/account
//! ```
//!
//! Only Edges may call it. It signs user tokens with `pki/token.key`.

use babel_account::{Account, TABLES};
use babel_common::Role;
use babel_net::{Node, NodeArgs, StoreArgs, pki::read_private_key, policies};
use clap::Parser;

#[derive(Parser)]
#[command(about = "BabelChat Account service (development stand-in)")]
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
    let node = Node::load(&args.node, Role::Account)?;
    let store = node.open_store(&args.store, TABLES)?;
    let token_key = read_private_key(&node.cluster.token_private_key_path())?;
    let account = Account::open(store.clone(), Some(token_key))?;
    node.serve_replicated(
        babel_api_account::router(account),
        policies::account(),
        store,
    )
    .await
}
