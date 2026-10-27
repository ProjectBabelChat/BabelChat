//! The models file: which models an engine serves and how it reaches them.
//!
//! ```toml
//! # The first model is the default.
//! [[model]]
//! id = "hy-mt2-1.8b"                    # what space policies name it by
//! name = "Hy-MT2 1.8B"                  # shown to owners; defaults to id
//! backend = "openai"                    # openai | anthropic | mock
//! base_url = "http://127.0.0.1:8000/v1"
//! style = "plain"                       # json (default) | plain
//!
//! [[model]]
//! id = "qwen3.5-4b"
//! backend = "openai"
//! base_url = "http://127.0.0.1:8001/v1"
//! model = "qwen3.5-4b-instruct"         # the server's name; defaults to id
//! [model.extra]                         # merged into every request body
//! chat_template_kwargs = { enable_thinking = false }
//!
//! [[model]]
//! id = "claude-haiku-5-5"
//! backend = "anthropic"
//! api_key_env = "ANTHROPIC_API_KEY"
//! effort = "low"
//! ```
//!
//! Models on the internet (`egress = true`, the default for `anthropic`) are
//! reached through the engine's `--egress-proxy`.

use std::{path::Path, sync::Arc, time::Duration};

use anyhow::{Context, bail};
use serde::Deserialize;

use crate::{
    Backend, Model,
    anthropic::{AnthropicBackend, AnthropicConfig},
    mock::{MockBackend, MockConfig},
    openai::{OpenAiCompatBackend, OpenAiCompatConfig},
    prompt::PromptStyle,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum BackendKind {
    Mock,
    Openai,
    Anthropic,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelsFile {
    #[serde(rename = "model", default)]
    models: Vec<ModelConfig>,
}

fn yes() -> bool {
    true
}

/// One `[[model]]` of the models file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub id: String,
    pub name: Option<String>,
    pub backend: BackendKind,
    /// For `openai` (required) and `anthropic`.
    pub base_url: Option<String>,
    /// The model's name at the server or provider; defaults to `id`.
    pub model: Option<String>,
    /// Environment variable holding the API key.
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub style: PromptStyle,
    /// `openai`: ask for `response_format: json_object`.
    #[serde(default = "yes")]
    pub json_mode: bool,
    /// `openai`: send `max_completion_tokens` instead of `max_tokens`.
    #[serde(default)]
    pub max_completion_tokens: bool,
    /// Reach the model through `--egress-proxy`.
    pub egress: Option<bool>,
    pub timeout_secs: Option<u64>,
    /// `anthropic`: `output_config.effort`.
    pub effort: Option<String>,
    /// `openai`: fields merged into every request body.
    #[serde(default)]
    pub extra: toml::Table,
    /// `mock`: simulated latency.
    pub latency_ms: Option<u64>,
}

/// Model IDs end up in stored policies and URLs: keep them plain.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_'))
}

/// Reads a models file.
pub fn load(path: &Path) -> anyhow::Result<Vec<ModelConfig>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse(&text).with_context(|| format!("in {}", path.display()))
}

pub fn parse(text: &str) -> anyhow::Result<Vec<ModelConfig>> {
    let file: ModelsFile = toml::from_str(text)?;
    if file.models.is_empty() {
        bail!("no [[model]]");
    }
    for (index, model) in file.models.iter().enumerate() {
        if !valid_id(&model.id) {
            bail!(
                "model id {:?}: use lowercase letters, digits, '.', '-' and '_'",
                model.id
            );
        }
        if file.models[..index]
            .iter()
            .any(|other| other.id == model.id)
        {
            bail!("two models are named {:?}", model.id);
        }
    }
    Ok(file.models)
}

impl ModelConfig {
    /// A model of `backend` called `id`, everything else by default.
    pub fn new(id: impl Into<String>, backend: BackendKind) -> Self {
        Self {
            id: id.into(),
            name: None,
            backend,
            base_url: None,
            model: None,
            api_key_env: None,
            style: PromptStyle::Json,
            json_mode: true,
            max_completion_tokens: false,
            egress: None,
            timeout_secs: None,
            effort: None,
            extra: toml::Table::new(),
            latency_ms: None,
        }
    }

    fn api_key(&self) -> anyhow::Result<Option<String>> {
        self.api_key_env
            .as_ref()
            .map(|name| {
                std::env::var(name).with_context(|| {
                    format!(
                        "model {:?}: environment variable {name} is not set",
                        self.id
                    )
                })
            })
            .transpose()
    }

