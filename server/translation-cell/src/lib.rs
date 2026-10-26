//! The Translation Cell: translations, context and language policy of the
//! spaces it holds.
//!
//! It is the part other platforms adopt, and BabelChat's own Message Cells
//! use it through the same API (types/translation_cell.tyml). Messages come
//! in through `ingest`; translation starts right away, from the source
//! language directly into every LLM language of the space (N to N).
//!
//! ```text
//! ingest ─▶ detect language ─▶ append to window ─▶ queue job ─┐   (one batch)
//!                                                            ▼
//!               event "translated" ◀─ store ◀─ Engine.translate (async)
//! ```
//!
//! See docs/architecture/translation-cell.md.

mod context;
pub mod detect;
mod policy;
mod records;

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
    time::Duration,
};

use async_trait::async_trait;
use babel_api_engine::types::{
    Engine, EngineError, SourceMessage, SummarizeRequest, SummaryPurpose, TargetLanguage,
    TranslateRequest, Usage,
};
use babel_api_translation_cell::types::{
    ContextNode, IngestMessage, MessageRef, MessageTranslation, NodeKind, Plan, PolicyUpdate,
    SpaceContext, SpacePolicy, SpaceUsage, Translation, TranslationCellError, TranslationEvent,
    TranslationEventBatch, TranslationEventKind, TranslationQuery, TranslationStatus,
    ViewerLanguage,
};
use babel_common::{LogSignals, now_ms, poll_timeout};
use babel_lang::Normalizer;
use babel_store::{Batch, Key, KvStore, Scan, StoreError};
use parking_lot::Mutex;
pub use policy::plan_limits;
pub use records::TABLES;
use records::{
    EVENTS, JOBS, Job, NODES, NodeRecord, SPACES, SpaceRecord, TENANT, TRANSLATIONS,
    TranslationRecord, WINDOWS, WindowEntry, WindowRecord, event_key, job_key, node_key, space_key,
    translation_key, window_key,
};
use tokio::sync::{OwnedMutexGuard, Semaphore};

#[derive(Debug, Clone)]
pub struct TranslationCellConfig {
    /// Plan of new spaces until billing sets one.
    pub default_plan: Plan,
    /// LLM-translated messages per space per UTC day. Past it, clients fall
    /// back to on-device translation.
    pub daily_budget: Option<i64>,
    /// A window folds once it holds more messages than this...
    pub window_max: usize,
    /// ...keeping this many of the newest.
    pub window_keep: usize,
    /// Messages of the parent copied into a new thread's fork snapshot.
    pub fork_messages: usize,
    /// Descriptions longer than this are digested by the engine.
    pub digest_threshold: usize,
    pub summary_chars: i64,
    /// Engine requests in flight at once.
    pub concurrency: usize,
}

impl Default for TranslationCellConfig {
    fn default() -> Self {
        Self {
            default_plan: Plan::Free,
            daily_budget: None,
            window_max: 40,
            window_keep: 15,
            fork_messages: 5,
            digest_threshold: 600,
            summary_chars: 1200,
            concurrency: 16,
        }
    }
}

/// Windows that cannot fold (the engine is down) are cut beyond this many
/// extra messages, losing the oldest without a summary.
const WINDOW_HARD_EXTRA: usize = 20;
const MAX_ID_BYTES: usize = 256;
const MAX_TEXT_CHARS: usize = 4000;
const POLL_LIMIT: usize = 100;
const ENGINE_ATTEMPTS: u32 = 3;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

type CellResult<T> = Result<T, TranslationCellError>;

fn storage(error: StoreError) -> TranslationCellError {
    tracing::error!(%error, "translation cell storage failed");
    TranslationCellError::Unavailable
}

#[derive(Clone)]
pub struct TranslationCell {
    inner: Arc<Inner>,
}

struct Inner {
    store: Arc<dyn KvStore>,
    engine: Arc<dyn Engine>,
    normalizer: Normalizer,
    config: TranslationCellConfig,
    /// Serializes read-modify-write per space.
    locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Wakes `poll_events` per space.
    signals: LogSignals<String>,
    /// Windows with a fold in flight, as (space, conversation).
    folding: Mutex<HashSet<(String, String)>>,
    next_job: AtomicI64,
    permits: Arc<Semaphore>,
}

