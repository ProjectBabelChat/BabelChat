//! The chat loop: commands in, messages out, one line at a time.
//!
//! The open channel prints its messages once their translation has
//! settled, so each appears once in the reader's language. One still
//! translating after [`SETTLE_WAIT`] prints as it is and its translation
//! follows on a `↳` line. Other channels speak up only through
//! notifications (`🔔`), as the guild's notification settings say.
//! Messages get short numbers (`#3`) for `/reply`, `/edit`, `/delete`,
//! `/react` and `/thread`. A line ending in `\` goes on on the next one,
//! for messages of several lines.

use std::{
    collections::{HashMap, HashSet},
    io::Write,
    time::{Duration, Instant},
};

use babel_sdk::{
    ChannelKind, ChannelNotify, ChatMessage, GuildChange, GuildView, LanguagePolicyUpdate,
    MessageChange, Notification, NotifyLevel, NotifyReason, RegionMode, Session, Shown, Unread,
    Update,
};

/// How long a new message may wait for its translation before printing.
pub const SETTLE_WAIT: Duration = Duration::from_secs(3);
const PAGE: i64 = 20;

#[derive(Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Quit,
}

pub struct Repl {
    session: Session,
    out: Box<dyn Write + Send>,
    guild: Option<String>,
    channel: Option<String>,
    /// Number → (channel, message), and back.
    numbers: Vec<(String, String)>,
    number_of: HashMap<String, usize>,
    /// Text last printed per message.
    printed: HashMap<String, String>,
    /// New messages waiting for their translation.
    waiting: Vec<(String, String, Instant)>,
    /// Language and notification summaries last shown per guild.
    languages: HashMap<String, String>,
    notifications: HashMap<String, String>,
    /// Channels whose history has been loaded.
    opened: HashSet<String>,
    /// Names of guilds, channels and people seen, for when they are gone.
    names: HashMap<String, String>,
    /// Lines of a message still being typed (each ended in `\`).
    continued: String,
    /// Who was last shown writing in the open channel.
    writing: Vec<String>,
}

