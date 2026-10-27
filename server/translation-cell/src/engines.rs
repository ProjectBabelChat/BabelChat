//! The engines a Translation Cell translates with, and the models they
//! serve.
//!
//! Each engine lists its models (`info`); together they form the cell's
//! catalog, in the order the engines were given, so the first engine's
//! default model is the cell's default. Several engines serving the same
//! model are replicas: requests of one space always start at the same one
//! (hashing the space), so its prompt prefix stays warm in that replica's
//! cache, and move to the next only when it fails.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use babel_api_engine::types::{
    Engine, EngineError, SummarizeRequest, SummarizeResponse, TranslateRequest, TranslateResponse,
};
use babel_api_translation_cell::types::TranslationModel;
use parking_lot::RwLock;

/// How long a catalog is trusted before engines are asked again.
const CATALOG_TTL: Duration = Duration::from_secs(60);

struct CatalogModel {
    id: String,
    name: String,
    /// Indexes into `Engines::engines`.
    engines: Vec<usize>,
}

#[derive(Default)]
struct Catalog {
    /// The default first.
    models: Vec<CatalogModel>,
    refreshed: Option<Instant>,
}

pub struct Engines {
    engines: Vec<Arc<dyn Engine>>,
    catalog: RwLock<Catalog>,
}

impl From<Arc<dyn Engine>> for Engines {
    fn from(engine: Arc<dyn Engine>) -> Self {
        Self::new(vec![engine])
    }
}

impl<E: Engine> From<Arc<E>> for Engines {
    fn from(engine: Arc<E>) -> Self {
        Self::new(vec![engine])
    }
}

/// A stable hash (FNV-1a), so that every cell sends a space to the same
/// replica, across restarts too.
fn affinity_hash(key: &str) -> usize {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in key.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash as usize
}

impl Engines {
    /// # Panics
    /// Without engines.
    pub fn new(engines: Vec<Arc<dyn Engine>>) -> Self {
        assert!(!engines.is_empty(), "a Translation Cell needs an engine");
        Self {
            engines,
            catalog: RwLock::default(),
        }
    }

    /// Asks every engine for its models.
    async fn refresh(&self) {
        let mut models: Vec<CatalogModel> = Vec::new();
        for (index, engine) in self.engines.iter().enumerate() {
            for model in engine.info().await.models {
                match models.iter_mut().find(|known| known.id == model.id) {
                    Some(known) => known.engines.push(index),
                    None => models.push(CatalogModel {
                        id: model.id,
                        name: model.name,
                        engines: vec![index],
                    }),
                }
            }
        }
        let mut catalog = self.catalog.write();
        // Keep the last good catalog while every engine is unreachable.
        if !models.is_empty() || catalog.models.is_empty() {
            catalog.models = models;
        }
        catalog.refreshed = Some(Instant::now());
    }

    async fn refresh_if_stale(&self) {
        let stale = {
            let catalog = self.catalog.read();
            catalog.models.is_empty()
                || catalog
                    .refreshed
                    .is_none_or(|refreshed| refreshed.elapsed() > CATALOG_TTL)
        };
        if stale {
            self.refresh().await;
        }
    }

    /// Every model of the catalog, the default first. Empty while no engine
    /// answers.
    pub async fn models(&self) -> Vec<TranslationModel> {
        self.refresh_if_stale().await;
        self.catalog
            .read()
            .models
            .iter()
            .map(|model| TranslationModel {
                id: model.id.clone(),
                name: model.name.clone(),
            })
            .collect()
    }

    /// The model to use for a space that chose `chosen`, and the engines
    /// that serve it, starting with the space's own replica. A model no
    /// engine serves any more falls back to the default.
    async fn route(&self, chosen: Option<&str>, affinity: &str) -> (Option<String>, Vec<usize>) {
        self.refresh_if_stale().await;
        let catalog = self.catalog.read();
        let model = chosen
            .and_then(|id| catalog.models.iter().find(|model| model.id == id))
            .or_else(|| {
                if let Some(id) = chosen {
                    tracing::warn!(model = id, "no engine serves the model; using the default");
                }
                catalog.models.first()
            });
        let (id, mut engines) = match model {
            Some(model) => (Some(model.id.clone()), model.engines.clone()),
            // Nothing known yet: any engine, its own default.
            None => (None, (0..self.engines.len()).collect()),
        };
        let start = affinity_hash(affinity) % engines.len();
        engines.rotate_left(start);
        (id, engines)
    }

    /// Runs `call` on the engines serving the request's model, moving to
    /// the next replica when one is unavailable.
    async fn call<T, F, Fut>(
        &self,
        chosen: Option<&str>,
        affinity: &str,
        call: F,
    ) -> Result<T, EngineError>
    where
        F: Fn(Arc<dyn Engine>, Option<String>) -> Fut,
        Fut: Future<Output = Result<T, EngineError>>,
    {
        let (model, engines) = self.route(chosen, affinity).await;
        let mut last = EngineError::Unavailable;
        for index in engines {
            match call(self.engines[index].clone(), model.clone()).await {
                Ok(response) => return Ok(response),
                Err(EngineError::UnknownModel) => {
                    // The engine's models changed: learn them for next time.
                    self.catalog.write().refreshed = None;
                    last = EngineError::UnknownModel;
                }
                Err(error @ (EngineError::Unavailable | EngineError::RateLimited)) => last = error,
                Err(error) => return Err(error),
            }
        }
        if last == EngineError::UnknownModel {
            // Retried later as unavailable, by when the catalog is current.
            return Err(EngineError::Unavailable);
        }
        Err(last)
    }