impl TranslationCell {
    /// Opens the cell on its store and resumes jobs left by a previous run.
    /// Must be called within a Tokio runtime.
    pub fn open(
        store: Arc<dyn KvStore>,
        engine: Arc<dyn Engine>,
        config: TranslationCellConfig,
    ) -> Result<Self, StoreError> {
        let jobs = store.scan_values::<Job>(Scan::prefix(JOBS, &[]))?;
        let next_job = jobs.last().map(|(_, job)| job.seq + 1).unwrap_or(1);

        let inner = Arc::new(Inner {
            store,
            engine,
            normalizer: Normalizer::new(),
            permits: Arc::new(Semaphore::new(config.concurrency)),
            config,
            locks: Mutex::default(),
            signals: LogSignals::new(),
            folding: Mutex::default(),
            next_job: AtomicI64::new(next_job),
        });

        if !jobs.is_empty() {
            tracing::info!(jobs = jobs.len(), "resuming translation jobs");
        }
        for (_, job) in jobs {
            tokio::spawn(inner.clone().run_job(job));
        }

        Ok(Self { inner })
    }
}

fn zero_usage(space: &str) -> SpaceUsage {
    SpaceUsage {
        space: space.to_string(),
        translated_messages: 0,
        input_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        output_tokens: 0,
    }
}

fn add_usage(total: &mut SpaceUsage, usage: &Usage) {
    total.input_tokens += usage.input_tokens;
    total.cache_read_tokens += usage.cache_read_tokens;
    total.cache_write_tokens += usage.cache_write_tokens;
    total.output_tokens += usage.output_tokens;
}

fn check_id(id: &str) -> CellResult<()> {
    if id.is_empty() || id.len() > MAX_ID_BYTES {
        return Err(TranslationCellError::InvalidRequest);
    }
    Ok(())
}

fn check_ref(message: &MessageRef) -> CellResult<()> {
    check_id(&message.space)?;
    check_id(&message.conversation)?;
    check_id(&message.id)
}

fn message_translation(message: &MessageRef, record: &TranslationRecord) -> MessageTranslation {
    MessageTranslation {
        message: message.clone(),
        rev: record.rev.clone(),
        source_lang: record.source_lang.clone(),
        status: record.status,
        translations: record.translations.clone(),
    }
}

/// An event to append to a space's log.
enum Event {
    Translated(MessageTranslation),
    Removed(MessageRef),
    Policy(SpacePolicy),
}

impl Inner {
    async fn lock(&self, space: &str) -> OwnedMutexGuard<()> {
        let lock = self
            .locks
            .lock()
            .entry(space.to_string())
            .or_default()
            .clone();
        lock.lock_owned().await
    }

    fn load_space(&self, space: &str) -> CellResult<Option<SpaceRecord>> {
        self.store
            .get_value(SPACES, &space_key(space))
            .map_err(storage)
    }

    fn space_or_default(&self, space: &str) -> CellResult<SpaceRecord> {
        Ok(self.load_space(space)?.unwrap_or_else(|| SpaceRecord {
            context: None,
            context_version: 0,
            digest: None,
            policy: policy::default_policy(space, self.config.default_plan),
            usage: zero_usage(space),
            budget_day: 0,
            budget_used: 0,
            last_event: 0,
        }))
    }

    fn load_window(&self, space: &str, conversation: &str) -> CellResult<WindowRecord> {
        Ok(self
            .store
            .get_value(WINDOWS, &window_key(space, conversation))
            .map_err(storage)?
            .unwrap_or_default())
    }

    /// Appends an event to the batch and returns its sequence number. Call
    /// [`Self::publish`] once the batch is written.
    fn push_event(
        &self,
        batch: &mut Batch,
        space: &str,
        record: &mut SpaceRecord,
        event: Event,
    ) -> i64 {
        record.last_event += 1;
        let seq = record.last_event;
        let (kind, translated, removed, policy) = match event {
            Event::Translated(translation) => (
                TranslationEventKind::Translated,
                Some(translation),
                None,
                None,
            ),
            Event::Removed(message) => (TranslationEventKind::Removed, None, Some(message), None),
            Event::Policy(policy) => (TranslationEventKind::Policy, None, None, Some(policy)),
        };
        batch.put_value(
            EVENTS,
            event_key(space, seq),
            &TranslationEvent {
                seq,
                kind,
                translated,
                removed,
                policy,
            },
        );
        seq
    }

