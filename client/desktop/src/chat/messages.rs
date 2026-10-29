//! The open channel: its header, the message list and the composer.

use babel_sdk::{ChannelKind, ChatMessage, GuildView, Shown};
use chrono::{DateTime, Local, TimeZone as _};
use gpui::prelude::*;
use gpui::{AnyElement, Context, ExternalPaths, IntoElement, SharedString, Window, div, list, px};
use gpui_md3e::prelude::*;

use super::{ChatView, Compose, Dialog};
use crate::{
    i18n::{Strings, fill, language_name},
    widgets,
};

/// Messages from one author this close together share a header.
const GROUP_MS: i64 = 5 * 60 * 1000;

impl ChatView {
    pub(super) fn render_channel(
        &mut self,
        guild: Option<&GuildView>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.md().colors;
        let t = self.t();
        let channel = self
            .channel
            .as_deref()
            .and_then(|id| self.session.channel(id));
        let Some(channel) = channel else {
            let text = if guild.is_some() {
                t.no_channels
            } else {
                t.no_guilds_body
            };
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(widgets::supporting(text, cx))
                .into_any_element();
        };

        let mut header = div()
            .flex_none()
            .h(px(64.0))
            .px_6()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .border_b_1()
            .border_color(colors.outline_variant)
            .child(icon(Icon::Tag).color(colors.on_surface_variant))
            .child(
                div()
                    .md_type(TypeRole::TitleLarge, cx)
                    .flex_none()
                    .child(channel.name.clone()),
            );
        header = header.child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .md_type(TypeRole::BodyMedium, cx)
                .text_color(colors.on_surface_variant)
                .children(channel.topic.clone()),
        );
        if self.session.may_manage(&channel.id) {
            let channel_id = channel.id.clone();
            let label = if channel.kind == ChannelKind::Thread {
                t.edit_thread
            } else {
                t.edit_channel
            };
            header = header.child(
                IconButton::new("channel-settings", Icon::Settings)
                    .label(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.dialog = Dialog::edit_channel(this, &channel_id, window, cx);
                        cx.notify();
                    })),
            );
        }
        header = header
            .child(
                IconButton::new("channel-notifications", Icon::Notifications)
                    .label(t.notifications)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.dialog = Some(Dialog::notifications(this, window, cx));
                        cx.notify();
                    })),
            )
            .child(
                IconButton::new("channel-invite", Icon::PersonAdd)
                    .label(t.invite_people)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.dialog = Some(Dialog::invite(this, window, cx));
                        cx.notify();
                    })),
            )
            .child(
                IconButton::new("toggle-members", Icon::Group)
                    .label(t.members)
                    .selected(self.members_open)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.members_open = !this.members_open;
                        cx.notify();
                    })),
            );

        let messages = list(
            self.list.clone(),
            cx.processor(|this, ix, window, cx| this.render_message(ix, window, cx)),
        )
        .flex_1()
        .size_full();

        let loading = self.history_loading.contains(&channel.id);
        let mut body = div().flex_1().min_h_0().relative().flex().flex_col();
        if self.rows.is_empty() {
            let text = if loading {
                t.loading.to_string()
            } else {
                fill(t.channel_start, &[&channel.name])
            };
            body = body.child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .justify_end()
                    .p_6()
                    .child(widgets::supporting(text, cx)),
            );
        } else {
            body = body.child(messages);
        }

        // Files dropped anywhere on the channel are attached.
        let dropping = colors.primary_container.opacity(0.35);
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .drag_over::<ExternalPaths>(move |style, _, _, _| style.bg(dropping))
            .on_drop(
                cx.listener(|this, paths: &ExternalPaths, _, cx| this.on_drop_files(paths, cx)),
            )
            .child(header)
            .child(body)
            .child(self.render_composer(cx))
            .into_any_element()
    }

    fn render_composer(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.md().colors;
        let t = self.t();
        let mut column = div().flex_none().flex().flex_col().px_4().pb_4().pt_2();

        let banner = match &self.compose {
            Compose::Message => None,
            Compose::Reply(id) => {
                let author = self
                    .channel
                    .as_deref()
                    .and_then(|channel| self.session.message(channel, id))
                    .map(|chat| chat.message.author_name)
                    .unwrap_or_default();
                Some((Icon::Reply, fill(t.replying_to, &[&author])))
            }
            Compose::Edit(_) => Some((Icon::Edit, t.editing.to_string())),
        };
        if let Some((glyph, text)) = banner {
            column = column.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_4()
                    .py_1()
                    .mb_1()
                    .rounded(px(12.0))
                    .bg(colors.secondary_container)
                    .text_color(colors.on_secondary_container)
                    .md_type(TypeRole::LabelMedium, cx)
                    .child(
                        icon(glyph)
                            .size(px(16.0))
                            .color(colors.on_secondary_container),
                    )
                    .child(div().flex_1().child(text))
                    .child(t.esc_to_cancel)
                    .child(
                        IconButton::new("cancel-compose", Icon::Close)
                            .size(ButtonSize::XSmall)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.cancel(&super::Cancel, window, cx)
                            })),
                    ),
            );
        }

        if let Some(uploads) = self.render_uploads(cx) {
            column = column.child(uploads);
        }

        let composer = self.composer.clone();
        // A line kept for it, so the composer does not jump.
        let typing = div()
            .h(px(18.0))
            .px_4()
            .pt_1()
            .md_type(TypeRole::LabelSmall, cx)
            .text_color(colors.on_surface_variant)
            .truncate()
            .children(self.typing_text());
        column
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap_3()
                    .child(
                        div().pb(px(8.0)).child(
                            IconButton::new("attach", Icon::AttachFile)
                                .size(ButtonSize::Medium)
                                .on_click(cx.listener(|this, _, _, cx| this.pick_files(cx))),
                        ),
                    )
                    .child(div().flex_1().min_w_0().child(self.composer.clone()))
                    // Level with the composer's last line as it grows.
                    .child(
                        div().pb(px(8.0)).child(
                            IconButton::new("send", Icon::Send)
                                .style(IconButtonStyle::Filled)
                                .size(ButtonSize::Medium)
                                .width(IconButtonWidth::Wide)
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    let text = composer.read(cx).text().clone();
                                    if this.submit(&text, window, cx) {
                                        composer
                                            .update(cx, |field, cx| field.set_text("", window, cx));
                                    }
                                })),
                        ),
                    ),
            )
            .child(typing)
            .into_any_element()
    }

    pub(super) fn render_message(
        &mut self,
        ix: usize,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.md().colors;
        let t = self.t();
        let (Some(channel), Some(id)) = (self.channel.clone(), self.rows.get(ix).cloned()) else {
            return div().into_any_element();
        };
        let Some(chat) = self.session.message(&channel, &id) else {
            return div().h(px(1.0)).into_any_element();
        };
        let me = self.session.user_id().to_string();
        let message = &chat.message;
        let mine = message.author_id == me;
        let previous = ix
            .checked_sub(1)
            .and_then(|prev| self.rows.get(prev))
            .and_then(|prev| self.session.message(&channel, prev));
        let grouped = message.reply_to.is_none()
            && previous.as_ref().is_some_and(|prev| {
                prev.message.author_id == message.author_id
                    && message.created_at - prev.message.created_at < GROUP_MS
            });
        let mentioned = message.mentions.contains(&me)
            || message
                .reply_to
                .as_deref()
                .and_then(|reply| self.session.message(&channel, reply))
                .is_some_and(|replied| replied.message.author_id == me && !mine);
        let hovered = self.hovered_message.as_deref() == Some(&id);
        let show_original = self.originals.contains(&id);
        let settings = self.session.settings();

        let mut content = div().flex_1().min_w_0().flex().flex_col();
        if let Some(reply_to) = &message.reply_to {
            let (author, text) = match self.session.message(&channel, reply_to) {
                Some(replied) => (replied.message.author_name, replied.display.text),
                None => (String::new(), t.deleted_message.to_string()),
            };
            content = content.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .md_type(TypeRole::BodySmall, cx)
                    .text_color(colors.on_surface_variant)
                    .child(
                        icon(Icon::Reply)
                            .size(px(14.0))
                            .color(colors.on_surface_variant),
                    )
                    .child(
                        div()
                            .md_type_emphasized(TypeRole::LabelMedium, cx)
                            .child(author),
                    )
                    .child(div().min_w_0().truncate().child(text)),
            );
        }
        if !grouped {
            let mut header = div()
                .flex()
                .flex_row()
                .items_baseline()
                .gap_2()
                .child(
                    div()
                        .md_type_emphasized(TypeRole::TitleSmall, cx)
                        .text_color(if mine {
                            colors.primary
                        } else {
                            colors.on_surface
                        })
                        .child(message.author_name.clone()),
                )
                .child(
                    div()
                        .md_type(TypeRole::LabelSmall, cx)
                        .text_color(colors.on_surface_variant)
                        .child(format_time(message.created_at, t)),
                );
            if message.edited_at.is_some() {
                header = header.child(
                    div()
                        .md_type(TypeRole::LabelSmall, cx)
                        .text_color(colors.on_surface_variant)
                        .child(t.edited),
                );
            }
            content = content.child(header);
        }

        let body = if show_original {
            message.text.clone()
        } else {
            chat.display.text.clone()
        };
        // Files alone have no text.
        if !body.is_empty() {
            content = content.child(
                div()
                    .md_type(TypeRole::BodyLarge, cx)
                    .text_color(colors.on_surface)
                    .child(SelectableText::new(
                        SharedString::from(format!("message-text-{id}")),
                        body,
                    )),
            );
        }
        if settings.show_original && chat.display.is_translated() && !show_original {
            content = content.child(
                div()
                    .md_type(TypeRole::BodySmall, cx)
                    .text_color(colors.on_surface_variant)
                    .child(SelectableText::new(
                        SharedString::from(format!("message-original-{id}")),
                        message.text.clone(),
                    )),
            );
        }
        if let Some(meta) = self.translation_meta(&chat, show_original, cx) {
            content = content.child(meta);
        }
        if !message.attachments.is_empty() {
            content = content.child(self.render_attachments(&id, &message.attachments, cx));
        }
        if let Some(threads) = self.thread_links(&channel, &id, cx) {
            content = content.child(threads);
        }
        if !message.reactions.is_empty() {
            content = content.child(self.reaction_chips(&chat, cx));
        }

        let mut row = div()
            .id(SharedString::from(format!("message-{id}")))
            .relative()
            .w_full()
            .flex()
            .flex_row()
            .gap_3()
            .px_6()
            .pt(px(if grouped { 2.0 } else { 12.0 }))
            .pb(px(2.0))
            .on_hover(cx.listener({
                let id = id.clone();
                move |this, hovered: &bool, _, cx| {
                    if *hovered {
                        this.hovered_message = Some(id.clone());
                    } else if this.hovered_message.as_deref() == Some(&id) {
                        this.hovered_message = None;
                    }
                    cx.notify();
                }
            }));
        if mentioned {
            row = row
                .bg(colors.tertiary_container.opacity(0.45))
                .border_l_2()
                .border_color(colors.tertiary);
        } else if hovered {
            row = row.bg(colors.surface_container_low);
        }
        row = row.child(if grouped {
            div().w(px(40.0)).flex_none()
        } else {
            widgets::avatar(&message.author_name, 40.0, cx)
        });
        row = row.child(content);

        let picking = self
            .picker
            .as_ref()
            .is_some_and(|picker| picker.message_id == id);
        if hovered || picking {
            let guild_owner = self
                .current_guild()
                .is_some_and(|guild| guild.guild.owner_id == me);
            let in_text_channel = self
                .session
                .channel(&channel)
                .is_some_and(|channel| channel.kind == ChannelKind::Text);
            let mut tools = Toolbar::new()
                .child(
                    IconButton::new("react", Icon::AddReaction)
                        .size(ButtonSize::XSmall)
                        .label(t.add_reaction)
                        .on_click(cx.listener({
                            let id = id.clone();
                            move |this, event: &gpui::ClickEvent, window, cx| {
                                this.open_picker(id.clone(), event.position(), window, cx)
                            }
                        })),
                )
                .child(
                    IconButton::new("reply", Icon::Reply)
                        .size(ButtonSize::XSmall)
                        .label(t.reply)
                        .on_click(cx.listener({
                            let id = id.clone();
                            move |this, _, window, cx| this.start_reply(id.clone(), window, cx)
                        })),
                );
            // Threads grow from messages of text channels, not of threads.
            if in_text_channel {
                tools = tools.child(
                    IconButton::new("thread", Icon::Forum)
                        .size(ButtonSize::XSmall)
                        .label(t.start_thread)
                        .on_click(cx.listener({
                            let id = id.clone();
                            move |this, _, window, cx| {
                                this.dialog = Some(Dialog::start_thread(this, &id, window, cx));
                                cx.notify();
                            }
                        })),
                );
            }
            if mine {
                tools = tools.child(
                    IconButton::new("edit", Icon::Edit)
                        .size(ButtonSize::XSmall)
                        .label(t.edit)
                        .on_click(cx.listener({
                            let id = id.clone();
                            move |this, _, window, cx| this.start_edit(id.clone(), window, cx)
                        })),
                );
            }
            if mine || guild_owner {
                tools = tools.child(
                    IconButton::new("delete", Icon::Delete)
                        .size(ButtonSize::XSmall)
                        .label(t.delete)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.dialog = Some(Dialog::delete_message(this, &id, window, cx));
                            cx.notify();
                        })),
                );
            }
            row = row.child(div().absolute().top(px(-8.0)).right(px(24.0)).child(tools));
        }
        row.into_any_element()
    }

    /// "👍 2" for each emoji people reacted with; the user's own stand
    /// out. Clicking one adds or takes back the user's reaction.
    fn reaction_chips(&self, chat: &ChatMessage, cx: &mut Context<Self>) -> AnyElement {
        let colors = cx.md().colors;
        let id = chat.message.id.clone();
        let mut row = div().flex().flex_row().flex_wrap().gap_1().pt_1();
        for (ix, reaction) in chat.message.reactions.iter().enumerate() {
            let (bg, border, fg) = if reaction.me {
                (
                    colors.secondary_container,
                    colors.secondary,
                    colors.on_secondary_container,
                )
            } else {
                (
                    colors.surface_container,
                    colors.outline_variant,
                    colors.on_surface_variant,
                )
            };
            let (message_id, emoji) = (id.clone(), reaction.emoji.clone());
            row = row.child(
                div()
                    .id(SharedString::from(format!("reaction-{id}-{ix}")))
                    .h(px(28.0))
                    .px_2()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .rounded_full()
                    .border_1()
                    .border_color(border)
                    .bg(bg)
                    .text_color(fg)
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_container_highest))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_reaction(message_id.clone(), emoji.clone(), cx)
                    }))
                    .child(div().text_size(px(16.0)).child(reaction.emoji.clone()))
                    .child(
                        div()
                            .md_type(TypeRole::LabelMedium, cx)
                            .child(reaction.count.to_string()),
                    ),
            );
        }
        row.child(
            IconButton::new(
                SharedString::from(format!("more-reactions-{id}")),
                Icon::AddReaction,
            )
            .size(ButtonSize::XSmall)
            .label(self.t().add_reaction)
            .on_click(cx.listener(
                move |this, event: &gpui::ClickEvent, window, cx| {
                    this.open_picker(id.clone(), event.position(), window, cx)
                },
            )),
        )
        .into_any_element()
    }

    /// Threads started from a message, each opening on click.
    fn thread_links(
        &self,
        channel: &str,
        message_id: &str,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let colors = cx.md().colors;
        let threads = self
            .current_guild()?
            .channels
            .into_iter()
            .filter(|thread| {
                thread.kind == ChannelKind::Thread
                    && thread.parent_id.as_deref() == Some(channel)
                    && thread.starter_message_id.as_deref() == Some(message_id)
            })
            .collect::<Vec<_>>();
        if threads.is_empty() {
            return None;
        }
        // Each link as wide as its name.
        let mut column = div().flex().flex_col().items_start().gap_1().pt_1();
        for thread in threads {
            let thread_id = thread.id.clone();
            column = column.child(
                div()
                    .id(SharedString::from(format!("thread-link-{}", thread.id)))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .rounded(px(12.0))
                    .bg(colors.surface_container)
                    .cursor_pointer()
                    .hover(|style| style.bg(colors.surface_container_high))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_channel(&thread_id, window, cx)
                    }))
                    .child(icon(Icon::Forum).size(px(16.0)).color(colors.primary))
                    .child(
                        div()
                            .md_type(TypeRole::LabelLarge, cx)
                            .text_color(colors.primary)
                            .child(thread.name),
                    ),
            );
        }
        Some(column.into_any_element())
    }

    /// "Translated from Japanese · Show original" under a message.
    fn translation_meta(
        &self,
        chat: &ChatMessage,
        show_original: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let colors = cx.md().colors;
        let t = self.t();
        let id = chat.message.id.clone();
        let line = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap_1()
            .md_type(TypeRole::LabelSmall, cx)
            .text_color(colors.on_surface_variant);
        let label = match &chat.display.shown {
            Shown::Original => return None,
            Shown::Translating => {
                return Some(
                    line.child(
                        LoadingIndicator::new(SharedString::from(format!("translating-{id}")))
                            .size(px(16.0)),
                    )
                    .child(t.translating)
                    .into_any_element(),
                );
            }
            Shown::Untranslated { .. } => {
                return Some(line.child(t.untranslated).into_any_element());
            }
            Shown::Server { from, .. } => fill(t.translated_by_server, &[&language_name(from, t)]),
            Shown::Device { from, .. } => fill(t.translated_on_device, &[&language_name(from, t)]),
        };
        let label = if show_original {
            let from = match &chat.display.shown {
                Shown::Server { from, .. } | Shown::Device { from, .. } => language_name(from, t),
                _ => String::new(),
            };
            fill(t.original_in, &[&from])
        } else {
            label
        };
        Some(
            line.child(icon(Icon::Translate).size(px(14.0)).color(colors.primary))
                .child(label)
                .child("·")
                .child(
                    div()
                        .id(SharedString::from(format!("toggle-original-{id}")))
                        .cursor_pointer()
                        .text_color(colors.primary)
                        .hover(|style| style.underline())
                        .child(if show_original {
                            t.show_translation
                        } else {
                            t.show_original
                        })
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.toggle_original(id.clone(), cx)),
                        ),
                )
                .into_any_element(),
        )
    }
}

/// "Today at 23:30", "Yesterday at 08:15", "2026/10/24 21:03".
fn format_time(ms: i64, t: &Strings) -> String {
    let Some(time): Option<DateTime<Local>> = Local.timestamp_millis_opt(ms).single() else {
        return String::new();
    };
    let today = Local::now().date_naive();
    let clock = time.format("%H:%M").to_string();
    if time.date_naive() == today {
        fill(t.today_at, &[&clock])
    } else if today.pred_opt() == Some(time.date_naive()) {
        fill(t.yesterday_at, &[&clock])
    } else {
        time.format("%Y/%m/%d %H:%M").to_string()
    }
}
