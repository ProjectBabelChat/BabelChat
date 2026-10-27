//! A backend for OpenAI-compatible chat completion servers: vLLM, llama.cpp
//! (`llama-server`), Ollama, LM Studio and similar, and hosted APIs that
//! speak the same protocol (OpenAI itself among them). This is the
//! self-hosting path for people running their own models (e.g. Hy-MT2 or
//! Qwen3.5).
//!
//! These servers cache prompt prefixes automatically, so keeping the system
//! prompt and the layered user prompt stable is all it takes. When several
//! replicas serve one model, route requests by space so that each space's
//! prefix stays warm on one replica (see docs/architecture/translation-engine.md).

use std::time::Duration;

use async_trait::async_trait;
use babel_api_engine::types::{
    EngineError, SummarizeRequest, SummarizeResponse, TranslateRequest, TranslateResponse,
    Translation, Usage,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{
    Backend, add_usage, no_usage,
    prompt::{
        Prompt, PromptStyle, extractive_summary, parse_translations, plain_translation_prompt,
        strip_reasoning, summary_prompt, translation_prompt,
    },
};

#[derive(Debug, Clone)]
pub struct OpenAiCompatConfig {
    /// Base URL including the API version, e.g. `http://localhost:8000/v1`.
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    pub style: PromptStyle,
    /// Ask for `response_format: json_object` with [`PromptStyle::Json`].
    /// Most servers support it.
    pub json_mode: bool,
    /// Send the output limit as `max_completion_tokens`, which OpenAI's
    /// newer models require, instead of `max_tokens`.
    pub max_completion_tokens: bool,
    /// Merged into every request body, e.g.
    /// `chat_template_kwargs: {enable_thinking: false}` for models that
    /// would otherwise think before translating.
    pub extra: Map<String, Value>,
    pub timeout: Duration,
    /// Send requests through this proxy (the egress proxy,
    /// `http://host:port`) instead of directly.
    pub proxy: Option<String>,
}

impl OpenAiCompatConfig {
    /// A config with the defaults for everything but where to reach the
    /// model.
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_key: None,
            model: model.into(),
            style: PromptStyle::Json,
            json_mode: true,
            max_completion_tokens: false,
            extra: Map::new(),
            timeout: Duration::from_secs(60),
            proxy: None,
        }
    }
}

pub struct OpenAiCompatBackend {
    client: reqwest::Client,
    config: OpenAiCompatConfig,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<ChatUsage>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize)]
struct ChatUsage {
    #[serde(default)]
    prompt_tokens: i64,
    #[serde(default)]
    completion_tokens: i64,
    #[serde(default)]
    prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Deserialize)]
struct PromptTokensDetails {
    #[serde(default)]
    cached_tokens: i64,
}

/// Room for a translation of `text` into `targets` languages: scripts differ
/// a lot in tokens per character, so generously.
fn translation_budget(text: &str, targets: usize, overhead: u32) -> u32 {
    overhead + text.chars().count() as u32 * 3 * targets as u32
}

impl OpenAiCompatBackend {
    /// Fails if the proxy URL is not one.
    pub fn new(config: OpenAiCompatConfig) -> Result<Self, reqwest::Error> {
        let mut client = reqwest::Client::builder().timeout(config.timeout);
        client = match &config.proxy {
            Some(proxy) => client.proxy(reqwest::Proxy::all(proxy)?),
            // Never a proxy from the environment by accident.
            None => client.no_proxy(),
        };
        Ok(Self {
            client: client.build()?,
            config,
        })
    }

