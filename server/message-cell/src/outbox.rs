//! Forwards committed guild events to the services that mirror them.
//!
//! Each write that other services must learn about puts an outbox entry in
//! the same batch as the log entry, so nothing is lost if the process dies
//! right after committing. The worker delivers entries in commit order and
//! deletes them once delivered: at-least-once delivery, made safe by
//! idempotent receivers (ingest is keyed by message and revision).
//!
//! ```text
//! guild_created    ─▶ Translation Cell put_space
//! channel_created  ─▶ Translation Cell put_node
//! member_joined    ─▶ Directory add_membership
//! member_left,
//! member_removed   ─▶ Directory remove_membership
//! message_created  ─▶ Translation Cell ingest
//! message_edited   ─▶ Translation Cell ingest (new revision)
//! message_deleted  ─▶ Translation Cell delete_message
//! ```

use std::{sync::Arc, time::Duration};

use babel_api_directory::types::DirectoryError;
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
                    let mut batch = Batch::new();
                    batch.delete(OUTBOX, key);
                    if let Err(error) = inner.store.write(batch) {
                        tracing::error!(%error, "acknowledging an outbox entry failed");
                    }
                }
                Delivery::Retry => {
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
        GuildEventKind::GuildCreated => {
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
        GuildEventKind::ChannelCreated => {
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
            outcome(
                translation_cell
                    .delete_message(MessageRef {
                        space: guild_id,
                        conversation: deleted.channel_id,
                        id: deleted.message_id,
                    })
                    .await,
                entry,
            )
        }
        // Only Edges care about one member's markers and settings.
        GuildEventKind::ReadStateUpdated | GuildEventKind::NotificationsUpdated => Delivery::Done,
    }
}
