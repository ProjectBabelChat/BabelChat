//! Which text a reader sees for a message.
//!
//! In order of preference:
//!
//! 1. The original, when it is the reader's own message or already in the
//!    reader's language.
//! 2. The server's LLM translation into the reader's language.
//! 3. An on-device translation: from the server's English translation when
//!    there is one (one step), otherwise from the original through English
//!    (two steps). Free guilds always end here; so do readers outside the
//!    guild's languages, and paid guilds whose LLM failed or ran out of
//!    budget.
//! 4. The original, marked as untranslated.
//!
//! The decision is a pure function of the message, its translations and the
//! reader's [`LanguageView`]; the session only adds the device results.

use babel_api_edge_client::types::{LanguageView, Message, MessageTranslation};
use babel_lang::Normalizer;

/// What to show for a message, and where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Display {
    pub text: String,
    pub shown: Shown,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Shown {
    /// The original, which is what the reader wants.
    Original,
    /// The original, while a translation is on its way.
    Translating,
    /// Translated by the server's LLM, with context.
    Server { from: String, to: String },
    /// Translated on this device. `via_english` when the text went through
    /// English, from the server's English translation or in two steps.
    Device {
        from: String,
        to: String,
        via_english: bool,
    },
    /// The original, because no translation is available.
    Untranslated { reason: String },
}

impl Display {
    pub fn is_settled(&self) -> bool {
        self.shown != Shown::Translating
    }

    pub fn is_translated(&self) -> bool {
        matches!(self.shown, Shown::Server { .. } | Shown::Device { .. })
    }
}

/// Where a message stands before device results are known.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Plan {
    Show(Display),
    /// Translate `text` (in `from`) on the device into `to`.
    Device(DeviceJob),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct DeviceJob {
    pub message_id: String,
    pub rev: i64,
    pub from: String,
    pub to: String,
    pub text: String,
    /// The original's language, for display.
    pub source: String,
}

/// Language detection gave up.
const UNKNOWN: &str = "und";

pub(crate) fn plan(
    normalizer: &Normalizer,
    me: &str,
    message: &Message,
    translation: Option<&MessageTranslation>,
    view: &LanguageView,
) -> Plan {
    let original = |shown| {
        Plan::Show(Display {
            text: message.text.clone(),
            shown,
        })
    };
    // Files alone carry no text to translate.
    if message.author_id == me
        || message.text.trim().is_empty()
        || !(view.llm || view.client_translation)
    {
        return original(Shown::Original);
    }
    // Translations of an older revision do not count.
    let Some(translation) = translation.filter(|t| t.rev == message.rev) else {
        return original(Shown::Translating);
    };
    let source = &translation.source_lang;
    match translation.status.as_str() {
        "pending" => return original(Shown::Translating),
        _ if source == UNKNOWN || *source == view.effective => return original(Shown::Original),
        _ => {}
    }

    if let Some(translated) = translation
        .translations
        .iter()
        .find(|t| t.lang == view.effective)
    {
        return Plan::Show(Display {
            text: translated.text.clone(),
            shown: Shown::Server {
                from: source.clone(),
                to: view.effective.clone(),
            },
        });
    }
    // A regional variant nobody translates: close enough.
    if normalizer.same_language(source, &view.effective) {
        return original(Shown::Original);
    }

    let english = translation
        .translations
        .iter()
        .find(|t| normalizer.is_english(&t.lang));
    let (from, text) = match english {
        Some(english) => (english.lang.clone(), english.text.clone()),
        None => (source.clone(), message.text.clone()),
    };
    Plan::Device(DeviceJob {
        message_id: message.id.clone(),
        rev: message.rev,
        from,
        to: view.effective.clone(),
        text,
        source: source.clone(),
    })
}

