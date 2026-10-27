/// Root context of a space, kept as a translation-ready copy. The platform
/// stays the source of truth and re-sends it when it changes.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SpaceContext {
    pub space: String,
    pub name: String,
    pub description: Option<String>,
    /// Language the description is written in, when known. English is
    /// recommended to owners but not required.
    pub description_lang: Option<String>,
    pub glossary: Vec<Term>,
}

/// A glossary entry: names and jargon the engine should keep consistent.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Term {
    pub term: String,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum TranslationCellError {
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "invalid_language")]
    InvalidLanguage,
    #[serde(rename = "limit_exceeded")]
    LimitExceeded,
    #[serde(rename = "invalid_request")]
    InvalidRequest,
    #[serde(rename = "unavailable")]
    Unavailable,
}

/// A conversation node under a space. Nodes form a tree (category > channel
/// > thread); prompts are built by walking from a conversation up to the root.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ContextNode {
    pub space: String,
    pub id: String,
    /// Parent node. Null attaches the node directly to the space.
    pub parent: Option<String>,
    pub kind: NodeKind,
    pub title: String,
    pub description: Option<String>,
    /// For threads: the message the thread was started from.
    pub starter: Option<MessageRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum NodeKind {
    #[serde(rename = "category")]
    Category,
    #[serde(rename = "channel")]
    Channel,
    #[serde(rename = "thread")]
    Thread,
}

/// A message as the platform identifies it.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct MessageRef {
    /// A guild, workspace or server.
    pub space: String,
    /// A channel or thread: the unit of conversation context.
    pub conversation: String,
    pub id: String,
}

