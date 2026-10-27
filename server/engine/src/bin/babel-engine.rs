//! Serves the Engine API on its own, for self-hosting next to a local model:
//!
//! ```sh
//! # llama-server, vLLM, Ollama ... listening on :8000
//! babel-engine --backend openai --base-url http://127.0.0.1:8000/v1 --model hy-mt2 --style plain
//! babel-server --engine-url http://127.0.0.1:7100
//! ```
//!
//! Several models, local and hosted, from a models file (see
//! `babel_engine::config`):
//!
//! ```sh
//! babel-engine --models models.toml
//! ```
//!
//! In a split deployment, `--name` makes it a service of the cluster: it
//! serves over mutual TLS, to Translation Cells only.
//!
//! ```sh
//! babel-engine --cluster cluster.toml --name engine-0 --models models.toml
//! ```

use std::path::PathBuf;

use babel_common::Role;
use babel_engine::{
    Engine,
    config::{self, BackendKind, ModelConfig},
    prompt::PromptStyle,
};
use babel_net::{Node, NodeArgs, policies};
use clap::{Parser, ValueEnum};

#[derive(Clone, Copy, ValueEnum)]
enum Style {
    Json,
    Plain,
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
    /// Where the certificates are, instead of the cluster file's `pki`.
    #[arg(long, env = "BABEL_PKI")]
    pki: Option<PathBuf>,
    /// The models to serve. Without it, one model from the options below.
    #[arg(long, env = "BABEL_MODELS", conflicts_with_all = ["backend", "base_url", "model", "api_key_env", "no_json_mode", "style"])]
    models: Option<PathBuf>,
    #[arg(long, value_enum, default_value = "mock")]
    backend: BackendKind,
    /// OpenAI-compatible base URL, including `/v1`.
    #[arg(long, default_value = "http://127.0.0.1:8000/v1")]
    base_url: String,
    /// The model's name at the server, also its ID.
    #[arg(long)]
    model: Option<String>,
    /// Environment variable holding the API key, if the server needs one.
    #[arg(long)]
    api_key_env: Option<String>,
    /// Do not request `response_format: json_object`.
    #[arg(long)]
    no_json_mode: bool,
    /// `plain` for translation models such as Hy-MT2.
    #[arg(long, value_enum, default_value = "json")]
    style: Style,
    /// Reach models on the internet through the egress proxy
    /// (`babel-egress`), e.g. `http://10.8.0.9:3128`.
    #[arg(long, env = "BABEL_EGRESS_PROXY")]
    egress_proxy: Option<String>,
}

/// The single model the command-line options describe.
fn model_from_args(args: &Args) -> ModelConfig {
    let id = args.model.clone().unwrap_or_else(|| match args.backend {
        BackendKind::Mock => "mock".into(),
        _ => "default".into(),
    });
    let mut model = ModelConfig::new(id, args.backend);
    model.base_url = Some(args.base_url.clone());
    model.api_key_env = args.api_key_env.clone();
    model.json_mode = !args.no_json_mode;
    model.style = match args.style {
        Style::Json => PromptStyle::Json,
        Style::Plain => PromptStyle::Plain,
    };
    // A single model given on the command line is the one the operator
    // pointed at; let `--egress-proxy` decide.
    model.egress = Some(args.egress_proxy.is_some());
    model
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    babel_net::init_tracing();
    let args = Args::parse();

    let configs = match &args.models {
        Some(path) => config::load(path)?,
        None => vec![model_from_args(&args)],
    };
    let models = configs
        .iter()
        .map(|model| model.build(args.egress_proxy.as_deref()))
        .collect::<anyhow::Result<Vec<_>>>()?;
    for model in &models {
        tracing::info!(id = %model.id, backend = model.backend.kind(), "serving model");
    }
    let engine = Engine::new(models);

    match args.name {
        Some(name) => {
            let node = Node::load(
                &NodeArgs {
                    cluster: args.cluster,
                    name,
                    pki: args.pki,
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
