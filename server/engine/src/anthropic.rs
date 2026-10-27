//! A backend for Claude through Anthropic's Messages API.
//!
//! Anthropic's OpenAI-compatible endpoint ignores `response_format` and has
//! no prompt caching, which the layered prompts depend on to stay cheap; this
//! backend speaks the native API instead. Every breakpoint of the prompt
//! (`prompt.rs`) becomes a `cache_control` marker: the instructions, the
//! space, the conversation and the end of the window, the four the API
//! allows.

use std::time::Duration;

use async_trait::async_trait;
use babel_api_engine::types::{
    EngineError, SummarizeRequest, SummarizeResponse, TranslateRequest, TranslateResponse, Usage,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{
    Backend,
    prompt::{Prompt, parse_translations, summary_prompt, translation_prompt},
};

const API_VERSION: &str = "2026-10-21";
/// The most `cache_control` markers one request may carry.
const MAX_BREAKPOINTS: usize = 4;
/// Output room for thinking, which some models always do before answering.
const THINKING_ROOM: u32 = 4096;

#[derive(Debug, Clone)]
pub struct AnthropicConfig {
    /// `https://api.anthropic.com` unless a gateway sits in between.
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    /// `output_config.effort`: how hard the model thinks. Translation needs
    /// little; `low` keeps it fast and cheap.
    pub effort: Option<String>,
    pub timeout: Duration,
    /// Send requests through this proxy (the egress proxy).
    pub proxy: Option<String>,
}

impl AnthropicConfig {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: "https://api.anthropic.com".into(),
            api_key: api_key.into(),
            model: model.into(),
            effort: Some("low".into()),
            timeout: Duration::from_secs(120),
            proxy: None,
        }
    }
}

pub struct AnthropicBackend {
    client: reqwest::Client,
    config: AnthropicConfig,
}

#[derive(Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
    #[serde(default)]
    stop_reason: Option<String>,
    usage: MessagesUsage,
}

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct MessagesUsage {
    #[serde(default)]
    input_tokens: i64,
    #[serde(default)]
    cache_creation_input_tokens: Option<i64>,
    #[serde(default)]
    cache_read_input_tokens: Option<i64>,
    #[serde(default)]
    output_tokens: i64,
}

fn text_block(text: &str, cache: bool) -> Value {
    let mut block = json!({ "type": "text", "text": text });
    if cache {
        block["cache_control"] = json!({ "type": "ephemeral" });
    }
    block
}

impl AnthropicBackend {
    /// Fails if the proxy URL is not one.
    pub fn new(config: AnthropicConfig) -> Result<Self, reqwest::Error> {
        let mut client = reqwest::Client::builder().timeout(config.timeout);
        client = match &config.proxy {
            Some(proxy) => client.proxy(reqwest::Proxy::all(proxy)?),
            None => client.no_proxy(),
        };
        Ok(Self {
            client: client.build()?,
            config,
        })
    }

    /// The request body for `prompt`: its first segment as the system
    /// prompt, the rest as one user message of one block per segment.
    fn body(&self, prompt: &Prompt, max_tokens: u32) -> Value {
        let mut breakpoints = 0;
        let mut blocks = Vec::new();
        for segment in &prompt.segments {
            // The API refuses empty text blocks.
            if segment.text.is_empty() {
                continue;
            }
            let cache = segment.breakpoint && breakpoints < MAX_BREAKPOINTS;
            breakpoints += usize::from(cache);
            blocks.push(text_block(&segment.text, cache));
        }
        let user = if prompt.system().is_empty() {
            blocks.clone()
        } else {
            blocks.split_off(1)
        };

        let mut body = Map::new();
        body.insert("model".into(), json!(self.config.model));
        body.insert("max_tokens".into(), json!(max_tokens));
        if !prompt.system().is_empty() {
            body.insert("system".into(), Value::Array(blocks));
        }
        body.insert(
            "messages".into(),
            json!([{ "role": "user", "content": user }]),
        );
        if let Some(effort) = &self.config.effort {
            body.insert("output_config".into(), json!({ "effort": effort }));
        }
        Value::Object(body)
    }

