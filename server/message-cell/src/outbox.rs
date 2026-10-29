//! Forwards committed guild events to the services that mirror them.
//!
//! Each write that other services must learn about puts an outbox entry in
//! the same batch as the log entry, so nothing is lost if the process dies
//! right after committing. The worker delivers entries in commit order and
//! deletes them once delivered: at-least-once delivery, made safe by
//! idempotent receivers (ingest is keyed by message and revision).
//!
//! ```text
//! guild_created,
//! guild_updated    ─▶ Translation Cell put_space
//! guild_deleted    ─▶ Translation Cell delete_space, every Media service delete_guild
//! channel_created,
//! channel_updated  ─▶ Translation Cell put_node
//! channel_deleted  ─▶ Translation Cell delete_node, every Media service delete_channel
//! member_joined    ─▶ Directory add_membership
//! member_left,
//! member_removed   ─▶ Directory remove_membership
//! message_created  ─▶ Translation Cell ingest (unless it has no text),
//!                     Media attach_blobs (its attachments)
//! message_edited   ─▶ Translation Cell ingest (new revision)
//! message_deleted  ─▶ Translation Cell delete_message, Media delete_blobs
//! ```

use std::{collections::HashMap, sync::Arc, time::Duration};

use babel_api_directory::types::DirectoryError;
use babel_api_media::types::{BlobRef, BlobRefs, Media, MediaError};
use babel_api_message_cell::types::{ChannelKind, GuildEvent, GuildEventKind};
use babel_api_translation_cell::types::{
    ContextNode, IngestMessage, MessageRef, NodeKind, SpaceContext, TranslationCellError,
};
use babel_store::{Batch, Scan};

use crate::{
    Inner,
    records::{GUILDS, GuildRecord, LOG, OUTBOX, OutboxEntry, guild_key, log_key},
};

const BATCH: usize = 64;
const RETRY: Duration = Duration::from_secs(1);

enum Delivery {
    Done,
    /// The receiver is unavailable; keep the entry and try again later.
    Retry,
}

pub(crate) async fn run(inner: Arc<Inner>) {
    loop {
        let entries = match inner
            .store
            .scan_values::<OutboxEntry>(Scan::prefix(OUTBOX, &[]).limit(BATCH))
        {
            Ok(entries) => entries,
            Err(error) => {
                tracing::error!(%error, "reading the outbox failed");
                tokio::time::sleep(RETRY).await;
                continue;
            }
        };

        if entries.is_empty() {
            let _ = tokio::time::timeout(RETRY, inner.outbox_signal.notified()).await;
            continue;
        }

        for (key, entry) in entries {
            match deliver(&inner, &entry).await {
                Delivery::Done => {
                    metrics::counter!("babel_outbox_delivered_total").increment(1);
                    let mut batch = Batch::new();
                    batch.delete(OUTBOX, key);
                    if let Err(error) = inner.store.write(batch) {
                        tracing::error!(%error, "acknowledging an outbox entry failed");
                    }
                }
                Delivery::Retry => {
                    metrics::counter!("babel_outbox_retries_total").increment(1);
                    // Keep commit order: stop here and retry from this entry.
                    tokio::time::sleep(RETRY).await;
                    break;
                }
            }
        }
    }
}

fn outcome(result: Result<(), TranslationCellError>, entry: &OutboxEntry) -> Delivery {
    match result {
        Ok(()) => Delivery::Done,
        Err(TranslationCellError::Unavailable) => Delivery::Retry,
        Err(error) => {
            // A permanent rejection would block the outbox forever.
            tracing::warn!(?error, ?entry, "dropping an outbox entry");
            Delivery::Done
        }
    }
}

fn directory_outcome(result: Result<(), DirectoryError>, entry: &OutboxEntry) -> Delivery {
    match result {
        Ok(()) => Delivery::Done,
        Err(DirectoryError::Unavailable) => Delivery::Retry,
        Err(error) => {
            tracing::warn!(?error, ?entry, "dropping an outbox entry");
            Delivery::Done
        }
    }
}

