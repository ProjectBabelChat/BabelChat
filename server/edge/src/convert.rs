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
        // Edges follow a moved guild; this is what is left if it keeps moving.
        cell::MessageCellError::Moved => public::ApiError::Unavailable,
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
        AccountError::Unavailable => public::ApiError::Unavailable,
        AccountError::SignupsClosed => public::ApiError::SignupsClosed,
        AccountError::InvalidSignupCode => public::ApiError::InvalidSignupCode,
    }
}

pub fn token(token: babel_api_account::types::Token) -> public::Token {
    public::Token {
        jwt: token.jwt,
        user_id: token.user_id,
        name: token.name,
        expires_at: token.expires_at,
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
        last_message_id: channel.last_message_id.clone(),
        created_by: channel.created_by.clone(),
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

/// Invite codes carry the guild ID so any Edge can route them, and a secret
/// only the guild's cell can check. The guild ID gives nothing away: knowing
/// it does not let anyone see or join the guild.
pub fn invite(invite: &cell::Invite) -> public::Invite {
    public::Invite {
        code: format!("{}-{}", invite.guild_id, invite.secret),
        guild_id: invite.guild_id.clone(),
        created_by: invite.created_by.clone(),
        created_at: invite.created_at,
        expires_at: invite.expires_at,
        max_uses: invite.max_uses,
        uses: invite.uses,
    }
}

/// Splits an invite code into guild ID and secret.
pub fn parse_invite(code: &str) -> Option<(&str, &str)> {
    let (guild, secret) = code.trim().split_once('-')?;
    let hex = |part: &str| {
        !part.is_empty() && part.len() <= 64 && part.bytes().all(|b| b.is_ascii_hexdigit())
    };
    (hex(guild) && hex(secret)).then_some((guild, secret))
}

/// A message as `reader` sees it: whether they reacted, not who else did.
pub fn message(message: &cell::Message, reader: &str) -> public::Message {
    // `author_locale` stays inside.
    public::Message {
        id: message.id.clone(),
        guild_id: message.guild_id.clone(),
        channel_id: message.channel_id.clone(),
        author_id: message.author_id.clone(),
        author_name: message.author_name.clone(),
        text: message.text.clone(),
        reply_to: message.reply_to.clone(),
        mentions: message.mentions.clone().unwrap_or_default(),
        rev: message.rev,
        created_at: message.created_at,
        edited_at: message.edited_at,
        reactions: message
            .reactions
            .iter()
            .flatten()
            .map(|reaction| public::Reaction {
                emoji: reaction.emoji.clone(),
                count: reaction.user_ids.len() as i64,
                me: reaction.user_ids.iter().any(|user| user == reader),
            })
            .collect(),
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
        model: policy.model.clone(),
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
        model: update.model,
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

// ---- Reading and notifications ---------------------------------------------

pub fn read_state(state: &cell::ReadState) -> public::ReadState {
    public::ReadState {
        channel_id: state.channel_id.clone(),
        last_read_id: state.last_read_id.clone(),
        mention_count: state.mention_count,
    }
}

fn notify_level(level: cell::NotifyLevel) -> public::NotifyLevel {
    match level {
        cell::NotifyLevel::All => public::NotifyLevel::All,
        cell::NotifyLevel::Mentions => public::NotifyLevel::Mentions,
        cell::NotifyLevel::None => public::NotifyLevel::None,
    }
}

fn internal_notify_level(level: public::NotifyLevel) -> cell::NotifyLevel {
    match level {
        public::NotifyLevel::All => cell::NotifyLevel::All,
        public::NotifyLevel::Mentions => cell::NotifyLevel::Mentions,
        public::NotifyLevel::None => cell::NotifyLevel::None,
    }
}

pub fn notifications(settings: &cell::NotificationSettings) -> public::NotificationSettings {
    public::NotificationSettings {
        guild_id: settings.guild_id.clone(),
        level: notify_level(settings.level),
        channels: settings
            .channels
            .iter()
            .map(|entry| public::ChannelNotify {
                channel_id: entry.channel_id.clone(),
                level: notify_level(entry.level),
            })
            .collect(),
    }
}

pub fn internal_notifications(
    settings: public::NotificationSettings,
) -> cell::NotificationSettings {
    cell::NotificationSettings {
        guild_id: settings.guild_id,
        level: internal_notify_level(settings.level),
        channels: settings
            .channels
            .into_iter()
            .map(|entry| cell::ChannelNotify {
                channel_id: entry.channel_id,
                level: internal_notify_level(entry.level),
            })
            .collect(),
    }
}

pub fn ban(ban: &cell::Ban) -> public::Ban {
    public::Ban {
        user_id: ban.user_id.clone(),
        name: ban.name.clone(),
        banned_by: ban.banned_by.clone(),
        banned_at: ban.banned_at,
    }
}
