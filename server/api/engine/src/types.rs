#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TranslateRequest {
    pub layers: Vec<ContextLayer>,
    /// Recent messages of the conversation, oldest first. The window only
    /// grows between resets, so a prefix cache keeps hitting.
    pub window: Vec<SourceMessage>,
    pub message: SourceMessage,
    /// Normalized tag of the source language.
    pub source_lang: String,
    /// Every target is translated directly from the source (N to N): no
    /// pivot language, so errors do not compound.
    pub targets: Vec<TargetLanguage>,
    /// One of `info().models`. Null: the engine's default.
    pub model: Option<String>,
}

/// One layer of context, ordered from the broadest (the space) to the
/// narrowest (the rolling summary of the conversation).
///
/// A given `id` and `version` always carry the same `content`, so an engine
/// may treat the pair as a cache key for the rendered prefix.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ContextLayer {
    pub id: String,
    pub version: i64,
    pub kind: LayerKind,
    /// Language the content is written in, when known.
    pub lang: Option<String>,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum LayerKind {
    /// Server name, description digest, glossary and style of a space.
    #[serde(rename = "space")]
    Space,
    /// A category or channel: its name and topic.
    #[serde(rename = "conversation")]
    Conversation,
    /// Where a thread forked from: parent conversation and starter message,
    /// frozen when the thread was created.
    #[serde(rename = "fork")]
    Fork,
    /// Rolling summary of the messages that left the window.
    #[serde(rename = "summary")]
    Summary,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SourceMessage {
    pub author: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TargetLanguage {
    /// Normalized BCP 47 tag, e.g. "pt-BR".
    pub tag: String,
    /// English name for prompts, e.g. "Portuguese (Brazil)".
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TranslateResponse {
    pub translations: Vec<Translation>,
    pub usage: Usage,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Translation {
    pub lang: String,
    pub text: String,
}

/// Token accounting. The mock backend estimates these so that cache hit
/// rates can be measured before real models are wired in.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Usage {
    pub input_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub output_tokens: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum EngineError {
    #[serde(rename = "unavailable")]
    Unavailable,
    #[serde(rename = "rate_limited")]
    RateLimited,
    #[serde(rename = "invalid_output")]
    InvalidOutput,
    #[serde(rename = "unsupported_language")]
    UnsupportedLanguage,
    /// The request names a model the engine does not serve.
    #[serde(rename = "unknown_model")]
    UnknownModel,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SummarizeRequest {
    pub purpose: SummaryPurpose,
    /// Summary to extend, if any.
    pub previous: Option<String>,
    pub messages: Vec<SourceMessage>,
    pub max_chars: i64,
    /// Language to write the summary in. Null keeps the source language.
    pub lang: Option<String>,
    /// One of `info().models`. Null: the engine's default.
    pub model: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum SummaryPurpose {
    /// Condense descriptions and reference text into a context layer.
    #[serde(rename = "digest")]
    Digest,
    /// Fold messages that left the window into the rolling summary.
    #[serde(rename = "conversation")]
    Conversation,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SummarizeResponse {
    pub text: String,
    pub usage: Usage,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct EngineInfo {
    /// Every model the engine serves; the first is its default.
    pub models: Vec<ModelInfo>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ModelInfo {
    /// What requests and space policies name the model by.
    pub id: String,
    /// Shown to people choosing a model.
    pub name: String,
    /// How the engine reaches it: "mock", "openai-compatible", "anthropic".
    pub backend: String,
}



use serde::{Serialize, Deserialize};
use async_trait::async_trait;

#[async_trait]
pub trait Engine: Send + Sync + 'static {
    async fn translate(&self, body: TranslateRequest) -> Result<TranslateResponse, EngineError>;
    async fn summarize(&self, body: SummarizeRequest) -> Result<SummarizeResponse, EngineError>;
    async fn info(&self) -> EngineInfo;
}