    fn publish(&self, space: &str, seq: i64) {
        self.signals.publish(&space.to_string(), seq);
    }

    /// The conversation's node and its ancestors, nearest first.
    fn node_chain(&self, space: &str, conversation: &str) -> CellResult<Vec<NodeRecord>> {
        let mut chain = Vec::new();
        let mut next = Some(conversation.to_string());
        while let Some(id) = next.take() {
            if chain.len() >= context::MAX_DEPTH {
                break;
            }
            let Some(record) = self
                .store
                .get_value::<NodeRecord>(NODES, &node_key(space, &id))
                .map_err(storage)?
            else {
                break;
            };
            next = record.node.parent.clone();
            chain.push(record);
        }
        Ok(chain)
    }

    // ---- Context ----------------------------------------------------------

    async fn put_space(self: &Arc<Self>, context: SpaceContext) -> CellResult<()> {
        check_id(&context.space)?;
        let space = context.space.clone();
        let _guard = self.lock(&space).await;

        let mut record = self.space_or_default(&space)?;
        if record.context.as_ref() == Some(&context) {
            return Ok(());
        }

        let needs_digest = context
            .description
            .as_ref()
            .is_some_and(|description| description.chars().count() > self.config.digest_threshold);
        record.context = Some(context.clone());
        record.context_version += 1;
        record.digest = None;

        let mut batch = Batch::new();
        batch.put_value(SPACES, space_key(&space), &record);
        self.store.write(batch).map_err(storage)?;

        if needs_digest {
            tokio::spawn(self.clone().digest(context, record.context_version));
        }
        Ok(())
    }

    /// Condenses a long description into the space layer.
    async fn digest(self: Arc<Self>, context: SpaceContext, version: i64) {
        let request = SummarizeRequest {
            purpose: SummaryPurpose::Digest,
            previous: None,
            messages: vec![SourceMessage {
                author: context.name.clone(),
                text: context.description.clone().unwrap_or_default(),
            }],
            max_chars: self.config.summary_chars,
            lang: context.description_lang.clone(),
        };
        let Ok(response) = self.engine.summarize(request).await else {
            tracing::warn!(space = %context.space, "digest failed; using the raw description");
            return;
        };

        let _guard = self.lock(&context.space).await;
        let Ok(Some(mut record)) = self.load_space(&context.space) else {
            return;
        };
        if record.context_version != version {
            // The context changed meanwhile; a newer digest is on its way.
            return;
        }
        record.digest = Some(response.text);
        record.context_version += 1;
        add_usage(&mut record.usage, &response.usage);

        let mut batch = Batch::new();
        batch.put_value(SPACES, space_key(&context.space), &record);
        if let Err(error) = self.store.write(batch) {
            tracing::error!(%error, "storing digest failed");
        }
    }

    async fn put_node(&self, node: ContextNode) -> CellResult<()> {
        check_id(&node.space)?;
        check_id(&node.id)?;
        let _guard = self.lock(&node.space).await;

        let key = node_key(&node.space, &node.id);
        let previous = self
            .store
            .get_value::<NodeRecord>(NODES, &key)
            .map_err(storage)?;
        if previous
            .as_ref()
            .is_some_and(|previous| previous.node == node)
        {
            return Ok(());
        }

        // A thread's fork is taken once, when the thread first appears.
        let fork = match (previous.as_ref(), node.kind, &node.parent) {
            (Some(previous), _, _) => previous.fork.clone(),
            (None, NodeKind::Thread, Some(parent)) => {
                let parent_node = self
                    .store
                    .get_value::<NodeRecord>(NODES, &node_key(&node.space, parent))
                    .map_err(storage)?;
                let parent_window = self.load_window(&node.space, parent)?;
                Some(context::fork_snapshot(
                    &node.title,
                    parent_node.as_ref(),
                    &parent_window,
                    node.starter.as_ref().map(|starter| starter.id.as_str()),
                    self.config.fork_messages,
                ))
            }
            _ => None,
        };

        let record = NodeRecord {
            version: previous.map(|previous| previous.version + 1).unwrap_or(1),
            node,
            fork,
        };
        let mut batch = Batch::new();
        batch.put_value(NODES, key, &record);
        self.store.write(batch).map_err(storage)
    }