    async fn complete(
        &self,
        prompt: &Prompt,
        max_tokens: u32,
        json: bool,
    ) -> Result<(String, Usage), EngineError> {
        let mut messages = Vec::new();
        if !prompt.system().is_empty() {
            messages.push(json!({ "role": "system", "content": prompt.system() }));
        }
        messages.push(json!({ "role": "user", "content": prompt.user() }));

        let mut body = Map::new();
        body.insert("model".into(), json!(self.config.model));
        body.insert("messages".into(), Value::Array(messages));
        let limit = if self.config.max_completion_tokens {
            "max_completion_tokens"
        } else {
            "max_tokens"
        };
        body.insert(limit.into(), json!(max_tokens));
        if json && self.config.json_mode {
            body.insert("response_format".into(), json!({ "type": "json_object" }));
        }
        for (key, value) in &self.config.extra {
            body.insert(key.clone(), value.clone());
        }

        let url = format!(
            "{}/chat/completions",
            self.config.base_url.trim_end_matches('/')
        );
        let mut request = self.client.post(url).json(&body);
        if let Some(api_key) = &self.config.api_key {
            request = request.bearer_auth(api_key);
        }

        let response = request.send().await.map_err(|error| {
            tracing::warn!(%error, "chat completion request failed");
            EngineError::Unavailable
        })?;
        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(EngineError::RateLimited);
        }
        if !status.is_success() {
            let detail = response.text().await.unwrap_or_default();
            let detail = detail.chars().take(500).collect::<String>();
            tracing::warn!(%status, %detail, "chat completion failed");
            return Err(EngineError::Unavailable);
        }

        let response: ChatResponse = response
            .json()
            .await
            .map_err(|_| EngineError::InvalidOutput)?;
        let text = response
            .choices
            .into_iter()
            .next()
            .and_then(|choice| choice.message.content)
            .map(|content| strip_reasoning(&content).to_string())
            .ok_or(EngineError::InvalidOutput)?;

        let usage = response
            .usage
            .map(|usage| {
                let cached = usage
                    .prompt_tokens_details
                    .map(|details| details.cached_tokens)
                    .unwrap_or(0);
                Usage {
                    input_tokens: usage.prompt_tokens - cached,
                    cache_read_tokens: cached,
                    cache_write_tokens: 0,
                    output_tokens: usage.completion_tokens,
                }
            })
            .unwrap_or_else(no_usage);

        Ok((text, usage))
    }
}

