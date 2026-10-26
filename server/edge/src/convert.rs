//! The public boundary: internal types in, public types out.
//!
//! Public types are generated from types/edge.tyml, separately from the
//! internal ones, so a field added inside (a cell name, an epoch, an author's
//! locale) cannot leak by accident. Every crossing is written out here.

use babel_api_account::types::AccountError;
use babel_api_directory::types::DirectoryError;
use babel_api_edge::types as public;
use babel_api_message_cell::types as cell;
use babel_api_translation_cell::types as translation;

// ---- Errors ----------------------------------------------------------------

pub fn cell_error(error: cell::MessageCellError) -> public::ApiError {
    match error {
        // Indistinguishable on purpose: IDs cannot be probed.
        cell::MessageCellError::NotFound | cell::MessageCellError::Forbidden => {
            public::ApiError::NotFound
        }
        cell::MessageCellError::InvalidRequest => public::ApiError::InvalidRequest,
        cell::MessageCellError::Unavailable => public::ApiError::Unavailable,
    }
}

pub fn directory_error(error: DirectoryError) -> public::ApiError {
    match error {
        DirectoryError::NotFound => public::ApiError::NotFound,
        DirectoryError::NoCapacity | DirectoryError::Unavailable => public::ApiError::Unavailable,
    }
}

pub fn translation_error(error: translation::TranslationCellError) -> public::ApiError {
    match error {
        translation::TranslationCellError::NotFound => public::ApiError::NotFound,
        translation::TranslationCellError::InvalidLanguage => public::ApiError::InvalidLanguage,
        translation::TranslationCellError::LimitExceeded => public::ApiError::LimitExceeded,
        translation::TranslationCellError::InvalidRequest => public::ApiError::InvalidRequest,
        translation::TranslationCellError::Unavailable => public::ApiError::Unavailable,
    }
}

pub fn account_error(error: AccountError) -> public::ApiError {
    match error {
        AccountError::NameTaken => public::ApiError::NameTaken,
        AccountError::InvalidCredentials => public::ApiError::InvalidCredentials,
        AccountError::InvalidRequest => public::ApiError::InvalidRequest,
        AccountError::NotFound => public::ApiError::NotFound,
    }
}

// ---- Guilds ----------------------------------------------------------------

pub fn guild(guild: &cell::Guild) -> public::Guild {
    public::Guild {
        id: guild.id.clone(),
        name: guild.name.clone(),
        description: guild.description.clone(),
        owner_id: guild.owner_id.clone(),
        created_at: guild.created_at,
    }
}

pub fn channel_kind(kind: cell::ChannelKind) -> public::ChannelKind {
    match kind {
        cell::ChannelKind::Category => public::ChannelKind::Category,
        cell::ChannelKind::Text => public::ChannelKind::Text,
        cell::ChannelKind::Thread => public::ChannelKind::Thread,
    }
}

pub fn internal_channel_kind(kind: public::ChannelKind) -> cell::ChannelKind {
    match kind {
        public::ChannelKind::Category => cell::ChannelKind::Category,
        public::ChannelKind::Text => cell::ChannelKind::Text,
        public::ChannelKind::Thread => cell::ChannelKind::Thread,
    }
}

pub fn channel(channel: &cell::Channel) -> public::Channel {
    public::Channel {
        id: channel.id.clone(),
        guild_id: channel.guild_id.clone(),
        kind: channel_kind(channel.kind),
        name: channel.name.clone(),
        topic: channel.topic.clone(),
        parent_id: channel.parent_id.clone(),
        starter_message_id: channel.starter_message_id.clone(),
        created_at: channel.created_at,
    }
}

pub fn member(member: &cell::Member) -> public::Member {
    public::Member {
        user_id: member.user_id.clone(),
        name: member.name.clone(),
        owner: member.role == cell::Role::Owner,
        joined_at: member.joined_at,
    }
}

pub fn message(message: &cell::Message) -> public::Message {
    // `author_locale` stays inside.
    public::Message {
        id: message.id.clone(),
        guild_id: message.guild_id.clone(),
        channel_id: message.channel_id.clone(),
        author_id: message.author_id.clone(),
        author_name: message.author_name.clone(),
        text: message.text.clone(),
        rev: message.rev,
        created_at: message.created_at,
        edited_at: message.edited_at,
    }
}

// ---- Languages -------------------------------------------------------------

pub fn plan_name(plan: translation::Plan) -> String {
    match plan {
        translation::Plan::Free => "free",
        translation::Plan::Paid1 => "paid1",
        translation::Plan::Paid2 => "paid2",
        translation::Plan::Paid3 => "paid3",
    }
    .to_string()
}

pub fn region_mode(mode: translation::RegionMode) -> public::RegionMode {
    match mode {
        translation::RegionMode::Merged => public::RegionMode::Merged,
        translation::RegionMode::Distinct => public::RegionMode::Distinct,
    }
}

pub fn policy(policy: &translation::SpacePolicy) -> public::LanguagePolicy {
    public::LanguagePolicy {
        guild_id: policy.space.clone(),
        plan: plan_name(policy.plan),
        languages: policy.languages.clone(),
        llm_languages: policy.llm_languages.clone(),
        max_languages: policy.max_languages,
        max_llm_languages: policy.max_llm_languages,
        region_mode: region_mode(policy.region_mode),
        aliases: policy
            .aliases
            .iter()
            .map(|alias| public::LanguageAlias {
                from: alias.from.clone(),
                to: alias.to.clone(),
            })
            .collect(),
        translate_regional_variants: policy.translate_regional_variants,
    }
}

pub fn policy_update(update: public::LanguagePolicyUpdate) -> translation::PolicyUpdate {
    translation::PolicyUpdate {
        space: update.guild_id,
        languages: update.languages,
        llm_languages: update.llm_languages,
        region_mode: match update.region_mode {
            public::RegionMode::Merged => translation::RegionMode::Merged,
            public::RegionMode::Distinct => translation::RegionMode::Distinct,
        },
        aliases: update
            .aliases
            .into_iter()
            .map(|alias| translation::LanguageAlias {
                from: alias.from,
                to: alias.to,
            })
            .collect(),
        translate_regional_variants: update.translate_regional_variants,
    }
}

pub fn status_name(status: translation::TranslationStatus) -> String {
    match status {
        translation::TranslationStatus::Done => "done",
        translation::TranslationStatus::Pending => "pending",
        translation::TranslationStatus::BudgetExceeded => "budget_exceeded",
        translation::TranslationStatus::Failed => "failed",
    }
    .to_string()
}

/// A message's translations, keeping only the languages in `langs`.
pub fn translation(
    translation: &translation::MessageTranslation,
    langs: &[String],
) -> public::MessageTranslation {
    public::MessageTranslation {
        message_id: translation.message.id.clone(),
        channel_id: translation.message.conversation.clone(),
        rev: translation.rev.parse().unwrap_or(0),
        source_lang: translation.source_lang.clone(),
        status: status_name(translation.status),
        translations: translation
            .translations
            .iter()
            .filter(|translated| langs.contains(&translated.lang))
            .map(|translated| public::Translation {
                lang: translated.lang.clone(),
                text: translated.text.clone(),
            })
            .collect(),
    }
}
