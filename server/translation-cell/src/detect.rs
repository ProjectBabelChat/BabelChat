//! Source language detection.
//!
//! Text tells the language but rarely the region (en-GB vs en-US) and, for
//! Chinese, not even the script. The author's locale fills in what detection
//! cannot tell; it is trusted outright when the text is too short to detect.

use babel_lang::{Normalizer, RegionPolicy};

/// Tag for text whose language could not be determined.
pub const UNKNOWN: &str = "und";

/// ISO 639-3 codes reported by whatlang, to ISO 639-1.
const ISO_639_3_TO_1: &[(&str, &str)] = &[
    ("afr", "af"),
    ("amh", "am"),
    ("ara", "ar"),
    ("aze", "az"),
    ("bel", "be"),
    ("ben", "bn"),
    ("bul", "bg"),
    ("cat", "ca"),
    ("ces", "cs"),
    ("cmn", "zh"),
    ("dan", "da"),
    ("deu", "de"),
    ("ell", "el"),
    ("eng", "en"),
    ("epo", "eo"),
    ("est", "et"),
    ("fin", "fi"),
    ("fra", "fr"),
    ("guj", "gu"),
    ("heb", "he"),
    ("hin", "hi"),
    ("hrv", "hr"),
    ("hun", "hu"),
    ("hye", "hy"),
    ("ind", "id"),
    ("ita", "it"),
    ("jav", "jv"),
    ("jpn", "ja"),
    ("kan", "kn"),
    ("kat", "ka"),
    ("khm", "km"),
    ("kor", "ko"),
    ("lat", "la"),
    ("lav", "lv"),
    ("lit", "lt"),
    ("mal", "ml"),
    ("mar", "mr"),
    ("mkd", "mk"),
    ("mya", "my"),
    ("nep", "ne"),
    ("nld", "nl"),
    ("nob", "nb"),
    ("ori", "or"),
    ("pan", "pa"),
    ("pes", "fa"),
    ("pol", "pl"),
    ("por", "pt"),
    ("ron", "ro"),
    ("rus", "ru"),
    ("sin", "si"),
    ("slk", "sk"),
    ("slv", "sl"),
    ("sna", "sn"),
    ("spa", "es"),
    ("srp", "sr"),
    ("swe", "sv"),
    ("tam", "ta"),
    ("tel", "te"),
    ("tgl", "tl"),
    ("tha", "th"),
    ("tuk", "tk"),
    ("tur", "tr"),
    ("ukr", "uk"),
    ("urd", "ur"),
    ("uzb", "uz"),
    ("vie", "vi"),
    ("yid", "yi"),
    ("zul", "zu"),
];

/// Below this confidence the author's locale wins over detection.
const MIN_CONFIDENCE: f64 = 0.5;

/// The detected language and whether detection is confident about it.
fn detect_language(text: &str) -> Option<(&'static str, bool)> {
    // An emoji's name would read as Latin words of no language in particular.
    let info = whatlang::detect(&babel_emoji::words(text))?;
    let code = info.lang().code();
    ISO_639_3_TO_1
        .iter()
        .find(|(three, _)| *three == code)
        .map(|(_, one)| (*one, info.confidence() >= MIN_CONFIDENCE))
}

fn primary_language(tag: &str) -> &str {
    tag.split(['-', '_']).next().unwrap_or(tag)
}

/// The normalized source language of a message, or [`UNKNOWN`].
pub fn source_language(
    normalizer: &Normalizer,
    text: &str,
    author_locale: Option<&str>,
    policy: &RegionPolicy,
) -> String {
    let normalize = |tag: &str| normalizer.normalize(tag, policy).ok();
    let detected = detect_language(text);

    let resolved = match (detected, author_locale) {
        // The author writes in their own language: keep their region and
        // script (a Taiwanese author's Chinese is Traditional).
        (Some((detected, _)), Some(author))
            if primary_language(author).eq_ignore_ascii_case(detected) =>
        {
            normalize(author)
        }
        (Some((detected, true)), _) => normalize(detected),
        (Some((_, false)), Some(author)) => normalize(author),
        (Some((_, false)), None) => None,
        (None, Some(author)) => normalize(author),
        (None, None) => None,
    };

    resolved.unwrap_or_else(|| UNKNOWN.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_uses_the_author_locale_for_what_text_cannot_tell() {
        let n = Normalizer::new();
        let merged = RegionPolicy::default();
        let distinct = RegionPolicy {
            mode: babel_lang::RegionMode::Distinct,
            aliases: Vec::new(),
        };

        let english = "I think we should meet tomorrow at the station near the park.";
        assert_eq!(
            source_language(&n, english, Some("en-GB"), &distinct),
            "en-GB"
        );
        assert_eq!(
            source_language(&n, english, Some("en-GB"), &merged),
            "en-US"
        );
        // A Japanese author writing English is still writing English.
        assert_eq!(
            source_language(&n, english, Some("ja-JP"), &merged),
            "en-US"
        );

        let chinese = "我们明天在公园附近的车站见面吧，好不好？";
        assert_eq!(
            source_language(&n, chinese, Some("zh-TW"), &merged),
            "zh-Hant-TW"
        );
        assert_eq!(source_language(&n, chinese, None, &merged), "zh-CN");

        // Emoji names are not words.
        assert_eq!(
            source_language(&n, "鍵を渡します！:rage:", Some("en-US"), &merged),
            "ja-JP"
        );

        // Too short to detect: trust the author.
        assert_eq!(source_language(&n, "ok", Some("ko-KR"), &merged), "ko-KR");
        assert_eq!(source_language(&n, "ok", None, &merged), UNKNOWN);
    }
}
