//! Serves the Engine API on its own, for self-hosting next to a local model:
//!
//! ```sh
//! # llama-server, vLLM, Ollama ... listening on :8000
//! babel-engine --backend openai --base-url http://127.0.0.1:8000/v1 --model hy-mt2
//! babel-server --engine-url http://127.0.0.1:7100
//! ```
//!
//! In a split deployment, `--name` makes it a service of the cluster: it
//! serves over mutual TLS, to Translation Cells only.
//!
//! ```sh
//! babel-engine --cluster cluster.toml --name engine-0 --backend openai ...
//! ```

use std::{path::PathBuf, sync::Arc, time::Duration};

use babel_common::Role;
use babel_engine::{
    Backend, Engine,
    mock::{MockBackend, MockConfig},
    openai::{OpenAiCompatBackend, OpenAiCompatConfig},
};
use babel_net::{Node, NodeArgs, policies};
use clap::{Parser, ValueEnum};

#[derive(Clone, Copy, ValueEnum)]
enum BackendKind {
    Mock,
    Openai,
}

#[derive(Parser)]
#[command(about = "BabelChat translation engine")]
struct Args {
    /// Plain HTTP address, when not part of a cluster.
    #[arg(long, default_value = "127.0.0.1:7100")]
    listen: String,
    /// The cluster file, with `--name`.
    #[arg(long, env = "BABEL_CLUSTER", default_value = "cluster.toml")]
    cluster: PathBuf,
    /// This engine's name in the cluster file: serve over mutual TLS.
    #[arg(long, env = "BABEL_NAME")]
    name: Option<String>,
    #[arg(long, value_enum, default_value = "mock")]
    backend: BackendKind,
    /// OpenAI-compatible base URL, including `/v1`.
    #[arg(long, default_value = "http://127.0.0.1:8000/v1")]
    base_url: String,
    #[arg(long, default_value = "default")]
    model: String,
    /// Environment variable holding the API key, if the server needs one.
    #[arg(long)]
    api_key_env: Option<String>,
    /// Do not request `response_format: json_object`.
    #[arg(long)]
    no_json_mode: bool,
    /// Reach the model server through the egress proxy (`babel-egress`),
    /// e.g. `http://10.8.0.9:3128`. Use it for model servers on the
    /// internet, not for one on this machine.
    #[arg(long, env = "BABEL_EGRESS_PROXY")]
    egress_proxy: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    babel_net::init_tracing();
    let args = Args::parse();

    let backend: Arc<dyn Backend> = match args.backend {
        BackendKind::Mock => Arc::new(MockBackend::new(MockConfig::default())),
        BackendKind::Openai => Arc::new(OpenAiCompatBackend::new(OpenAiCompatConfig {
            base_url: args.base_url,
            api_key: args
                .api_key_env
                .map(std::env::var)
                .transpose()
                .map_err(|_| anyhow::anyhow!("API key variable is not set"))?,
            model: args.model,
            json_mode: !args.no_json_mode,
            timeout: Duration::from_secs(60),
            proxy: args.egress_proxy,
        })?),
    };
    tracing::info!(backend = ?backend.info(), "engine");
    let engine = Engine::new(backend);

    match args.name {
        Some(name) => {
            let node = Node::load(
                &NodeArgs {
                    cluster: args.cluster,
                    name,
                },
                Role::Engine,
            )?;
            node.serve(babel_api_engine::router(engine), policies::engine())
                .await
        }
        None => {
            tracing::info!(listen = %args.listen, "serving");
            babel_api_engine::serve(engine, &args.listen).await?;
            Ok(())
        }
    }
}