    async fn complete(
        &self,
        prompt: &Prompt,
        max_tokens: u32,
    ) -> Result<(String, Usage), EngineError> {
        let url = format!("{}/v1/messages", self.config.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(url)
            .header("x-api-key", &self.config.api_key)
            .header("anthropic-version", API_VERSION)
            .json(&self.body(prompt, max_tokens))
            .send()
            .await
            .map_err(|error| {
                tracing::warn!(%error, "messages request failed");
                EngineError::Unavailable
            })?;
        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(EngineError::RateLimited);
        }
        if !status.is_success() {
            let detail = response.text().await.unwrap_or_default();
            let detail = detail.chars().take(500).collect::<String>();
            tracing::warn!(%status, %detail, "messages request refused");
            return Err(EngineError::Unavailable);
        }

        let response: MessagesResponse = response
            .json()
            .await
            .map_err(|_| EngineError::InvalidOutput)?;
        if let Some(reason @ ("refusal" | "max_tokens")) = response.stop_reason.as_deref() {
            tracing::warn!(reason, "the model stopped without an answer");
            return Err(EngineError::InvalidOutput);
        }
        let text = response
            .content
            .iter()
            .filter(|block| block.kind == "text")
            .filter_map(|block| block.text.as_deref())
            .collect::<String>();

        let usage = response.usage;
        Ok((
            text,
            Usage {
                input_tokens: usage.input_tokens,
                cache_read_tokens: usage.cache_read_input_tokens.unwrap_or(0),
                cache_write_tokens: usage.cache_creation_input_tokens.unwrap_or(0),
                output_tokens: usage.output_tokens,
            },
        ))
    }
}

#[async_trait]
impl Backend for AnthropicBackend {
    fn kind(&self) -> &'static str {
        "anthropic"
    }

    async fn translate(
        &self,
        request: &TranslateRequest,
    ) -> Result<TranslateResponse, EngineError> {
        let source_chars = request.message.text.chars().count() as u32;
        let max_tokens = THINKING_ROOM + 256 + source_chars * 3 * request.targets.len() as u32;
        let (output, usage) = self
            .complete(&translation_prompt(request), max_tokens)
            .await?;
        Ok(TranslateResponse {
            translations: parse_translations(&output, request)?,
            usage,
        })
    }

    async fn summarize(
        &self,
        request: &SummarizeRequest,
    ) -> Result<SummarizeResponse, EngineError> {
        let max_tokens = THINKING_ROOM + (request.max_chars.max(64) as u32).saturating_mul(2);
        let (text, usage) = self.complete(&summary_prompt(request), max_tokens).await?;
        Ok(SummarizeResponse {
            text: text.trim().to_string(),
            usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use axum::{Json, Router, http::HeaderMap, routing::post};
    use babel_api_engine::types::{ContextLayer, LayerKind, SourceMessage, TargetLanguage};

    use super::*;

    /// A fake Messages API that checks the request shape.
    async fn serve_fake() -> String {
        async fn messages(headers: HeaderMap, Json(body): Json<Value>) -> Json<Value> {
            assert_eq!(headers["x-api-key"], "secret");
            assert_eq!(headers["anthropic-version"], API_VERSION);
            assert_eq!(body["model"], "claude-test");
            assert_eq!(body["output_config"]["effort"], "low");
            assert!(body.get("temperature").is_none());

            let system = body["system"].as_array().unwrap();
            assert_eq!(system.len(), 1);
            assert_eq!(system[0]["cache_control"]["type"], "ephemeral");
            let content = body["messages"][0]["content"].as_array().unwrap();
            let markers = system
                .iter()
                .chain(content)
                .filter(|block| block.get("cache_control").is_some())
                .count();
            assert_eq!(markers, 4);
            assert!(
                content.last().unwrap()["text"]
                    .as_str()
                    .unwrap()
                    .contains("## TRANSLATE")
            );

            Json(json!({
                "content": [
                    { "type": "thinking", "thinking": "" },
                    { "type": "text", "text": "{\"translations\": {\"ja-JP\": \"やあ\"}}" }
                ],
                "stop_reason": "end_turn",
                "usage": { "input_tokens": 20, "cache_creation_input_tokens": 300,
                           "cache_read_input_tokens": 900, "output_tokens": 15 }
            }))
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route("/v1/messages", post(messages));
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn translates_through_messages_with_cache_markers() {
        let mut config = AnthropicConfig::new("secret", "claude-test");
        config.base_url = serve_fake().await;
        let backend = AnthropicBackend::new(config).unwrap();

        let response = backend
            .translate(&TranslateRequest {
                layers: vec![ContextLayer {
                    id: "space".into(),
                    version: 1,
                    kind: LayerKind::Space,
                    lang: None,
                    content: "A guild about rhythm games.".into(),
                }],
                window: vec![SourceMessage {
                    author: "bob".into(),
                    text: "anyone?".into(),
                }],
                message: SourceMessage {
                    author: "alice".into(),
                    text: "hi".into(),
                },
                source_lang: "en-US".into(),
                targets: vec![TargetLanguage {
                    tag: "ja-JP".into(),
                    name: "Japanese".into(),
                }],
                model: None,
            })
            .await
            .unwrap();

        assert_eq!(response.translations[0].text, "やあ");
        assert_eq!(response.usage.cache_read_tokens, 900);
        assert_eq!(response.usage.cache_write_tokens, 300);
        assert_eq!(response.usage.input_tokens, 20);
    }
}
