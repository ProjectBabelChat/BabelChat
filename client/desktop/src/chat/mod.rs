//! The chat screen: guilds on the left, then channels, messages and
//! members, like Discord. State comes from the SDK session; this view only
//! remembers what is on screen.

mod dialogs;
mod emoji;
mod messages;
mod sidebar;

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use babel_sdk::{
    ChannelKind, GuildChange, GuildView, MessageChange, Notification, NotifyReason, Session, Update,
};
use gpui::prelude::*;
use gpui::{
    Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, ListAlignment, ListState,
    Pixels, Point, Render, SharedString, Subscription, SystemNotification, Task, Window, actions,
    anchored, deferred, div, px,
};
use gpui_md3e::prelude::*;
use tokio::sync::broadcast::error::RecvError;

pub use dialogs::Dialog;
use emoji::{EmojiEvent, EmojiPicker};

use crate::{
    i18n::{self, Strings, fill},
    runtime::{self, Config, describe},
};

actions!(babel, [Cancel]);

pub enum ChatEvent {
    /// `expired`: the server rejected the token; otherwise the user signed
    /// out.
    SignedOut { expired: bool },
    /// The user signed the other devices out or changed the password:
    /// save the new token.
    TokenRenewed { token: babel_sdk::Token },
}

/// What Enter does with the composer's text.
#[derive(Clone, PartialEq, Eq)]
enum Compose {
    Message,
    Reply(String),
    Edit(String),
}

/// A short message at the bottom of the window.
struct Toast {
    text: SharedString,
    /// The channel to open from the toast (a notification).
    open: Option<(String, String)>,
    visible: bool,
}

/// The emoji picker, open for one message.
struct Picker {
    message_id: String,
    view: Entity<EmojiPicker>,
    /// Where it was asked for, on screen.
    position: Point<Pixels>,
    _subscription: Subscription,
}

/// Lines the composer grows to before it scrolls.
const COMPOSER_LINES: usize = 8;
/// How long a toast stays up.
const TOAST_TIME: Duration = Duration::from_secs(5);
/// Messages fetched per history page.
const HISTORY_PAGE: i64 = 50;