pub const HELP: &str = "\
Type to send to the current channel; @name notifies someone. End a line
with \\ to go on on the next one. Commands:
  /guilds                      list your guilds          /guild <n|name>   switch guild
  /channels                    list channels             /open <n|name>    switch channel
  /unread                      unread channels           /read             mark this channel read
  /more                        older messages            /members          who is here
  /reply <#n> <text>           answer a message          /thread <#n> <name>  thread from it
  /edit <#n> <text>            /delete <#n>
  /react <#n> <emoji>          react (👍 or :thumbsup:)  /unreact <#n> <emoji>
  /create <name>               create a guild            /channel <name>   create a channel
  /rename <name>               rename this channel       /topic [text]     its topic
  /delete-channel <name>       delete this channel, its threads and messages
                               (channels: the owner, or a thread's creator)
  /rename-guild <name>         rename this guild         /describe [text]  its description
  /delete-guild <name>         delete this guild and everything in it (owner)
  /invite [days] [uses]        invite code (0 days = never expires)
  /invites                     list invites              /revoke <code>    revoke one
  /join <code>                 join with an invite code  /leave            leave this guild
  /kick <name>  /ban <name>  /unban <name>  /bans        (owner)
  /notify [all|mentions|none] [here]                     what notifies you (here: this channel)
  /languages                   show the guild's languages
  /languages <tags..> [llm <tags..>] [distinct]     set them (owner)
  /model [<id>|default]       what the server translates with; choose one (owner)
  /locale <tag>                your language             /original on|off  show originals too
  /outside on|off              translate languages the guild does not declare, on device
  /status                      connection                /quit";

impl Repl {
    pub fn new(session: Session, out: Box<dyn Write + Send>) -> Self {
        Self {
            session,
            out,
            guild: None,
            channel: None,
            numbers: Vec::new(),
            number_of: HashMap::new(),
            printed: HashMap::new(),
            waiting: Vec::new(),
            opened: HashSet::new(),
            languages: HashMap::new(),
            notifications: HashMap::new(),
            names: HashMap::new(),
            continued: String::new(),
            writing: Vec::new(),
        }
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    fn say(&mut self, text: impl AsRef<str>) {
        let _ = writeln!(self.out, "{}", text.as_ref());
        let _ = self.out.flush();
    }

    /// Greets the user and opens their first guild.
    pub async fn start(&mut self) {
        let settings = self.session.settings();
        for view in self.session.guilds() {
            self.remember(&view);
        }
        self.say(format!(
            "Signed in as {} (reading {}). /help for commands.",
            self.session.user_name(),
            settings.locale
        ));
        match self.session.guilds().first() {
            Some(view) => {
                let id = view.guild.id.clone();
                self.switch_guild(&id).await;
            }
            None => self.say("You are in no guild yet: /create <name>, or /join <invite code>."),
        }
    }

    pub async fn line(&mut self, line: &str) -> Flow {
        let line = line.trim_end();
        if let Some(start) = line.strip_suffix('\\') {
            self.continued.push_str(start);
            self.continued.push('\n');
            return Flow::Continue;
        }
        let line = std::mem::take(&mut self.continued) + line;
        let line = line.trim();
        if line.is_empty() {
            return Flow::Continue;
        }
        let Some(command) = line.strip_prefix('/') else {
            self.send(line).await;
            return Flow::Continue;
        };
        let (name, rest) = command.split_once(' ').unwrap_or((command, ""));
        let rest = rest.trim();
        match name {
            "help" | "h" => self.say(HELP),
            "quit" | "q" | "exit" => return Flow::Quit,
            "guilds" => self.list_guilds(),
            "guild" | "g" => match self.find_guild(rest) {
                Some(id) => self.switch_guild(&id).await,
                None => self.say("No such guild. /guilds lists them."),
            },
            "channels" => self.list_channels(),
            "open" | "c" => match self.find_channel(rest) {
                Some(id) => self.open(&id).await,
                None => self.say("No such channel. /channels lists them."),
            },
            "more" => self.more().await,
            "members" => self.members(),
            "create" if !rest.is_empty() => self.create_guild(rest).await,
            "channel" if !rest.is_empty() => self.create_channel(rest).await,
            "thread" => self.thread(rest).await,
            "invite" => self.invite(rest).await,
            "join" if !rest.is_empty() => self.join(rest).await,
            "languages" | "langs" => self.languages(rest).await,
            "model" => self.model(rest).await,
            "locale" if !rest.is_empty() => {
                let locale = rest.to_string();
                self.settings(|settings| settings.locale = locale).await
            }
            "original" => match on_off(rest) {
                Some(on) => self.settings(|settings| settings.show_original = on).await,
                None => self.say("/original on|off"),
            },
            "outside" => match on_off(rest) {
                Some(on) => {
                    self.settings(|settings| settings.translate_outside_set = on)
                        .await
                }
                None => self.say("/outside on|off"),
            },
            "edit" => self.edit(rest).await,
            "react" => self.react(rest, true).await,
            "unreact" => self.react(rest, false).await,
            "rename" if !rest.is_empty() => self.rename_channel(Some(rest), None).await,
            "topic" => self.rename_channel(None, Some(rest)).await,
            "delete-channel" => self.delete_channel(rest).await,
            "rename-guild" if !rest.is_empty() => self.rename_guild(Some(rest), None).await,
            "describe" => self.rename_guild(None, Some(rest)).await,
            "delete-guild" => self.delete_guild(rest).await,
            "delete" => self.delete(rest).await,
            "reply" | "r" => self.reply(rest).await,
            "unread" => self.unread(),
            "read" => self.mark_read().await,
            "invites" => self.invites().await,
            "revoke" if !rest.is_empty() => match self.session.revoke_invite(rest).await {
                Ok(()) => self.say("Revoked."),
                Err(error) => self.say(format!("! {error}")),
            },
            "leave" => self.leave().await,
            "kick" if !rest.is_empty() => self.remove(rest, false).await,
            "ban" if !rest.is_empty() => self.remove(rest, true).await,
            "unban" if !rest.is_empty() => self.unban(rest).await,
            "bans" => self.bans().await,
            "notify" => self.notify(rest).await,
            "status" => self.status(),
            _ => self.say("Unknown command or missing argument. /help lists commands."),
        }
        Flow::Continue
    }

    /// Reacts to the session. `Quit` when signed out.
    pub fn update(&mut self, update: Update) -> Flow {
        match update {
            Update::Message {
                channel_id,
                message_id,
                change,
                ..
            } => self.message_update(&channel_id, &message_id, change),
            Update::Guild { guild_id, change } => self.guild_update(&guild_id, change),
            Update::Notification(notification) => self.notification(&notification),
            // Shown on request (/unread, /channels).
            Update::Unread { .. } => {}
            Update::Typing { channel_id, .. } => self.say_writing(&channel_id),
            Update::Offline { error } => self.say(format!("! offline ({error}); retrying")),
            Update::Online { reloaded } => self.say(if reloaded {
                "! in sync with the server"
            } else {
                "! back online"
            }),
            Update::SignedOut => {
                self.say("! signed out: the server rejected the token. Run `babel login`.");
                return Flow::Quit;
            }
        }
        Flow::Continue
    }

    /// Prints messages that waited long enough for a translation.
    pub fn tick(&mut self) {
        let due = self
            .waiting
            .iter()
            .filter(|(_, _, since)| since.elapsed() >= SETTLE_WAIT)
            .map(|(channel, message, _)| (channel.clone(), message.clone()))
            .collect::<Vec<_>>();
        for (channel, message) in due {
            self.waiting.retain(|(_, id, _)| *id != message);
            if let Some(message) = self.session.message(&channel, &message) {
                self.print(&message, "");
            }
        }
    }

    // ---- Messages -----------------------------------------------------------

    fn message_update(&mut self, channel: &str, id: &str, change: MessageChange) {
        // Other channels speak up through notifications.
        if self.channel.as_deref() != Some(channel) && !self.printed.contains_key(id) {
            return;
        }
        if change == MessageChange::Deleted {
            self.waiting.retain(|(_, waiting, _)| waiting != id);
            if let Some(number) = self.number_of.get(id) {
                let label = self.channel_label(channel);
                self.say(format!("{label} #{number} deleted"));
            }
            return;
        }
        let Some(message) = self.session.message(channel, id) else {
            return;
        };
        let waiting = self.waiting.iter().any(|(_, waiting, _)| waiting == id);
        match change {
            MessageChange::Created if self.printed.contains_key(id) || waiting => {}
            MessageChange::Created if !message.display.is_settled() => {
                self.waiting
                    .push((channel.to_string(), id.to_string(), Instant::now()));
            }
            MessageChange::Created => self.print(&message, ""),
            MessageChange::Edited => {
                self.waiting.retain(|(_, waiting, _)| waiting != id);
                self.print(&message, "✎ ");
            }
            MessageChange::Translated if waiting => {
                if message.display.is_settled() {
                    self.waiting.retain(|(_, waiting, _)| waiting != id);
                    self.print(&message, "");
                }
            }
            MessageChange::Translated => {
                let changed = self
                    .printed
                    .get(id)
                    .is_some_and(|text| *text != message.display.text);
                if changed && message.display.is_settled() {
                    self.print(&message, "  ↳ ");
                }
            }
            MessageChange::Reactions if self.printed.contains_key(id) => {
                self.say_reactions(&message)
            }
            MessageChange::Reactions | MessageChange::Deleted => {}
        }
    }

    fn number(&mut self, channel: &str, message: &str) -> usize {
        if let Some(number) = self.number_of.get(message) {
            return *number;
        }
        self.numbers
            .push((channel.to_string(), message.to_string()));
        let number = self.numbers.len();
        self.number_of.insert(message.to_string(), number);
        number
    }

    fn print(&mut self, message: &ChatMessage, lead: &str) {
        let m = &message.message;
        let number = self.number(&m.channel_id, &m.id);
        let label = self.channel_label(&m.channel_id);
        let replying = match &m.reply_to {
            Some(id) => match self.number_of.get(id) {
                Some(number) => format!(" ↪#{number}"),
                None => " ↪".to_string(),
            },
            None => String::new(),
        };
        // Lines after the first line up under it.
        let mut text = format!(
            "{lead}{label} #{number} {}{replying}: {}{}{}",
            m.author_name,
            message.display.text.replace('\n', "\n      "),
            marker(&message.display.shown),
            reactions_text(message)
        );
        if message.display.is_translated() && self.session.settings().show_original {
            text.push_str(&format!("\n      ┆ {}", m.text.replace('\n', "\n      ┆ ")));
        }
        self.printed
            .insert(m.id.clone(), message.display.text.clone());
        self.say(text);
    }

    fn notification(&mut self, notification: &Notification) {
        let message = &notification.message;
        if self.channel.as_deref() == Some(message.message.channel_id.as_str())
            || self.printed.contains_key(&message.message.id)
        {
            return;
        }
        let why = match notification.reason {
            NotifyReason::Mention => "mention",
            NotifyReason::Reply => "reply",
            NotifyReason::Message => "new",
        };
        self.print(message, &format!("🔔 {why} "));
    }

    async fn send(&mut self, text: &str) {
        let Some(channel) = self.channel.clone() else {
            self.say("Open a channel first: /channels, then /open <name>.");
            return;
        };
        match self.session.send(&channel, text).await {
            Ok(message) => self.print(&message, ""),
            Err(error) => self.say(format!("! not sent: {error}")),
        }
    }

    async fn reply(&mut self, rest: &str) {
        let (number, text) = rest.split_once(' ').unwrap_or((rest, ""));
        let (Some((channel, message)), false) = (self.numbered(number), text.trim().is_empty())
        else {
            self.say("/reply <#n> <text>");
            return;
        };
        match self.session.reply(&channel, &message, text.trim()).await {
            Ok(message) => self.print(&message, ""),
            Err(error) => self.say(format!("! not sent: {error}")),
        }
    }

    fn numbered(&self, arg: &str) -> Option<(String, String)> {
        let number = arg.trim_start_matches('#').parse::<usize>().ok()?;
        self.numbers.get(number.checked_sub(1)?).cloned()
    }

    async fn edit(&mut self, rest: &str) {
        let (number, text) = rest.split_once(' ').unwrap_or((rest, ""));
        let (Some((channel, message)), false) = (self.numbered(number), text.trim().is_empty())
        else {
            self.say("/edit <#n> <new text>");
            return;
        };
        if let Err(error) = self.session.edit(&channel, &message, text.trim()).await {
            self.say(format!("! not edited: {error}"));
        }
    }

    async fn delete(&mut self, rest: &str) {
        let Some((channel, message)) = self.numbered(rest) else {
            self.say("/delete <#n>");
            return;
        };
        if let Err(error) = self.session.delete(&channel, &message).await {
            self.say(format!("! not deleted: {error}"));
        }
    }

    async fn react(&mut self, rest: &str, add: bool) {
        let (number, emoji) = rest.split_once(' ').unwrap_or((rest, ""));
        let (Some((channel, message)), Some(emoji)) = (self.numbered(number), emoji_named(emoji))
        else {
            self.say(if add {
                "/react <#n> <emoji>, e.g. 👍 or :thumbsup:"
            } else {
                "/unreact <#n> <emoji>"
            });
            return;
        };
        let result = if add {
            self.session.react(&channel, &message, &emoji).await
        } else {
            self.session.unreact(&channel, &message, &emoji).await
        };
        if let Err(error) = result {
            self.say(format!("! {error}"));
        }
    }

    /// Prints a message's reactions when they change.
    fn say_reactions(&mut self, message: &ChatMessage) {
        let m = &message.message;
        let number = self.number(&m.channel_id, &m.id);
        let label = self.channel_label(&m.channel_id);
        let reactions = reactions_text(message);
        self.say(if reactions.is_empty() {
            format!("{label} #{number} (no reactions)")
        } else {
            format!("{label} #{number}{reactions}")
        });
    }

    /// Says who started writing in the open channel.
    fn say_writing(&mut self, channel: &str) {
        if self.channel.as_deref() != Some(channel) {
            return;
        }
        let writing = self
            .session
            .typing_members(channel)
            .into_iter()
            .map(|member| member.name)
            .collect::<Vec<_>>();
        let started = writing
            .iter()
            .filter(|name| !self.writing.contains(name))
            .cloned()
            .collect::<Vec<_>>();
        self.writing = writing;
        if !started.is_empty() {
            self.say(format!("… {} writing", started.join(", ")));
        }
    }

    // ---- Guilds and channels ---------------------------------------------

    /// Renames the open channel, or sets its topic (empty: none).
    async fn rename_channel(&mut self, name: Option<&str>, topic: Option<&str>) {
        let Some(channel) = self
            .channel
            .clone()
            .and_then(|id| self.session.channel(&id))
        else {
            self.say("Open a channel first.");
            return;
        };
        let name = name
            .map(|name| name.trim_start_matches('#'))
            .unwrap_or(&channel.name);
        let topic = match topic {
            Some(topic) => Some(topic).filter(|topic| !topic.is_empty()),
            None => channel.topic.as_deref(),
        };
        if let Err(error) = self.session.update_channel(&channel.id, name, topic).await {
            self.say(format!("! {error}"));
        }
    }

    /// Deletes the open channel; its name must be typed to confirm.
    async fn delete_channel(&mut self, confirm: &str) {
        let Some(channel) = self
            .channel
            .clone()
            .and_then(|id| self.session.channel(&id))
        else {
            self.say("Open a channel first.");
            return;
        };
        if confirm.trim_start_matches('#') != channel.name {
            self.say(format!(
                "This deletes #{} with its threads and messages, for everyone. \
                 To go ahead: /delete-channel {}",
                channel.name, channel.name
            ));
            return;
        }
        if let Err(error) = self.session.delete_channel(&channel.id).await {
            self.say(format!("! {error}"));
        }
    }

    /// Renames the current guild, or sets its description (empty: none).
    async fn rename_guild(&mut self, name: Option<&str>, description: Option<&str>) {
        let Some(view) = self.current_view() else {
            self.say("Switch to a guild first.");
            return;
        };
        let name = name.unwrap_or(&view.guild.name);
        let description = match description {
            Some(description) => Some(description).filter(|text| !text.is_empty()),
            None => view.guild.description.as_deref(),
        };
        if let Err(error) = self
            .session
            .update_guild(&view.guild.id, name, description)
            .await
        {
            self.say(format!("! {error}"));
        }
    }

    /// Deletes the current guild; its name must be typed to confirm.
    async fn delete_guild(&mut self, confirm: &str) {
        let Some(view) = self.current_view() else {
            self.say("Switch to a guild first.");
            return;
        };
        if confirm != view.guild.name {
            self.say(format!(
                "This deletes {} and everything in it, for everyone. To go ahead: /delete-guild {}",
                view.guild.name, view.guild.name
            ));
            return;
        }
        if let Err(error) = self.session.delete_guild(&view.guild.id).await {
            self.say(format!("! {error}"));
        }
    }

    /// Keeps names around for when their guild, channel or member is gone.
    fn remember(&mut self, view: &GuildView) {
        self.names
            .insert(view.guild.id.clone(), view.guild.name.clone());
        for channel in &view.channels {
            self.names
                .insert(channel.id.clone(), format!("#{}", channel.name));
        }
        for member in &view.members {
            self.names
                .insert(member.user_id.clone(), member.name.clone());
        }
    }

    fn name_of(&self, id: &str) -> String {
        self.names
            .get(id)
            .cloned()
            .unwrap_or_else(|| id.to_string())
    }

    fn guild_update(&mut self, guild_id: &str, change: GuildChange) {
        let Some(view) = self.session.guild(guild_id) else {
            let name = self.name_of(guild_id);
            match change {
                GuildChange::Left => self.say(format!("! you are no longer in {name}")),
                GuildChange::Removed => self.say(format!("! you were removed from {name}")),
                GuildChange::Deleted => self.say(format!("! {name} was deleted")),
                _ => return,
            }
            if self.guild.as_deref() == Some(guild_id) {
                self.guild = None;
                self.channel = None;
                self.session.focus(None);
            }
            return;
        };
        let old_name = self.name_of(guild_id);
        self.remember(&view);
        let name = view.guild.name.clone();
        match change {
            GuildChange::Updated if old_name != name => {
                self.say(format!("✎ {old_name} is now {name}"))
            }
            GuildChange::Updated => {
                let description = view.guild.description.as_deref().unwrap_or("(none)");
                self.say(format!("✎ {name}: {description}"));
            }
            GuildChange::ChannelUpdated { channel_id } => {
                if let Some(channel) = view.channels.iter().find(|c| c.id == channel_id) {
                    let topic = channel.topic.as_deref().unwrap_or("(no topic)");
                    self.say(format!("✎ #{} in {name}: {topic}", channel.name));
                }
            }
            GuildChange::ChannelDeleted { channel_id } => {
                let channel = self.name_of(&channel_id);
                self.say(format!("- {channel} in {name} was deleted"));
                if self.channel.as_deref() == Some(channel_id.as_str()) {
                    self.channel = None;
                    self.session.focus(None);
                    self.say("Open another channel: /channels.");
                }
            }
            GuildChange::Joined if self.guild.as_deref() != Some(guild_id) => {
                self.say(format!("★ you are in {name}"))
            }
            GuildChange::MemberJoined { user_id } => {
                let member = self.name_of(&user_id);
                self.say(format!("→ {member} joined {name}"));
            }
            GuildChange::MemberLeft { user_id } => {
                let member = self.name_of(&user_id);
                self.say(format!("← {member} left {name}"));
            }
            GuildChange::MemberRemoved { user_id } => {
                let member = self.name_of(&user_id);
                self.say(format!("← {member} was removed from {name}"));
            }
            GuildChange::Notifications => self.say_notifications(&view),
            // Not the one just created and opened here.
            GuildChange::ChannelCreated { channel_id }
                if self.channel.as_deref() != Some(channel_id.as_str()) =>
            {
                if let Some(channel) = view.channels.iter().find(|c| c.id == channel_id) {
                    self.say(format!("+ #{} in {name}", channel.name));
                }
            }
            GuildChange::Languages => self.say_languages(&view),
            GuildChange::Joined
            | GuildChange::Deleted
            | GuildChange::ChannelCreated { .. }
            | GuildChange::Left
            | GuildChange::Removed
            | GuildChange::Refreshed => {}
        }
    }

    /// Shows how a guild is read, unless that is what was last shown.
    fn say_languages(&mut self, view: &GuildView) {
        let summary = languages_summary(view);
        if self.languages.get(&view.guild.id) != Some(&summary) {
            self.languages
                .insert(view.guild.id.clone(), summary.clone());
            self.say(summary);
        }
    }

    /// Shows what notifies the user in a guild, unless already shown.
    fn say_notifications(&mut self, view: &GuildView) {
        let summary = notify_summary(view, self.channel.as_deref());
        if self.notifications.get(&view.guild.id) != Some(&summary) {
            self.notifications
                .insert(view.guild.id.clone(), summary.clone());
            self.say(summary);
        }
    }

    fn find_guild(&self, query: &str) -> Option<String> {
        let guilds = self.session.guilds();
        if let Ok(number) = query.parse::<usize>() {
            return guilds
                .get(number.checked_sub(1)?)
                .map(|view| view.guild.id.clone());
        }
        guilds
            .iter()
            .find(|view| view.guild.name.eq_ignore_ascii_case(query))
            .map(|view| view.guild.id.clone())
    }

    fn list_guilds(&mut self) {
        let guilds = self.session.guilds();
        if guilds.is_empty() {
            self.say("No guilds yet: /create <name>, or /join <invite code>.");
        }
        for (i, view) in guilds.iter().enumerate() {
            let current = if self.guild.as_deref() == Some(view.guild.id.as_str()) {
                "*"
            } else {
                " "
            };
            let unread = unread_mark(self.session.guild_unread(&view.guild.id));
            self.say(format!(
                "{current}{}. {} ({} members){unread}",
                i + 1,
                view.guild.name,
                view.members.len()
            ));
        }
    }

    async fn switch_guild(&mut self, guild_id: &str) {
        let Some(view) = self.session.guild(guild_id) else {
            return;
        };
        self.guild = Some(guild_id.to_string());
        self.channel = None;
        self.say(format!("== {} ==", view.guild.name));
        self.say_languages(&view);
        let first = view
            .channels
            .iter()
            .find(|channel| channel.kind == ChannelKind::Text)
            .map(|channel| channel.id.clone());
        match first {
            Some(channel) => self.open(&channel).await,
            None => {
                self.session.focus(None);
                self.say("No channels yet: /channel <name>.");
            }
        }
    }

    fn current_view(&self) -> Option<GuildView> {
        self.session.guild(self.guild.as_deref()?)
    }

    /// Text channels and threads of the current guild, in display order.
    fn channel_list(&self) -> Vec<(String, String)> {
        let Some(view) = self.current_view() else {
            return Vec::new();
        };
        let mut list = Vec::new();
        for channel in view.channels.iter().filter(|c| c.kind == ChannelKind::Text) {
            list.push((channel.id.clone(), format!("#{}", channel.name)));
            for thread in view.channels.iter().filter(|c| {
                c.kind == ChannelKind::Thread && c.parent_id.as_deref() == Some(channel.id.as_str())
            }) {
                list.push((thread.id.clone(), format!("  └ {}", thread.name)));
            }
        }
        list
    }

    fn find_channel(&self, query: &str) -> Option<String> {
        let list = self.channel_list();
        if let Ok(number) = query.parse::<usize>() {
            return list.get(number.checked_sub(1)?).map(|(id, _)| id.clone());
        }
        let name = query.trim_start_matches('#');
        let view = self.current_view()?;
        view.channels
            .iter()
            .find(|channel| channel.kind != ChannelKind::Category && channel.name == name)
            .map(|channel| channel.id.clone())
    }

    fn list_channels(&mut self) {
        let list = self.channel_list();
        if list.is_empty() {
            self.say("No channels: /channel <name>.");
        }
        for (i, (id, name)) in list.into_iter().enumerate() {
            let current = if self.channel.as_deref() == Some(id.as_str()) {
                "*"
            } else {
                " "
            };
            let unread = unread_mark(self.session.unread(&id));
            self.say(format!("{current}{}. {name}{unread}", i + 1));
        }
    }

    fn channel_label(&self, channel_id: &str) -> String {
        let Some(channel) = self.session.channel(channel_id) else {
            return "#?".to_string();
        };
        let name = match channel.kind {
            ChannelKind::Thread => {
                format!("#{}/{}", self.parent_name(&channel.parent_id), channel.name)
            }
            _ => format!("#{}", channel.name),
        };
        if self.guild.as_deref() == Some(channel.guild_id.as_str()) {
            name
        } else {
            let guild = self
                .session
                .guild(&channel.guild_id)
                .map(|view| view.guild.name)
                .unwrap_or_default();
            format!("{guild}{name}")
        }
    }

    fn parent_name(&self, parent: &Option<String>) -> String {
        parent
            .as_deref()
            .and_then(|id| self.session.channel(id))
            .map(|channel| channel.name)
            .unwrap_or_default()
    }

    async fn open(&mut self, channel_id: &str) {
        self.channel = Some(channel_id.to_string());
        self.writing.clear();
        // Read as it grows, and quiet.
        self.session.focus(Some(channel_id));
        let label = self.channel_label(channel_id);
        self.say(format!("-- {label} --"));
        if self.opened.insert(channel_id.to_string()) {
            match self.session.load_history(channel_id, PAGE).await {
                // Offline: what the cache kept will do.
                Err(babel_sdk::Error::Transport(_))
                    if !self.session.messages(channel_id).is_empty() => {}
                Err(error) => self.say(format!("! could not load history: {error}")),
                Ok(_) => {}
            }
        }
        let messages = self.session.messages(channel_id);
        let start = messages.len().saturating_sub(PAGE as usize);
        for message in &messages[start..] {
            self.print(message, "");
        }
    }

    async fn more(&mut self) {
        let Some(channel) = self.channel.clone() else {
            return;
        };
        let oldest = self
            .session
            .messages(&channel)
            .first()
            .map(|message| message.message.id.clone());
        match self.session.load_history(&channel, PAGE).await {
            Ok(0) => self.say("(no older messages)"),
            Ok(_) => {
                let older = self
                    .session
                    .messages(&channel)
                    .into_iter()
                    .take_while(|message| Some(&message.message.id) != oldest.as_ref())
                    .collect::<Vec<_>>();
                for message in &older {
                    self.print(message, "");
                }
            }
            Err(error) => self.say(format!("! {error}")),
        }
    }

    fn members(&mut self) {
        let Some(view) = self.current_view() else {
            return;
        };
        let names = view
            .members
            .iter()
            .map(|m| {
                if m.owner {
                    format!("{} (owner)", m.name)
                } else {
                    m.name.clone()
                }
            })
            .collect::<Vec<_>>();
        self.say(names.join(", "));
    }

    async fn create_guild(&mut self, name: &str) {
        match self.session.create_guild(name, None).await {
            Ok(view) => self.switch_guild(&view.guild.id).await,
            Err(error) => self.say(format!("! {error}")),
        }
    }

    async fn create_channel(&mut self, name: &str) {
        let Some(guild) = self.guild.clone() else {
            self.say("Switch to a guild first.");
            return;
        };
        match self
            .session
            .create_channel(&guild, name.trim_start_matches('#'), None)
            .await
        {
            Ok(channel) => self.open(&channel.id).await,
            Err(error) => self.say(format!("! {error}")),
        }
    }

    async fn thread(&mut self, rest: &str) {
        let (number, name) = rest.split_once(' ').unwrap_or((rest, ""));
        let (Some((channel, message)), false) = (self.numbered(number), name.trim().is_empty())
        else {
            self.say("/thread <#n> <name>");
            return;
        };
        match self
            .session
            .start_thread(&channel, &message, name.trim())
            .await
        {
            Ok(thread) => self.open(&thread.id).await,
            Err(error) => self.say(format!("! {error}")),
        }
    }

    async fn invite(&mut self, rest: &str) {
        let Some(guild) = self.guild.clone() else {
            self.say("Switch to a guild first.");
            return;
        };
        let mut args = rest.split_whitespace().map(str::parse::<i64>);
        let (days, uses) = (args.next(), args.next());
        let (Ok(days), Ok(uses)) = (days.transpose(), uses.transpose()) else {
            self.say("/invite [days] [uses]");
            return;
        };
        match self
            .session
            .create_invite(&guild, days.map(|days| days * 24 * 60 * 60), uses)
            .await
        {
            Ok(invite) => self.say(format!("Invite code: {}", invite.code)),
            Err(error) => self.say(format!("! {error}")),
        }
    }

    async fn join(&mut self, code: &str) {
        match self.session.join(code).await {
            Ok(view) => self.switch_guild(&view.guild.id).await,
            Err(babel_sdk::Error::Api(babel_sdk::ApiError::NotFound)) => {
                self.say("! that invite is invalid, expired or used up")
            }
            Err(error) => self.say(format!("! {error}")),
        }
    }

    async fn languages(&mut self, rest: &str) {
        let Some(view) = self.current_view() else {
            self.say("Switch to a guild first.");
            return;
        };
        if rest.is_empty() {
            let summary = languages_summary(&view);
            self.say(summary);
            return;
        }
        let mut languages = Vec::new();
        let mut llm = Vec::new();
        let mut in_llm = false;
        let mut region_mode = RegionMode::Merged;
        for word in rest.split_whitespace() {
            match word {
                "llm" => in_llm = true,
                "distinct" => region_mode = RegionMode::Distinct,
                tag if in_llm => llm.push(tag.to_string()),
                tag => languages.push(tag.to_string()),
            }
        }
        let update = LanguagePolicyUpdate {
            guild_id: view.guild.id.clone(),
            languages,
            llm_languages: llm,
            region_mode,
            aliases: view.policy.aliases.clone(),
            translate_regional_variants: view.policy.translate_regional_variants,
            model: view.policy.model.clone(),
        };
        if let Err(error) = self.session.set_languages(update).await {
            self.say(format!(
                "! {error} (plan {}: up to {} languages, {} by LLM)",
                view.policy.plan, view.policy.max_languages, view.policy.max_llm_languages
            ));
        }
    }

    async fn model(&mut self, rest: &str) {
        let Some(view) = self.current_view() else {
            self.say("Switch to a guild first.");
            return;
        };
        let models = match self.session.models(&view.guild.id).await {
            Ok(models) => models,
            Err(error) => {
                self.say(format!("! {error}"));
                return;
            }
        };
        if models.is_empty() {
            self.say(format!(
                "Plan {} translates on each reader's device: no server model.",
                view.policy.plan
            ));
            return;
        }
        if rest.is_empty() {
            let current = view.policy.model.as_deref().unwrap_or(&models[0].id);
            for (index, model) in models.iter().enumerate() {
                self.say(format!(
                    "{} {} ({}){}",
                    if model.id == current { "*" } else { " " },
                    model.id,
                    model.name,
                    if index == 0 { ", the default" } else { "" }
                ));
            }
            return;
        }
        let choice = match rest {
            "default" => None,
            id if models.iter().any(|model| model.id == id) => Some(id.to_string()),
            _ => {
                self.say(format!("! no model {rest}; /model lists them"));
                return;
            }
        };
        match self.session.set_model(&view.guild.id, choice).await {
            Ok(policy) => self.say(format!(
                "Translating with {}.",
                policy.model.as_deref().unwrap_or(&models[0].id)
            )),
            Err(error) => self.say(format!("! {error}")),
        }
    }

    // ---- Reading and notifications ----------------------------------------

    fn unread(&mut self) {
        let mut any = false;
        for view in self.session.guilds() {
            for channel in &view.channels {
                let unread = self.session.unread(&channel.id);
                if unread.unread || unread.mentions > 0 {
                    any = true;
                    let label = self.channel_label(&channel.id);
                    let label = if label.starts_with('#') {
                        format!("{}{label}", view.guild.name)
                    } else {
                        label
                    };
                    self.say(format!("{label}{}", unread_mark(unread)));
                }
            }
        }
        if !any {
            self.say("All read.");
        }
    }

    async fn mark_read(&mut self) {
        let Some(channel) = self.channel.clone() else {
            return;
        };
        if let Err(error) = self.session.mark_read(&channel).await {
            self.say(format!("! {error}"));
        }
    }

    async fn notify(&mut self, rest: &str) {
        let Some(view) = self.current_view() else {
            self.say("Switch to a guild first.");
            return;
        };
        let mut words = rest.split_whitespace();
        let level = match words.next() {
            None => {
                let summary = notify_summary(&view, self.channel.as_deref());
                self.say(summary);
                return;
            }
            Some("all") => NotifyLevel::All,
            Some("mentions") => NotifyLevel::Mentions,
            Some("none") => NotifyLevel::None,
            Some(_) => {
                self.say("/notify [all|mentions|none] [here]");
                return;
            }
        };
        let mut settings = view.notifications.clone();
        match (words.next(), &self.channel) {
            (Some("here"), Some(channel)) => {
                settings
                    .channels
                    .retain(|entry| entry.channel_id != *channel);
                settings.channels.push(ChannelNotify {
                    channel_id: channel.clone(),
                    level,
                });
            }
            (Some(_), _) => {
                self.say("/notify [all|mentions|none] [here]");
                return;
            }
            (None, _) => settings.level = level,
        }
        match self.session.set_notifications(settings).await {
            Ok(_) => {
                if let Some(view) = self.current_view() {
                    self.say_notifications(&view);
                }
            }
            Err(error) => self.say(format!("! {error}")),
        }
    }

    fn status(&mut self) {
        let state = if !self.session.is_online() {
            "offline, retrying"
        } else if self.session.uses_websocket() {
            "events by WebSocket"
        } else {
            "events by long polling"
        };
        self.say(format!(
            "{} on {}: {state}.",
            self.session.user_name(),
            self.session.client().server()
        ));
    }

    // ---- Membership -------------------------------------------------------

    async fn invites(&mut self) {
        let Some(guild) = self.guild.clone() else {
            self.say("Switch to a guild first.");
            return;
        };
        match self.session.invites(&guild).await {
            Ok(invites) if invites.is_empty() => self.say("No invites. /invite makes one."),
            Ok(invites) => {
                for invite in invites {
                    let uses = match invite.max_uses {
                        Some(max) => format!("{}/{max} uses", invite.uses),
                        None => format!("{} uses", invite.uses),
                    };
                    let by = self.name_of(&invite.created_by);
                    self.say(format!("{}  by {by}, {uses}", invite.code));
                }
            }
            Err(error) => self.say(format!("! {error}")),
        }
    }

    async fn leave(&mut self) {
        let Some(guild) = self.guild.clone() else {
            self.say("Switch to a guild first.");
            return;
        };
        match self.session.leave(&guild).await {
            // The session reports it (GuildChange::Left).
            Ok(()) => {}
            Err(babel_sdk::Error::Api(babel_sdk::ApiError::InvalidRequest)) => {
                self.say("! owners cannot leave their guild")
            }
            Err(error) => self.say(format!("! {error}")),
        }
    }

    fn member_named(&self, name: &str) -> Option<String> {
        let name = name.trim_start_matches('@');
        self.current_view()?
            .members
            .into_iter()
            .find(|member| member.name.eq_ignore_ascii_case(name))
            .map(|member| member.user_id)
    }

    async fn remove(&mut self, name: &str, ban: bool) {
        let (Some(guild), Some(user)) = (self.guild.clone(), self.member_named(name)) else {
            self.say("No such member here. /members lists them.");
            return;
        };
        if let Err(error) = self.session.remove_member(&guild, &user, ban).await {
            self.say(format!("! {error}"));
        }
    }

    async fn bans(&mut self) {
        let Some(guild) = self.guild.clone() else {
            return;
        };
        match self.session.bans(&guild).await {
            Ok(bans) if bans.is_empty() => self.say("Nobody is banned."),
            Ok(bans) => {
                let names = bans.iter().map(|ban| ban.name.clone()).collect::<Vec<_>>();
                self.say(format!("Banned: {}", names.join(", ")));
            }
            Err(error) => self.say(format!("! {error}")),
        }
    }

    async fn unban(&mut self, name: &str) {
        let Some(guild) = self.guild.clone() else {
            return;
        };
        let name = name.trim_start_matches('@');
        let user = match self.session.bans(&guild).await {
            Ok(bans) => bans
                .into_iter()
                .find(|ban| ban.name.eq_ignore_ascii_case(name))
                .map(|ban| ban.user_id),
            Err(error) => {
                self.say(format!("! {error}"));
                return;
            }
        };
        let Some(user) = user else {
            self.say("Nobody by that name is banned.");
            return;
        };
        match self.session.unban(&guild, &user).await {
            Ok(()) => self.say(format!("{name} may join again.")),
            Err(error) => self.say(format!("! {error}")),
        }
    }

    async fn settings(&mut self, change: impl FnOnce(&mut babel_sdk::UserSettings)) {
        let mut settings = self.session.settings();
        change(&mut settings);
        match self.session.set_settings(settings).await {
            Ok(settings) => self.say(format!(
                "Reading {}; originals {}; outside languages {}.",
                settings.locale,
                if settings.show_original {
                    "shown"
                } else {
                    "hidden"
                },
                if settings.translate_outside_set {
                    "translated on device"
                } else {
                    "left as written"
                }
            )),
            Err(error) => self.say(format!("! {error}")),
        }
    }
}

fn on_off(word: &str) -> Option<bool> {
    match word {
        "on" | "yes" | "true" => Some(true),
        "off" | "no" | "false" => Some(false),
        _ => None,
    }
}

fn marker(shown: &Shown) -> String {
    match shown {
        Shown::Original => String::new(),
        Shown::Translating => "  ‹translating…›".to_string(),
        Shown::Server { from, to } => format!("  ‹{from}→{to}›"),
        Shown::Device {
            from,
            to,
            via_english,
        } => {
            let via = if *via_english { "en→" } else { "" };
            format!("  ‹{from}→{via}{to} on device›")
        }
        Shown::Untranslated { reason } => format!("  ‹untranslated: {reason}›"),
    }
}

/// `  [👍 2  ❤️ 1*]`, the user's own marked; empty without reactions.
fn reactions_text(message: &ChatMessage) -> String {
    let reactions = &message.message.reactions;
    if reactions.is_empty() {
        return String::new();
    }
    let list = reactions
        .iter()
        .map(|reaction| {
            let me = if reaction.me { "*" } else { "" };
            format!("{} {}{me}", reaction.emoji, reaction.count)
        })
        .collect::<Vec<_>>();
    format!("  [{}]", list.join("  "))
}

/// An emoji typed as itself or by its shortcode (`:thumbsup:`).
fn emoji_named(text: &str) -> Option<String> {
    let text = text.trim();
    let emoji = match text
        .strip_prefix(':')
        .and_then(|rest| rest.strip_suffix(':'))
    {
        Some(code) => emojis::get_by_shortcode(code),
        None => emojis::get(text),
    };
    emoji.map(|emoji| emoji.as_str().to_string())
}

fn languages_summary(view: &GuildView) -> String {
    let policy = &view.policy;
    let language = &view.language;
    let how = if language.llm {
        "translated by the server"
    } else if language.client_translation {
        "translated on this device"
    } else {
        "not translated"
    };
    format!(
        "Languages: {} (LLM: {}; plan {}{}). You read {}, {how}.",
        policy.languages.join(" "),
        if policy.llm_languages.is_empty() {
            "none".to_string()
        } else {
            policy.llm_languages.join(" ")
        },
        policy.plan,
        match &policy.model {
            Some(model) => format!("; model {model}"),
            None => String::new(),
        },
        language.effective,
    )
}

/// ` • @2` for an unread channel with two mentions.
fn unread_mark(unread: Unread) -> String {
    let mut mark = String::new();
    if unread.unread {
        mark.push_str("  •");
    }
    if unread.mentions > 0 {
        mark.push_str(&format!("  @{}", unread.mentions));
    }
    mark
}

fn level_name(level: NotifyLevel) -> &'static str {
    match level {
        NotifyLevel::All => "every message",
        NotifyLevel::Mentions => "mentions and replies",
        NotifyLevel::None => "nothing",
    }
}

fn notify_summary(view: &GuildView, channel: Option<&str>) -> String {
    let settings = &view.notifications;
    let mut summary = format!(
        "Notifications in {}: {}",
        view.guild.name,
        level_name(settings.level)
    );
    for entry in &settings.channels {
        if let Some(known) = view.channels.iter().find(|c| c.id == entry.channel_id) {
            let here = if channel == Some(known.id.as_str()) {
                " (here)"
            } else {
                ""
            };
            summary.push_str(&format!(
                "; #{}{here}: {}",
                known.name,
                level_name(entry.level)
            ));
        }
    }
    summary.push('.');
    summary
}