fn media_outcome(result: Result<(), MediaError>, entry: &OutboxEntry) -> Delivery {
    match result {
        Ok(()) => Delivery::Done,
        Err(MediaError::Unavailable) => Delivery::Retry,
        Err(error) => {
            tracing::warn!(?error, ?entry, "dropping an outbox entry");
            Delivery::Done
        }
    }
}

/// Goes on with `next` once `first` is done: an entry is retried whole,
/// which receivers allow.
async fn then(first: Delivery, next: impl Future<Output = Delivery>) -> Delivery {
    match first {
        Delivery::Done => next.await,
        Delivery::Retry => Delivery::Retry,
    }
}

/// Calls every Media service: a channel's or a guild's files may be on
/// any of them.
async fn every_media<F, Fut>(inner: &Inner, entry: &OutboxEntry, call: F) -> Delivery
where
    F: Fn(Arc<dyn Media>) -> Fut,
    Fut: Future<Output = Result<(), MediaError>>,
{
    for media in inner.media.values() {
        if let Delivery::Retry = media_outcome(call(media.clone()).await, entry) {
            return Delivery::Retry;
        }
    }
    Delivery::Done
}

/// Attaches (`attach`) or deletes a message's files, on the Media services
/// their IDs name.
async fn message_files(
    inner: &Inner,
    entry: &OutboxEntry,
    guild_id: &str,
    channel_id: &str,
    ids: Vec<String>,
    attach: bool,
) -> Delivery {
    let mut by_media = HashMap::<&str, Vec<BlobRef>>::new();
    for id in &ids {
        let name = id.split_once('.').map_or("", |(name, _)| name);
        by_media.entry(name).or_default().push(BlobRef {
            guild_id: guild_id.to_string(),
            channel_id: Some(channel_id.to_string()),
            id: id.clone(),
        });
    }
    for (name, blobs) in by_media {
        let Some(media) = inner.media.get(name) else {
            tracing::warn!(media = name, "attachment on an unknown media service");
            continue;
        };
        let refs = BlobRefs { blobs };
        let result = if attach {
            media.attach_blobs(refs).await
        } else {
            media.delete_blobs(refs).await
        };
        if let Delivery::Retry = media_outcome(result, entry) {
            return Delivery::Retry;
        }
    }
    Delivery::Done
}