pub struct ChatView {
    session: Arc<Session>,
    focus_handle: FocusHandle,
    guild: Option<String>,
    /// The channel last open in each guild.
    channel_of: HashMap<String, String>,
    channel: Option<String>,
    /// Message IDs of the open channel, oldest first, as the list shows them.
    rows: Vec<String>,
    list: ListState,
    /// Channels whose newest page has been asked for in this run.
    loaded: HashSet<String>,
    history_loading: HashSet<String>,
    composer: Entity<gpui_md3e::components::TextField>,
    compose: Compose,
    drafts: HashMap<String, String>,
    /// Messages shown in their original language on request.
    originals: HashSet<String>,
    hovered_message: Option<String>,
    hovered_member: Option<String>,
    members_open: bool,
    picker: Option<Picker>,
    guild_menu: bool,
    dialog: Option<Dialog>,
    toast: Option<Toast>,
    toast_epoch: u64,
    /// Whether the window is active; the open channel is only read while
    /// it is.
    active: bool,
    online: bool,
    /// Names of the guilds, for saying which one the user was removed from.
    guild_names: HashMap<String, String>,
    /// A guild or channel to open once it shows up in the session.
    pending_guild: Option<String>,
    pending_channel: Option<String>,
    _updates: Task<()>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ChatEvent> for ChatView {}

impl ChatView {
    pub fn new(session: Arc<Session>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut updates = session.updates();
        let updates_task = cx.spawn_in(window, async move |this, cx| {
            loop {
                let update = match updates.recv().await {
                    Ok(update) => Some(update),
                    Err(RecvError::Lagged(_)) => None,
                    Err(RecvError::Closed) => break,
                };
                if this
                    .update_in(cx, |this, window, cx| this.on_update(update, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        });

        let list = ListState::new(0, ListAlignment::Bottom, px(800.0));
        list.set_follow_mode(gpui::FollowMode::Tail);
        list.set_scroll_handler(
            cx.listener(|this, event: &gpui::ListScrollEvent, window, cx| {
                if event.visible_range.start == 0 {
                    this.load_older(window, cx);
                }
            }),
        );

        let this = cx.entity().downgrade();
        let composer = cx.new(|cx| {
            gpui_md3e::components::TextField::new(cx)
                .multiline(COMPOSER_LINES)
                .on_submit(move |text, window, cx| {
                    this.update(cx, |this, cx| this.submit(text, window, cx))
                        .unwrap_or(false)
                })
        });

        let subscriptions = vec![
            cx.observe_window_activation(window, |this, window, cx| {
                this.active = window.is_window_active();
                this.update_focus();
                cx.notify();
            }),
            // Others see the user writing; not while editing a message.
            cx.subscribe(
                &composer,
                |this, composer, event: &gpui_md3e::components::TextFieldEvent, cx| {
                    let writing = !composer.read(cx).text().trim().is_empty();
                    if *event == gpui_md3e::components::TextFieldEvent::Edited
                        && writing
                        && !matches!(this.compose, Compose::Edit(_))
                        && let Some(channel) = &this.channel
                    {
                        this.session.typing(channel);
                    }
                },
            ),
        ];

        let weak = cx.entity().downgrade();
        let window_handle = window.window_handle();
        cx.on_system_notification_response(move |response, cx| {
            let channel_id = response.tag.to_string();
            let weak = weak.clone();
            let _ = window_handle.update(cx, |_, window, cx| {
                window.activate_window();
                let _ = weak.update(cx, |this: &mut ChatView, cx| {
                    this.open_channel(&channel_id, window, cx)
                });
            });
        });

        let mut view = Self {
            online: session.is_online(),
            session,
            focus_handle: cx.focus_handle(),
            guild: None,
            channel_of: HashMap::new(),
            channel: None,
            rows: Vec::new(),
            list,
            loaded: HashSet::new(),
            history_loading: HashSet::new(),
            composer,
            compose: Compose::Message,
            drafts: HashMap::new(),
            originals: HashSet::new(),
            hovered_message: None,
            hovered_member: None,
            members_open: true,
            picker: None,
            guild_menu: false,
            dialog: None,
            toast: None,
            toast_epoch: 0,
            active: window.is_window_active(),
            guild_names: HashMap::new(),
            pending_guild: None,
            pending_channel: None,
            _updates: updates_task,
            _subscriptions: subscriptions,
        };
        view.guild_names = view
            .session
            .guilds()
            .into_iter()
            .map(|guild| (guild.guild.id, guild.guild.name))
            .collect();
        if let Some(first) = view.session.guilds().first() {
            let id = first.guild.id.clone();
            view.select_guild(&id, window, cx);
        }
        view
    }

    /// The UI strings for the language the user reads.
    fn t(&self) -> &'static Strings {
        i18n::strings(&self.session.settings().locale)
    }

    fn current_guild(&self) -> Option<GuildView> {
        self.guild.as_deref().and_then(|id| self.session.guild(id))
    }

    fn is_owner(&self, guild: &GuildView) -> bool {
        guild.guild.owner_id == self.session.user_id()
    }

    // ---- Events from the session ---------------------------------------

    fn on_update(&mut self, update: Option<Update>, window: &mut Window, cx: &mut Context<Self>) {
        match update {
            // Fell behind: redraw everything.
            None => self.sync_rows(true),
            Some(Update::Message {
                channel_id,
                message_id,
                change,
                ..
            }) => {
                if self.channel.as_deref() == Some(&channel_id) {
                    self.sync_rows(false);
                    if matches!(
                        change,
                        MessageChange::Edited
                            | MessageChange::Translated
                            | MessageChange::Reactions
                    ) && let Some(ix) = self.rows.iter().position(|id| *id == message_id)
                    {
                        self.list.remeasure_items(ix..ix + 1);
                    }
                }
            }
            // The line under the composer reads the session.
            Some(Update::Typing { .. }) => {}
            Some(Update::Notification(notification)) => self.notify(notification, cx),
            Some(Update::Guild { guild_id, change }) => {
                self.on_guild_change(guild_id, change, window, cx)
            }
            Some(Update::Online { reloaded }) => {
                self.online = true;
                if reloaded {
                    self.sync_rows(true);
                }
                self.ensure_selection(window, cx);
            }
            Some(Update::Offline { .. }) => self.online = false,
            Some(Update::SignedOut) => cx.emit(ChatEvent::SignedOut { expired: true }),
            Some(Update::Unread { .. }) => {}
        }
        self.guild_names = self
            .session
            .guilds()
            .into_iter()
            .map(|view| (view.guild.id, view.guild.name))
            .collect();
        // Something asked for may have arrived.
        if let Some(guild) = self.pending_guild.clone()
            && self.session.guild(&guild).is_some()
        {
            self.pending_guild = None;
            self.select_guild(&guild, window, cx);
        }
        if let Some(channel) = self.pending_channel.clone()
            && self.session.channel(&channel).is_some()
        {
            self.pending_channel = None;
            self.open_channel(&channel, window, cx);
        }
        cx.notify();
    }

    fn on_guild_change(
        &mut self,
        guild_id: String,
        change: GuildChange,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Messages show the threads started from them.
        if matches!(
            change,
            GuildChange::ChannelCreated { .. }
                | GuildChange::ChannelUpdated { .. }
                | GuildChange::ChannelDeleted { .. }
        ) && self.guild.as_deref() == Some(&guild_id)
        {
            self.list.remeasure();
        }
        match change {
            GuildChange::Removed => {
                let name = self.guild_names.get(&guild_id).cloned().unwrap_or_default();
                let text = fill(self.t().removed_from, &[&name]);
                self.show_toast(text, None, cx);
                self.ensure_selection(window, cx);
            }
            GuildChange::Deleted => {
                let name = self.guild_names.get(&guild_id).cloned().unwrap_or_default();
                let text = fill(self.t().guild_deleted, &[&name]);
                self.show_toast(text, None, cx);
                self.ensure_selection(window, cx);
            }
            GuildChange::Left => self.ensure_selection(window, cx),
            GuildChange::Joined | GuildChange::Refreshed => self.ensure_selection(window, cx),
            GuildChange::ChannelCreated { .. } => {
                if self.guild.as_deref() == Some(&guild_id) && self.channel.is_none() {
                    self.select_guild(&guild_id, window, cx);
                }
            }
            GuildChange::ChannelDeleted { channel_id } => {
                self.channel_of.retain(|_, channel| *channel != channel_id);
                self.drafts.remove(&channel_id);
                self.ensure_selection(window, cx);
            }
            GuildChange::ChannelUpdated { channel_id } => {
                if self.channel.as_deref() == Some(&channel_id)
                    && let Some(channel) = self.session.channel(&channel_id)
                {
                    let placeholder = fill(self.t().message_placeholder, &[&channel.name]);
                    self.composer
                        .update(cx, |field, cx| field.set_placeholder(placeholder, cx));
                }
            }
            GuildChange::Languages => self.list.remeasure(),
            _ => {}
        }
    }

    /// Keeps the selection pointing at something that exists.
    fn ensure_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let guilds = self.session.guilds();
        let guild_ok = self
            .guild
            .as_deref()
            .is_some_and(|id| guilds.iter().any(|view| view.guild.id == id));
        if !guild_ok {
            self.guild = None;
            self.channel = None;
            self.rows.clear();
            self.list.reset(0);
            if let Some(first) = guilds.first() {
                let id = first.guild.id.clone();
                self.select_guild(&id, window, cx);
            }
            return;
        }
        let channel_ok = self
            .channel
            .as_deref()
            .is_some_and(|id| self.session.channel(id).is_some());
        if !channel_ok && let Some(guild) = self.guild.clone() {
            self.select_guild(&guild, window, cx);
        }
    }

    // ---- Selection -------------------------------------------------------

    fn select_guild(&mut self, guild_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(view) = self.session.guild(guild_id) else {
            self.pending_guild = Some(guild_id.to_string());
            return;
        };
        self.guild = Some(guild_id.to_string());
        self.guild_menu = false;
        let remembered = self
            .channel_of
            .get(guild_id)
            .filter(|id| view.channels.iter().any(|channel| channel.id == **id))
            .cloned();
        let channel = remembered.or_else(|| {
            view.channels
                .iter()
                .find(|channel| channel.kind == ChannelKind::Text)
                .map(|channel| channel.id.clone())
        });
        match channel {
            Some(channel) => self.open_channel(&channel, window, cx),
            None => {
                self.save_draft(cx);
                self.channel = None;
                self.rows.clear();
                self.list.reset(0);
                self.update_focus();
            }
        }
        cx.notify();
    }

    fn open_channel(&mut self, channel_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(channel) = self.session.channel(channel_id) else {
            self.pending_channel = Some(channel_id.to_string());
            return;
        };
        if self.guild.as_deref() != Some(&channel.guild_id) {
            self.guild = Some(channel.guild_id.clone());
        }
        if self.channel.as_deref() != Some(channel_id) {
            self.save_draft(cx);
            self.channel = Some(channel_id.to_string());
            self.compose = Compose::Message;
            let draft = self.drafts.remove(channel_id).unwrap_or_default();
            let placeholder = fill(self.t().message_placeholder, &[&channel.name]);
            self.composer.update(cx, |field, cx| {
                field.set_text(draft, window, cx);
                field.set_placeholder(placeholder, cx);
            });
            self.rows.clear();
            self.list.reset(0);
            self.sync_rows(true);
            self.list.scroll_to_end();
        }
        self.channel_of
            .insert(channel.guild_id, channel_id.to_string());
        self.update_focus();
        if self.loaded.insert(channel_id.to_string()) {
            self.load_history(channel_id.to_string(), cx);
        }
        window.focus(&self.composer.focus_handle(cx), cx);
        cx.notify();
    }

    fn save_draft(&mut self, cx: &mut Context<Self>) {
        if let Some(channel) = &self.channel {
            let text = self.composer.read(cx).text().to_string();
            if self.compose == Compose::Message && !text.is_empty() {
                self.drafts.insert(channel.clone(), text);
            }
        }
    }

    /// Tells the session which channel is on screen: it is read as it grows
    /// and does not notify. Nothing is on screen while the window is in the
    /// background.
    fn update_focus(&self) {
        let channel = self.channel.as_deref().filter(|_| self.active);
        self.session.focus(channel);
    }

    // ---- Messages ----------------------------------------------------------

    /// Brings `rows` in line with the session, telling the list which rows
    /// changed so it keeps its scroll position.
    fn sync_rows(&mut self, remeasure: bool) {
        let Some(channel) = &self.channel else {
            return;
        };
        let new: Vec<String> = self
            .session
            .messages(channel)
            .into_iter()
            .map(|chat| chat.message.id)
            .collect();
        let old = &self.rows;
        let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
        let suffix = old[prefix..]
            .iter()
            .rev()
            .zip(new[prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        if prefix + suffix != old.len() || prefix + suffix != new.len() {
            self.list
                .splice(prefix..old.len() - suffix, new.len() - prefix - suffix);
            // The row after a change may start or stop a group.
            if prefix + (new.len() - prefix - suffix) < new.len() {
                let next = new.len() - suffix;
                self.list.remeasure_items(next..next + 1);
            }
        }
        self.rows = new;
        if remeasure {
            self.list.remeasure();
        }
    }

    fn load_history(&mut self, channel_id: String, cx: &mut Context<Self>) {
        if !self.history_loading.insert(channel_id.clone()) {
            return;
        }
        let session = self.session.clone();
        let task = runtime::spawn(cx, {
            let channel_id = channel_id.clone();
            async move { session.load_history(&channel_id, HISTORY_PAGE).await }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.history_loading.remove(&channel_id);
                if let Err(error) = result {
                    let text = describe(&error, this.t());
                    this.show_toast(text, None, cx);
                }
                if this.channel.as_deref() == Some(&channel_id) {
                    this.sync_rows(false);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn load_older(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(channel) = self.channel.clone() else {
            return;
        };
        if self.rows.is_empty() || self.session.history_complete(&channel) {
            return;
        }
        self.load_history(channel, cx);
    }

    /// Sends, replies or edits with the composer's text. Returns whether
    /// the composer should clear.
    fn submit(&mut self, text: &SharedString, _: &mut Window, cx: &mut Context<Self>) -> bool {
        let text = text.trim().to_string();
        let Some(channel) = self.channel.clone() else {
            return false;
        };
        if text.is_empty() {
            return false;
        }
        let compose = std::mem::replace(&mut self.compose, Compose::Message);
        let session = self.session.clone();
        let task = runtime::spawn(cx, async move {
            match compose {
                Compose::Message => session.send(&channel, &text).await.map(drop),
                Compose::Reply(id) => session.reply(&channel, &id, &text).await.map(drop),
                Compose::Edit(id) => session.edit(&channel, &id, &text).await.map(drop),
            }
        });
        self.list.scroll_to_end();
        cx.spawn(async move |this, cx| {
            if let Err(error) = task.await {
                let _ = this.update(cx, |this, cx| {
                    let text = describe(&error, this.t());
                    this.show_toast(text, None, cx);
                });
            }
        })
        .detach();
        cx.notify();
        true
    }

    fn start_reply(&mut self, message_id: String, window: &mut Window, cx: &mut Context<Self>) {
        self.compose = Compose::Reply(message_id);
        window.focus(&self.composer.focus_handle(cx), cx);
        cx.notify();
    }

    fn start_edit(&mut self, message_id: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(channel) = self.channel.clone() else {
            return;
        };
        let Some(chat) = self.session.message(&channel, &message_id) else {
            return;
        };
        self.save_draft(cx);
        self.compose = Compose::Edit(message_id);
        self.composer.update(cx, |field, cx| {
            field.set_text(chat.message.text, window, cx)
        });
        window.focus(&self.composer.focus_handle(cx), cx);
        cx.notify();
    }

    /// Adds the user's reaction, or takes it back if they already reacted
    /// with this emoji.
    fn toggle_reaction(&mut self, message_id: String, emoji: String, cx: &mut Context<Self>) {
        let Some(channel) = self.channel.clone() else {
            return;
        };
        let reacted = self
            .session
            .message(&channel, &message_id)
            .is_some_and(|chat| {
                chat.message
                    .reactions
                    .iter()
                    .any(|reaction| reaction.emoji == emoji && reaction.me)
            });
        let session = self.session.clone();
        let task = runtime::spawn(cx, async move {
            if reacted {
                session.unreact(&channel, &message_id, &emoji).await
            } else {
                session.react(&channel, &message_id, &emoji).await
            }
        });
        cx.spawn(async move |this, cx| {
            if let Err(error) = task.await {
                let _ = this.update(cx, |this, cx| {
                    let text = describe(&error, this.t());
                    this.show_toast(text, None, cx);
                });
            }
        })
        .detach();
    }

    /// Opens the emoji picker for a message, where it was asked for.
    fn open_picker(
        &mut self,
        message_id: String,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let t = self.t();
        let view = cx.new(|cx| EmojiPicker::new(t.search_emoji, t.no_emoji, window, cx));
        let subscription = cx.subscribe_in(&view, window, {
            let message_id = message_id.clone();
            move |this, _, event: &EmojiEvent, window, cx| {
                let EmojiEvent::Picked(emoji) = event;
                this.picker = None;
                this.toggle_reaction(message_id.clone(), emoji.clone(), cx);
                window.focus(&this.composer.focus_handle(cx), cx);
                cx.notify();
            }
        });
        self.picker = Some(Picker {
            message_id,
            view,
            position,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// "alice is typing…" for the open channel, or nothing.
    fn typing_text(&self) -> Option<String> {
        let t = self.t();
        let names = self
            .session
            .typing_members(self.channel.as_deref()?)
            .into_iter()
            .map(|member| member.name)
            .collect::<Vec<_>>();
        match names.as_slice() {
            [] => None,
            [one] => Some(fill(t.typing_one, &[one])),
            [one, two] => Some(fill(t.typing_two, &[one, two])),
            _ => Some(t.typing_many.to_string()),
        }
    }

    fn cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker.is_some() {
            self.picker = None;
        } else if self.dialog.is_some() {
            self.dialog = None;
        } else if self.guild_menu {
            self.guild_menu = false;
        } else if self.compose != Compose::Message {
            if matches!(self.compose, Compose::Edit(_)) {
                let draft = self
                    .channel
                    .as_ref()
                    .and_then(|channel| self.drafts.remove(channel))
                    .unwrap_or_default();
                self.composer
                    .update(cx, |field, cx| field.set_text(draft, window, cx));
            }
            self.compose = Compose::Message;
        }
        cx.notify();
    }

    fn toggle_original(&mut self, message_id: String, cx: &mut Context<Self>) {
        if !self.originals.remove(&message_id) {
            self.originals.insert(message_id.clone());
        }
        if let Some(ix) = self.rows.iter().position(|id| *id == message_id) {
            self.list.remeasure_items(ix..ix + 1);
        }
        cx.notify();
    }

    // ---- Notifications and toasts ---------------------------------------------

    fn notify(&mut self, notification: Notification, cx: &mut Context<Self>) {
        let t = self.t();
        let message = &notification.message.message;
        let channel = self
            .session
            .channel(&message.channel_id)
            .map(|channel| channel.name)
            .unwrap_or_default();
        let template = match notification.reason {
            NotifyReason::Mention => t.mention_title,
            NotifyReason::Reply => t.reply_title,
            NotifyReason::Message => t.message_title,
        };
        let title = fill(template, &[&message.author_name, &channel]);
        let body = notification.message.display.text.clone();
        let target = (notification.guild_id.clone(), message.channel_id.clone());
        if self.active {
            self.show_toast(format!("{title}: {body}"), Some(target), cx);
        } else if cx.global::<Config>().system_notifications {
            cx.show_system_notification(SystemNotification {
                tag: message.channel_id.clone().into(),
                title: title.into(),
                body: body.into(),
                actions: Vec::new(),
            });
        }
    }

    fn show_toast(
        &mut self,
        text: impl Into<SharedString>,
        open: Option<(String, String)>,
        cx: &mut Context<Self>,
    ) {
        self.toast = Some(Toast {
            text: text.into(),
            open,
            visible: true,
        });
        self.toast_epoch += 1;
        let epoch = self.toast_epoch;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(TOAST_TIME).await;
            let _ = this.update(cx, |this, cx| {
                if this.toast_epoch == epoch
                    && let Some(toast) = &mut this.toast
                {
                    toast.visible = false;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn sign_out(&mut self, cx: &mut Context<Self>) {
        self.session.forget_cache();
        cx.emit(ChatEvent::SignedOut { expired: false });
    }
}

impl Focusable for ChatView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ChatView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.md().colors;
        let guild = self.current_guild();

        let mut panel = div()
            .size_full()
            .flex()
            .flex_row()
            .rounded(px(24.0))
            .overflow_hidden()
            .bg(colors.surface)
            .child(self.render_channel(guild.as_ref(), window, cx));
        if self.members_open
            && let Some(guild) = &guild
        {
            panel = panel.child(self.render_members(guild, cx));
        }
        let main = div().flex_1().min_w_0().h_full().py_2().pr_2().child(panel);

        let mut root = div()
            .id("chat")
            .key_context("Chat")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::cancel))
            .relative()
            .size_full()
            .flex()
            .flex_row()
            .bg(colors.surface_container)
            .text_color(colors.on_surface)
            .md_type(TypeRole::BodyMedium, cx)
            .child(self.render_guild_rail(window, cx))
            .child(self.render_sidebar(guild.as_ref(), cx))
            .child(main);

        if self.guild_menu
            && let Some(guild) = &guild
        {
            root = root.child(self.render_guild_menu(guild, cx));
        }
        if let Some(toast) = &self.toast {
            let open = toast.open.clone();
            let mut snackbar = Snackbar::new("toast", toast.text.clone()).visible(toast.visible);
            if let Some((_, channel)) = open {
                snackbar = snackbar.action(
                    self.t().open,
                    cx.listener(move |this, _, window, cx| {
                        this.open_channel(&channel, window, cx);
                        if let Some(toast) = &mut this.toast {
                            toast.visible = false;
                        }
                    }),
                );
            }
            root = root.child(
                div()
                    .absolute()
                    .bottom(px(96.0))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(div().max_w(px(560.0)).child(snackbar)),
            );
        }
        if let Some(picker) = &self.picker {
            // Above everything, kept inside the window.
            root = root.child(deferred(
                anchored().position(picker.position).snap_to_window().child(
                    div()
                        .occlude()
                        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                            this.picker = None;
                            cx.notify();
                        }))
                        .child(picker.view.clone()),
                ),
            ));
        }
        if self.dialog.is_some() {
            root = root.child(self.render_dialog(window, cx));
        }
        root
    }
}
