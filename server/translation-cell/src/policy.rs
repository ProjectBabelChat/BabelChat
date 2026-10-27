//! Language policies: plans, limits and normalization of a space's languages.

use babel_api_translation_cell::types::{
    LanguageAlias, Plan, PolicyUpdate, RegionMode, SpacePolicy, TranslationCellError,
};
use babel_lang::{Alias, Normalizer, RegionPolicy};

/// (languages a space may declare, of which the LLM translates)
///
/// Free spaces translate on device only. Billing will set plans through
/// `set_plan`; until then the default plan comes from configuration.
pub fn plan_limits(plan: Plan) -> (i64, i64) {
    match plan {
        Plan::Free => (3, 0),
        Plan::Paid1 => (3, 2),
        Plan::Paid2 => (3, 3),
        Plan::Paid3 => (5, 5),
    }
}

pub fn default_policy(space: &str, plan: Plan) -> SpacePolicy {
    let (max_languages, max_llm_languages) = plan_limits(plan);
    SpacePolicy {
        space: space.to_string(),
        version: 0,
        plan,
        languages: Vec::new(),
        llm_languages: Vec::new(),
        max_languages,
        max_llm_languages,
        region_mode: RegionMode::Merged,
        aliases: Vec::new(),
        translate_regional_variants: false,
        model: None,
    }
}

pub fn region_policy(mode: RegionMode, aliases: &[LanguageAlias]) -> RegionPolicy {
    RegionPolicy {
        mode: match mode {
            RegionMode::Merged => babel_lang::RegionMode::Merged,
            RegionMode::Distinct => babel_lang::RegionMode::Distinct,
        },
        aliases: aliases
            .iter()
            .map(|alias| Alias {
                from: alias.from.clone(),
                to: alias.to.clone(),
            })
            .collect(),
    }
}

pub fn region_policy_of(policy: &SpacePolicy) -> RegionPolicy {
    region_policy(policy.region_mode, &policy.aliases)
}

/// Normalizes and deduplicates tags, keeping their order.
fn normalize_all(
    normalizer: &Normalizer,
    tags: &[String],
    region: &RegionPolicy,
) -> Result<Vec<String>, TranslationCellError> {
    let mut normalized = Vec::new();
    for tag in tags {
        let tag = normalizer
            .normalize(tag, region)
            .map_err(|_| TranslationCellError::InvalidLanguage)?;
        if !normalized.contains(&tag) {
            normalized.push(tag);
        }
    }
    Ok(normalized)
}

/// Applies an owner's update to a policy. Languages are counted after
/// normalization, so en-GB and en-US are one language in merged mode.
pub fn apply_update(
    normalizer: &Normalizer,
    current: &SpacePolicy,
    update: &PolicyUpdate,
) -> Result<SpacePolicy, TranslationCellError> {
    for alias in &update.aliases {
        for tag in [&alias.from, &alias.to] {
            normalizer
                .base(tag)
                .map_err(|_| TranslationCellError::InvalidLanguage)?;
        }
    }

    let region = region_policy(update.region_mode, &update.aliases);
    let languages = normalize_all(normalizer, &update.languages, &region)?;
    let llm_languages = normalize_all(normalizer, &update.llm_languages, &region)?;

    if llm_languages.iter().any(|tag| !languages.contains(tag)) {
        return Err(TranslationCellError::InvalidRequest);
    }
    if languages.len() as i64 > current.max_languages
        || llm_languages.len() as i64 > current.max_llm_languages
    {
        return Err(TranslationCellError::LimitExceeded);
    }

    Ok(SpacePolicy {
        version: current.version + 1,
        languages,
        llm_languages,
        region_mode: update.region_mode,
        aliases: update.aliases.clone(),
        translate_regional_variants: update.translate_regional_variants,
        model: update.model.clone(),
        ..current.clone()
    })
}

