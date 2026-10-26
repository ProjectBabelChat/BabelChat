//! On-device translation, always through English.
//!
//! Device models (Bergamot and the like) come in pairs with English on one
//! side, so a device needs 2 models per language instead of one per pair.
//! The SDK chains them: Japanese to French is ja → en → fr. When the server
//! already has an English (LLM) translation, the chain starts from it and
//! takes one step: en → fr.

use std::time::Duration;

use async_trait::async_trait;
use babel_lang::Normalizer;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DeviceError {
    /// No model for this language.
    #[error("no on-device model for {0}")]
    Unsupported(String),
    #[error("on-device translation failed: {0}")]
    Failed(String),
}

/// Translates on the user's device. Implementations are only asked for
/// pairs with English on one side; tags are normalized BCP 47 tags such as
/// "ja-JP", "en-US" or plain "en".
#[async_trait]
pub trait DeviceTranslator: Send + Sync {
    async fn translate(&self, text: &str, from: &str, to: &str) -> Result<String, DeviceError>;
}

/// Stands in for real models: prefixes the text with the target tag, so
/// chains stay visible ("[device fr-FR] [en-US] こんにちは").
#[derive(Debug, Default)]
pub struct MockDeviceTranslator {
    pub latency: Duration,
}

#[async_trait]
impl DeviceTranslator for MockDeviceTranslator {
    async fn translate(&self, text: &str, _from: &str, to: &str) -> Result<String, DeviceError> {
        if !self.latency.is_zero() {
            tokio::time::sleep(self.latency).await;
        }
        Ok(format!("[device {to}] {text}"))
    }
}

/// The pivot language for device chains.
const ENGLISH: &str = "en";

/// The steps from `from` to `to`, each with English on one side.
pub(crate) fn steps(normalizer: &Normalizer, from: &str, to: &str) -> Vec<(String, String)> {
    if normalizer.same_language(from, to) {
        Vec::new()
    } else if normalizer.is_english(from) || normalizer.is_english(to) {
        vec![(from.to_string(), to.to_string())]
    } else {
        vec![
            (from.to_string(), ENGLISH.to_string()),
            (ENGLISH.to_string(), to.to_string()),
        ]
    }
}

pub(crate) async fn translate(
    translator: &dyn DeviceTranslator,
    normalizer: &Normalizer,
    text: &str,
    from: &str,
    to: &str,
) -> Result<String, DeviceError> {
    let mut text = text.to_string();
    for (from, to) in steps(normalizer, from, to) {
        text = translator.translate(&text, &from, &to).await?;
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chains_go_through_english() {
        let normalizer = Normalizer::new();
        let pairs = |from, to| {
            steps(&normalizer, from, to)
                .into_iter()
                .map(|(from, to)| format!("{from}>{to}"))
                .collect::<Vec<_>>()
        };
        assert_eq!(pairs("ja-JP", "fr-FR"), ["ja-JP>en", "en>fr-FR"]);
        assert_eq!(pairs("en-US", "fr-FR"), ["en-US>fr-FR"]);
        assert_eq!(pairs("ja-JP", "en-GB"), ["ja-JP>en-GB"]);
        assert!(pairs("pt-PT", "pt-BR").is_empty());
    }
}
