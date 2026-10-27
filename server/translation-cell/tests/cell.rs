//! Translation Cell behavior, end to end on an in-memory store and the mock
//! engine.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use babel_api_engine::types::{
    Engine, EngineError, EngineInfo, LayerKind, SummarizeRequest, SummarizeResponse,
    TranslateRequest, TranslateResponse,
};
use babel_api_translation_cell::types::{
    ContextNode, IngestMessage, MessageRef, NodeKind, Plan, PolicyUpdate, RegionMode, SpaceContext,
    TranslationCell as _, TranslationEventKind, TranslationQuery, TranslationStatus,
};
use babel_engine::mock::{MockBackend, MockConfig};
use babel_store::{KvStore, MemStore};
use babel_translation_cell::{TranslationCell, TranslationCellConfig};
use parking_lot::Mutex;

/// The mock engine, recording every translate request.
#[derive(Default)]
struct Recorder {
    requests: Mutex<Vec<TranslateRequest>>,
}

struct RecordingEngine {
    inner: babel_engine::Engine,
    recorder: Arc<Recorder>,
}

#[async_trait]
impl Engine for RecordingEngine {
    async fn translate(&self, body: TranslateRequest) -> Result<TranslateResponse, EngineError> {
        self.recorder.requests.lock().push(body.clone());
        self.inner.translate(body).await
    }

    async fn summarize(&self, body: SummarizeRequest) -> Result<SummarizeResponse, EngineError> {
        self.inner.summarize(body).await
    }

    async fn info(&self) -> EngineInfo {
        self.inner.info().await
    }
}

fn engine() -> (Arc<dyn Engine>, Arc<Recorder>) {
    let recorder = Arc::new(Recorder::default());
    let engine = RecordingEngine {
        inner: babel_engine::Engine::new(Arc::new(MockBackend::new(MockConfig {
            latency: Duration::ZERO,
            ..MockConfig::default()
        }))),
        recorder: recorder.clone(),
    };
    (Arc::new(engine), recorder)
}

fn open(config: TranslationCellConfig) -> (TranslationCell, Arc<Recorder>) {
    let store: Arc<dyn KvStore> = Arc::new(MemStore::new());
    let (engine, recorder) = engine();
    (
        TranslationCell::open(store, engine, config).unwrap(),
        recorder,
    )
}

fn message(conversation: &str, id: &str, rev: &str, text: &str, locale: &str) -> IngestMessage {
    IngestMessage {
        message: MessageRef {
            space: "guild".into(),
            conversation: conversation.into(),
            id: id.into(),
        },
        rev: rev.into(),
        author: "alice".into(),
        author_locale: Some(locale.into()),
        text: text.into(),
        reply_to: None,
        sent_at: 0,
    }
}

async fn paid_space(cell: &TranslationCell) {
    cell.set_plan("guild".into(), Plan::Paid3).await.unwrap();
    cell.put_policy(PolicyUpdate {
        space: "guild".into(),
        languages: vec!["ja".into(), "en".into(), "ko".into()],
        llm_languages: vec!["ja".into(), "en".into(), "ko".into()],
        region_mode: RegionMode::Merged,
        aliases: Vec::new(),
        translate_regional_variants: false,
    })
    .await
    .unwrap();
}

