use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum ThrowsOrOtherError<T> {
    Throws(T),
    Other(reqwest::Error),
}

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
    /// What the LLM translates with, one of `list_models`. Null: the
    /// default model.
    pub model: Option<String>,
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
    pub model: Option<String>,
}

/// A model spaces can translate with.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct TranslationModel {
    pub id: String,
    pub name: String,
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



pub struct TranslationCell {
    pub url: String,
    pub client: reqwest::Client,
}

impl TranslationCell {
    pub fn new(url: impl Into<String>) -> Self {
        let client = reqwest::Client::new();
        Self { url: url.into(), client }
    }

    pub async fn put_space(&self, __body: SpaceContext) -> Result<(), ThrowsOrOtherError<TranslationCellError>> {
        let mut request = self.client.post(format!("{}/translation_cell/put_space", &self.url));
        request = request.header("Accept", "application/json");
        request = request.json(&__body);
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    pub async fn put_node(&self, __body: ContextNode) -> Result<(), ThrowsOrOtherError<TranslationCellError>> {
        let mut request = self.client.post(format!("{}/translation_cell/put_node", &self.url));
        request = request.header("Accept", "application/json");
        request = request.json(&__body);
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// A conversation ended: its node, window and translations go. Other
    /// nodes naming it as their parent keep theirs.
    pub async fn delete_node(&self, space: String, node: String) -> Result<(), ThrowsOrOtherError<TranslationCellError>> {
        let mut request = self.client.post(format!("{}/translation_cell/delete_node", &self.url));
        request = request.query(&[("space", serde_json::to_string(&space).unwrap()), ("node", serde_json::to_string(&node).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    pub async fn delete_space(&self, space: String) -> Result<(), ThrowsOrOtherError<TranslationCellError>> {
        let mut request = self.client.post(format!("{}/translation_cell/delete_space", &self.url));
        request = request.query(&[("space", serde_json::to_string(&space).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    pub async fn get_policy(&self, space: String) -> Result<SpacePolicy, ThrowsOrOtherError<TranslationCellError>> {
        let mut request = self.client.get(format!("{}/translation_cell/get_policy", &self.url));
        request = request.query(&[("space", serde_json::to_string(&space).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Called by the platform after it checked the caller may manage the
    /// space; the Translation Cell has no notion of roles.
    pub async fn put_policy(&self, __body: PolicyUpdate) -> Result<SpacePolicy, ThrowsOrOtherError<TranslationCellError>> {
        let mut request = self.client.post(format!("{}/translation_cell/put_policy", &self.url));
        request = request.header("Accept", "application/json");
        request = request.json(&__body);
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Billing hook. Until billing exists, plans are set by configuration.
    pub async fn set_plan(&self, space: String, plan: Plan) -> Result<SpacePolicy, ThrowsOrOtherError<TranslationCellError>> {
        let mut request = self.client.post(format!("{}/translation_cell/set_plan", &self.url));
        request = request.query(&[("space", serde_json::to_string(&space).unwrap()), ("plan", serde_json::to_string(&plan).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Models the space may translate with; the first is the default.
    pub async fn list_models(&self, space: String) -> Result<Vec<TranslationModel>, ThrowsOrOtherError<TranslationCellError>> {
        let mut request = self.client.get(format!("{}/translation_cell/list_models", &self.url));
        request = request.query(&[("space", serde_json::to_string(&space).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Normalizes a language tag with the space's policy. Rust callers use
    /// the babel-lang crate on a policy snapshot instead of calling this.
    pub async fn normalize(&self, space: String, tag: String) -> Result<String, ThrowsOrOtherError<TranslationCellError>> {
        let mut request = self.client.get(format!("{}/translation_cell/normalize", &self.url));
        request = request.query(&[("space", serde_json::to_string(&space).unwrap()), ("tag", serde_json::to_string(&tag).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    pub async fn resolve_viewer(&self, space: String, locale: String, translate_outside_set: bool) -> Result<ViewerLanguage, ThrowsOrOtherError<TranslationCellError>> {
        let mut request = self.client.get(format!("{}/translation_cell/resolve_viewer", &self.url));
        request = request.query(&[("space", serde_json::to_string(&space).unwrap()), ("locale", serde_json::to_string(&locale).unwrap()), ("translate_outside_set", serde_json::to_string(&translate_outside_set).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Translation starts as soon as a message arrives. Idempotent for the
    /// same message and revision.
    pub async fn ingest(&self, __body: IngestMessage) -> Result<(), ThrowsOrOtherError<TranslationCellError>> {
        let mut request = self.client.post(format!("{}/translation_cell/ingest", &self.url));
        request = request.header("Accept", "application/json");
        request = request.json(&__body);
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    pub async fn delete_message(&self, __body: MessageRef) -> Result<(), ThrowsOrOtherError<TranslationCellError>> {
        let mut request = self.client.post(format!("{}/translation_cell/delete_message", &self.url));
        request = request.header("Accept", "application/json");
        request = request.json(&__body);
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                Ok(())
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    pub async fn get_translations(&self, __body: TranslationQuery) -> Result<Vec<MessageTranslation>, ThrowsOrOtherError<TranslationCellError>> {
        let mut request = self.client.post(format!("{}/translation_cell/get_translations", &self.url));
        request = request.header("Accept", "application/json");
        request = request.json(&__body);
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    /// Long poll: returns as soon as there are events after `after`, or
    /// after `wait_ms` with none.
    pub async fn poll_events(&self, space: String, after: i64, wait_ms: i64) -> Result<TranslationEventBatch, ThrowsOrOtherError<TranslationCellError>> {
        let mut request = self.client.get(format!("{}/translation_cell/poll_events", &self.url));
        request = request.query(&[("space", serde_json::to_string(&space).unwrap()), ("after", serde_json::to_string(&after).unwrap()), ("wait_ms", serde_json::to_string(&wait_ms).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
    pub async fn get_usage(&self, space: String) -> Result<SpaceUsage, ThrowsOrOtherError<TranslationCellError>> {
        let mut request = self.client.get(format!("{}/translation_cell/get_usage", &self.url));
        request = request.query(&[("space", serde_json::to_string(&space).unwrap())]);
        request = request.header("Accept", "application/json");
        let response = request.send().await.map_err(|error| ThrowsOrOtherError::Other(error))?;

        match response.error_for_status_ref() {
            Ok(_) => {
                response.json().await.map_err(|error| ThrowsOrOtherError::Other(error))
            }
            Err(error) => {
                match response.json().await {
                    Ok(value) => Err(ThrowsOrOtherError::Throws(value)),
                    Err(_) => Err(ThrowsOrOtherError::Other(error)),
                }
            }
        }
    }
}