    async fn delete_space(&self, space: String) -> CellResult<()> {
        let _guard = self.lock(&space).await;
        let prefix = Key::new().str(TENANT).str(&space).build();

        let mut batch = Batch::new();
        batch.delete(SPACES, space_key(&space));
        for table in [NODES, WINDOWS, TRANSLATIONS, EVENTS] {
            batch.delete_prefix(table, prefix.clone());
        }
        // Queued jobs find their records gone and drop themselves.
        self.store.write(batch).map_err(storage)
    }

    // ---- Language policy --------------------------------------------------

    async fn put_policy(&self, update: PolicyUpdate) -> CellResult<SpacePolicy> {
        check_id(&update.space)?;
        let space = update.space.clone();
        let _guard = self.lock(&space).await;

        let mut record = self.space_or_default(&space)?;
        record.policy = policy::apply_update(&self.normalizer, &record.policy, &update)?;
        self.store_policy(&space, record)
    }

    async fn set_plan(&self, space: String, plan: Plan) -> CellResult<SpacePolicy> {
        check_id(&space)?;
        let _guard = self.lock(&space).await;

        let mut record = self.space_or_default(&space)?;
        record.policy = policy::apply_plan(&record.policy, plan);
        self.store_policy(&space, record)
    }

    fn store_policy(&self, space: &str, mut record: SpaceRecord) -> CellResult<SpacePolicy> {
        let policy = record.policy.clone();
        let mut batch = Batch::new();
        let seq = self.push_event(
            &mut batch,
            space,
            &mut record,
            Event::Policy(policy.clone()),
        );
        batch.put_value(SPACES, space_key(space), &record);
        self.store.write(batch).map_err(storage)?;
        self.publish(space, seq);
        Ok(policy)
    }

    fn policy(&self, space: &str) -> CellResult<SpacePolicy> {
        Ok(self.space_or_default(space)?.policy)
    }

    // ---- Messages ---------------------------------------------------------

