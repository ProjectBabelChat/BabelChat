use async_trait::async_trait;
use babel_api_engine::types::{
    EngineError, EngineInfo, SummarizeRequest, SummarizeResponse, TranslateRequest,
    TranslateResponse,
};
use babel_api_engine_client::types::{self as client, ThrowsOrOtherError};
use babel_common::convert;

/// An engine in another process, reached over HTTP: a self-hosted
/// `babel-engine` or BabelChat Cloud. Callers cannot tell it from an
/// in-process [`crate::Engine`].
pub struct RemoteEngine {
    client: client::Engine,
}

impl RemoteEngine {
    /// `url` is the engine's base URL, e.g. `http://127.0.0.1:7100`.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            client: client::Engine::new(url),
        }
    }

    /// Calls the engine with `client`, e.g. one set up for mutual TLS
    /// inside a cluster.
    pub fn with_client(url: impl Into<String>, client: reqwest::Client) -> Self {
        Self {
            client: client::Engine {
                url: url.into().trim_end_matches('/').to_string(),
                client,
            },
        }
    }
}

fn engine_error(error: ThrowsOrOtherError<client::EngineError>) -> EngineError {
    match error {
        ThrowsOrOtherError::Throws(error) => convert(&error),
        ThrowsOrOtherError::Other(error) => {
            tracing::warn!(%error, "remote engine unreachable");
            EngineError::Unavailable
        }
    }
}

#[async_trait]
impl babel_api_engine::types::Engine for RemoteEngine {
    async fn translate(&self, body: TranslateRequest) -> Result<TranslateResponse, EngineError> {
        self.client
            .translate(convert(&body))
            .await
            .map(|response| convert(&response))
            .map_err(engine_error)
    }

    async fn summarize(&self, body: SummarizeRequest) -> Result<SummarizeResponse, EngineError> {
        self.client
            .summarize(convert(&body))
            .await
            .map(|response| convert(&response))
            .map_err(engine_error)
    }

    async fn info(&self) -> EngineInfo {
        match self.client.info().await {
            Ok(info) => convert(&info),
            Err(_) => EngineInfo {
                backend: "remote (unreachable)".into(),
                model: String::new(),
            },
        }
    }
}
