//! Messages on their way, as on Discord: from Enter until the server has
//! them, they stand greyed at the end of the channel, so Enter never waits
//! for files still uploading or for a slow network. A message waits for its
//! files, then is sent; a channel's messages go in the order they were
//! written. One that could not be sent stays, with why, to send again or
//! throw away.

use gpui::prelude::*;
use gpui::{AnyElement, Context, SharedString, div, px};
use gpui_md3e::prelude::*;

use super::{
    ChatView,
    messages::{GROUP_MS, format_time},
};
use crate::{
    i18n::fill,
    runtime::{self, describe},
    widgets,
};

/// Rows of messages on their way: `pending:<key>`, after the channel's.
const ROW_PREFIX: &str = "pending:";

/// A message written but not on the server yet.
pub(super) struct Pending {
    key: u64,
    channel_id: String,
    text: String,
    reply_to: Option<String>,
    written_at: i64,
    state: State,
}

enum State {
    /// For its files, or for the messages before it.
    Waiting,
    Sending,
    Failed(String),
}

/// The messages on their way, oldest first.
#[derive(Default)]
pub(super) struct Outbox {
    messages: Vec<Pending>,
    next_key: u64,
}

/// The key of a message on its way shown in `row`.
pub(super) fn pending_key(row: &str) -> Option<u64> {
    row.strip_prefix(ROW_PREFIX)?.parse().ok()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as i64)
}

impl ChatView {
    /// Puts a message, with the composer's files in `channel`, on its way.
    /// Returns false if there is nothing to send.
    pub(super) fn queue_message(
        &mut self,
        channel: String,
        text: String,
        reply_to: Option<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        let key = self.outbox.next_key;
        let files = self.take_uploads(&channel, key);
        if text.is_empty() && files == 0 {
            return false;
        }
        self.outbox.next_key += 1;
        self.outbox.messages.push(Pending {
            key,
            channel_id: channel,
            text,
            reply_to,
            written_at: now_ms(),
            state: State::Waiting,
        });
        self.sync_rows(false);
        self.list.scroll_to_end();
        self.send_pending(cx);
        true
    }

