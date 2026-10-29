//! Dialogs: adding a guild, inviting, channels, languages, notifications,
//! settings, the password and confirmations.

use babel_sdk::{ChannelNotify, LanguagePolicyUpdate, NotifyLevel, TranslationModel, UserSettings};
use gpui::Focusable;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClipboardItem, Context, Entity, IntoElement, SharedString, Window, div, px,
};
use gpui_md3e::prelude::*;

use super::ChatView;
use crate::{
    i18n::{COMMON_LANGUAGES, fill, language_name},
    runtime::{self, describe},
    widgets,
};

/// How long new invites work.
const INVITE_AGE_SECS: i64 = 7 * 24 * 60 * 60;

pub enum Dialog {
    AddGuild {
        join: bool,
        name: Entity<TextField>,
        invite: Entity<TextField>,
        languages: Vec<String>,
        busy: bool,
        error: Option<String>,
    },
    Invite {
        guild_id: String,
        code: Option<String>,
        copied: bool,
        error: Option<String>,
    },
    /// Creating or editing a channel, or starting a thread.
    Channel {
        target: ChannelTarget,
        name: Entity<TextField>,
        /// Threads have none.
        topic: Option<Entity<TextField>>,
        busy: bool,
        error: Option<String>,
    },
    /// The guild's name and description (its owner).
    Guild {
        guild_id: String,
        name: Entity<TextField>,
        description: Entity<TextField>,
        busy: bool,
        error: Option<String>,
    },
    Languages {
        guild_id: String,
        languages: Vec<String>,
        llm: Vec<String>,
        /// What the guild may translate with, once loaded.
        models: Vec<TranslationModel>,
        /// None: the default model.
        model: Option<String>,
        busy: bool,
        error: Option<String>,
    },
    Notifications {
        guild_id: String,
        channel_id: Option<String>,
    },
    Settings {
        locale: Entity<TextField>,
        show_original: bool,
        translate_outside_set: bool,
        busy: bool,
        error: Option<String>,
    },
    Password {
        current: Entity<TextField>,
        new: Entity<TextField>,
        again: Entity<TextField>,
        busy: bool,
        error: Option<String>,
    },
    Confirm {
        title: String,
        body: String,
        action_label: &'static str,
        action: Confirm,
        busy: bool,
        error: Option<String>,
    },
}

pub enum Confirm {
    Leave {
        guild_id: String,
    },
    Remove {
        guild_id: String,
        user_id: String,
        ban: bool,
    },
    Delete {
        channel_id: String,
        message_id: String,
    },
    DeleteChannel {
        channel_id: String,
    },
    DeleteGuild {
        guild_id: String,
    },
    SignOutOthers,
}

#[derive(Clone)]
pub enum ChannelTarget {
    Create {
        guild_id: String,
    },
    Edit {
        channel_id: String,
        thread: bool,
    },
    Thread {
        channel_id: String,
        message_id: String,
    },
}

fn field(label: &str, cx: &mut Context<ChatView>) -> Entity<TextField> {
    let label = label.to_string();
    let submit = submit(cx);
    cx.new(|cx| TextField::new(cx).label(label).on_submit(submit))
}

/// Enter in a dialog's field confirms the dialog.
fn submit(
    cx: &mut Context<ChatView>,
) -> impl Fn(&SharedString, &mut Window, &mut App) -> bool + Clone + 'static {
    let this = cx.entity().downgrade();
    move |_, window, cx| {
        // The field is still being updated; read it afterwards.
        let this = this.clone();
        window.defer(cx, move |window, cx| {
            let _ = this.update(cx, |this, cx| this.confirm_dialog(window, cx));
        });
        false
    }
}

/// The base tag of a language tag: "ja-JP" → "ja".
fn base(tag: &str) -> String {
    tag.split(['-', '_']).next().unwrap_or(tag).to_lowercase()
}

impl Dialog {
    pub fn add_guild(chat: &ChatView, window: &mut Window, cx: &mut Context<ChatView>) -> Self {
        let t = chat.t();
        let submit = submit(cx);
        let name = field(t.guild_name, cx);
        let invite = cx.new(|cx| {
            TextField::new(cx)
                .label(t.invite_code)
                .supporting_text(t.invite_code_hint)
                .on_submit(submit.clone())
        });
        window.focus(&name.focus_handle(cx), cx);
        let mine = base(&chat.session.settings().locale);
        let mut languages = vec![mine.clone()];
        if mine != "en" {
            languages.push("en".to_string());
        }
        Dialog::AddGuild {
            join: false,
            name,
            invite,
            languages,
            busy: false,
            error: None,
        }
    }