/// The language policy of a space. The Translation Cell is its source of
/// truth; Edges keep a copy to resolve each viewer's language.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SpacePolicy {
    pub space: String,
    /// Increases on every change.
    pub version: i64,
    pub plan: Plan,
    /// Languages the space declares, normalized. Includes the LLM ones.
    pub languages: Vec<String>,
    /// Languages translated by the LLM as soon as a message arrives.
    pub llm_languages: Vec<String>,
    pub max_languages: i64,
    pub max_llm_languages: i64,
    pub region_mode: RegionMode,
    pub aliases: Vec<LanguageAlias>,
    /// Whether to translate between regional variants of one language
    /// (pt-BR to pt-PT) in distinct mode.
    pub translate_regional_variants: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum Plan {
    #[serde(rename = "free")]
    Free,
    #[serde(rename = "paid1")]
    Paid1,
    #[serde(rename = "paid2")]
    Paid2,
    #[serde(rename = "paid3")]
    Paid3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum RegionMode {
    /// Regional variants collapse to one tag: en-GB becomes en-US.
    #[serde(rename = "merged")]
    Merged,
    /// Regional variants stay distinct: en-GB and en-US are two languages.
    #[serde(rename = "distinct")]
    Distinct,
}

/// A space-specific normalization override, e.g. "pt" to "pt-PT".
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct LanguageAlias {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct PolicyUpdate {
    pub space: String,
    pub languages: Vec<String>,
    pub llm_languages: Vec<String>,
    pub region_mode: RegionMode,
    pub aliases: Vec<LanguageAlias>,
    pub translate_regional_variants: bool,
}

/// How one viewer reads a space.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct ViewerLanguage {
    /// The viewer's locale normalized with the space's policy.
    pub effective: String,
    /// Whether `effective` is one of the space's languages.
    pub in_set: bool,
    /// Whether the LLM translates into `effective`.
    pub llm: bool,
    /// Whether the client should translate on device into `effective`.
    pub client_translation: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct IngestMessage {
    pub message: MessageRef,
    /// Opaque revision chosen by the platform; it changes when the text is
    /// edited. Translations carry the revision they were made from.
    pub rev: String,
    pub author: String,
    /// The author's locale, used to fill in the region (and script) that
    /// language detection cannot tell from the text.
    pub author_locale: Option<String>,
    pub text: String,
    pub reply_to: Option<MessageRef>,
    pub sent_at: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TranslationQuery {
    pub messages: Vec<MessageRef>,
    /// Languages to return. Edges ask for the viewer's language and English
    /// (the pivot for on-device translation).
    pub langs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct MessageTranslation {
    pub message: MessageRef,
    pub rev: String,
    /// Detected language, normalized. Clients pick their on-device models
    /// from it, so all clients agree on the source language.
    pub source_lang: String,
    pub status: TranslationStatus,
    pub translations: Vec<Translation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum TranslationStatus {
    /// Translations are ready (possibly none, for free spaces).
    #[serde(rename = "done")]
    Done,
    #[serde(rename = "pending")]
    Pending,
    /// The daily translation budget of the space ran out; clients fall back
    /// to on-device translation.
    #[serde(rename = "budget_exceeded")]
    BudgetExceeded,
    #[serde(rename = "failed")]
    Failed,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Translation {
    pub lang: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TranslationEventBatch {
    pub events: Vec<TranslationEvent>,
    /// Pass as `after` to the next poll.
    pub next: i64,
    /// Sequence number of the newest event of the space. Polling with an
    /// `after` beyond it returns at once, so followers can start at the head.
    pub head: i64,
    /// Events up to this sequence number were trimmed from the log. A
    /// follower whose `after` is below it has missed some.
    pub trimmed: i64,
}

/// One event of a space. Exactly one payload is set, chosen by `kind`.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TranslationEvent {
    pub seq: i64,
    pub kind: TranslationEventKind,
    pub translated: Option<MessageTranslation>,
    pub removed: Option<MessageRef>,
    pub policy: Option<SpacePolicy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum TranslationEventKind {
    #[serde(rename = "translated")]
    Translated,
    #[serde(rename = "removed")]
    Removed,
    #[serde(rename = "policy")]
    Policy,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct SpaceUsage {
    pub space: String,
    pub translated_messages: i64,
    pub input_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub output_tokens: i64,
}



use serde::{Serialize, Deserialize};
use async_trait::async_trait;

#[async_trait]
pub trait TranslationCell: Send + Sync + 'static {
    async fn put_space(&self, body: SpaceContext) -> Result<(), TranslationCellError>;
    async fn put_node(&self, body: ContextNode) -> Result<(), TranslationCellError>;
    async fn delete_space(&self, space: String) -> Result<(), TranslationCellError>;
    async fn get_policy(&self, space: String) -> Result<SpacePolicy, TranslationCellError>;
    /// Called by the platform after it checked the caller may manage the
    /// space; the Translation Cell has no notion of roles.
    async fn put_policy(&self, body: PolicyUpdate) -> Result<SpacePolicy, TranslationCellError>;
    /// Billing hook. Until billing exists, plans are set by configuration.
    async fn set_plan(&self, space: String, plan: Plan) -> Result<SpacePolicy, TranslationCellError>;
    /// Normalizes a language tag with the space's policy. Rust callers use
    /// the babel-lang crate on a policy snapshot instead of calling this.
    async fn normalize(&self, space: String, tag: String) -> Result<String, TranslationCellError>;
    async fn resolve_viewer(&self, space: String, locale: String, translate_outside_set: bool) -> Result<ViewerLanguage, TranslationCellError>;
    /// Translation starts as soon as a message arrives. Idempotent for the
    /// same message and revision.
    async fn ingest(&self, body: IngestMessage) -> Result<(), TranslationCellError>;
    async fn delete_message(&self, body: MessageRef) -> Result<(), TranslationCellError>;
    async fn get_translations(&self, body: TranslationQuery) -> Result<Vec<MessageTranslation>, TranslationCellError>;
    /// Long poll: returns as soon as there are events after `after`, or
    /// after `wait_ms` with none.
    async fn poll_events(&self, space: String, after: i64, wait_ms: i64) -> Result<TranslationEventBatch, TranslationCellError>;
    async fn get_usage(&self, space: String) -> Result<SpaceUsage, TranslationCellError>;
}

