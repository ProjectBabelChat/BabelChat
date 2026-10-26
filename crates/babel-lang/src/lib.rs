//! Language tag normalization.
//!
//! Every component that compares languages runs this same code on the same
//! policy snapshot: Translation Cells when they store translations, Edges when
//! they pick what a viewer reads. A difference here would split translation
//! keys and shake the prefix of cached prompts, so the rules live in one place.
//!
//! Normalization collapses regions only, never scripts: `zh-TW` and `zh-CN`
//! look like regional variants but are Traditional and Simplified Chinese.
//!
//! See docs/architecture/languages.md.

use icu_locale::{
    LanguageIdentifier, LocaleExpander,
    subtags::{Language, Region, Script},
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LangError {
    #[error("invalid language tag: {0:?}")]
    InvalidTag(String),
}

/// How a space treats regional variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RegionMode {
    /// `en-GB` becomes `en-US`: one tag per language (and script).
    #[default]
    Merged,
    /// `en-GB` and `en-US` stay two languages.
    Distinct,
}

/// A space-specific override, e.g. `pt` to `pt-PT`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alias {
    pub from: String,
    pub to: String,
}

/// The part of a space's language policy that affects normalization.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegionPolicy {
    pub mode: RegionMode,
    pub aliases: Vec<Alias>,
}

/// Representative region of each language in merged mode, where it differs
/// from (or is worth stating next to) the likely region from CLDR.
///
/// Keys are a language, or a language and script.
const DEFAULT_REGIONS: &[(&str, &str)] = &[
    ("en", "US"),
    ("pt", "BR"),
    // Most Spanish speakers live in Latin America. Spanish communities can
    // alias `es` to `es-ES`.
    ("es", "419"),
    ("zh-Hans", "CN"),
    ("zh-Hant", "TW"),
];

/// A parsed tag with every subtag filled in where CLDR knows one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Parts {
    language: Language,
    script: Option<Script>,
    /// The region as written, before filling in.
    written_region: Option<Region>,
    region: Option<Region>,
}

pub struct Normalizer {
    expander: LocaleExpander,
}

// Servers share one normalizer across tasks.
const _: () = {
    const fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Normalizer>();
};

impl Default for Normalizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Normalizer {
    pub fn new() -> Self {
        Self {
            expander: LocaleExpander::new_common(),
        }
    }

    /// Normalizes `tag` under `policy`.
    ///
    /// Accepts `_` as well as `-` (`en_GB`), any letter case, and drops
    /// variants and extensions. The result is the shortest canonical form:
    /// the script is omitted when it is the language's default (`en-GB`, but
    /// `zh-Hant-TW`).
    pub fn normalize(&self, tag: &str, policy: &RegionPolicy) -> Result<String, LangError> {
        let parts = self.parse(tag)?;

        if let Some(target) = self.find_alias(&parts, policy)? {
            return Ok(self.render(&target));
        }

        let region = match policy.mode {
            RegionMode::Merged => self.default_region(&parts),
            RegionMode::Distinct => parts.written_region.or(parts.region),
        };

        Ok(self.render(&Parts { region, ..parts }))
    }

    /// Language and script without region, e.g. `en` or `zh-Hant`.
    pub fn base(&self, tag: &str) -> Result<String, LangError> {
        let parts = self.parse(tag)?;
        Ok(self.render(&Parts {
            region: None,
            ..parts
        }))
    }

    /// Whether two tags name the same language and script, ignoring region.
    pub fn same_language(&self, a: &str, b: &str) -> bool {
        match (self.parse(a), self.parse(b)) {
            (Ok(a), Ok(b)) => a.language == b.language && a.script == b.script,
            _ => false,
        }
    }

    /// Whether `tag` is English in any region. English is the pivot for
    /// on-device translation.
    pub fn is_english(&self, tag: &str) -> bool {
        self.parse(tag)
            .is_ok_and(|parts| parts.language.as_str() == "en")
    }

