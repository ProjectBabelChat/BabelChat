//! Serves the Engine API on its own, for self-hosting next to a local model:
//!
//! ```sh
//! # llama-server, vLLM, Ollama ... listening on :8000
//! babel-engine --backend openai --base-url http://127.0.0.1:8000/v1 --model hy-mt2
//! babel-server --engine-url http://127.0.0.1:7100
//! ```

use std::{sync::Arc, time::Duration};

use babel_engine::{
    Backend, Engine,
    mock::{MockBackend, MockConfig},
    openai::{OpenAiCompatBackend, OpenAiCompatConfig},
};
use clap::{Parser, ValueEnum};

#[derive(Clone, Copy, ValueEnum)]
enum BackendKind {
    Mock,
    Openai,
}

#[derive(Parser)]
#[command(about = "BabelChat translation engine")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:7100")]
    listen: String,
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
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
        })),
    };

    tracing::info!(listen = %args.listen, backend = ?backend.info(), "serving engine");
    babel_api_engine::serve(Engine::new(backend), &args.listen).await?;
    Ok(())
}
