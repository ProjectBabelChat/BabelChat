//! The reference translation engine.
//!
//! [`Engine`] implements the Engine API (types/engine.tyml) on top of a
//! [`Backend`]. Translation Cells hold an `Arc<dyn Engine>`: this struct
//! in-process, or [`RemoteEngine`] when the engine runs elsewhere (a
//! self-hosted `babel-engine` next to a local LLM, or BabelChat Cloud).
//!
//! See docs/architecture/translation-engine.md.

pub mod mock;
pub mod openai;
pub mod prompt;
mod remote;

use std::sync::Arc;

use async_trait::async_trait;
use babel_api_engine::types::{
    EngineError, EngineInfo, SummarizeRequest, SummarizeResponse, TranslateRequest,
    TranslateResponse, Usage,
};
pub use remote::RemoteEngine;

/// Where translations actually come from.
#[async_trait]
pub trait Backend: Send + Sync + 'static {
    fn info(&self) -> EngineInfo;

    async fn translate(&self, request: &TranslateRequest)
    -> Result<TranslateResponse, EngineError>;

    async fn summarize(&self, request: &SummarizeRequest)
    -> Result<SummarizeResponse, EngineError>;
}

/// The Engine API over a backend, with the checks every backend needs.
pub struct Engine {
    backend: Arc<dyn Backend>,
}

impl Engine {
    pub fn new(backend: Arc<dyn Backend>) -> Self {
        Self { backend }
    }
}

/// Translations longer than this many times the source (plus slack) are
/// rejected: a sign the model followed instructions hidden in the message or
/// started rambling.
const MAX_LENGTH_RATIO: usize = 4;
const LENGTH_SLACK: usize = 64;

#[async_trait]
impl babel_api_engine::types::Engine for Engine {
    async fn translate(&self, body: TranslateRequest) -> Result<TranslateResponse, EngineError> {
        if body.targets.is_empty() {
            return Ok(TranslateResponse {
                translations: Vec::new(),
                usage: Usage {
                    input_tokens: 0,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                    output_tokens: 0,
                },
            });
        }

        let response = self.backend.translate(&body).await?;

        let limit = body.message.text.chars().count() * MAX_LENGTH_RATIO + LENGTH_SLACK;
        let complete = body.targets.iter().all(|target| {
            response
                .translations
                .iter()
                .any(|translation| translation.lang == target.tag)
        });
        let sane = response
            .translations
            .iter()
            .all(|translation| translation.text.chars().count() <= limit);
        if !complete || !sane {
            return Err(EngineError::InvalidOutput);
        }

        Ok(response)
    }

    async fn summarize(&self, body: SummarizeRequest) -> Result<SummarizeResponse, EngineError> {
        let mut response = self.backend.summarize(&body).await?;
        let max_chars = body.max_chars.max(0) as usize;
        if response.text.chars().count() > max_chars {
            response.text = response.text.chars().take(max_chars).collect();
        }
        Ok(response)
    }

    async fn info(&self) -> EngineInfo {
        self.backend.info()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use babel_api_engine::types::{Engine as _, SourceMessage, TargetLanguage};

    use super::*;
    use crate::mock::{MockBackend, MockConfig};

    fn mock_engine() -> Engine {
        Engine::new(Arc::new(MockBackend::new(MockConfig {
            latency: Duration::ZERO,
            ..MockConfig::default()
        })))
    }

    fn request(targets: &[&str]) -> TranslateRequest {
        TranslateRequest {
            layers: Vec::new(),
            window: Vec::new(),
            message: SourceMessage {
                author: "alice".into(),
                text: "hello".into(),
            },
            source_lang: "en-US".into(),
            targets: targets
                .iter()
                .map(|tag| TargetLanguage {
                    tag: tag.to_string(),
                    name: tag.to_string(),
                })
                .collect(),
        }
    }

    #[tokio::test]
    async fn remote_engine_matches_local() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, babel_api_engine::router(mock_engine()))
                .await
                .unwrap()
        });

        let remote = RemoteEngine::new(format!("http://{address}"));
        let response = remote
            .translate(request(&["ja-JP", "ko-KR"]))
            .await
            .unwrap();
        assert_eq!(response.translations.len(), 2);
        assert_eq!(response.translations[1].text, "[ko-KR] hello");
        assert_eq!(remote.info().await.backend, "mock");
    }

    #[tokio::test]
    async fn no_targets_costs_nothing() {
        let response = mock_engine().translate(request(&[])).await.unwrap();
        assert!(response.translations.is_empty());
        assert_eq!(response.usage.input_tokens, 0);
    }
}