    fn parse(&self, tag: &str) -> Result<Parts, LangError> {
        let invalid = || LangError::InvalidTag(tag.to_string());

        let cleaned = tag.trim().replace('_', "-");
        let parsed = LanguageIdentifier::try_from_str(&cleaned).map_err(|_| invalid())?;
        if parsed.language == Language::UNKNOWN {
            return Err(invalid());
        }

        let mut maximized = LanguageIdentifier {
            language: parsed.language,
            script: parsed.script,
            region: parsed.region,
            variants: Default::default(),
        };
        self.expander.maximize(&mut maximized);

        Ok(Parts {
            language: parsed.language,
            script: maximized.script.or(parsed.script),
            written_region: parsed.region,
            region: maximized.region,
        })
    }

    /// The most specific alias whose `from` matches: one naming a region
    /// beats one naming a script, which beats one naming only the language.
    fn find_alias(&self, parts: &Parts, policy: &RegionPolicy) -> Result<Option<Parts>, LangError> {
        let mut best: Option<(u8, &Alias)> = None;

        for alias in &policy.aliases {
            let from = self.parse(&alias.from)?;
            let from_script = LanguageIdentifier::try_from_str(&alias.from.replace('_', "-"))
                .ok()
                .and_then(|id| id.script);

            if from.language != parts.language {
                continue;
            }
            if from.script != parts.script {
                continue;
            }
            if from.written_region.is_some() && from.written_region != parts.region {
                continue;
            }

            let specificity = match (from.written_region, from_script) {
                (Some(_), _) => 2,
                (None, Some(_)) => 1,
                (None, None) => 0,
            };
            if best.is_none_or(|(best, _)| specificity > best) {
                best = Some((specificity, alias));
            }
        }

        match best {
            Some((_, alias)) => {
                let target = self.parse(&alias.to)?;
                Ok(Some(Parts {
                    region: target.written_region.or(target.region),
                    ..target
                }))
            }
            None => Ok(None),
        }
    }

    fn default_region(&self, parts: &Parts) -> Option<Region> {
        let with_script = parts
            .script
            .map(|script| format!("{}-{}", parts.language, script));

        DEFAULT_REGIONS
            .iter()
            .find(|(key, _)| {
                with_script.as_deref() == Some(*key) || *key == parts.language.as_str()
            })
            .and_then(|(_, region)| Region::try_from_str(region).ok())
            .or_else(|| {
                // Likely region of the language and script, ignoring the
                // region that was written.
                let mut id = LanguageIdentifier {
                    language: parts.language,
                    script: parts.script,
                    region: None,
                    variants: Default::default(),
                };
                self.expander.maximize(&mut id);
                id.region
            })
    }

    fn render(&self, parts: &Parts) -> String {
        let mut default = LanguageIdentifier {
            language: parts.language,
            script: None,
            region: None,
            variants: Default::default(),
        };
        self.expander.maximize(&mut default);

        let mut tag = parts.language.to_string();
        if let Some(script) = parts.script
            && default.script != Some(script)
        {
            tag.push('-');
            tag.push_str(script.as_str());
        }
        if let Some(region) = parts.region {
            tag.push('-');
            tag.push_str(region.as_str());
        }
        tag
    }
}

/// How one viewer reads a space.
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// Resolves what a viewer reads, given a space's policy and the viewer's
/// personal setting for languages the space does not declare.
///
/// `languages` and `llm_languages` must already be normalized.
pub fn resolve_viewer(
    normalizer: &Normalizer,
    locale: &str,
    policy: &RegionPolicy,
    languages: &[String],
    llm_languages: &[String],
    translate_outside_set: bool,
) -> Result<ViewerLanguage, LangError> {
    let effective = normalizer.normalize(locale, policy)?;
    let in_set = languages.contains(&effective);
    let llm = llm_languages.contains(&effective);
    let client_translation = match (llm, in_set) {
        (true, _) => false,
        // Declared but not translated by the LLM (free spaces).
        (false, true) => true,
        (false, false) => translate_outside_set,
    };

    Ok(ViewerLanguage {
        effective,
        in_set,
        llm,
        client_translation,
    })
}

