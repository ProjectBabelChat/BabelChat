//! A backend for OpenAI-compatible chat completion servers: vLLM, llama.cpp
//! (`llama-server`), Ollama, LM Studio and similar. This is the self-hosting
//! path for people running their own models (e.g. Hy-MT2 or Qwen3).
//!
//! These servers cache prompt prefixes automatically, so keeping the system
//! prompt and the layered user prompt stable is all it takes. When several
//! replicas serve one model, route requests by space so that each space's
//! prefix stays warm on one replica (see docs/architecture/translation-engine.md).

use std::time::Duration;

use async_trait::async_trait;
use babel_api_engine::types::{
    EngineError, EngineInfo, SummarizeRequest, SummarizeResponse, TranslateRequest,
    TranslateResponse, Usage,
};
use serde::{Deserialize, Serialize};

use crate::{
    Backend,
    prompt::{Prompt, parse_translations, summary_prompt, translation_prompt},
};

#[derive(Debug, Clone)]
pub struct OpenAiCompatConfig {
    /// Base URL including the API version, e.g. `http://localhost:8000/v1`.
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
    /// Ask for `response_format: json_object`. Most servers support it.
    pub json_mode: bool,
    pub timeout: Duration,
}

pub struct OpenAiCompatBackend {
    client: reqwest::Client,
    config: OpenAiCompatConfig,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: [ChatMessage<'a>; 2],
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct ResponseFormat {
    r#type: &'static str,
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

impl OpenAiCompatBackend {
    pub fn new(config: OpenAiCompatConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .expect("reqwest client builds");
        Self { client, config }
    }

    async fn complete(
        &self,
        prompt: &Prompt,
        max_tokens: u32,
        json: bool,
    ) -> Result<(String, Usage), EngineError> {
        let user = prompt.user();
        let body = ChatRequest {
            model: &self.config.model,
            messages: [
                ChatMessage {
                    role: "system",
                    content: prompt.system(),
                },
                ChatMessage {
                    role: "user",
                    content: &user,
                },
            ],
            max_tokens,
            response_format: (json && self.config.json_mode).then_some(ResponseFormat {
                r#type: "json_object",
            }),
        };

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
        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(EngineError::RateLimited);
        }
        if !response.status().is_success() {
            tracing::warn!(status = %response.status(), "chat completion failed");
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
            .unwrap_or(Usage {
                input_tokens: 0,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                output_tokens: 0,
            });

        Ok((text, usage))
    }
}

#[async_trait]
impl Backend for OpenAiCompatBackend {
    fn info(&self) -> EngineInfo {
        EngineInfo {
            backend: "openai-compatible".into(),
            model: self.config.model.clone(),
        }
    }

    async fn translate(
        &self,
        request: &TranslateRequest,
    ) -> Result<TranslateResponse, EngineError> {
        // Generous room for every target plus JSON overhead.
        let source_chars = request.message.text.chars().count() as u32;
        let max_tokens = 256 + source_chars * 3 * request.targets.len() as u32;

        let (text, usage) = self
            .complete(&translation_prompt(request), max_tokens, true)
            .await?;
        Ok(TranslateResponse {
            translations: parse_translations(&text, request)?,
            usage,
        })
    }

    async fn summarize(
        &self,
        request: &SummarizeRequest,
    ) -> Result<SummarizeResponse, EngineError> {
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

    /// A fake chat completion server that checks the request shape.
    async fn serve_fake() -> String {
        async fn complete(Json(body): Json<serde_json::Value>) -> Json<serde_json::Value> {
            assert_eq!(body["model"], "test-model");
            assert_eq!(body["messages"][0]["role"], "system");
            assert_eq!(body["response_format"]["type"], "json_object");
            let user = body["messages"][1]["content"].as_str().unwrap();
            assert!(user.contains("## TRANSLATE"));

            Json(serde_json::json!({
                "choices": [{ "message": { "content": "{\"translations\": {\"ja-JP\": \"こんにちは\"}}" } }],
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

    #[tokio::test]
    async fn translates_through_chat_completions() {
        let backend = OpenAiCompatBackend::new(OpenAiCompatConfig {
            base_url: serve_fake().await,
            api_key: Some("secret".into()),
            model: "test-model".into(),
            json_mode: true,
            timeout: Duration::from_secs(5),
        });

        let response = backend
            .translate(&TranslateRequest {
                layers: Vec::new(),
                window: Vec::new(),
                message: SourceMessage {
                    author: "alice".into(),
                    text: "hello".into(),
                },
                source_lang: "en-US".into(),
                targets: vec![TargetLanguage {
                    tag: "ja-JP".into(),
                    name: "Japanese".into(),
                }],
            })
            .await
            .unwrap();

        assert_eq!(response.translations[0].text, "こんにちは");
        assert_eq!(response.usage.cache_read_tokens, 1000);
        assert_eq!(response.usage.input_tokens, 200);
    }
}