async fn deliver(inner: &Inner, entry: &OutboxEntry) -> Delivery {
    let (event, record) = match (
        inner
            .store
            .get_value::<GuildEvent>(LOG, &log_key(&entry.guild, entry.seq)),
        inner
            .store
            .get_value::<GuildRecord>(GUILDS, &guild_key(&entry.guild)),
    ) {
        (Ok(Some(event)), Ok(Some(record))) => (event, record),
        (Ok(_), Ok(_)) => return Delivery::Done,
        _ => return Delivery::Retry,
    };

    let Some(translation_cell) = inner.translation_cells.get(&record.translation_cell) else {
        tracing::error!(cell = %record.translation_cell, "unknown translation cell");
        return Delivery::Retry;
    };
    let guild_id = record.guild.id.clone();

    match event.kind {
        GuildEventKind::GuildCreated | GuildEventKind::GuildUpdated => {
            let Some(guild) = event.guild else {
                return Delivery::Done;
            };
            outcome(
                translation_cell
                    .put_space(SpaceContext {
                        space: guild.id,
                        name: guild.name,
                        description: guild.description,
                        description_lang: None,
                        glossary: Vec::new(),
                    })
                    .await,
                entry,
            )
        }
        GuildEventKind::GuildDeleted => {
            let deleted = outcome(translation_cell.delete_space(guild_id.clone()).await, entry);
            then(
                deleted,
                every_media(inner, entry, |media| {
                    let guild_id = guild_id.clone();
                    async move { media.delete_guild(guild_id).await }
                }),
            )
            .await
        }
        GuildEventKind::ChannelCreated | GuildEventKind::ChannelUpdated => {
            let Some(channel) = event.channel else {
                return Delivery::Done;
            };
            let starter = match (&channel.parent_id, &channel.starter_message_id) {
                (Some(parent), Some(starter)) => Some(MessageRef {
                    space: guild_id.clone(),
                    conversation: parent.clone(),
                    id: starter.clone(),
                }),
                _ => None,
            };
            outcome(
                translation_cell
                    .put_node(ContextNode {
                        space: guild_id,
                        id: channel.id,
                        parent: channel.parent_id,
                        kind: match channel.kind {
                            ChannelKind::Category => NodeKind::Category,
                            ChannelKind::Text => NodeKind::Channel,
                            ChannelKind::Thread => NodeKind::Thread,
                        },
                        title: channel.name,
                        description: channel.topic,
                        starter,
                    })
                    .await,
                entry,
            )
        }
        GuildEventKind::ChannelDeleted => {
            let Some(channel) = event.channel else {
                return Delivery::Done;
            };
            let deleted = outcome(
                translation_cell
                    .delete_node(guild_id.clone(), channel.id.clone())
                    .await,
                entry,
            );
            then(
                deleted,
                every_media(inner, entry, |media| {
                    let (guild_id, channel_id) = (guild_id.clone(), channel.id.clone());
                    async move { media.delete_channel(guild_id, channel_id).await }
                }),
            )
            .await
        }
        GuildEventKind::MemberJoined => {
            let (Some(member), Some(directory)) = (event.member, inner.directory.get()) else {
                return Delivery::Retry;
            };
            directory_outcome(
                directory.add_membership(member.user_id, guild_id).await,
                entry,
            )
        }
        GuildEventKind::MemberLeft | GuildEventKind::MemberRemoved => {
            let (Some(member), Some(directory)) = (event.member, inner.directory.get()) else {
                return Delivery::Retry;
            };
            directory_outcome(
                directory.remove_membership(member.user_id, guild_id).await,
                entry,
            )
        }
        GuildEventKind::MessageCreated | GuildEventKind::MessageEdited => {
            let Some(message) = event.message else {
                return Delivery::Done;
            };
            if event.kind == GuildEventKind::MessageCreated
                && let Some(attachments) = &message.attachments
            {
                let ids = attachments.iter().map(|file| file.id.clone()).collect();
                let attached =
                    message_files(inner, entry, &guild_id, &message.channel_id, ids, true).await;
                if let Delivery::Retry = attached {
                    return Delivery::Retry;
                }
            }
            // Files alone: nothing to translate.
            if message.text.trim().is_empty() {
                return Delivery::Done;
            }
            outcome(
                translation_cell
                    .ingest(IngestMessage {
                        reply_to: message.reply_to.map(|id| MessageRef {
                            space: guild_id.clone(),
                            conversation: message.channel_id.clone(),
                            id,
                        }),
                        message: MessageRef {
                            space: guild_id,
                            conversation: message.channel_id,
                            id: message.id,
                        },
                        rev: message.rev.to_string(),
                        author: message.author_name,
                        author_locale: message.author_locale,
                        text: message.text,
                        sent_at: message.created_at,
                    })
                    .await,
                entry,
            )
        }
        GuildEventKind::MessageDeleted => {
            let Some(deleted) = event.deleted else {
                return Delivery::Done;
            };
            let forgotten = outcome(
                translation_cell
                    .delete_message(MessageRef {
                        space: guild_id.clone(),
                        conversation: deleted.channel_id.clone(),
                        id: deleted.message_id,
                    })
                    .await,
                entry,
            );
            let ids = deleted.attachments.unwrap_or_default();
            then(
                forgotten,
                message_files(inner, entry, &guild_id, &deleted.channel_id, ids, false),
            )
            .await
        }
        // Only Edges care about these.
        GuildEventKind::ReactionAdded
        | GuildEventKind::ReactionRemoved
        | GuildEventKind::Typing
        | GuildEventKind::ReadStateUpdated
        | GuildEventKind::NotificationsUpdated => Delivery::Done,
    }
}