/// English name of a normalized tag, for prompts: "Portuguese (Brazil)".
/// Falls back to the tag itself for languages not listed here.
pub fn english_name(tag: &str) -> String {
    const LANGUAGES: &[(&str, &str)] = &[
        ("ar", "Arabic"),
        ("de", "German"),
        ("en", "English"),
        ("es", "Spanish"),
        ("fr", "French"),
        ("hi", "Hindi"),
        ("id", "Indonesian"),
        ("it", "Italian"),
        ("ja", "Japanese"),
        ("ko", "Korean"),
        ("nl", "Dutch"),
        ("pl", "Polish"),
        ("pt", "Portuguese"),
        ("ru", "Russian"),
        ("sv", "Swedish"),
        ("th", "Thai"),
        ("tr", "Turkish"),
        ("uk", "Ukrainian"),
        ("vi", "Vietnamese"),
        ("zh", "Chinese"),
    ];
    const SCRIPTS: &[(&str, &str)] = &[
        ("Hans", "Simplified"),
        ("Hant", "Traditional"),
        ("Latn", "Latin"),
        ("Cyrl", "Cyrillic"),
    ];
    const REGIONS: &[(&str, &str)] = &[
        ("419", "Latin America"),
        ("AU", "Australia"),
        ("BR", "Brazil"),
        ("CA", "Canada"),
        ("CN", "China"),
        ("DE", "Germany"),
        ("ES", "Spain"),
        ("FR", "France"),
        ("GB", "United Kingdom"),
        ("HK", "Hong Kong"),
        ("IN", "India"),
        ("JP", "Japan"),
        ("KR", "South Korea"),
        ("MX", "Mexico"),
        ("PT", "Portugal"),
        ("TW", "Taiwan"),
        ("US", "United States"),
    ];

    let lookup = |table: &[(&str, &'static str)], key: &str| {
        table.iter().find(|(k, _)| *k == key).map(|(_, name)| *name)
    };

    let mut subtags = tag.split('-');
    let Some(language) = subtags
        .next()
        .and_then(|language| lookup(LANGUAGES, language))
    else {
        return tag.to_string();
    };

    let qualifiers = subtags
        .map(|subtag| {
            lookup(SCRIPTS, subtag)
                .or_else(|| lookup(REGIONS, subtag))
                .unwrap_or(subtag)
        })
        .collect::<Vec<_>>();

    if qualifiers.is_empty() {
        language.to_string()
    } else {
        format!("{} ({})", language, qualifiers.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn merged() -> RegionPolicy {
        RegionPolicy::default()
    }

    fn distinct() -> RegionPolicy {
        RegionPolicy {
            mode: RegionMode::Distinct,
            aliases: Vec::new(),
        }
    }

    #[test]
    fn merged_collapses_regions_to_the_representative() {
        let n = Normalizer::new();
        assert_eq!(n.normalize("en_GB", &merged()).unwrap(), "en-US");
        assert_eq!(n.normalize("EN-gb", &merged()).unwrap(), "en-US");
        assert_eq!(n.normalize("en", &merged()).unwrap(), "en-US");
        assert_eq!(n.normalize("pt-PT", &merged()).unwrap(), "pt-BR");
        assert_eq!(n.normalize("es-ES", &merged()).unwrap(), "es-419");
        assert_eq!(n.normalize("es-MX", &merged()).unwrap(), "es-419");
        assert_eq!(n.normalize("ja", &merged()).unwrap(), "ja-JP");
        assert_eq!(n.normalize("fr-CA", &merged()).unwrap(), "fr-FR");
    }

    #[test]
    fn scripts_never_collapse() {
        let n = Normalizer::new();
        assert_eq!(n.normalize("zh-TW", &merged()).unwrap(), "zh-Hant-TW");
        assert_eq!(n.normalize("zh-HK", &merged()).unwrap(), "zh-Hant-TW");
        assert_eq!(n.normalize("zh-CN", &merged()).unwrap(), "zh-CN");
        assert_eq!(n.normalize("zh-Hans-SG", &merged()).unwrap(), "zh-CN");
        assert_eq!(n.normalize("zh-Hant", &merged()).unwrap(), "zh-Hant-TW");
        assert_eq!(n.normalize("sr-Latn", &merged()).unwrap(), "sr-Latn-RS");
        assert_eq!(n.normalize("sr", &merged()).unwrap(), "sr-RS");
    }

    #[test]
    fn distinct_keeps_regions() {
        let n = Normalizer::new();
        assert_eq!(n.normalize("en_GB", &distinct()).unwrap(), "en-GB");
        assert_eq!(n.normalize("en", &distinct()).unwrap(), "en-US");
        assert_eq!(n.normalize("es-MX", &distinct()).unwrap(), "es-MX");
        assert_eq!(n.normalize("zh-HK", &distinct()).unwrap(), "zh-Hant-HK");
    }

    #[test]
    fn aliases_override_defaults() {
        let n = Normalizer::new();
        let policy = RegionPolicy {
            mode: RegionMode::Merged,
            aliases: vec![
                Alias {
                    from: "pt".into(),
                    to: "pt-PT".into(),
                },
                Alias {
                    from: "en-AU".into(),
                    to: "en-GB".into(),
                },
            ],
        };
        assert_eq!(n.normalize("pt-BR", &policy).unwrap(), "pt-PT");
        assert_eq!(n.normalize("pt", &policy).unwrap(), "pt-PT");
        // The more specific alias wins; other English still merges.
        assert_eq!(n.normalize("en-AU", &policy).unwrap(), "en-GB");
        assert_eq!(n.normalize("en-CA", &policy).unwrap(), "en-US");
    }

    #[test]
    fn invalid_tags_are_rejected() {
        let n = Normalizer::new();
        assert!(n.normalize("", &merged()).is_err());
        assert!(n.normalize("und", &merged()).is_err());
        assert!(n.normalize("not a tag", &merged()).is_err());
    }

    #[test]
    fn same_language_ignores_region_only() {
        let n = Normalizer::new();
        assert!(n.same_language("en-US", "en-GB"));
        assert!(n.same_language("pt", "pt-PT"));
        assert!(!n.same_language("zh-TW", "zh-CN"));
        assert!(!n.same_language("ja", "en"));
        assert_eq!(n.base("zh-HK").unwrap(), "zh-Hant");
        assert_eq!(n.base("en-GB").unwrap(), "en");
    }

    #[test]
    fn english_names() {
        assert_eq!(english_name("pt-BR"), "Portuguese (Brazil)");
        assert_eq!(english_name("zh-Hant-TW"), "Chinese (Traditional, Taiwan)");
        assert_eq!(english_name("es-419"), "Spanish (Latin America)");
        assert_eq!(english_name("ja"), "Japanese");
        assert_eq!(english_name("xx-YY"), "xx-YY");
    }
}

#[cfg(test)]
mod viewer_tests {
    use super::*;

    #[test]
    fn viewers_resolve_by_policy_and_setting() {
        let n = Normalizer::new();
        let policy = RegionPolicy::default();
        let languages = [
            "ja-JP".to_string(),
            "en-US".to_string(),
            "ko-KR".to_string(),
        ];
        let llm = ["ja-JP".to_string(), "en-US".to_string()];

        let british = resolve_viewer(&n, "en_GB", &policy, &languages, &llm, false).unwrap();
        assert_eq!(british.effective, "en-US");
        assert!(british.llm && !british.client_translation);

        let korean = resolve_viewer(&n, "ko", &policy, &languages, &llm, false).unwrap();
        assert!(korean.in_set && !korean.llm && korean.client_translation);

        let french = resolve_viewer(&n, "fr-FR", &policy, &languages, &llm, false).unwrap();
        assert!(!french.in_set && !french.client_translation);
        let french = resolve_viewer(&n, "fr-FR", &policy, &languages, &llm, true).unwrap();
        assert!(french.client_translation);
    }
}
