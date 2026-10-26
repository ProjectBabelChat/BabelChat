//! On-device translation with the Firefox Translations models, the
//! descendants of Project Bergamot: small CPU models, one per direction,
//! each with English on one side, which is exactly what
//! [`DeviceTranslator`] asks for. The SDK chains them through English.
//!
//! [fxtranslate](https://crates.io/crates/fxtranslate) runs the models in
//! Rust (the same int8 GEMM kernels as Firefox) and finds, downloads,
//! verifies and caches them. Models are fetched on first use of a
//! direction (some 20–40 MB each) and loaded once per process.
//!
//! The models are discovered through Firefox's Remote Settings and served
//! by Firefox's CDN. That hosting is meant for Firefox: a released app must
//! mirror the models it needs (see docs/architecture/client.md).

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Arc, mpsc},
};

use async_trait::async_trait;
use babel_sdk::{DeviceError, DeviceTranslator};
use fxtranslate::{
    cache::{Cache, ModelFiles},
    engine::Engine,
    fetch::NetworkFetch,
    loader,
};
use parking_lot::Mutex;
use tokio::sync::{OnceCell, oneshot};

#[derive(Debug, Clone)]
pub struct BergamotConfig {
    /// Where models are kept, one directory per direction.
    pub models_dir: PathBuf,
    /// Fetch missing models. Off: only models already in `models_dir`.
    pub download: bool,
}

type Pair = (String, String);

/// One translation for the model thread.
struct Job {
    pair: Pair,
    files: ModelFiles,
    text: String,
    reply: oneshot::Sender<Result<String, DeviceError>>,
}

pub struct BergamotTranslator {
    config: BergamotConfig,
    /// Model files per direction, found (or downloaded) once.
    files: Mutex<HashMap<Pair, Arc<OnceCell<ModelFiles>>>>,
    /// Directions no model exists for, so they are not looked up again.
    unsupported: Mutex<HashSet<Pair>>,
    jobs: Mutex<mpsc::Sender<Job>>,
}

impl BergamotTranslator {
    /// Starts the thread that owns the loaded models. Engines hold native
    /// buffers that may be shared but must stay on the thread that made
    /// them, so one thread loads, runs and drops them all; it ends with the
    /// translator.
    pub fn new(config: BergamotConfig) -> Self {
        let (jobs, queue) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("babel-bergamot".into())
            .spawn(move || run_models(queue))
            .expect("spawning the model thread");
        Self {
            config,
            files: Mutex::default(),
            unsupported: Mutex::default(),
            jobs: Mutex::new(jobs),
        }
    }

    /// The files of one direction's model, downloading them once.
    async fn files(&self, pair: &Pair) -> Result<ModelFiles, DeviceError> {
        if self.unsupported.lock().contains(pair) {
            return Err(DeviceError::Unsupported(format!("{}→{}", pair.0, pair.1)));
        }
        let cell = self.files.lock().entry(pair.clone()).or_default().clone();
        let found = cell
            .get_or_try_init(|| {
                let (config, pair) = (self.config.clone(), pair.clone());
                async move {
                    tokio::task::spawn_blocking(move || find(&config, &pair.0, &pair.1))
                        .await
                        .map_err(|error| DeviceError::Failed(error.to_string()))?
                }
            })
            .await;
        match found {
            Ok(files) => Ok(files.clone()),
            Err(DeviceError::Unsupported(name)) => {
                self.unsupported.lock().insert(pair.clone());
                Err(DeviceError::Unsupported(name))
            }
            Err(error) => Err(error),
        }
    }
}

#[async_trait]
impl DeviceTranslator for BergamotTranslator {
    async fn translate(&self, text: &str, from: &str, to: &str) -> Result<String, DeviceError> {
        let pair = (model_lang(from), model_lang(to));
        let files = self.files(&pair).await?;
        let (reply, answer) = oneshot::channel();
        self.jobs
            .lock()
            .send(Job {
                pair,
                files,
                text: text.to_string(),
                reply,
            })
            .map_err(|_| DeviceError::Failed("the model thread stopped".into()))?;
        answer
            .await
            .map_err(|_| DeviceError::Failed("the model thread stopped".into()))?
    }
}

/// The model thread: loads each direction on first use and keeps it.
fn run_models(queue: mpsc::Receiver<Job>) {
    let mut engines = HashMap::<Pair, Engine>::new();
    for job in queue {
        let engine = match engines.entry(job.pair) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let files = &job.files;
                match Engine::load(&files.model, &files.src_vocab, &files.trg_vocab) {
                    Ok(engine) => entry.insert(engine),
                    Err(error) => {
                        let _ = job.reply.send(Err(DeviceError::Failed(error)));
                        continue;
                    }
                }
            }
        };
        let _ = job
            .reply
            .send(Ok(restore_bytes(&engine.translate_long(&job.text))));
    }
}