    pub fn invite(chat: &ChatView, _: &mut Window, cx: &mut Context<ChatView>) -> Self {
        let guild_id = chat.guild.clone().unwrap_or_default();
        let session = chat.session.clone();
        let task = runtime::spawn(cx, {
            let guild_id = guild_id.clone();
            async move {
                session
                    .create_invite(&guild_id, Some(INVITE_AGE_SECS), None)
                    .await
            }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let t = this.t();
                if let Some(Dialog::Invite { code, error, .. }) = &mut this.dialog {
                    match result {
                        Ok(invite) => *code = Some(invite.code),
                        Err(failure) => *error = Some(describe(&failure, t)),
                    }
                }
                cx.notify();
            });
        })
        .detach();
        Dialog::Invite {
            guild_id,
            code: None,
            copied: false,
            error: None,
        }
    }

    pub fn create_channel(
        chat: &ChatView,
        window: &mut Window,
        cx: &mut Context<ChatView>,
    ) -> Self {
        let t = chat.t();
        let name = field(t.channel_name, cx);
        let topic = field(t.channel_topic, cx);
        window.focus(&name.focus_handle(cx), cx);
        Dialog::Channel {
            target: ChannelTarget::Create {
                guild_id: chat.guild.clone().unwrap_or_default(),
            },
            name,
            topic: Some(topic),
            busy: false,
            error: None,
        }
    }

    /// Renaming a channel or thread, or deleting it.
    pub fn edit_channel(
        chat: &ChatView,
        channel_id: &str,
        window: &mut Window,
        cx: &mut Context<ChatView>,
    ) -> Option<Self> {
        let t = chat.t();
        let channel = chat.session.channel(channel_id)?;
        let thread = channel.kind == babel_sdk::ChannelKind::Thread;
        let name = field(
            if thread {
                t.thread_name
            } else {
                t.channel_name
            },
            cx,
        );
        name.update(cx, |field, cx| {
            field.set_text(channel.name.clone(), window, cx)
        });
        let topic = (!thread).then(|| {
            let topic = field(t.channel_topic, cx);
            let text = channel.topic.clone().unwrap_or_default();
            topic.update(cx, |field, cx| field.set_text(text, window, cx));
            topic
        });
        window.focus(&name.focus_handle(cx), cx);
        Some(Dialog::Channel {
            target: ChannelTarget::Edit {
                channel_id: channel_id.to_string(),
                thread,
            },
            name,
            topic,
            busy: false,
            error: None,
        })
    }

    /// Starting a thread from a message.
    pub fn start_thread(
        chat: &ChatView,
        message_id: &str,
        window: &mut Window,
        cx: &mut Context<ChatView>,
    ) -> Self {
        let t = chat.t();
        let name = field(t.thread_name, cx);
        // Named after the message's start, as Discord does; easy to change.
        let suggestion = chat
            .channel
            .as_deref()
            .and_then(|channel| chat.session.message(channel, message_id))
            .map(|chat| {
                let line = chat.display.text.lines().next().unwrap_or_default();
                line.chars().take(40).collect::<String>()
            })
            .unwrap_or_default();
        name.update(cx, |field, cx| field.set_text(suggestion, window, cx));
        window.focus(&name.focus_handle(cx), cx);
        Dialog::Channel {
            target: ChannelTarget::Thread {
                channel_id: chat.channel.clone().unwrap_or_default(),
                message_id: message_id.to_string(),
            },
            name,
            topic: None,
            busy: false,
            error: None,
        }
    }

    pub fn edit_guild(chat: &ChatView, window: &mut Window, cx: &mut Context<ChatView>) -> Self {
        let t = chat.t();
        let guild = chat.current_guild();
        let name = field(t.guild_name, cx);
        let description = field(t.guild_description, cx);
        if let Some(guild) = &guild {
            let (text, about) = (
                guild.guild.name.clone(),
                guild.guild.description.clone().unwrap_or_default(),
            );
            name.update(cx, |field, cx| field.set_text(text, window, cx));
            description.update(cx, |field, cx| field.set_text(about, window, cx));
        }
        window.focus(&name.focus_handle(cx), cx);
        Dialog::Guild {
            guild_id: chat.guild.clone().unwrap_or_default(),
            name,
            description,
            busy: false,
            error: None,
        }
    }

    pub fn delete_channel(chat: &ChatView, channel_id: &str) -> Self {
        let t = chat.t();
        let name = chat
            .session
            .channel(channel_id)
            .map(|channel| channel.name)
            .unwrap_or_default();
        Dialog::Confirm {
            title: t.delete_channel.to_string(),
            body: fill(t.delete_channel_confirm, &[&name]),
            action_label: t.delete,
            action: Confirm::DeleteChannel {
                channel_id: channel_id.to_string(),
            },
            busy: false,
            error: None,
        }
    }

    pub fn delete_guild(chat: &ChatView, guild_id: &str) -> Self {
        let t = chat.t();
        let name = chat
            .session
            .guild(guild_id)
            .map(|view| view.guild.name)
            .unwrap_or_default();
        Dialog::Confirm {
            title: t.delete_guild.to_string(),
            body: fill(t.delete_guild_confirm, &[&name]),
            action_label: t.delete,
            action: Confirm::DeleteGuild {
                guild_id: guild_id.to_string(),
            },
            busy: false,
            error: None,
        }
    }

    pub fn languages(chat: &ChatView, _: &mut Window, _: &mut Context<ChatView>) -> Self {
        let guild = chat.current_guild();
        let (languages, llm, model) = guild
            .map(|guild| {
                (
                    guild.policy.languages,
                    guild.policy.llm_languages,
                    guild.policy.model,
                )
            })
            .unwrap_or_default();
        Dialog::Languages {
            guild_id: chat.guild.clone().unwrap_or_default(),
            languages,
            llm,
            models: Vec::new(),
            model,
            busy: false,
            error: None,
        }
    }

    pub fn notifications(chat: &ChatView, _: &mut Window, _: &mut Context<ChatView>) -> Self {
        Dialog::Notifications {
            guild_id: chat.guild.clone().unwrap_or_default(),
            channel_id: chat.channel.clone(),
        }
    }

    pub fn settings(chat: &ChatView, window: &mut Window, cx: &mut Context<ChatView>) -> Self {
        let t = chat.t();
        let submit = submit(cx);
        let settings = chat.session.settings();
        let locale = cx.new(|cx| {
            TextField::new(cx)
                .label(t.locale)
                .supporting_text(t.locale_hint)
                .leading_icon(Icon::Translate)
                .on_submit(submit.clone())
        });
        locale.update(cx, |field, cx| field.set_text(settings.locale, window, cx));
        Dialog::Settings {
            locale,
            show_original: settings.show_original,
            translate_outside_set: settings.translate_outside_set,
            busy: false,
            error: None,
        }
    }

    pub fn password(chat: &ChatView, window: &mut Window, cx: &mut Context<ChatView>) -> Self {
        let t = chat.t();
        let submit = submit(cx);
        let masked = |label: &'static str, cx: &mut Context<ChatView>| {
            let submit = submit.clone();
            cx.new(|cx| {
                TextField::new(cx)
                    .label(label)
                    .masked(true)
                    .leading_icon(Icon::Lock)
                    .on_submit(submit)
            })
        };
        let current = masked(t.current_password, cx);
        let new = masked(t.new_password, cx);
        let again = masked(t.new_password_again, cx);
        window.focus(&current.focus_handle(cx), cx);
        Dialog::Password {
            current,
            new,
            again,
            busy: false,
            error: None,
        }
    }

    pub fn sign_out_others(chat: &ChatView, _: &mut Window, _: &mut Context<ChatView>) -> Self {
        let t = chat.t();
        Dialog::Confirm {
            title: t.sign_out_others.to_string(),
            body: t.sign_out_others_confirm.to_string(),
            action_label: t.sign_out_others,
            action: Confirm::SignOutOthers,
            busy: false,
            error: None,
        }
    }

    pub fn leave(chat: &ChatView, _: &mut Window, _: &mut Context<ChatView>) -> Self {
        let t = chat.t();
        let guild = chat.current_guild();
        let name = guild.map(|guild| guild.guild.name).unwrap_or_default();
        Dialog::Confirm {
            title: t.leave_guild.to_string(),
            body: fill(t.leave_confirm, &[&name]),
            action_label: t.leave,
            action: Confirm::Leave {
                guild_id: chat.guild.clone().unwrap_or_default(),
            },
            busy: false,
            error: None,
        }
    }

    pub fn remove_member(
        chat: &ChatView,
        user_id: &str,
        name: &str,
        ban: bool,
        _: &mut Window,
        _: &mut Context<ChatView>,
    ) -> Self {
        let t = chat.t();
        Dialog::Confirm {
            title: if ban { t.ban } else { t.kick }.to_string(),
            body: fill(if ban { t.ban_confirm } else { t.kick_confirm }, &[name]),
            action_label: if ban { t.ban } else { t.kick },
            action: Confirm::Remove {
                guild_id: chat.guild.clone().unwrap_or_default(),
                user_id: user_id.to_string(),
                ban,
            },
            busy: false,
            error: None,
        }
    }

    pub fn delete_message(
        chat: &ChatView,
        message_id: &str,
        _: &mut Window,
        _: &mut Context<ChatView>,
    ) -> Self {
        let t = chat.t();
        Dialog::Confirm {
            title: t.delete.to_string(),
            body: t.delete_confirm.to_string(),
            action_label: t.delete,
            action: Confirm::Delete {
                channel_id: chat.channel.clone().unwrap_or_default(),
                message_id: message_id.to_string(),
            },
            busy: false,
            error: None,
        }
    }
}