    /// Rows for the messages on their way in `channel`.
    pub(super) fn pending_rows(&self, channel: &str) -> impl Iterator<Item = String> + '_ {
        let channel = channel.to_string();
        self.outbox
            .messages
            .iter()
            .filter(move |pending| pending.channel_id == channel)
            .map(|pending| format!("{ROW_PREFIX}{}", pending.key))
    }

    /// Sends, in each channel, the oldest message waiting once its files
    /// are uploaded and the one before it is through.
    pub(super) fn send_pending(&mut self, cx: &mut Context<Self>) {
        let mut busy = Vec::new();
        let mut ready = Vec::new();
        let mut empty = Vec::new();
        for pending in &self.outbox.messages {
            let channel = &pending.channel_id;
            match pending.state {
                State::Failed(_) => continue,
                _ if busy.contains(channel) => continue,
                State::Sending => {}
                State::Waiting => match self.message_files(pending.key) {
                    // Its files all failed to upload, and it has no text.
                    Some(files) if files.is_empty() && pending.text.is_empty() => {
                        empty.push(pending.key);
                        continue;
                    }
                    Some(files) => ready.push((pending.key, files)),
                    None => {}
                },
            }
            busy.push(channel.clone());
        }
        if !empty.is_empty() {
            self.outbox
                .messages
                .retain(|pending| !empty.contains(&pending.key));
            self.sync_rows(false);
        }
        for (key, files) in ready {
            let Some(pending) = self.outbox.messages.iter_mut().find(|p| p.key == key) else {
                continue;
            };
            pending.state = State::Sending;
            let session = self.session.clone();
            let (channel, text, reply_to) = (
                pending.channel_id.clone(),
                pending.text.clone(),
                pending.reply_to.clone(),
            );
            let task = runtime::spawn(cx, async move {
                session
                    .send_with(&channel, &text, reply_to.as_deref(), &files)
                    .await
                    .map(drop)
            });
            cx.spawn(async move |this, cx| {
                let result = task.await;
                let _ = this.update(cx, |this, cx| {
                    match result {
                        // The message is the session's now.
                        Ok(()) => {
                            this.outbox.messages.retain(|pending| pending.key != key);
                            this.forget_uploads(key);
                            this.sync_rows(false);
                        }
                        Err(error) => {
                            let why = describe(&error, this.t());
                            if let Some(pending) =
                                this.outbox.messages.iter_mut().find(|p| p.key == key)
                            {
                                pending.state = State::Failed(why);
                            }
                        }
                    }
                    this.send_pending(cx);
                    cx.notify();
                });
            })
            .detach();
        }
        cx.notify();
    }

    fn retry_pending(&mut self, key: u64, cx: &mut Context<Self>) {
        if let Some(pending) = self.outbox.messages.iter_mut().find(|p| p.key == key) {
            pending.state = State::Waiting;
        }
        self.send_pending(cx);
    }

    fn discard_pending(&mut self, key: u64, cx: &mut Context<Self>) {
        self.outbox.messages.retain(|pending| pending.key != key);
        self.forget_uploads(key);
        self.sync_rows(false);
        cx.notify();
    }

    /// A message on its way: greyed, its files by name, and if it could not
    /// be sent, why, with Retry and Delete.
    pub(super) fn render_pending(
        &mut self,
        ix: usize,
        key: u64,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(pending) = self.outbox.messages.iter().find(|p| p.key == key) else {
            return div().h(px(1.0)).into_any_element();
        };
        let colors = cx.md().colors;
        let t = self.t();
        let (channel, text, reply_to, written_at) = (
            pending.channel_id.clone(),
            pending.text.clone(),
            pending.reply_to.clone(),
            pending.written_at,
        );
        let failed = match &pending.state {
            State::Failed(why) => Some(why.clone()),
            _ => None,
        };
        // Grouped like the user's own messages.
        let previous = ix.checked_sub(1).and_then(|prev| self.rows.get(prev));
        let grouped = reply_to.is_none()
            && previous.is_some_and(|prev| {
                pending_key(prev).is_some()
                    || self.session.message(&channel, prev).is_some_and(|prev| {
                        prev.message.author_id == self.session.user_id()
                            && written_at - prev.message.created_at < GROUP_MS
                    })
            });

        let mut content = div().flex_1().min_w_0().flex().flex_col();
        if let Some(reply_to) = &reply_to {
            content = content.child(self.reply_preview(&channel, reply_to, cx));
        }
        if !grouped {
            content = content.child(
                div()
                    .flex()
                    .flex_row()
                    .items_baseline()
                    .gap_2()
                    .child(
                        div()
                            .md_type_emphasized(TypeRole::TitleSmall, cx)
                            .text_color(colors.primary)
                            .child(self.session.user_name().to_string()),
                    )
                    .child(
                        div()
                            .md_type(TypeRole::LabelSmall, cx)
                            .text_color(colors.on_surface_variant)
                            .child(format_time(written_at, t)),
                    ),
            );
        }
        if !text.is_empty() {
            let parts = self.text_parts(&text, cx);
            content = content.child(
                div()
                    .md_type(TypeRole::BodyLarge, cx)
                    .text_color(colors.on_surface_variant.opacity(0.7))
                    .child(SelectableText::parts(
                        SharedString::from(format!("pending-text-{key}")),
                        parts,
                    )),
            );
        }
        let files = self.message_file_names(key);
        if !files.is_empty() {
            let mut row = div().flex().flex_row().flex_wrap().gap_2().pt_1();
            for (name, uploading) in files {
                let mut chip = div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .max_w(px(280.0))
                    .rounded(px(12.0))
                    .bg(colors.surface_container_high)
                    .text_color(colors.on_surface_variant)
                    .child(
                        icon(Icon::AttachFile)
                            .size(px(18.0))
                            .color(colors.on_surface_variant),
                    )
                    .child(
                        div()
                            .md_type(TypeRole::LabelLarge, cx)
                            .min_w_0()
                            .truncate()
                            .child(name),
                    );
                if uploading {
                    chip = chip.child(div().md_type(TypeRole::LabelSmall, cx).child(t.uploading));
                }
                row = row.child(chip);
            }
            content = content.child(row);
        }
        if let Some(why) = failed {
            content = content
                .child(
                    div()
                        .pt_1()
                        .md_type(TypeRole::BodySmall, cx)
                        .text_color(colors.error)
                        .child(fill(t.not_sent, &[&why])),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(
                            Button::new(
                                SharedString::from(format!("pending-retry-{key}")),
                                t.retry,
                            )
                            .style(ButtonStyle::Text)
                            .size(ButtonSize::XSmall)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.retry_pending(key, cx)),
                            ),
                        )
                        .child(
                            Button::new(
                                SharedString::from(format!("pending-delete-{key}")),
                                t.delete,
                            )
                            .style(ButtonStyle::Text)
                            .size(ButtonSize::XSmall)
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.discard_pending(key, cx)),
                            ),
                        ),
                );
        }

        div()
            .id(SharedString::from(format!("pending-{key}")))
            .w_full()
            .flex()
            .flex_row()
            .gap_3()
            .px_6()
            .pt(px(if grouped { 2.0 } else { 12.0 }))
            .pb(px(2.0))
            .child(if grouped {
                div().w(px(40.0)).flex_none().into_any_element()
            } else {
                widgets::avatar(self.session.user_name(), 40.0, cx).into_any_element()
            })
            .child(content)
            .into_any_element()
    }
}
