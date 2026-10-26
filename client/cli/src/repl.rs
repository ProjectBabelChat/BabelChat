//! The chat loop: commands in, messages out, one line at a time.
//!
//! Messages print once their translation has settled, so each appears once
//! in the reader's language. One still translating after
//! [`SETTLE_WAIT`] prints as it is and its translation follows on a `↳`
//! line. Messages get short numbers (`#3`) for `/edit`, `/delete` and
//! `/thread`.

use std::{
    collections::{HashMap, HashSet},
    io::Write,
    time::{Duration, Instant},
};

use babel_sdk::{
    ChannelKind, ChatMessage, GuildChange, GuildView, LanguagePolicyUpdate, MessageChange,
    RegionMode, Session, Shown, Update,
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
    /// Language summary last shown per guild.
    languages: HashMap<String, String>,
    /// Channels whose history has been loaded.
    opened: HashSet<String>,
}

pub const HELP: &str = "\
Type to send to the current channel. Commands:
  /guilds                      list your guilds          /guild <n|name>   switch guild
  /channels                    list channels             /open <n|name>    switch channel
  /more                        older messages            /members          who is here
  /create <name>               create a guild            /channel <name>   create a channel
  /thread <#n> <name>          thread from a message
  /invite [days] [uses]        invite code (0 days = never expires)
  /join <code>                 join with an invite code
  /languages                   show the guild's languages
  /languages <tags..> [llm <tags..>] [distinct]     set them (owner)
  /locale <tag>                your language             /original on|off  show originals too
  /outside on|off              translate languages the guild does not declare, on device
  /edit <#n> <text>            /delete <#n>              /quit";

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
        self.say(format!(
            "Connected as {} (reading {}). /help for commands.",
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
            "delete" => self.delete(rest).await,
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
            Update::Offline { error } => self.say(format!("! offline ({error}); retrying")),
            Update::Online { reloaded } => self.say(if reloaded {
                "! back online (reloaded)"
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
            MessageChange::Deleted => {}
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
        let mut text = format!(
            "{lead}{label} #{number} {}: {}{}",
            m.author_name,
            message.display.text,
            marker(&message.display.shown)
        );
        if message.display.is_translated() && self.session.settings().show_original {
            text.push_str(&format!("\n      ┆ {}", m.text));
        }
        self.printed
            .insert(m.id.clone(), message.display.text.clone());
        self.say(text);
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

    // ---- Guilds and channels ---------------------------------------------

    fn guild_update(&mut self, guild_id: &str, change: GuildChange) {
        let Some(view) = self.session.guild(guild_id) else {
            if change == GuildChange::Left {
                self.say("! you are no longer in a guild");
                if self.guild.as_deref() == Some(guild_id) {
                    self.guild = None;
                    self.channel = None;
                }
            }
            return;
        };
        let name = view.guild.name.clone();
        match change {
            GuildChange::Joined if self.guild.as_deref() != Some(guild_id) => {
                self.say(format!("★ you are in {name}"))
            }
            GuildChange::MemberJoined { user_id } => {
                if let Some(member) = view.members.iter().find(|m| m.user_id == user_id) {
                    self.say(format!("→ {} joined {name}", member.name));
                }
            }
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
            | GuildChange::ChannelCreated { .. }
            | GuildChange::Left
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
            self.say(format!(
                "{current}{}. {} ({} members)",
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
            None => self.say("No channels yet: /channel <name>."),
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
            self.say(format!("{current}{}. {name}", i + 1));
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
        let label = self.channel_label(channel_id);
        self.say(format!("-- {label} --"));
        if self.opened.insert(channel_id.to_string())
            && let Err(error) = self.session.load_history(channel_id, PAGE).await
        {
            self.say(format!("! could not load history: {error}"));
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
        };
        if let Err(error) = self.session.set_languages(update).await {
            self.say(format!(
                "! {error} (plan {}: up to {} languages, {} by LLM)",
                view.policy.plan, view.policy.max_languages, view.policy.max_llm_languages
            ));
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
        "Languages: {} (LLM: {}; plan {}). You read {}, {how}.",
        policy.languages.join(" "),
        if policy.llm_languages.is_empty() {
            "none".to_string()
        } else {
            policy.llm_languages.join(" ")
        },
        policy.plan,
        language.effective,
    )
}