/// A text button in the error color, at the start of a dialog's actions:
/// it opens a confirmation, never acts by itself.
fn destructive(
    id: &'static str,
    label: &'static str,
    on_click: impl Fn(&mut ChatView, &mut Window, &mut Context<ChatView>) + 'static,
    cx: &mut Context<ChatView>,
) -> AnyElement {
    let colors = cx.md().colors;
    div()
        .flex_1()
        .child(
            div()
                .id(id)
                .px_3()
                .py_2()
                .rounded_full()
                .cursor_pointer()
                .text_color(colors.error)
                .md_type(TypeRole::LabelLarge, cx)
                .hover(move |style| style.bg(colors.error.opacity(0.08)))
                .on_click(cx.listener(move |this, _, window, cx| on_click(this, window, cx)))
                .child(label),
        )
        .into_any_element()
}

/// Marks the open dialog busy or failed.
fn set_status(dialog: &mut Option<Dialog>, now_busy: bool, failure: Option<String>) {
    let (busy, error) = match dialog {
        Some(Dialog::AddGuild { busy, error, .. })
        | Some(Dialog::Channel { busy, error, .. })
        | Some(Dialog::Guild { busy, error, .. })
        | Some(Dialog::Languages { busy, error, .. })
        | Some(Dialog::Settings { busy, error, .. })
        | Some(Dialog::Password { busy, error, .. })
        | Some(Dialog::Confirm { busy, error, .. }) => (busy, error),
        _ => return,
    };
    *busy = now_busy;
    *error = failure;
}