/// Waits for the translation of `id` to leave the pending state.
async fn settled(
    cell: &TranslationCell,
    id: &str,
) -> babel_api_translation_cell::types::MessageTranslation {
    for _ in 0..200 {
        let found = cell
            .get_translations(TranslationQuery {
                messages: vec![MessageRef {
                    space: "guild".into(),
                    conversation: "general".into(),
                    id: id.into(),
                }],
                langs: Vec::new(),
            })
            .await
            .unwrap();
        if let Some(translation) = found.into_iter().next()
            && translation.status != TranslationStatus::Pending
        {
            return translation;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("translation of {id} never settled");
}

const JAPANESE: &str = "明日の夜に駅の近くでみんなと集まって一緒に遊びませんか";

#[tokio::test]
async fn free_spaces_only_detect_the_language() {
    let (cell, recorder) = open(TranslationCellConfig::default());
    cell.ingest(message("general", "1", "1", JAPANESE, "ja-JP"))
        .await
        .unwrap();

    let batch = cell.poll_events("guild".into(), 0, 1000).await.unwrap();
    let event = &batch.events[0];
    assert_eq!(event.kind, TranslationEventKind::Translated);
    let translation = event.translated.as_ref().unwrap();
    assert_eq!(translation.source_lang, "ja-JP");
    assert_eq!(translation.status, TranslationStatus::Done);
    assert!(translation.translations.is_empty());
    assert!(recorder.requests.lock().is_empty());
}

#[tokio::test]
async fn messages_are_translated_into_every_other_llm_language() {
    let (cell, recorder) = open(TranslationCellConfig::default());
    paid_space(&cell).await;
    cell.ingest(message("general", "1", "1", JAPANESE, "ja-JP"))
        .await
        .unwrap();

    let translation = settled(&cell, "1").await;
    assert_eq!(translation.status, TranslationStatus::Done);
    let langs = translation
        .translations
        .iter()
        .map(|t| t.lang.as_str())
        .collect::<Vec<_>>();
    assert_eq!(langs, ["en-US", "ko-KR"]);
    assert_eq!(recorder.requests.lock().len(), 1);

    // Idempotent for the same revision.
    cell.ingest(message("general", "1", "1", JAPANESE, "ja-JP"))
        .await
        .unwrap();
    assert_eq!(recorder.requests.lock().len(), 1);

    let usage = cell.get_usage("guild".into()).await.unwrap();
    assert_eq!(usage.translated_messages, 1);
}

#[tokio::test]
async fn edits_retranslate_and_keep_the_window_in_place() {
    let (cell, recorder) = open(TranslationCellConfig::default());
    paid_space(&cell).await;
    cell.ingest(message("general", "1", "1", JAPANESE, "ja-JP"))
        .await
        .unwrap();
    settled(&cell, "1").await;
    cell.ingest(message(
        "general",
        "2",
        "1",
        "二つ目のメッセージです、よろしくお願いします",
        "ja-JP",
    ))
    .await
    .unwrap();
    settled(&cell, "2").await;

    cell.ingest(message(
        "general",
        "1",
        "2",
        "編集された最初のメッセージです、確認してください",
        "ja-JP",
    ))
    .await
    .unwrap();
    let translation = settled(&cell, "1").await;
    assert_eq!(translation.rev, "2");
    assert!(translation.translations[0].text.contains("編集"));

    // The edited message keeps its place: its request sees no earlier window.
    let requests = recorder.requests.lock();
    assert!(requests.last().unwrap().window.is_empty());
}

#[tokio::test]
async fn context_flows_from_space_to_thread() {
    let (cell, recorder) = open(TranslationCellConfig::default());
    paid_space(&cell).await;
    cell.put_space(SpaceContext {
        space: "guild".into(),
        name: "Rhythm Club".into(),
        description: Some("We play rhythm games together.".into()),
        description_lang: Some("en".into()),
        glossary: Vec::new(),
    })
    .await
    .unwrap();
    cell.put_node(ContextNode {
        space: "guild".into(),
        id: "general".into(),
        parent: None,
        kind: NodeKind::Channel,
        title: "general".into(),
        description: Some("Anything goes".into()),
        starter: None,
    })
    .await
    .unwrap();
    cell.ingest(message("general", "1", "1", JAPANESE, "ja-JP"))
        .await
        .unwrap();
    settled(&cell, "1").await;

    // A new thread starts with the space, the channel and its fork.
    cell.put_node(ContextNode {
        space: "guild".into(),
        id: "thread".into(),
        parent: Some("general".into()),
        kind: NodeKind::Thread,
        title: "meetup".into(),
        description: None,
        starter: Some(MessageRef {
            space: "guild".into(),
            conversation: "general".into(),
            id: "1".into(),
        }),
    })
    .await
    .unwrap();
    cell.ingest(message(
        "thread",
        "t1",
        "1",
        "何時に集まるのがいいと思いますか、教えてください",
        "ja-JP",
    ))
    .await
    .unwrap();

    for _ in 0..200 {
        if recorder.requests.lock().len() == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let request = recorder.requests.lock()[1].clone();
    assert!(request.window.is_empty());
    let kinds = request.layers.iter().map(|l| l.kind).collect::<Vec<_>>();
    assert_eq!(
        kinds,
        [LayerKind::Space, LayerKind::Conversation, LayerKind::Fork]
    );
    assert!(request.layers[0].content.contains("Rhythm Club"));
    assert!(request.layers[2].content.contains(JAPANESE));
}

#[tokio::test]
async fn full_windows_fold_into_a_summary() {
    let (cell, recorder) = open(TranslationCellConfig {
        window_max: 4,
        window_keep: 2,
        ..TranslationCellConfig::default()
    });
    paid_space(&cell).await;
    for i in 0..6 {
        let id = i.to_string();
        cell.ingest(message(
            "general",
            &id,
            "1",
            &format!("{JAPANESE} {i}"),
            "ja-JP",
        ))
        .await
        .unwrap();
        settled(&cell, &id).await;
        // Let the fold finish before the next message.
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let requests = recorder.requests.lock();
    let last = requests.last().unwrap();
    assert!(last.window.len() <= 3, "window was {}", last.window.len());
    assert!(last.layers.iter().any(|l| l.kind == LayerKind::Summary));
}

#[tokio::test]
async fn budgets_fall_back_to_on_device_translation() {
    let (cell, _) = open(TranslationCellConfig {
        daily_budget: Some(1),
        ..TranslationCellConfig::default()
    });
    paid_space(&cell).await;
    cell.ingest(message("general", "1", "1", JAPANESE, "ja-JP"))
        .await
        .unwrap();
    assert_eq!(settled(&cell, "1").await.status, TranslationStatus::Done);
    cell.ingest(message("general", "2", "1", JAPANESE, "ja-JP"))
        .await
        .unwrap();
    assert_eq!(
        settled(&cell, "2").await.status,
        TranslationStatus::BudgetExceeded
    );
}

/// An engine that never answers, standing in for a crash mid-translation.
struct StuckEngine;

#[async_trait]
impl Engine for StuckEngine {
    async fn translate(&self, _: TranslateRequest) -> Result<TranslateResponse, EngineError> {
        std::future::pending().await
    }

    async fn summarize(&self, _: SummarizeRequest) -> Result<SummarizeResponse, EngineError> {
        std::future::pending().await
    }

    async fn info(&self) -> EngineInfo {
        EngineInfo {
            backend: "stuck".into(),
            model: String::new(),
        }
    }
}

#[tokio::test]
async fn queued_jobs_resume_after_a_restart() {
    let store: Arc<dyn KvStore> = Arc::new(MemStore::new());
    let stuck = TranslationCell::open(
        store.clone(),
        Arc::new(StuckEngine),
        TranslationCellConfig::default(),
    )
    .unwrap();
    paid_space(&stuck).await;
    stuck
        .ingest(message("general", "1", "1", JAPANESE, "ja-JP"))
        .await
        .unwrap();

    let (engine, _) = engine();
    let restarted = TranslationCell::open(store, engine, TranslationCellConfig::default()).unwrap();
    assert_eq!(
        settled(&restarted, "1").await.status,
        TranslationStatus::Done
    );
}

#[tokio::test]
async fn tenants_cannot_see_each_others_spaces() {
    let (cell, _) = open(TranslationCellConfig::default());
    let acme = babel_common::Identity {
        role: babel_common::Role::Tenant,
        name: "acme".into(),
    };
    // BabelChat's own guild, and a platform's space with the same ID.
    paid_space(&cell).await;
    babel_common::with_caller(acme.clone(), async {
        cell.ingest(message("general", "1", "1", JAPANESE, "ja-JP"))
            .await
            .unwrap();
    })
    .await;

    let policy = cell.get_policy("guild".into()).await.unwrap();
    assert_eq!(policy.plan, Plan::Paid3);
    let theirs = babel_common::with_caller(acme.clone(), cell.get_policy("guild".into()))
        .await
        .unwrap();
    assert_eq!(theirs.plan, Plan::Free);

    // Their message is theirs alone, and so are its events.
    let query = || TranslationQuery {
        messages: vec![MessageRef {
            space: "guild".into(),
            conversation: "general".into(),
            id: "1".into(),
        }],
        langs: Vec::new(),
    };
    assert!(cell.get_translations(query()).await.unwrap().is_empty());
    let found = babel_common::with_caller(acme.clone(), cell.get_translations(query()))
        .await
        .unwrap();
    assert_eq!(found[0].source_lang, "ja-JP");
    let ours = cell.poll_events("guild".into(), 0, 0).await.unwrap();
    assert!(
        ours.events
            .iter()
            .all(|event| event.kind == TranslationEventKind::Policy)
    );

    // Edges and Message Cells are BabelChat itself.
    let edge = babel_common::Identity {
        role: babel_common::Role::Edge,
        name: "edge-0".into(),
    };
    let via_edge = babel_common::with_caller(edge, cell.get_policy("guild".into()))
        .await
        .unwrap();
    assert_eq!(via_edge.plan, Plan::Paid3);
}

#[tokio::test]
async fn space_logs_are_trimmed() {
    let (cell, _) = open(TranslationCellConfig::default());
    for id in ["1", "2", "3"] {
        cell.ingest(message("general", id, "1", JAPANESE, "ja-JP"))
            .await
            .unwrap();
    }
    let head = cell.poll_events("guild".into(), 0, 0).await.unwrap().head;
    assert_eq!(head, 3);
    assert_eq!(cell.trim_logs(Duration::from_secs(3600)).await.unwrap(), 0);
    assert_eq!(cell.trim_logs(Duration::ZERO).await.unwrap(), 3);

    let behind = cell.poll_events("guild".into(), 0, 0).await.unwrap();
    assert!(behind.events.is_empty());
    assert_eq!((behind.trimmed, behind.head), (3, 3));
    cell.ingest(message("general", "4", "1", JAPANESE, "ja-JP"))
        .await
        .unwrap();
    let batch = cell.poll_events("guild".into(), 3, 0).await.unwrap();
    assert_eq!(batch.events.len(), 1);
}
