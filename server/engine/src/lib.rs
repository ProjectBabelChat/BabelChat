//! The reference translation engine.
//!
//! [`Engine`] implements the Engine API (types/engine.tyml) on top of one or
//! more [`Model`]s, each served by a [`Backend`]. Translation Cells hold an
//! `Arc<dyn Engine>`: this struct in-process, or [`RemoteEngine`] when the
//! engine runs elsewhere (a self-hosted `babel-engine` next to a local LLM,
//! or BabelChat Cloud).
//!
//! See docs/architecture/translation-engine.md.

pub mod anthropic;
pub mod config;
pub mod mock;
pub mod openai;
pub mod prompt;
mod remote;

use std::sync::Arc;

use async_trait::async_trait;
use babel_api_engine::types::{
    EngineError, EngineInfo, ModelInfo, SummarizeRequest, SummarizeResponse, TranslateRequest,
    TranslateResponse, Usage,
};
pub use remote::RemoteEngine;

/// Where translations actually come from.
#[async_trait]
pub trait Backend: Send + Sync + 'static {
    /// How the model is reached: "mock", "openai-compatible", "anthropic".
    fn kind(&self) -> &'static str;

    async fn translate(&self, request: &TranslateRequest)
    -> Result<TranslateResponse, EngineError>;

    async fn summarize(&self, request: &SummarizeRequest)
    -> Result<SummarizeResponse, EngineError>;
}

/// A model an engine serves.
pub struct Model {
    /// What requests and space policies name it by.
    pub id: String,
    /// Shown to people choosing a model.
    pub name: String,
    pub backend: Arc<dyn Backend>,
}

impl Model {
    pub fn new(id: impl Into<String>, name: impl Into<String>, backend: Arc<dyn Backend>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            backend,
        }
    }
}

/// The Engine API over its models, with the checks every backend needs.
pub struct Engine {
    /// The first is the default.
    models: Vec<Model>,
}

impl Engine {
    /// Serves `models`; the first is the default.
    ///
    /// # Panics
    /// If `models` is empty or two models share an ID.
    pub fn new(models: Vec<Model>) -> Self {
        assert!(!models.is_empty(), "an engine serves at least one model");
        for (index, model) in models.iter().enumerate() {
            assert!(
                models[..index].iter().all(|other| other.id != model.id),
                "two models are named {:?}",
                model.id
            );
        }
        Self { models }
    }

    /// One mock model, `mock`: for development and tests.
    pub fn mock(config: mock::MockConfig) -> Self {
        Self::new(vec![Model::new(
            "mock",
            "Mock",
            Arc::new(mock::MockBackend::new(config)),
        )])
    }

    fn model(&self, id: Option<&str>) -> Result<&Model, EngineError> {
        match id {
            None => Ok(&self.models[0]),
            Some(id) => self
                .models
                .iter()
                .find(|model| model.id == id)
                .ok_or(EngineError::UnknownModel),
        }
    }
}

/// Translations longer than this many times the source (plus slack) are
/// rejected: a sign the model followed instructions hidden in the message or
/// started rambling.
const MAX_LENGTH_RATIO: usize = 4;
const LENGTH_SLACK: usize = 64;

pub(crate) fn no_usage() -> Usage {
    Usage {
        input_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        output_tokens: 0,
    }
}

pub(crate) fn add_usage(total: &mut Usage, usage: &Usage) {
    total.input_tokens += usage.input_tokens;
    total.cache_read_tokens += usage.cache_read_tokens;
    total.cache_write_tokens += usage.cache_write_tokens;
    total.output_tokens += usage.output_tokens;
}

#[async_trait]
impl babel_api_engine::types::Engine for Engine {
    async fn translate(&self, body: TranslateRequest) -> Result<TranslateResponse, EngineError> {
        let model = self.model(body.model.as_deref())?;
        if body.targets.is_empty() {
            return Ok(TranslateResponse {
                translations: Vec::new(),
                usage: no_usage(),
            });
        }

        let response = model.backend.translate(&body).await?;

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
            tracing::warn!(model = %model.id, complete, sane, "rejected a translation");
            return Err(EngineError::InvalidOutput);
        }

        Ok(response)
    }

    async fn summarize(&self, body: SummarizeRequest) -> Result<SummarizeResponse, EngineError> {
        let model = self.model(body.model.as_deref())?;
        let mut response = model.backend.summarize(&body).await?;
        let max_chars = body.max_chars.max(0) as usize;
        if response.text.chars().count() > max_chars {
            response.text = response.text.chars().take(max_chars).collect();
        }
        Ok(response)
    }

    async fn info(&self) -> EngineInfo {
        EngineInfo {
            models: self
                .models
                .iter()
                .map(|model| ModelInfo {
                    id: model.id.clone(),
                    name: model.name.clone(),
                    backend: model.backend.kind().to_string(),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use babel_api_engine::types::{Engine as _, SourceMessage, TargetLanguage};

    use super::*;
    use crate::mock::{MockBackend, MockConfig};

    fn instant() -> MockConfig {
        MockConfig {
            latency: Duration::ZERO,
            ..MockConfig::default()
        }
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
            model: None,
        }
    }

    #[tokio::test]
    async fn remote_engine_matches_local() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, babel_api_engine::router(Engine::mock(instant())))
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
        let info = remote.info().await;
        assert_eq!(info.models[0].id, "mock");
        assert_eq!(info.models[0].backend, "mock");
    }

    #[tokio::test]
    async fn no_targets_costs_nothing() {
        let response = Engine::mock(instant())
            .translate(request(&[]))
            .await
            .unwrap();
        assert!(response.translations.is_empty());
        assert_eq!(response.usage.input_tokens, 0);
    }

    #[tokio::test]
    async fn requests_pick_a_model() {
        let engine = Engine::new(vec![
            Model::new("small", "Small", Arc::new(MockBackend::new(instant()))),
            Model::new("large", "Large", Arc::new(MockBackend::new(instant()))),
        ]);
        assert_eq!(engine.info().await.models.len(), 2);

        let mut large = request(&["ja-JP"]);
        large.model = Some("large".into());
        assert!(engine.translate(large).await.is_ok());

        let mut unknown = request(&["ja-JP"]);
        unknown.model = Some("huge".into());
        assert_eq!(
            engine.translate(unknown).await.unwrap_err(),
            EngineError::UnknownModel
        );
    }
}