impl ChatView {
    /// Opens the guild's language settings and loads the models it may
    /// translate with.
    pub(super) fn open_languages(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dialog = Some(Dialog::languages(self, window, cx));
        let Some(guild_id) = self.guild.clone() else {
            return;
        };
        let session = self.session.clone();
        let task = runtime::spawn(cx, async move { session.models(&guild_id).await });
        cx.spawn_in(window, async move |this, cx| {
            // Without the list the dialog just offers no choice.
            let Ok(loaded) = task.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(Dialog::Languages { models, .. }) = &mut this.dialog {
                    *models = loaded;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Runs `work` on the SDK runtime with the dialog marked busy; on
    /// success the dialog closes and `done` runs, on failure it shows the
    /// error.
    fn run_dialog<T: Send + 'static>(
        &mut self,
        work: impl std::future::Future<Output = babel_sdk::Result<T>> + Send + 'static,
        done: impl FnOnce(&mut ChatView, T, &mut Window, &mut Context<ChatView>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        set_status(&mut self.dialog, true, None);
        let task = runtime::spawn(cx, work);
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match result {
                    Ok(value) => {
                        this.dialog = None;
                        done(this, value, window, cx);
                    }
                    Err(failure) => {
                        let joining =
                            matches!(this.dialog, Some(Dialog::AddGuild { join: true, .. }));
                        let password = matches!(this.dialog, Some(Dialog::Password { .. }));
                        let text = match failure {
                            babel_sdk::Error::Api(babel_sdk::ApiError::NotFound) if joining => {
                                this.t().invite_invalid.to_string()
                            }
                            babel_sdk::Error::Api(babel_sdk::ApiError::InvalidCredentials)
                                if password =>
                            {
                                this.t().wrong_current_password.to_string()
                            }
                            failure => describe(&failure, this.t()),
                        };
                        set_status(&mut this.dialog, false, Some(text));
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn confirm_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = &self.dialog else {
            return;
        };
        let session = self.session.clone();
        match dialog {
            Dialog::AddGuild {
                join,
                name,
                invite,
                languages,
                ..
            } => {
                if *join {
                    let code = invite.read(cx).text().trim().to_string();
                    if code.is_empty() {
                        return;
                    }
                    self.run_dialog(
                        async move { session.join(&code).await },
                        |this, view, window, cx| {
                            let text = fill(this.t().joined, &[&view.guild.name]);
                            this.show_toast(text, None, cx);
                            this.select_guild(&view.guild.id, window, cx);
                        },
                        window,
                        cx,
                    );
                } else {
                    let name = name.read(cx).text().trim().to_string();
                    if name.is_empty() {
                        return;
                    }
                    let languages = languages.clone();
                    let channel_name = "general".to_string();
                    self.run_dialog(
                        async move {
                            let view = session.create_guild(&name, None).await?;
                            let policy = &view.policy;
                            let mut languages = languages;
                            languages.truncate(policy.max_languages.max(0) as usize);
                            let llm: Vec<String> = languages
                                .iter()
                                .take(policy.max_llm_languages.max(0) as usize)
                                .cloned()
                                .collect();
                            if !languages.is_empty() {
                                session
                                    .set_languages(LanguagePolicyUpdate {
                                        guild_id: view.guild.id.clone(),
                                        languages,
                                        llm_languages: llm,
                                        region_mode: policy.region_mode,
                                        aliases: policy.aliases.clone(),
                                        translate_regional_variants: policy
                                            .translate_regional_variants,
                                        model: policy.model.clone(),
                                    })
                                    .await?;
                            }
                            let channel = session
                                .create_channel(&view.guild.id, &channel_name, None)
                                .await?;
                            Ok((view.guild.id, channel.id))
                        },
                        |this, (guild_id, channel_id), window, cx| {
                            this.channel_of.insert(guild_id.clone(), channel_id.clone());
                            this.select_guild(&guild_id, window, cx);
                            this.open_channel(&channel_id, window, cx);
                        },
                        window,
                        cx,
                    );
                }
            }
            Dialog::Channel {
                target,
                name,
                topic,
                ..
            } => {
                let name = name.read(cx).text().trim().to_string();
                let topic = topic
                    .as_ref()
                    .map(|topic| topic.read(cx).text().trim().to_string())
                    .filter(|topic| !topic.is_empty());
                if name.is_empty() {
                    return;
                }
                let target = target.clone();
                self.run_dialog(
                    async move {
                        match target {
                            ChannelTarget::Create { guild_id } => {
                                session
                                    .create_channel(&guild_id, &name, topic.as_deref())
                                    .await
                            }
                            ChannelTarget::Edit { channel_id, .. } => {
                                session
                                    .update_channel(&channel_id, &name, topic.as_deref())
                                    .await
                            }
                            ChannelTarget::Thread {
                                channel_id,
                                message_id,
                            } => session.start_thread(&channel_id, &message_id, &name).await,
                        }
                    },
                    |this, channel, window, cx| this.open_channel(&channel.id, window, cx),
                    window,
                    cx,
                );
            }
            Dialog::Guild {
                guild_id,
                name,
                description,
                ..
            } => {
                let guild_id = guild_id.clone();
                let name = name.read(cx).text().trim().to_string();
                let description = description.read(cx).text().trim().to_string();
                if name.is_empty() {
                    return;
                }
                self.run_dialog(
                    async move {
                        let description = (!description.is_empty()).then_some(description);
                        session
                            .update_guild(&guild_id, &name, description.as_deref())
                            .await
                    },
                    |_, _, _, _| {},
                    window,
                    cx,
                );
            }
            Dialog::Languages {
                guild_id,
                languages,
                llm,
                model,
                ..
            } => {
                let Some(guild) = self.session.guild(guild_id) else {
                    return;
                };
                let policy = guild.policy;
                let update = LanguagePolicyUpdate {
                    guild_id: guild_id.clone(),
                    languages: languages.clone(),
                    llm_languages: llm.clone(),
                    region_mode: policy.region_mode,
                    aliases: policy.aliases,
                    translate_regional_variants: policy.translate_regional_variants,
                    model: model.clone(),
                };
                self.run_dialog(
                    async move { session.set_languages(update).await },
                    |this, _, _, _| this.list.remeasure(),
                    window,
                    cx,
                );
            }
            Dialog::Settings {
                locale,
                show_original,
                translate_outside_set,
                ..
            } => {
                let settings = UserSettings {
                    locale: locale.read(cx).text().trim().to_string(),
                    show_original: *show_original,
                    translate_outside_set: *translate_outside_set,
                };
                self.run_dialog(
                    async move { session.set_settings(settings).await },
                    |this, _, window, cx| {
                        this.list.remeasure();
                        // The placeholder is in the UI language.
                        if let Some(channel) = this
                            .channel
                            .as_deref()
                            .and_then(|id| this.session.channel(id))
                        {
                            let placeholder = fill(this.t().message_placeholder, &[&channel.name]);
                            this.composer
                                .update(cx, |field, cx| field.set_placeholder(placeholder, cx));
                        }
                        let _ = window;
                    },
                    window,
                    cx,
                );
            }
            Dialog::Password {
                current,
                new,
                again,
                ..
            } => {
                let current = current.read(cx).text().to_string();
                let new = new.read(cx).text().to_string();
                if new != again.read(cx).text().as_ref() {
                    let text = self.t().passwords_differ.to_string();
                    set_status(&mut self.dialog, false, Some(text));
                    cx.notify();
                    return;
                }
                let client = session.client().clone();
                self.run_dialog(
                    async move { client.change_password(&current, &new).await },
                    |this, token, _, cx| {
                        cx.emit(super::ChatEvent::TokenRenewed { token });
                        let text = this.t().password_changed;
                        this.show_toast(text.to_string(), None, cx);
                    },
                    window,
                    cx,
                );
            }
            Dialog::Confirm { action, .. } => match action {
                Confirm::Leave { guild_id } => {
                    let guild_id = guild_id.clone();
                    self.run_dialog(
                        async move { session.leave(&guild_id).await },
                        |this, _, window, cx| this.ensure_selection(window, cx),
                        window,
                        cx,
                    );
                }
                Confirm::Remove {
                    guild_id,
                    user_id,
                    ban,
                } => {
                    let (guild_id, user_id, ban) = (guild_id.clone(), user_id.clone(), *ban);
                    self.run_dialog(
                        async move { session.remove_member(&guild_id, &user_id, ban).await },
                        |_, _, _, _| {},
                        window,
                        cx,
                    );
                }
                Confirm::Delete {
                    channel_id,
                    message_id,
                } => {
                    let (channel_id, message_id) = (channel_id.clone(), message_id.clone());
                    self.run_dialog(
                        async move { session.delete(&channel_id, &message_id).await },
                        |_, _, _, _| {},
                        window,
                        cx,
                    );
                }
                Confirm::DeleteChannel { channel_id } => {
                    let channel_id = channel_id.clone();
                    self.run_dialog(
                        async move { session.delete_channel(&channel_id).await },
                        |this, _, window, cx| this.ensure_selection(window, cx),
                        window,
                        cx,
                    );
                }
                Confirm::DeleteGuild { guild_id } => {
                    let guild_id = guild_id.clone();
                    self.run_dialog(
                        async move { session.delete_guild(&guild_id).await },
                        |this, _, window, cx| this.ensure_selection(window, cx),
                        window,
                        cx,
                    );
                }
                Confirm::SignOutOthers => {
                    let client = session.client().clone();
                    self.run_dialog(
                        async move { client.sign_out_others().await },
                        |this, token, _, cx| {
                            cx.emit(super::ChatEvent::TokenRenewed { token });
                            let text = this.t().signed_out_others;
                            this.show_toast(text.to_string(), None, cx);
                        },
                        window,
                        cx,
                    );
                }
            },
            Dialog::Invite { .. } | Dialog::Notifications { .. } => self.dialog = None,
        }
    }

    fn set_notify_level(
        &mut self,
        guild_id: String,
        channel_id: Option<String>,
        level: Option<NotifyLevel>,
        cx: &mut Context<Self>,
    ) {
        let Some(mut settings) = self.session.notification_settings(&guild_id) else {
            return;
        };
        match (channel_id, level) {
            (None, Some(level)) => settings.level = level,
            (None, None) => return,
            (Some(channel_id), level) => {
                settings
                    .channels
                    .retain(|entry| entry.channel_id != channel_id);
                if let Some(level) = level {
                    settings.channels.push(ChannelNotify { channel_id, level });
                }
            }
        }
        let session = self.session.clone();
        let task = runtime::spawn(cx, async move { session.set_notifications(settings).await });
        cx.spawn(async move |this, cx| {
            if let Err(failure) = task.await {
                let _ = this.update(cx, |this, cx| {
                    let text = describe(&failure, this.t());
                    this.show_toast(text, None, cx);
                });
            }
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    pub(super) fn render_dialog(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let _ = window;
        let t = self.t();
        let colors = cx.md().colors;
        let Some(dialog) = &self.dialog else {
            return div().into_any_element();
        };
        let dismiss = {
            let this = cx.entity().downgrade();
            move |_: &mut Window, cx: &mut App| {
                let _ = this.update(cx, |this: &mut ChatView, cx| {
                    let busy = matches!(
                        &this.dialog,
                        Some(Dialog::AddGuild { busy: true, .. })
                            | Some(Dialog::Channel { busy: true, .. })
                            | Some(Dialog::Guild { busy: true, .. })
                            | Some(Dialog::Languages { busy: true, .. })
                            | Some(Dialog::Settings { busy: true, .. })
                            | Some(Dialog::Password { busy: true, .. })
                            | Some(Dialog::Confirm { busy: true, .. })
                    );
                    if !busy {
                        this.dialog = None;
                        cx.notify();
                    }
                });
            }
        };
        let cancel = |label: &'static str, cx: &mut Context<Self>| {
            Button::new("dialog-cancel", label)
                .style(ButtonStyle::Text)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.dialog = None;
                    cx.notify();
                }))
                .into_any_element()
        };
        let confirm = |label: &'static str, busy: bool, cx: &mut Context<Self>| {
            if busy {
                LoadingIndicator::new("dialog-busy")
                    .size(px(40.0))
                    .into_any_element()
            } else {
                Button::new("dialog-confirm", label)
                    .on_click(cx.listener(|this, _, window, cx| this.confirm_dialog(window, cx)))
                    .into_any_element()
            }
        };
        let error_line = |error: &Option<String>, cx: &App| {
            error.clone().map(|error| {
                div()
                    .text_color(colors.error)
                    .md_type(TypeRole::BodyMedium, cx)
                    .child(error)
            })
        };

        match dialog {
            Dialog::AddGuild {
                join,
                name,
                invite,
                languages,
                busy,
                error,
            } => {
                let join = *join;
                let tabs = div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        Chip::new("tab-create", t.create_guild)
                            .kind(ChipKind::Filter)
                            .selected(!join)
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Some(Dialog::AddGuild { join, name, .. }) = &mut this.dialog
                                {
                                    *join = false;
                                    window.focus(&name.focus_handle(cx), cx);
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Chip::new("tab-join", t.join_guild)
                            .kind(ChipKind::Filter)
                            .selected(join)
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Some(Dialog::AddGuild { join, invite, .. }) =
                                    &mut this.dialog
                                {
                                    *join = true;
                                    window.focus(&invite.focus_handle(cx), cx);
                                }
                                cx.notify();
                            })),
                    );
                let mut body = div().flex().flex_col().gap_4().child(tabs);
                if join {
                    body = body.child(invite.clone());
                } else {
                    let mut chips = div().flex().flex_row().flex_wrap().gap_2();
                    for code in COMMON_LANGUAGES {
                        let selected = languages.iter().any(|tag| tag == code);
                        chips = chips.child(
                            Chip::new(
                                SharedString::from(format!("lang-{code}")),
                                language_name(code, t),
                            )
                            .kind(ChipKind::Filter)
                            .selected(selected)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if let Some(Dialog::AddGuild { languages, .. }) =
                                        &mut this.dialog
                                    {
                                        if let Some(ix) =
                                            languages.iter().position(|tag| tag == code)
                                        {
                                            languages.remove(ix);
                                        } else {
                                            languages.push(code.to_string());
                                        }
                                    }
                                    cx.notify();
                                },
                            )),
                        );
                    }
                    body = body
                        .child(name.clone())
                        .child(
                            div()
                                .md_type(TypeRole::TitleSmall, cx)
                                .child(t.guild_languages),
                        )
                        .child(widgets::supporting(t.guild_languages_hint, cx))
                        .child(chips);
                }
                let body = body.children(error_line(error, cx));
                let label = if join { t.join } else { t.create };
                widgets::dialog(
                    "add-guild",
                    t.add_guild,
                    body,
                    vec![cancel(t.cancel, cx), confirm(label, *busy, cx)],
                    dismiss,
                    cx,
                )
            }
            Dialog::Invite {
                guild_id,
                code,
                copied,
                error,
            } => {
                let name = self
                    .session
                    .guild(guild_id)
                    .map(|guild| guild.guild.name)
                    .unwrap_or_default();
                let mut body = div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(widgets::supporting(t.invite_body, cx));
                match code {
                    Some(code) => {
                        let copy_code = code.clone();
                        body = body.child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .p_2()
                                .pl_4()
                                .rounded(px(16.0))
                                .bg(colors.surface_container_highest)
                                .child(
                                    div()
                                        .id("invite-code")
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_x_scroll()
                                        .md_type(TypeRole::BodyMedium, cx)
                                        .child(code.clone()),
                                )
                                .child(
                                    Button::new(
                                        "copy-invite",
                                        if *copied { t.copied } else { t.copy },
                                    )
                                    .style(ButtonStyle::Tonal)
                                    .icon(Icon::ContentCopy)
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            cx.write_to_clipboard(ClipboardItem::new_string(
                                                copy_code.clone(),
                                            ));
                                            if let Some(Dialog::Invite { copied, .. }) =
                                                &mut this.dialog
                                            {
                                                *copied = true;
                                            }
                                            cx.notify();
                                        },
                                    )),
                                ),
                        );
                    }
                    None if error.is_none() => {
                        body = body.child(LoadingIndicator::new("invite-loading").size(px(40.0)));
                    }
                    None => {}
                }
                let body = body.children(error_line(error, cx));
                widgets::dialog(
                    "invite",
                    format!("{} — {name}", t.invite_people),
                    body,
                    vec![confirm(t.close, false, cx)],
                    dismiss,
                    cx,
                )
            }
            Dialog::Channel {
                target,
                name,
                topic,
                busy,
                error,
            } => {
                let body = div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(name.clone())
                    .children(topic.clone())
                    .children(error_line(error, cx));
                let (title, action) = match target {
                    ChannelTarget::Create { .. } => (t.create_channel, t.create),
                    ChannelTarget::Edit { thread: false, .. } => (t.edit_channel, t.save),
                    ChannelTarget::Edit { thread: true, .. } => (t.edit_thread, t.save),
                    ChannelTarget::Thread { .. } => (t.start_thread, t.create),
                };
                let mut actions = Vec::new();
                if let ChannelTarget::Edit { channel_id, .. } = target {
                    let channel_id = channel_id.clone();
                    actions.push(destructive(
                        "delete-channel",
                        t.delete_channel,
                        move |this, _, cx| {
                            this.dialog = Some(Dialog::delete_channel(this, &channel_id));
                            cx.notify();
                        },
                        cx,
                    ));
                }
                actions.extend([cancel(t.cancel, cx), confirm(action, *busy, cx)]);
                widgets::dialog("channel", title, body, actions, dismiss, cx)
            }
            Dialog::Guild {
                guild_id,
                name,
                description,
                busy,
                error,
            } => {
                let body = div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(name.clone())
                    .child(description.clone())
                    .children(error_line(error, cx));
                let guild_id = guild_id.clone();
                let delete = destructive(
                    "delete-guild",
                    t.delete_guild,
                    move |this, _, cx| {
                        this.dialog = Some(Dialog::delete_guild(this, &guild_id));
                        cx.notify();
                    },
                    cx,
                );
                widgets::dialog(
                    "guild",
                    t.edit_guild,
                    body,
                    vec![delete, cancel(t.cancel, cx), confirm(t.save, *busy, cx)],
                    dismiss,
                    cx,
                )
            }
            Dialog::Languages {
                guild_id,
                languages,
                llm,
                models,
                model,
                busy,
                error,
            } => {
                let Some(guild) = self.session.guild(guild_id) else {
                    return div().into_any_element();
                };
                let owner = self.is_owner(&guild);
                let policy = &guild.policy;
                let view = &guild.language;
                let you_read = if view.llm {
                    t.you_read_server
                } else if view.client_translation {
                    t.you_read_device
                } else {
                    t.you_read_original
                };
                // The guild's tags are normalized ("ja-JP"): a common
                // language it has shows as its tag, not a second chip.
                let mut options: Vec<String> = COMMON_LANGUAGES
                    .iter()
                    .map(|common| {
                        languages
                            .iter()
                            .find(|tag| tag.split('-').next() == Some(*common))
                            .cloned()
                            .unwrap_or_else(|| common.to_string())
                    })
                    .collect();
                for tag in languages {
                    if !options.iter().any(|option| option == tag) {
                        options.push(tag.clone());
                    }
                }
                let mut chips = div().flex().flex_row().flex_wrap().gap_2();
                for tag in options {
                    let selected = languages.contains(&tag);
                    if !owner && !selected {
                        continue;
                    }
                    let id = tag.clone();
                    chips = chips.child(
                        Chip::new(
                            SharedString::from(format!("policy-{tag}")),
                            language_name(&tag, t),
                        )
                        .kind(ChipKind::Filter)
                        .selected(selected)
                        .disabled(!owner)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(Dialog::Languages { languages, llm, .. }) = &mut this.dialog
                            {
                                if let Some(ix) = languages.iter().position(|tag| *tag == id) {
                                    languages.remove(ix);
                                    llm.retain(|tag| *tag != id);
                                } else {
                                    languages.push(id.clone());
                                }
                            }
                            cx.notify();
                        })),
                    );
                }
                let mut body = div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(widgets::supporting(
                        fill(you_read, &[&language_name(&view.effective, t)]),
                        cx,
                    ))
                    .child(
                        div()
                            .md_type(TypeRole::TitleSmall, cx)
                            .child(t.languages_summary),
                    )
                    .child(chips);
                if policy.max_llm_languages > 0 {
                    let mut ai = div().flex().flex_row().flex_wrap().gap_2();
                    for tag in languages.clone() {
                        let selected = llm.contains(&tag);
                        if !owner && !selected {
                            continue;
                        }
                        let id = tag.clone();
                        ai = ai.child(
                            Chip::new(
                                SharedString::from(format!("llm-{tag}")),
                                language_name(&tag, t),
                            )
                            .kind(ChipKind::Filter)
                            .icon(Icon::AutoAwesome)
                            .selected(selected)
                            .disabled(!owner)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if let Some(Dialog::Languages { llm, .. }) = &mut this.dialog {
                                        if let Some(ix) = llm.iter().position(|tag| *tag == id) {
                                            llm.remove(ix);
                                        } else {
                                            llm.push(id.clone());
                                        }
                                    }
                                    cx.notify();
                                },
                            )),
                        );
                    }
                    body = body
                        .child(
                            div()
                                .md_type(TypeRole::TitleSmall, cx)
                                .child(t.ai_translation),
                        )
                        .child(ai);
                    if models.len() > 1 {
                        // The first model is the default; choosing it means
                        // following the default.
                        let current = model.as_deref().unwrap_or(&models[0].id);
                        let mut choices = div().flex().flex_row().flex_wrap().gap_2();
                        for (index, option) in models.iter().enumerate() {
                            let selected = option.id == current;
                            if !owner && !selected {
                                continue;
                            }
                            let label = if index == 0 {
                                fill(t.default_model, &[&option.name])
                            } else {
                                option.name.clone()
                            };
                            let choice = (index > 0).then(|| option.id.clone());
                            choices = choices.child(
                                Chip::new(
                                    SharedString::from(format!("model-{}", option.id)),
                                    label,
                                )
                                .kind(ChipKind::Filter)
                                .selected(selected)
                                .disabled(!owner)
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        if let Some(Dialog::Languages { model, .. }) =
                                            &mut this.dialog
                                        {
                                            *model = choice.clone();
                                        }
                                        cx.notify();
                                    },
                                )),
                            );
                        }
                        body = body
                            .child(div().md_type(TypeRole::TitleSmall, cx).child(t.ai_model))
                            .child(choices);
                    }
                } else {
                    body = body.child(widgets::supporting(t.languages_free, cx));
                }
                body = body.child(widgets::supporting(
                    fill(
                        t.languages_limit,
                        &[
                            &policy.plan,
                            &policy.max_languages.to_string(),
                            &policy.max_llm_languages.to_string(),
                        ],
                    ),
                    cx,
                ));
                let body = body.children(error_line(error, cx));
                let actions = if owner {
                    vec![cancel(t.cancel, cx), confirm(t.save, *busy, cx)]
                } else {
                    vec![confirm(t.close, false, cx)]
                };
                widgets::dialog("languages", t.guild_languages, body, actions, dismiss, cx)
            }
            Dialog::Notifications {
                guild_id,
                channel_id,
            } => {
                let settings = self.session.notification_settings(guild_id);
                let guild_level = settings
                    .as_ref()
                    .map_or(NotifyLevel::All, |settings| settings.level);
                let channel_level = channel_id.as_ref().and_then(|channel_id| {
                    settings.as_ref().and_then(|settings| {
                        settings
                            .channels
                            .iter()
                            .find(|entry| entry.channel_id == *channel_id)
                            .map(|entry| entry.level)
                    })
                });
                let levels = [
                    (NotifyLevel::All, t.notify_all),
                    (NotifyLevel::Mentions, t.notify_mentions),
                    (NotifyLevel::None, t.notify_none),
                ];
                let radio_row = |id: SharedString,
                                 label: &'static str,
                                 selected: bool,
                                 handler: Box<dyn Fn(&mut ChatView, &mut Context<ChatView>)>,
                                 cx: &mut Context<Self>| {
                    div()
                        .id(id.clone())
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| handler(this, cx)))
                        .child(RadioButton::new(id, selected))
                        .child(label)
                };
                let mut body = div().flex().flex_col().gap_1().child(
                    div()
                        .md_type(TypeRole::TitleSmall, cx)
                        .child(t.notify_guild),
                );
                for (level, label) in levels {
                    let guild_id = guild_id.clone();
                    let selected = guild_level == level;
                    body = body.child(radio_row(
                        SharedString::from(format!("notify-guild-{label}")),
                        label,
                        selected,
                        Box::new(move |this, cx| {
                            this.set_notify_level(guild_id.clone(), None, Some(level), cx)
                        }),
                        cx,
                    ));
                }
                if let Some(channel_id) = channel_id
                    && let Some(channel) = self.session.channel(channel_id)
                {
                    body = body.child(
                        div()
                            .pt_3()
                            .md_type(TypeRole::TitleSmall, cx)
                            .child(fill(t.notify_channel, &[&channel.name])),
                    );
                    let mut choices: Vec<(Option<NotifyLevel>, &'static str)> =
                        vec![(None, t.notify_inherit)];
                    choices.extend(levels.iter().map(|(level, label)| (Some(*level), *label)));
                    for (level, label) in choices {
                        let guild_id = guild_id.clone();
                        let channel_id = channel_id.clone();
                        let selected = channel_level == level;
                        body = body.child(radio_row(
                            SharedString::from(format!("notify-channel-{label}")),
                            label,
                            selected,
                            Box::new(move |this, cx| {
                                this.set_notify_level(
                                    guild_id.clone(),
                                    Some(channel_id.clone()),
                                    level,
                                    cx,
                                )
                            }),
                            cx,
                        ));
                    }
                }
                widgets::dialog(
                    "notifications",
                    t.notifications,
                    body,
                    vec![confirm(t.close, false, cx)],
                    dismiss,
                    cx,
                )
            }
            Dialog::Settings {
                locale,
                show_original,
                translate_outside_set,
                busy,
                error,
            } => {
                let switch_row =
                    |id: &'static str, label: &'static str, on: bool, cx: &mut Context<Self>| {
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .child(div().flex_1().child(label))
                            .child(Switch::new(id, on).on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if let Some(Dialog::Settings {
                                        show_original,
                                        translate_outside_set,
                                        ..
                                    }) = &mut this.dialog
                                    {
                                        let flag = if id == "show-original" {
                                            show_original
                                        } else {
                                            translate_outside_set
                                        };
                                        *flag = !*flag;
                                    }
                                    cx.notify();
                                },
                            )))
                    };
                let name = self.session.user_name().to_string();
                let body = div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .child(widgets::avatar(&name, 40.0, cx))
                            .child(
                                div()
                                    .flex_1()
                                    .md_type(TypeRole::TitleMedium, cx)
                                    .child(name),
                            )
                            .child(
                                Button::new("sign-out", t.sign_out)
                                    .style(ButtonStyle::Outlined)
                                    .icon(Icon::Logout)
                                    .on_click(cx.listener(|this, _, _, cx| this.sign_out(cx))),
                            ),
                    )
                    .child(locale.clone())
                    .child(switch_row(
                        "show-original",
                        t.show_originals_setting,
                        *show_original,
                        cx,
                    ))
                    .child(switch_row(
                        "translate-outside",
                        t.translate_outside_setting,
                        *translate_outside_set,
                        cx,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .flex_wrap()
                            .gap_2()
                            .child(
                                Button::new("change-password", t.change_password)
                                    .style(ButtonStyle::Outlined)
                                    .icon(Icon::Lock)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.dialog = Some(Dialog::password(this, window, cx));
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("sign-out-others", t.sign_out_others)
                                    .style(ButtonStyle::Outlined)
                                    .icon(Icon::Logout)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.dialog =
                                            Some(Dialog::sign_out_others(this, window, cx));
                                        cx.notify();
                                    })),
                            ),
                    )
                    .children(error_line(error, cx));
                widgets::dialog(
                    "settings",
                    t.settings,
                    body,
                    vec![cancel(t.cancel, cx), confirm(t.save, *busy, cx)],
                    dismiss,
                    cx,
                )
            }
            Dialog::Password {
                current,
                new,
                again,
                busy,
                error,
            } => {
                let body = div()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(current.clone())
                    .child(new.clone())
                    .child(again.clone())
                    .children(error_line(error, cx));
                widgets::dialog(
                    "password",
                    t.change_password,
                    body,
                    vec![cancel(t.cancel, cx), confirm(t.change_password, *busy, cx)],
                    dismiss,
                    cx,
                )
            }
            Dialog::Confirm {
                title,
                body,
                action_label,
                busy,
                error,
                ..
            } => {
                let body = div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(widgets::supporting(body.clone(), cx))
                    .children(error_line(error, cx));
                widgets::dialog(
                    "confirm",
                    title.clone(),
                    body,
                    vec![cancel(t.cancel, cx), confirm(action_label, *busy, cx)],
                    dismiss,
                    cx,
                )
            }
        }
    }
}