/// Finds a direction's model on disk, downloading it if allowed.
fn find(config: &BergamotConfig, from: &str, to: &str) -> Result<ModelFiles, DeviceError> {
    let cache = Cache::with_root(&config.models_dir);
    let pair = format!("{from}→{to}");
    // A model on disk is used as it is: no request leaves the device.
    // Newer versions come with an explicit download (later).
    if let Some(files) = cache.cached_model(from, to) {
        return Ok(files);
    }
    if !config.download {
        return Err(DeviceError::Unsupported(pair));
    }
    // Processes sharing the directory (several profiles) take turns, so
    // one downloads and the others find the result.
    std::fs::create_dir_all(&config.models_dir)
        .map_err(|error| DeviceError::Failed(error.to_string()))?;
    let lock = std::fs::File::create(config.models_dir.join(format!(".{from}-{to}.lock")))
        .and_then(|file| file.lock().map(|()| file))
        .map_err(|error| DeviceError::Failed(error.to_string()))?;
    tracing::info!(pair, "fetching an on-device model");
    // Another process may have fetched it while this one waited.
    let found = match cache.cached_model(from, to) {
        Some(files) => Ok(files),
        None => match loader::ensure_files(&NetworkFetch::new(), &cache, from, to) {
            Ok(files) => Ok(files),
            Err(error) if error.contains("no model") => Err(DeviceError::Unsupported(pair)),
            Err(error) => Err(DeviceError::Failed(error)),
        },
    };
    drop(lock);
    found
}

/// Turns SentencePiece byte pieces left in the output (`<0xC3><0x87>a`) back
/// into the text they encode (`Ça`). fxtranslate 0.5 emits them verbatim.
fn restore_bytes(text: &str) -> String {
    if !text.contains("<0x") {
        return text.to_string();
    }
    let mut out = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while !rest.is_empty() {
        let byte = rest
            .strip_prefix(b"<0x")
            .filter(|tail| tail.len() >= 3 && tail[2] == b'>')
            .and_then(|tail| std::str::from_utf8(&tail[..2]).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match byte {
            Some(byte) => {
                out.push(byte);
                rest = &rest[6..];
            }
            None => {
                out.push(rest[0]);
                rest = &rest[1..];
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The model's name for a BCP 47 tag: the language alone ("ja-JP" → "ja"),
/// except Chinese, whose models are per script.
pub fn model_lang(tag: &str) -> String {
    let mut parts = tag.split(['-', '_']);
    let language = parts.next().unwrap_or_default().to_ascii_lowercase();
    if language != "zh" {
        return language;
    }
    let traditional = parts.any(|part| {
        matches!(
            part.to_ascii_lowercase().as_str(),
            "hant" | "tw" | "hk" | "mo"
        )
    });
    if traditional {
        "zh-Hant".into()
    } else {
        "zh".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_pieces_become_text() {
        assert_eq!(restore_bytes("<0xC3><0x87>a va ?"), "Ça va ?");
        assert_eq!(restore_bytes("a <0x41> <0xZZ> <0x"), "a A <0xZZ> <0x");
        assert_eq!(restore_bytes("plain"), "plain");
    }

    #[test]
    fn tags_name_models() {
        assert_eq!(model_lang("ja-JP"), "ja");
        assert_eq!(model_lang("en"), "en");
        assert_eq!(model_lang("pt-BR"), "pt");
        assert_eq!(model_lang("zh-CN"), "zh");
        assert_eq!(model_lang("zh-Hant-TW"), "zh-Hant");
        assert_eq!(model_lang("zh-HK"), "zh-Hant");
    }

    #[tokio::test]
    async fn without_downloads_missing_models_are_unsupported() {
        let dir = tempfile::tempdir().unwrap();
        let translator = BergamotTranslator::new(BergamotConfig {
            models_dir: dir.path().to_path_buf(),
            download: false,
        });
        assert_eq!(
            translator.translate("hello", "en", "ja-JP").await,
            Err(DeviceError::Unsupported("en→ja".into()))
        );
    }

    /// Downloads real models: `cargo test -p babel-bergamot -- --ignored`.
    #[tokio::test]
    #[ignore = "downloads models from Mozilla's servers"]
    async fn real_models_translate_through_english() {
        let dir = std::env::var_os("BABEL_MODELS_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("babel-bergamot-test-models"));
        let translator = BergamotTranslator::new(BergamotConfig {
            models_dir: dir,
            download: true,
        });
        for (text, from, to) in [
            ("明日の夜、一緒に遊びませんか", "ja-JP", "en"),
            ("Shall we play together tomorrow night?", "en-US", "ja-JP"),
            ("Shall we play together tomorrow night?", "en-US", "fr-FR"),
        ] {
            let started = std::time::Instant::now();
            let translated = translator.translate(text, from, to).await.unwrap();
            println!("{from}→{to} ({:?}): {translated}", started.elapsed());
            assert!(!translated.trim().is_empty());
        }
        assert!(matches!(
            translator.translate("hello", "en", "tlh").await,
            Err(DeviceError::Unsupported(_))
        ));
    }
}