#[async_trait]
impl Backend for OpenAiCompatBackend {
    fn kind(&self) -> &'static str {
        "openai-compatible"
    }

    async fn translate(
        &self,
        request: &TranslateRequest,
    ) -> Result<TranslateResponse, EngineError> {
        let text = &request.message.text;
        match self.config.style {
            PromptStyle::Json => {
                let max_tokens = translation_budget(text, request.targets.len(), 256);
                let (output, usage) = self
                    .complete(&translation_prompt(request), max_tokens, true)
                    .await?;
                Ok(TranslateResponse {
                    translations: parse_translations(&output, request)?,
                    usage,
                })
            }
            PromptStyle::Plain => {
                // One target after the other: they share the background, so
                // a server with a single slot reuses it from its cache.
                let mut translations = Vec::new();
                let mut total = no_usage();
                for target in &request.targets {
                    let (output, usage) = self
                        .complete(
                            &plain_translation_prompt(request, target),
                            translation_budget(text, 1, 128),
                            false,
                        )
                        .await?;
                    add_usage(&mut total, &usage);
                    let output = output.trim();
                    if output.is_empty() {
                        return Err(EngineError::InvalidOutput);
                    }
                    translations.push(Translation {
                        lang: target.tag.clone(),
                        text: output.to_string(),
                    });
                }
                Ok(TranslateResponse {
                    translations,
                    usage: total,
                })
            }
        }
    }

    async fn summarize(
        &self,
        request: &SummarizeRequest,
    ) -> Result<SummarizeResponse, EngineError> {
        if self.config.style == PromptStyle::Plain {
            // Translation models only translate.
            return Ok(SummarizeResponse {
                text: extractive_summary(request),
                usage: no_usage(),
            });
        }
        let max_tokens = (request.max_chars.max(64) as u32).saturating_mul(2);
        let (text, usage) = self
            .complete(&summary_prompt(request), max_tokens, false)
            .await?;
        Ok(SummarizeResponse {
            text: text.trim().to_string(),
            usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use axum::{Json, Router, routing::post};
    use babel_api_engine::types::{SourceMessage, TargetLanguage};

    use super::*;

    /// A fake chat completion server that checks the request shape and
    /// answers in the style it was asked in.
    async fn serve_fake() -> String {
        async fn complete(Json(body): Json<Value>) -> Json<Value> {
            assert_eq!(body["model"], "test-model");
            assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
            let messages = body["messages"].as_array().unwrap();
            let user = messages.last().unwrap()["content"].as_str().unwrap();
            let content = if user.contains("## TRANSLATE") {
                assert_eq!(messages[0]["role"], "system");
                assert_eq!(body["response_format"]["type"], "json_object");
                assert!(body["max_tokens"].is_u64());
                "<think>\n\n</think>\n\n{\"translations\": {\"ja-JP\": \"こんにちは\", \"ko-KR\": \"안녕하세요\"}}"
            } else {
                assert_eq!(messages.len(), 1, "plain prompts have no system message");
                assert!(body.get("response_format").is_none());
                assert!(body["max_completion_tokens"].is_u64());
                if user.contains("into Japanese") {
                    " こんにちは\n"
                } else {
                    "안녕하세요"
                }
            };
            Json(json!({
                "choices": [{ "message": { "content": content } }],
                "usage": { "prompt_tokens": 1200, "completion_tokens": 9,
                           "prompt_tokens_details": { "cached_tokens": 1000 } }
            }))
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route("/v1/chat/completions", post(complete));
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{address}/v1")
    }

    fn request() -> TranslateRequest {
        TranslateRequest {
            layers: Vec::new(),
            window: Vec::new(),
            message: SourceMessage {
                author: "alice".into(),
                text: "hello".into(),
            },
            source_lang: "en-US".into(),
            targets: vec![
                TargetLanguage {
                    tag: "ja-JP".into(),
                    name: "Japanese".into(),
                },
                TargetLanguage {
                    tag: "ko-KR".into(),
                    name: "Korean".into(),
                },
            ],
            model: None,
        }
    }

    async fn backend(style: PromptStyle) -> OpenAiCompatBackend {
        let mut config = OpenAiCompatConfig::new(serve_fake().await, "test-model");
        config.api_key = Some("secret".into());
        config.style = style;
        config.max_completion_tokens = style == PromptStyle::Plain;
        config.extra.insert(
            "chat_template_kwargs".into(),
            json!({ "enable_thinking": false }),
        );
        OpenAiCompatBackend::new(config).unwrap()
    }

    #[tokio::test]
    async fn translates_through_chat_completions() {
        let response = backend(PromptStyle::Json)
            .await
            .translate(&request())
            .await
            .unwrap();
        assert_eq!(response.translations[0].text, "こんにちは");
        assert_eq!(response.translations[1].text, "안녕하세요");
        assert_eq!(response.usage.cache_read_tokens, 1000);
        assert_eq!(response.usage.input_tokens, 200);
    }

    #[tokio::test]
    async fn plain_models_get_one_request_per_target() {
        let backend = backend(PromptStyle::Plain).await;
        let response = backend.translate(&request()).await.unwrap();
        assert_eq!(response.translations[0].lang, "ja-JP");
        assert_eq!(response.translations[0].text, "こんにちは");
        assert_eq!(response.translations[1].text, "안녕하세요");
        assert_eq!(response.usage.cache_read_tokens, 2000);

        // Summaries do not reach the model.
        let summary = backend
            .summarize(&SummarizeRequest {
                purpose: babel_api_engine::types::SummaryPurpose::Conversation,
                previous: None,
                messages: vec![SourceMessage {
                    author: "alice".into(),
                    text: "hi".into(),
                }],
                max_chars: 100,
                lang: None,
                model: None,
            })
            .await
            .unwrap();
        assert_eq!(summary.text, "alice: hi");
        assert_eq!(summary.usage.input_tokens, 0);
    }
}
