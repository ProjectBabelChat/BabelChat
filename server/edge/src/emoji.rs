//! A guild's own emoji in what people write, wherever they write it.
//!
//! Text and reactions name a guild's emoji by code (`<:name:guild.id>`,
//! see `babel_emoji`). Whoever is a member of the emoji's guild may use it,
//! in that guild or any other, so the Edge checks each code against the
//! guild that owns it (whose cell may not be the one written to): codes
//! the writer may not use become `:name:` in text and are refused in
//! reactions. A guild's emoji are remembered for a minute, and forgotten
//! sooner when the Edge sees them change.

use std::{collections::HashMap, time::Instant};

use babel_api_edge::types::ApiError;
use babel_api_message_cell::types as cell;

use crate::{ApiResult, Inner};

/// How long a guild's emoji are used without asking its cell again.
const EMOJI_TTL: std::time::Duration = std::time::Duration::from_secs(60);
/// Guilds whose emoji one text may use; codes of any more become names.
const MAX_GUILDS: usize = 10;

impl Inner {
    /// A guild's emoji, if the user may use them (is a member).
    async fn usable_emoji(&self, user: &str, guild: &str) -> ApiResult<Vec<cell::Emoji>> {
        let known = self
            .emoji
            .lock()
            .get(guild)
            .filter(|(_, at)| at.elapsed() < EMOJI_TTL)
            .map(|(emoji, _)| emoji.clone());
        if let Some(emoji) = known {
            self.may_view(user, guild).await?;
            return Ok(emoji);
        }
        // Members only: it says whether the user may see the guild too.
        let emoji = on_cell!(self, guild, |cell| cell
            .get_emoji(guild.to_string(), user.to_string()))?;
        self.viewed(user, guild);
        let mut cache = self.emoji.lock();
        if cache.len() > 10_000 {
            cache.retain(|_, (_, at)| at.elapsed() < EMOJI_TTL);
        }
        cache.insert(guild.to_string(), (emoji.clone(), Instant::now()));
        Ok(emoji)
    }

    /// `text` with each emoji code the user may use written as its guild
    /// has it, and the rest (guilds the user is not in, emoji deleted)
    /// as `:name:`.
    pub(crate) async fn checked_text(&self, user: &str, text: &str) -> String {
        let found = babel_emoji::find(text);
        if found.is_empty() {
            return text.to_string();
        }
        let mut guilds = HashMap::<&str, Vec<cell::Emoji>>::new();
        for (_, emoji) in &found {
            if guilds.contains_key(emoji.guild_id) || guilds.len() >= MAX_GUILDS {
                continue;
            }
            // A guild that cannot be asked now counts as one without emoji:
            // the message goes, with names.
            let usable = self
                .usable_emoji(user, emoji.guild_id)
                .await
                .unwrap_or_default();
            guilds.insert(emoji.guild_id, usable);
        }
        let mut checked = String::with_capacity(text.len());
        let mut from = 0;
        for (range, emoji) in found {
            checked.push_str(&text[from..range.start]);
            let known = guilds
                .get(emoji.guild_id)
                .and_then(|usable| usable.iter().find(|known| known.id == emoji.id));
            match known {
                Some(known) => {
                    checked.push_str(&babel_emoji::code(&known.name, &known.guild_id, &known.id))
                }
                None => checked.push_str(&format!(":{}:", emoji.name)),
            }
            from = range.end;
        }
        checked.push_str(&text[from..]);
        checked
    }

    /// A reaction as the cell takes it: a Unicode emoji as it is, a guild's
    /// emoji only if the user may use it (`invalid_request` otherwise).
    pub(crate) async fn reaction_emoji(&self, user: &str, emoji: String) -> ApiResult<String> {
        let Some(custom) = babel_emoji::parse(&emoji) else {
            return Ok(emoji);
        };
        let usable = match self.usable_emoji(user, custom.guild_id).await {
            Ok(usable) => usable,
            Err(ApiError::NotFound) => return Err(ApiError::InvalidRequest),
            Err(error) => return Err(error),
        };
        usable
            .iter()
            .find(|known| known.id == custom.id)
            .map(|known| babel_emoji::code(&known.name, &known.guild_id, &known.id))
            .ok_or(ApiError::InvalidRequest)
    }
}