/// Changes the plan, trimming languages that no longer fit.
pub fn apply_plan(current: &SpacePolicy, plan: Plan) -> SpacePolicy {
    let (max_languages, max_llm_languages) = plan_limits(plan);
    let mut languages = current.languages.clone();
    languages.truncate(max_languages as usize);
    let mut llm_languages = current
        .llm_languages
        .iter()
        .filter(|tag| languages.contains(tag))
        .cloned()
        .collect::<Vec<_>>();
    llm_languages.truncate(max_llm_languages as usize);

    SpacePolicy {
        version: current.version + 1,
        plan,
        languages,
        llm_languages,
        max_languages,
        max_llm_languages,
        ..current.clone()
    }
}

/// Languages the LLM translates a message into: every LLM language except
/// the source itself (and, unless the space asks for it, regional variants
/// of the source).
pub fn targets(normalizer: &Normalizer, policy: &SpacePolicy, source_lang: &str) -> Vec<String> {
    policy
        .llm_languages
        .iter()
        .filter(|tag| tag.as_str() != source_lang)
        .filter(|tag| {
            policy.translate_regional_variants || !normalizer.same_language(tag, source_lang)
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(languages: &[&str], llm: &[&str]) -> PolicyUpdate {
        PolicyUpdate {
            space: "s".into(),
            languages: languages.iter().map(|tag| tag.to_string()).collect(),
            llm_languages: llm.iter().map(|tag| tag.to_string()).collect(),
            region_mode: RegionMode::Merged,
            aliases: Vec::new(),
            translate_regional_variants: false,
            model: None,
        }
    }

    #[test]
    fn languages_are_counted_after_normalization() {
        let n = Normalizer::new();
        let free = default_policy("s", Plan::Free);

        let policy =
            apply_update(&n, &free, &update(&["en_GB", "en-US", "ja", "ko"], &[])).unwrap();
        assert_eq!(policy.languages, ["en-US", "ja-JP", "ko-KR"]);

        assert_eq!(
            apply_update(&n, &free, &update(&["en", "ja", "ko", "fr"], &[])).unwrap_err(),
            TranslationCellError::LimitExceeded
        );
        assert_eq!(
            apply_update(&n, &free, &update(&["en"], &["en"])).unwrap_err(),
            TranslationCellError::LimitExceeded
        );
    }

    #[test]
    fn llm_languages_must_be_declared() {
        let n = Normalizer::new();
        let paid = default_policy("s", Plan::Paid3);
        assert_eq!(
            apply_update(&n, &paid, &update(&["en"], &["ja"])).unwrap_err(),
            TranslationCellError::InvalidRequest
        );
    }

    #[test]
    fn downgrades_trim_languages() {
        let n = Normalizer::new();
        let paid = default_policy("s", Plan::Paid3);
        let policy = apply_update(
            &n,
            &paid,
            &update(
                &["ja", "en", "ko", "fr", "de"],
                &["ja", "en", "ko", "fr", "de"],
            ),
        )
        .unwrap();

        let free = apply_plan(&policy, Plan::Free);
        assert_eq!(free.languages, ["ja-JP", "en-US", "ko-KR"]);
        assert!(free.llm_languages.is_empty());
    }

    #[test]
    fn targets_skip_the_source_language() {
        let n = Normalizer::new();
        let paid = default_policy("s", Plan::Paid3);
        let mut policy =
            apply_update(&n, &paid, &update(&["ja", "en", "ko"], &["ja", "en", "ko"])).unwrap();
        assert_eq!(targets(&n, &policy, "ja-JP"), ["en-US", "ko-KR"]);
        assert_eq!(targets(&n, &policy, "fr-FR"), ["ja-JP", "en-US", "ko-KR"]);

        policy.region_mode = RegionMode::Distinct;
        policy.llm_languages = vec!["en-GB".into(), "ja-JP".into()];
        assert_eq!(targets(&n, &policy, "en-US"), ["ja-JP"]);
        policy.translate_regional_variants = true;
        assert_eq!(targets(&n, &policy, "en-US"), ["en-GB", "ja-JP"]);
    }
}
