//! A backend that pretends to translate.
//!
//! It renders the same prompt a real backend would and simulates a prefix
//! cache on it, so token usage and cache hit rates can be measured — and the
//! layering tuned — before any model is involved.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use babel_api_engine::types::{
    EngineError, EngineInfo, SummarizeRequest, SummarizeResponse, TranslateRequest,
    TranslateResponse, Translation, Usage,
};
use parking_lot::Mutex;

use crate::{
    Backend,
    prompt::{Prompt, summary_prompt, translation_prompt},
};

#[derive(Debug, Clone)]
pub struct MockConfig {
    /// Simulated model latency.
    pub latency: Duration,
    /// Cache entry lifetime, refreshed on every hit (Claude's default TTL).
    pub cache_ttl: Duration,
    /// Prefixes shorter than this are never cached (Claude's current models).
    pub min_cacheable_tokens: i64,
}

impl Default for MockConfig {
    fn default() -> Self {
        Self {
            latency: Duration::from_millis(300),
            cache_ttl: Duration::from_secs(5 * 60),
            min_cacheable_tokens: 512,
        }
    }
}

pub struct MockBackend {
    config: MockConfig,
    /// Prefix hash at a breakpoint -> expiry.
    cache: Mutex<HashMap<[u8; 32], Instant>>,
}

impl MockBackend {
    pub fn new(config: MockConfig) -> Self {
        Self {
            config,
            cache: Mutex::default(),
        }
    }

    /// Simulates Claude-style prefix caching. Entries are written at
    /// breakpoints; a read matches the prefix up to any block boundary within
    /// the last [`LOOKBACK`] blocks before a breakpoint.
    fn account(&self, prompt: &Prompt, output: &str) -> Usage {
        const LOOKBACK: usize = 20;

        let now = Instant::now();
        let mut hasher = blake3::Hasher::new();
        let mut tokens = 0;
        // (prefix hash, prefix tokens, is a breakpoint) at each block boundary.
        let mut boundaries = Vec::new();
        for segment in &prompt.segments {
            hasher.update(segment.text.as_bytes());
            tokens += estimate_tokens(&segment.text);
            boundaries.push((*hasher.finalize().as_bytes(), tokens, segment.breakpoint));
        }
        let cacheable = |tokens: i64| tokens >= self.config.min_cacheable_tokens;

        let mut cache = self.cache.lock();
        cache.retain(|_, expiry| *expiry > now);

        let mut read = 0;
        for (index, _) in boundaries.iter().enumerate().filter(|(_, b)| b.2) {
            let window = &boundaries[index.saturating_sub(LOOKBACK)..=index];
            if let Some((hash, tokens, _)) = window
                .iter()
                .rev()
                .find(|(hash, tokens, _)| cacheable(*tokens) && cache.contains_key(hash))
                && *tokens > read
            {
                read = *tokens;
                cache.insert(*hash, now + self.config.cache_ttl);
            }
        }

        let mut written = read;
        for (hash, tokens, breakpoint) in &boundaries {
            if *breakpoint && *tokens > read && cacheable(*tokens) {
                cache.insert(*hash, now + self.config.cache_ttl);
                written = written.max(*tokens);
            }
        }

        Usage {
            input_tokens: tokens - written,
            cache_read_tokens: read,
            cache_write_tokens: written - read,
            output_tokens: estimate_tokens(output),
        }
    }
}

/// A rough token count: about three characters per token across scripts.
pub fn estimate_tokens(text: &str) -> i64 {
    (text.chars().count() as i64 + 2) / 3
}

#[async_trait]
impl Backend for MockBackend {
    fn info(&self) -> EngineInfo {
        EngineInfo {
            backend: "mock".into(),
            model: "mock".into(),
        }
    }

    async fn translate(
        &self,
        request: &TranslateRequest,
    ) -> Result<TranslateResponse, EngineError> {
        tokio::time::sleep(self.config.latency).await;

        let translations = request
            .targets
            .iter()
            .map(|target| Translation {
                lang: target.tag.clone(),
                text: format!("[{}] {}", target.tag, request.message.text),
            })
            .collect::<Vec<_>>();
        let output = translations
            .iter()
            .map(|translation| translation.text.as_str())
            .collect::<String>();

        Ok(TranslateResponse {
            usage: self.account(&translation_prompt(request), &output),
            translations,
        })
    }

    async fn summarize(
        &self,
        request: &SummarizeRequest,
    ) -> Result<SummarizeResponse, EngineError> {
        let mut text = request.previous.clone().unwrap_or_default();
        for message in &request.messages {
            if !text.is_empty() {
                text.push_str(" / ");
            }
            text.push_str(&message.author);
            text.push_str(": ");
            text.push_str(&message.text);
        }
        let max_chars = request.max_chars.max(0) as usize;
        if text.chars().count() > max_chars {
            // Keep the most recent part, like a rolling summary would.
            let skip = text.chars().count() - max_chars;
            text = text.chars().skip(skip).collect();
        }

        Ok(SummarizeResponse {
            usage: self.account(&summary_prompt(request), &text),
            text,
        })
    }
}

#[cfg(test)]
mod tests {
    use babel_api_engine::types::{ContextLayer, LayerKind, SourceMessage, TargetLanguage};

    use super::*;

    fn request(window: usize) -> TranslateRequest {
        TranslateRequest {
            layers: vec![ContextLayer {
                id: "space".into(),
                version: 1,
                kind: LayerKind::Space,
                lang: None,
                content: "A long description of the community. ".repeat(40),
            }],
            window: (0..window)
                .map(|i| SourceMessage {
                    author: "alice".into(),
                    text: format!("message number {i} with some words in it"),
                })
                .collect(),
            message: SourceMessage {
                author: "bob".into(),
                text: "hello".into(),
            },
            source_lang: "en-US".into(),
            targets: vec![TargetLanguage {
                tag: "ja-JP".into(),
                name: "Japanese".into(),
            }],
        }
    }

    #[tokio::test]
    async fn appending_to_the_window_hits_the_cache() {
        let backend = MockBackend::new(MockConfig {
            latency: Duration::ZERO,
            ..MockConfig::default()
        });

        let first = backend.translate(&request(5)).await.unwrap();
        assert_eq!(first.usage.cache_read_tokens, 0);
        assert!(first.usage.cache_write_tokens > 0);
        assert_eq!(first.translations[0].text, "[ja-JP] hello");

        // The next message sees one more line in the window: everything
        // written last time is read back.
        let second = backend.translate(&request(6)).await.unwrap();
        let first_cached = first.usage.cache_read_tokens + first.usage.cache_write_tokens;
        assert_eq!(second.usage.cache_read_tokens, first_cached);
        assert!(second.usage.cache_write_tokens > 0);
        assert!(second.usage.cache_write_tokens < first.usage.cache_write_tokens);
    }
}