    async fn ingest(self: &Arc<Self>, message: IngestMessage) -> CellResult<()> {
        check_ref(&message.message)?;
        if message.text.chars().count() > MAX_TEXT_CHARS {
            return Err(TranslationCellError::InvalidRequest);
        }

        let space = message.message.space.clone();
        let conversation = message.message.conversation.clone();
        let _guard = self.lock(&space).await;

        let mut record = self.space_or_default(&space)?;
        let key = translation_key(&message.message);
        let existing = self
            .store
            .get_value::<TranslationRecord>(TRANSLATIONS, &key)
            .map_err(storage)?;
        if existing
            .as_ref()
            .is_some_and(|existing| existing.rev == message.rev)
        {
            return Ok(());
        }

        // Window: append a new message, or update an edited one in place.
        let mut window = self.load_window(&space, &conversation)?;
        let entry = WindowEntry {
            id: message.message.id.clone(),
            rev: message.rev.clone(),
            author: message.author.clone(),
            text: message.text.clone(),
        };
        let before = match window
            .entries
            .iter()
            .position(|entry| entry.id == message.message.id)
        {
            Some(index) => {
                window.entries[index] = entry;
                window.entries[..index].to_vec()
            }
            None if existing.is_some() => window.entries.clone(),
            None => {
                let before = window.entries.clone();
                window.entries.push(entry);
                before
            }
        };

        let region = policy::region_policy_of(&record.policy);
        let source_lang = detect::source_language(
            &self.normalizer,
            &message.text,
            message.author_locale.as_deref(),
            &region,
        );
        let targets = policy::targets(&self.normalizer, &record.policy, &source_lang);

        let day = now_ms() / DAY_MS;
        if record.budget_day != day {
            record.budget_day = day;
            record.budget_used = 0;
        }
        let over_budget = self
            .config
            .daily_budget
            .is_some_and(|budget| record.budget_used >= budget);

        let status = if targets.is_empty() {
            TranslationStatus::Done
        } else if over_budget {
            TranslationStatus::BudgetExceeded
        } else {
            TranslationStatus::Pending
        };
        let translation = TranslationRecord {
            rev: message.rev.clone(),
            source_lang: source_lang.clone(),
            status,
            translations: Vec::new(),
        };

        let mut batch = Batch::new();
        let mut job = None;
        let mut seq = None;
        if status == TranslationStatus::Pending {
            record.budget_used += 1;

            let mut layers = Vec::new();
            layers.extend(context::space_layer(&space, &record));
            layers.extend(context::node_layers(
                &self.node_chain(&space, &conversation)?,
            ));
            layers.extend(context::summary_layer(&conversation, &window));

            let request = TranslateRequest {
                layers,
                window: before
                    .into_iter()
                    .map(|entry| SourceMessage {
                        author: entry.author,
                        text: entry.text,
                    })
                    .collect(),
                message: SourceMessage {
                    author: message.author.clone(),
                    text: message.text.clone(),
                },
                source_lang,
                targets: targets
                    .iter()
                    .map(|tag| TargetLanguage {
                        tag: tag.clone(),
                        name: babel_lang::english_name(tag),
                    })
                    .collect(),
            };
            let new_job = Job {
                seq: self.next_job.fetch_add(1, Ordering::Relaxed),
                message: message.message.clone(),
                rev: message.rev.clone(),
                request,
            };
            batch.put_value(JOBS, job_key(new_job.seq), &new_job);
            job = Some(new_job);
        } else {
            seq = Some(self.push_event(
                &mut batch,
                &space,
                &mut record,
                Event::Translated(message_translation(&message.message, &translation)),
            ));
        }

        let fold = self.trim_window(&space, &conversation, &mut window);
        batch.put_value(TRANSLATIONS, key, &translation);
        batch.put_value(WINDOWS, window_key(&space, &conversation), &window);
        batch.put_value(SPACES, space_key(&space), &record);
        self.store.write(batch).map_err(storage)?;

        if let Some(seq) = seq {
            self.publish(&space, seq);
        }
        if let Some(job) = job {
            tokio::spawn(self.clone().run_job(job));
        }
        if let Some(cut) = fold {
            tokio::spawn(self.clone().fold(space, conversation, cut, window.summary));
        }
        Ok(())
    }

    /// Starts folding a full window, or cuts it if folding keeps failing.
    /// Returns the entries to fold.
    fn trim_window(
        &self,
        space: &str,
        conversation: &str,
        window: &mut WindowRecord,
    ) -> Option<Vec<WindowEntry>> {
        let len = window.entries.len();
        let hard_limit = self.config.window_max + WINDOW_HARD_EXTRA;
        if len > hard_limit {
            window.entries.drain(..len - hard_limit);
        }
        if window.entries.len() <= self.config.window_max {
            return None;
        }

        let key = (space.to_string(), conversation.to_string());
        if !self.folding.lock().insert(key) {
            return None;
        }
        let cut = window.entries.len() - self.config.window_keep;
        Some(window.entries[..cut].to_vec())
    }

    /// Folds the oldest part of a window into its summary. The rest of the
    /// window becomes the new, shorter prefix: one cache miss, then hits
    /// again.
    async fn fold(
        self: Arc<Self>,
        space: String,
        conversation: String,
        cut: Vec<WindowEntry>,
        previous: Option<String>,
    ) {
        let request = SummarizeRequest {
            purpose: SummaryPurpose::Conversation,
            previous,
            messages: cut
                .iter()
                .map(|entry| SourceMessage {
                    author: entry.author.clone(),
                    text: entry.text.clone(),
                })
                .collect(),
            max_chars: self.config.summary_chars,
            lang: None,
        };
        let result = self.engine.summarize(request).await;

        let _guard = self.lock(&space).await;
        self.folding
            .lock()
            .remove(&(space.clone(), conversation.clone()));
        let Ok(response) = result else {
            tracing::warn!(%space, %conversation, "folding the window failed; will retry");
            return;
        };

        let stored = (|| -> CellResult<()> {
            let mut window = self.load_window(&space, &conversation)?;
            let folded = cut.iter().map(|entry| &entry.id).collect::<HashSet<_>>();
            window.entries.retain(|entry| !folded.contains(&entry.id));
            window.summary = Some(response.text);
            window.summary_version += 1;

            let mut batch = Batch::new();
            if let Some(mut record) = self.load_space(&space)? {
                add_usage(&mut record.usage, &response.usage);
                batch.put_value(SPACES, space_key(&space), &record);
            }
            batch.put_value(WINDOWS, window_key(&space, &conversation), &window);
            self.store.write(batch).map_err(storage)
        })();
        if stored.is_err() {
            tracing::error!(%space, %conversation, "storing the folded window failed");
        }
    }

