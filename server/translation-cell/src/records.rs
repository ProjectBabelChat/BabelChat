//! What a Translation Cell stores, and under which keys.
//!
//! ```text
//! table         key                                   value
//! spaces        tenant / space                        SpaceRecord
//! nodes         tenant / space / node                 NodeRecord
//! windows       tenant / space / conversation         WindowRecord
//! translations  tenant / space / conversation / id    TranslationRecord
//! events        tenant / space / seq                  TranslationEvent
//! jobs          seq                                   Job
//! ```
//!
//! Original text only lives in windows (the last few dozen messages of a
//! conversation) and in queued jobs, which are deleted once translated.

use babel_api_engine::types::TranslateRequest;
use babel_api_translation_cell::types::{
    ContextNode, MessageRef, SpaceContext, SpacePolicy, SpaceUsage, Translation, TranslationStatus,
};
use babel_store::{Key, Table};
use serde::{Deserialize, Serialize};

pub const SPACES: Table = "spaces";
pub const NODES: Table = "nodes";
pub const WINDOWS: Table = "windows";
pub const TRANSLATIONS: Table = "translations";
pub const EVENTS: Table = "events";
pub const JOBS: Table = "jobs";

pub const TABLES: &[Table] = &[SPACES, NODES, WINDOWS, TRANSLATIONS, EVENTS, JOBS];

/// Phase 0 serves one tenant. Later the tenant comes from the caller's
/// credentials, and every key already starts with it.
pub const TENANT: &str = "local";

pub fn space_key(space: &str) -> Vec<u8> {
    Key::new().str(TENANT).str(space).build()
}

pub fn node_key(space: &str, node: &str) -> Vec<u8> {
    Key::new().str(TENANT).str(space).str(node).build()
}

pub fn window_key(space: &str, conversation: &str) -> Vec<u8> {
    Key::new().str(TENANT).str(space).str(conversation).build()
}

pub fn translation_key(message: &MessageRef) -> Vec<u8> {
    Key::new()
        .str(TENANT)
        .str(&message.space)
        .str(&message.conversation)
        .str(&message.id)
        .build()
}

pub fn event_key(space: &str, seq: i64) -> Vec<u8> {
    Key::new().str(TENANT).str(space).u64(seq as u64).build()
}

pub fn job_key(seq: i64) -> Vec<u8> {
    Key::new().u64(seq as u64).build()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpaceRecord {
    pub context: Option<SpaceContext>,
    /// Version of the space context layer. Bumped when the context or its
    /// digest changes, which invalidates cached prompt prefixes once.
    pub context_version: i64,
    /// Summary of a long description, made by the engine.
    pub digest: Option<String>,
    pub policy: SpacePolicy,
    pub usage: SpaceUsage,
    /// UTC day (days since the epoch) that `budget_used` counts.
    pub budget_day: i64,
    pub budget_used: i64,
    pub last_event: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRecord {
    pub node: ContextNode,
    pub version: i64,
    /// For threads: the parent conversation as it was when the thread was
    /// created. Frozen, so the thread's prompt prefix never changes with the
    /// parent.
    pub fork: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WindowEntry {
    pub id: String,
    pub rev: String,
    pub author: String,
    pub text: String,
}

/// The recent messages of a conversation. It only grows, until it is long
/// enough to fold its oldest part into the summary at once: prompts built on
/// it keep a stable prefix in between.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WindowRecord {
    pub entries: Vec<WindowEntry>,
    pub summary: Option<String>,
    pub summary_version: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranslationRecord {
    pub rev: String,
    pub source_lang: String,
    pub status: TranslationStatus,
    pub translations: Vec<Translation>,
}

/// A translation waiting for the engine. The request is a snapshot taken at
/// ingest, so a job resumed after a restart sends exactly the same prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub seq: i64,
    pub message: MessageRef,
    pub rev: String,
    pub request: TranslateRequest,
}