    /// The proxy to reach the model through, if any.
    fn proxy(&self, egress_proxy: Option<&str>) -> Option<String> {
        let egress = self
            .egress
            .unwrap_or(self.backend == BackendKind::Anthropic);
        match (egress, egress_proxy) {
            (true, Some(proxy)) => Some(proxy.to_string()),
            (true, None) => {
                tracing::warn!(model = %self.id, "no --egress-proxy: reaching the model directly");
                None
            }
            (false, _) => None,
        }
    }

    /// Sets the model up. `egress_proxy` is the engine's `--egress-proxy`.
    pub fn build(&self, egress_proxy: Option<&str>) -> anyhow::Result<Model> {
        let remote_model = self.model.clone().unwrap_or_else(|| self.id.clone());
        let timeout = self.timeout_secs.map(Duration::from_secs);
        let backend: Arc<dyn Backend> = match self.backend {
            BackendKind::Mock => Arc::new(MockBackend::new(MockConfig {
                latency: self
                    .latency_ms
                    .map(Duration::from_millis)
                    .unwrap_or(MockConfig::default().latency),
                ..MockConfig::default()
            })),
            BackendKind::Openai => {
                let base_url = self
                    .base_url
                    .clone()
                    .with_context(|| format!("model {:?}: base_url is required", self.id))?;
                let mut config = OpenAiCompatConfig::new(base_url, remote_model);
                config.api_key = self.api_key()?;
                config.style = self.style;
                config.json_mode = self.json_mode;
                config.max_completion_tokens = self.max_completion_tokens;
                config.extra = match serde_json::to_value(&self.extra)? {
                    serde_json::Value::Object(extra) => extra,
                    _ => unreachable!("a TOML table is an object"),
                };
                config.timeout = timeout.unwrap_or(config.timeout);
                config.proxy = self.proxy(egress_proxy);
                Arc::new(OpenAiCompatBackend::new(config)?)
            }
            BackendKind::Anthropic => {
                let api_key = self
                    .api_key()?
                    .with_context(|| format!("model {:?}: anthropic needs api_key_env", self.id))?;
                let mut config = AnthropicConfig::new(api_key, remote_model);
                if let Some(base_url) = &self.base_url {
                    config.base_url = base_url.clone();
                }
                if self.effort.is_some() {
                    config.effort = self.effort.clone();
                }
                config.timeout = timeout.unwrap_or(config.timeout);
                config.proxy = self.proxy(egress_proxy);
                Arc::new(AnthropicBackend::new(config)?)
            }
        };
        Ok(Model::new(
            self.id.clone(),
            self.name.clone().unwrap_or_else(|| self.id.clone()),
            backend,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_models_file() {
        let models = parse(
            r#"
            [[model]]
            id = "hy-mt2-1.8b"
            name = "Hy-MT2 1.8B"
            backend = "openai"
            base_url = "http://127.0.0.1:8000/v1"
            style = "plain"

            [[model]]
            id = "qwen3.5-4b"
            backend = "openai"
            base_url = "http://127.0.0.1:8001/v1"
            [model.extra]
            chat_template_kwargs = { enable_thinking = false }

            [[model]]
            id = "mock"
            backend = "mock"
            "#,
        )
        .unwrap();
        assert_eq!(models.len(), 3);
        assert_eq!(models[0].style, PromptStyle::Plain);
        assert_eq!(models[1].style, PromptStyle::Json);
        assert_eq!(
            models[1].extra["chat_template_kwargs"]["enable_thinking"].as_bool(),
            Some(false)
        );

        let built = models
            .iter()
            .map(|model| model.build(None).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(built[0].name, "Hy-MT2 1.8B");
        assert_eq!(built[1].name, "qwen3.5-4b");
        assert_eq!(built[2].backend.kind(), "mock");
    }

    #[test]
    fn rejects_bad_files() {
        assert!(parse("").is_err());
        assert!(parse("[[model]]\nid = \"Big Model\"\nbackend = \"mock\"").is_err());
        assert!(parse("[[model]]\nid = \"a\"\nbackend = \"mock\"\ncolour = 1").is_err());
        let twice = "[[model]]\nid = \"a\"\nbackend = \"mock\"\n".repeat(2);
        assert!(parse(&twice).is_err());
        let no_url = &parse("[[model]]\nid = \"a\"\nbackend = \"openai\"").unwrap()[0];
        assert!(no_url.build(None).is_err());
    }
}