    /// Sends a job to the engine and stores the result.
    async fn run_job(self: Arc<Self>, job: Job) {
        let outcome = {
            let _permit = self.permits.clone().acquire_owned().await;
            let mut delay = Duration::from_millis(500);
            let mut attempt = 1;
            loop {
                match self.engine.translate(job.request.clone()).await {
                    Ok(response) => break Ok(response),
                    Err(EngineError::Unavailable | EngineError::RateLimited)
                        if attempt < ENGINE_ATTEMPTS =>
                    {
                        tokio::time::sleep(delay).await;
                        delay *= 2;
                        attempt += 1;
                    }
                    Err(error) => break Err(error),
                }
            }
        };

        let space = job.message.space.clone();
        let _guard = self.lock(&space).await;
        let mut batch = Batch::new();
        batch.delete(JOBS, job_key(job.seq));

        let result = (|| -> CellResult<Option<i64>> {
            let key = translation_key(&job.message);
            let (Some(mut record), Some(mut translation)) = (
                self.load_space(&space)?,
                self.store
                    .get_value::<TranslationRecord>(TRANSLATIONS, &key)
                    .map_err(storage)?,
            ) else {
                // Deleted meanwhile.
                return Ok(None);
            };
            if translation.rev != job.rev {
                // Edited meanwhile; a newer job carries the new text.
                return Ok(None);
            }

            match &outcome {
                Ok(response) => {
                    translation.status = TranslationStatus::Done;
                    translation.translations = response
                        .translations
                        .iter()
                        .map(|translated| Translation {
                            lang: translated.lang.clone(),
                            text: translated.text.clone(),
                        })
                        .collect();
                    record.usage.translated_messages += 1;
                    add_usage(&mut record.usage, &response.usage);
                }
                Err(error) => {
                    tracing::warn!(?error, message = ?job.message, "translation failed");
                    translation.status = TranslationStatus::Failed;
                }
            }

            let seq = self.push_event(
                &mut batch,
                &space,
                &mut record,
                Event::Translated(message_translation(&job.message, &translation)),
            );
            batch.put_value(TRANSLATIONS, key, &translation);
            batch.put_value(SPACES, space_key(&space), &record);
            Ok(Some(seq))
        })();

        match result.and_then(|seq| self.store.write(batch).map_err(storage).map(|()| seq)) {
            Ok(Some(seq)) => self.publish(&space, seq),
            Ok(None) => {}
            Err(_) => tracing::error!(message = ?job.message, "storing a translation failed"),
        }
    }

    async fn delete_message(&self, message: MessageRef) -> CellResult<()> {
        check_ref(&message)?;
        let space = message.space.clone();
        let _guard = self.lock(&space).await;

        let Some(mut record) = self.load_space(&space)? else {
            return Ok(());
        };
        let mut window = self.load_window(&space, &message.conversation)?;
        window.entries.retain(|entry| entry.id != message.id);

        let mut batch = Batch::new();
        batch.delete(TRANSLATIONS, translation_key(&message));
        batch.put_value(WINDOWS, window_key(&space, &message.conversation), &window);
        let seq = self.push_event(&mut batch, &space, &mut record, Event::Removed(message));
        batch.put_value(SPACES, space_key(&space), &record);
        self.store.write(batch).map_err(storage)?;
        self.publish(&space, seq);
        Ok(())
    }

    fn get_translations(&self, query: TranslationQuery) -> CellResult<Vec<MessageTranslation>> {
        let mut found = Vec::new();
        for message in &query.messages {
            check_ref(message)?;
            let Some(record) = self
                .store
                .get_value::<TranslationRecord>(TRANSLATIONS, &translation_key(message))
                .map_err(storage)?
            else {
                continue;
            };
            let mut translation = message_translation(message, &record);
            if !query.langs.is_empty() {
                translation
                    .translations
                    .retain(|translation| query.langs.contains(&translation.lang));
            }
            found.push(translation);
        }
        Ok(found)
    }