    /// Translates with the space's model (`request.model`); `affinity`
    /// names the space.
    pub async fn translate(
        &self,
        affinity: &str,
        request: TranslateRequest,
    ) -> Result<TranslateResponse, EngineError> {
        let chosen = request.model.clone();
        self.call(chosen.as_deref(), affinity, |engine, model| {
            let mut request = request.clone();
            request.model = model;
            async move { engine.translate(request).await }
        })
        .await
    }

    pub async fn summarize(
        &self,
        affinity: &str,
        request: SummarizeRequest,
    ) -> Result<SummarizeResponse, EngineError> {
        let chosen = request.model.clone();
        self.call(chosen.as_deref(), affinity, |engine, model| {
            let mut request = request.clone();
            request.model = model;
            async move { engine.summarize(request).await }
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use async_trait::async_trait;
    use babel_api_engine::types::{
        EngineInfo, ModelInfo, SourceMessage, TargetLanguage, Translation, Usage,
    };

    use super::*;

    /// An engine serving `models`, answering with its own name, or failing.
    struct Fake {
        name: &'static str,
        models: Vec<&'static str>,
        down: bool,
        calls: AtomicUsize,
    }

    impl Fake {
        fn new(name: &'static str, models: &[&'static str]) -> Arc<Self> {
            Arc::new(Self {
                name,
                models: models.to_vec(),
                down: false,
                calls: AtomicUsize::new(0),
            })
        }
    }

    #[async_trait]
    impl Engine for Fake {
        async fn translate(
            &self,
            body: TranslateRequest,
        ) -> Result<TranslateResponse, EngineError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if self.down {
                return Err(EngineError::Unavailable);
            }
            let model = body.model.ok_or(EngineError::InvalidOutput)?;
            if !self.models.contains(&model.as_str()) {
                return Err(EngineError::UnknownModel);
            }
            Ok(TranslateResponse {
                translations: vec![Translation {
                    lang: "ja-JP".into(),
                    text: format!("{} {model}", self.name),
                }],
                usage: Usage {
                    input_tokens: 0,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                    output_tokens: 0,
                },
            })
        }

        async fn summarize(
            &self,
            _body: SummarizeRequest,
        ) -> Result<SummarizeResponse, EngineError> {
            Err(EngineError::Unavailable)
        }

        async fn info(&self) -> EngineInfo {
            EngineInfo {
                models: self
                    .models
                    .iter()
                    .map(|id| ModelInfo {
                        id: id.to_string(),
                        name: id.to_uppercase(),
                        backend: "fake".into(),
                    })
                    .collect(),
            }
        }
    }

    fn request(model: Option<&str>) -> TranslateRequest {
        TranslateRequest {
            layers: Vec::new(),
            window: Vec::new(),
            message: SourceMessage {
                author: "a".into(),
                text: "hi".into(),
            },
            source_lang: "en-US".into(),
            targets: vec![TargetLanguage {
                tag: "ja-JP".into(),
                name: "Japanese".into(),
            }],
            model: model.map(str::to_string),
        }
    }

    async fn answer(engines: &Engines, space: &str, model: Option<&str>) -> String {
        engines
            .translate(space, request(model))
            .await
            .unwrap()
            .translations
            .remove(0)
            .text
    }

    #[tokio::test]
    async fn the_catalog_joins_every_engines_models() {
        let engines = Engines::new(vec![
            Fake::new("local", &["hy-mt2", "qwen"]),
            Fake::new("cloud", &["claude", "hy-mt2"]),
        ]);
        let models = engines.models().await;
        let ids = models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(ids, ["hy-mt2", "qwen", "claude"]);
        assert_eq!(models[2].name, "CLAUDE");

        // No choice, or a model nobody serves any more: the default.
        assert!(answer(&engines, "s", None).await.ends_with(" hy-mt2"));
        assert!(
            answer(&engines, "s", Some("gone"))
                .await
                .ends_with(" hy-mt2")
        );
        assert_eq!(answer(&engines, "s", Some("claude")).await, "cloud claude");
    }

    #[tokio::test]
    async fn a_space_sticks_to_one_replica_until_it_fails() {
        let a = Fake::new("a", &["m"]);
        let engines = Engines::new(vec![a.clone(), Fake::new("b", &["m"])]);

        // Spaces spread over the replicas, each always on the same one.
        let mut seen = std::collections::HashSet::new();
        for space in ["s1", "s2", "s3", "s4", "s5", "s6"] {
            let first = answer(&engines, space, None).await;
            for _ in 0..3 {
                assert_eq!(answer(&engines, space, None).await, first);
            }
            seen.insert(first);
        }
        assert_eq!(seen.len(), 2);

        // A replica that is down hands its spaces to the next.
        let failing = Arc::new(Fake {
            name: "b",
            models: vec!["m"],
            down: true,
            calls: AtomicUsize::new(0),
        });
        let engines = Engines::new(vec![a.clone(), failing.clone()]);
        for space in ["s1", "s2", "s3", "s4", "s5", "s6"] {
            assert_eq!(answer(&engines, space, None).await, "a m");
        }
        assert!(failing.calls.load(Ordering::Relaxed) > 0);
    }
}