/// The display once the device job has finished (or failed).
pub(crate) fn device_display(
    normalizer: &Normalizer,
    message: &Message,
    job: &DeviceJob,
    result: Option<&Result<String, String>>,
) -> Display {
    match result {
        None => Display {
            text: message.text.clone(),
            shown: Shown::Translating,
        },
        Some(Ok(text)) => Display {
            text: text.clone(),
            shown: Shown::Device {
                from: job.source.clone(),
                to: job.to.clone(),
                via_english: job.from != job.source
                    || crate::device::steps(normalizer, &job.from, &job.to).len() > 1,
            },
        },
        Some(Err(reason)) => Display {
            text: message.text.clone(),
            shown: Shown::Untranslated {
                reason: reason.clone(),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use babel_api_edge_client::types::Translation;

    use super::*;

    fn message(author: &str, rev: i64) -> Message {
        Message {
            id: "1".into(),
            guild_id: "g".into(),
            channel_id: "c".into(),
            author_id: author.into(),
            author_name: author.into(),
            text: "こんにちは".into(),
            reply_to: None,
            mentions: Vec::new(),
            rev,
            created_at: 0,
            edited_at: None,
            reactions: Vec::new(),
            attachments: Vec::new(),
        }
    }

    fn translation(status: &str, langs: &[&str]) -> MessageTranslation {
        MessageTranslation {
            message_id: "1".into(),
            channel_id: "c".into(),
            rev: 1,
            source_lang: "ja-JP".into(),
            status: status.into(),
            translations: langs
                .iter()
                .map(|lang| Translation {
                    lang: lang.to_string(),
                    text: format!("[{lang}] こんにちは"),
                })
                .collect(),
        }
    }

    fn view(effective: &str, llm: bool, client_translation: bool) -> LanguageView {
        LanguageView {
            effective: effective.into(),
            in_set: llm,
            llm,
            client_translation,
        }
    }

    fn shown(plan: Plan) -> Shown {
        match plan {
            Plan::Show(display) => display.shown,
            Plan::Device(job) => Shown::Device {
                from: job.from,
                to: job.to,
                via_english: false,
            },
        }
    }

    #[test]
    fn readers_see_the_best_text_available() {
        let n = Normalizer::new();
        let english = view("en-US", true, false);
        let done = translation("done", &["en-US"]);

        // Own messages and the reader's own language stay original.
        let p = plan(&n, "alice", &message("alice", 1), Some(&done), &english);
        assert_eq!(shown(p), Shown::Original);
        let p = plan(
            &n,
            "bob",
            &message("alice", 1),
            Some(&done),
            &view("ja-JP", true, false),
        );
        assert_eq!(shown(p), Shown::Original);

        // The LLM translation, once it is for the current revision.
        let p = plan(&n, "bob", &message("alice", 1), Some(&done), &english);
        assert_eq!(
            shown(p),
            Shown::Server {
                from: "ja-JP".into(),
                to: "en-US".into()
            }
        );
        let p = plan(&n, "bob", &message("alice", 2), Some(&done), &english);
        assert_eq!(shown(p), Shown::Translating);
        let p = plan(&n, "bob", &message("alice", 1), None, &english);
        assert_eq!(shown(p), Shown::Translating);
    }

    #[test]
    fn devices_start_from_english_when_they_can() {
        let n = Normalizer::new();
        let french = view("fr-FR", false, true);

        // Paid guild: one step from the LLM's English.
        let p = plan(
            &n,
            "carol",
            &message("alice", 1),
            Some(&translation("done", &["en-US"])),
            &french,
        );
        let Plan::Device(job) = p else { panic!() };
        assert_eq!(
            (job.from.as_str(), job.text.as_str()),
            ("en-US", "[en-US] こんにちは")
        );

        // Free guild: from the original, through English on the device.
        let p = plan(
            &n,
            "carol",
            &message("alice", 1),
            Some(&translation("done", &[])),
            &french,
        );
        let Plan::Device(job) = p else { panic!() };
        assert_eq!(
            (job.from.as_str(), job.text.as_str()),
            ("ja-JP", "こんにちは")
        );
        let display = device_display(&n, &message("alice", 1), &job, Some(&Ok("x".into())));
        assert_eq!(
            display.shown,
            Shown::Device {
                from: "ja-JP".into(),
                to: "fr-FR".into(),
                via_english: true
            }
        );

        // An LLM reader whose translation failed falls back to the device.
        let p = plan(
            &n,
            "bob",
            &message("alice", 1),
            Some(&translation("budget_exceeded", &[])),
            &view("en-US", true, false),
        );
        assert!(matches!(p, Plan::Device(_)));

        // Readers who want no translation get none.
        let p = plan(
            &n,
            "dave",
            &message("alice", 1),
            Some(&translation("done", &[])),
            &view("de-DE", false, false),
        );
        assert_eq!(shown(p), Shown::Original);
    }
}