    // ---- Events -----------------------------------------------------------

    async fn poll_events(
        &self,
        space: String,
        after: i64,
        wait_ms: i64,
    ) -> CellResult<TranslationEventBatch> {
        check_id(&space)?;
        let prefix = Key::new().str(TENANT).str(&space).build();
        let after_key = event_key(&space, after.max(0));
        let read = || {
            self.store
                .scan_values::<TranslationEvent>(
                    Scan::prefix(EVENTS, &prefix)
                        .after(&after_key)
                        .limit(POLL_LIMIT),
                )
                .map_err(storage)
        };

        let mut events = read()?;
        if events.is_empty()
            && self
                .signals
                .wait_past(&space, after, poll_timeout(wait_ms))
                .await
        {
            events = read()?;
        }

        let head = self
            .load_space(&space)?
            .map(|record| record.last_event)
            .unwrap_or(0);
        let events = events
            .into_iter()
            .map(|(_, event)| event)
            .collect::<Vec<_>>();
        Ok(TranslationEventBatch {
            next: events
                .last()
                .map(|event| event.seq)
                .unwrap_or(after.min(head)),
            head,
            events,
        })
    }
}

#[async_trait]
impl babel_api_translation_cell::types::TranslationCell for TranslationCell {
    async fn put_space(&self, body: SpaceContext) -> CellResult<()> {
        self.inner.put_space(body).await
    }

    async fn put_node(&self, body: ContextNode) -> CellResult<()> {
        self.inner.put_node(body).await
    }

    async fn delete_space(&self, space: String) -> CellResult<()> {
        self.inner.delete_space(space).await
    }

    async fn get_policy(&self, space: String) -> CellResult<SpacePolicy> {
        check_id(&space)?;
        self.inner.policy(&space)
    }

    async fn put_policy(&self, body: PolicyUpdate) -> CellResult<SpacePolicy> {
        self.inner.put_policy(body).await
    }

    async fn set_plan(&self, space: String, plan: Plan) -> CellResult<SpacePolicy> {
        self.inner.set_plan(space, plan).await
    }

    async fn normalize(&self, space: String, tag: String) -> CellResult<String> {
        check_id(&space)?;
        let policy = self.inner.policy(&space)?;
        self.inner
            .normalizer
            .normalize(&tag, &policy::region_policy_of(&policy))
            .map_err(|_| TranslationCellError::InvalidLanguage)
    }

    async fn resolve_viewer(
        &self,
        space: String,
        locale: String,
        translate_outside_set: bool,
    ) -> CellResult<ViewerLanguage> {
        check_id(&space)?;
        let policy = self.inner.policy(&space)?;
        let viewer = babel_lang::resolve_viewer(
            &self.inner.normalizer,
            &locale,
            &policy::region_policy_of(&policy),
            &policy.languages,
            &policy.llm_languages,
            translate_outside_set,
        )
        .map_err(|_| TranslationCellError::InvalidLanguage)?;

        Ok(ViewerLanguage {
            effective: viewer.effective,
            in_set: viewer.in_set,
            llm: viewer.llm,
            client_translation: viewer.client_translation,
        })
    }

    async fn ingest(&self, body: IngestMessage) -> CellResult<()> {
        self.inner.ingest(body).await
    }

    async fn delete_message(&self, body: MessageRef) -> CellResult<()> {
        self.inner.delete_message(body).await
    }

    async fn get_translations(
        &self,
        body: TranslationQuery,
    ) -> CellResult<Vec<MessageTranslation>> {
        self.inner.get_translations(body)
    }

    async fn poll_events(
        &self,
        space: String,
        after: i64,
        wait_ms: i64,
    ) -> CellResult<TranslationEventBatch> {
        self.inner.poll_events(space, after, wait_ms).await
    }

    async fn get_usage(&self, space: String) -> CellResult<SpaceUsage> {
        check_id(&space)?;
        Ok(self
            .inner
            .load_space(&space)?
            .map(|record| record.usage)
            .unwrap_or_else(|| zero_usage(&space)))
    }
}
