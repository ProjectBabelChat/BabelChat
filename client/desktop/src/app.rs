//! The root view: signing in, connecting, then the chat.

use std::sync::Arc;

use babel_sdk::{
    Client, Error, SessionConfig,
    profile::{self, Profile},
};
use gpui::prelude::*;
use gpui::{AnyView, Context, Entity, IntoElement, Render, Subscription, Window, actions, div, px};
use gpui_md3e::prelude::*;

use crate::{
    chat::{ChatEvent, ChatView},
    i18n,
    runtime::{self, Config, describe},
    sign_in::{SignIn, SignInEvent},
};

actions!(babelchat, [FocusNext, FocusPrev]);

enum Screen {
    Connecting,
    SignIn(Entity<SignIn>),
    Chat(Entity<ChatView>),
}

pub struct BabelApp {
    screen: Screen,
    _subscription: Option<Subscription>,
}

impl BabelApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut app = Self {
            screen: Screen::Connecting,
            _subscription: None,
        };
        let saved = {
            let config = cx.global::<Config>();
            profile::load(&config.dir, &config.profile)
        };
        match saved {
            Ok(Some(saved)) => app.connect(saved, window, cx),
            Ok(None) => app.show_sign_in(None, false, window, cx),
            Err(error) => app.show_sign_in(Some(error.to_string()), false, window, cx),
        }
        app
    }

    /// Connects with a saved sign-in. With a cache the chat shows at once
    /// and the session connects in the background.
    fn connect(&mut self, saved: Profile, window: &mut Window, cx: &mut Context<Self>) {
        self.screen = Screen::Connecting;
        self._subscription = None;
        let config = cx.global::<Config>();
        let session_config = SessionConfig {
            cache: config
                .cache
                .then(|| profile::cache_path(&config.dir, &config.profile)),
            transport: config.transport,
            ..SessionConfig::default()
        };
        let translator = config.translator.clone();
        let task = runtime::spawn(cx, async move {
            let client = Client::with_token(&saved.server, saved.token)?;
            client.connect_with(translator, session_config).await
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(session) => this.show_chat(Arc::new(session), window, cx),
                Err(Error::Unauthorized) => {
                    let t = i18n::strings(&profile::system_locale());
                    this.show_sign_in(Some(t.session_expired.to_string()), false, window, cx)
                }
                Err(error) => {
                    let t = i18n::strings(&profile::system_locale());
                    this.show_sign_in(Some(describe(&error, t)), true, window, cx)
                }
            });
        })
        .detach();
        cx.notify();
    }

    fn show_sign_in(
        &mut self,
        error: Option<String>,
        can_retry: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sign_in = cx.new(|cx| SignIn::new(error, can_retry, window, cx));
        self._subscription = Some(cx.subscribe_in(
            &sign_in,
            window,
            |this, _, event: &SignInEvent, window, cx| match event {
                SignInEvent::SignedIn { client, name } => {
                    let config = cx.global::<Config>();
                    let saved = Profile {
                        server: client.server().to_string(),
                        name: name.clone(),
                        token: client.token(),
                    };
                    if let Err(error) = profile::save(&config.dir, &config.profile, &saved) {
                        eprintln!("babelchat: could not save the sign-in: {error}");
                    }
                    this.connect(saved, window, cx);
                }
                SignInEvent::Retry => {
                    let saved = {
                        let config = cx.global::<Config>();
                        profile::load(&config.dir, &config.profile)
                    };
                    if let Ok(Some(saved)) = saved {
                        this.connect(saved, window, cx);
                    }
                }
            },
        ));
        self.screen = Screen::SignIn(sign_in);
        cx.notify();
    }

    fn show_chat(
        &mut self,
        session: Arc<babel_sdk::Session>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let chat = cx.new(|cx| ChatView::new(session, window, cx));
        self._subscription = Some(cx.subscribe_in(
            &chat,
            window,
            |this, _, event: &ChatEvent, window, cx| match event {
                ChatEvent::SignedOut { expired } => {
                    let t = i18n::strings(&profile::system_locale());
                    if !expired {
                        let config = cx.global::<Config>();
                        let _ = profile::remove(&config.dir, &config.profile);
                    }
                    // Dropping the chat view drops the session, which saves
                    // its cache and stops.
                    let error = expired.then(|| t.session_expired.to_string());
                    this.show_sign_in(error, false, window, cx);
                }
                ChatEvent::TokenRenewed { token } => {
                    let config = cx.global::<Config>();
                    if let Ok(Some(mut saved)) = profile::load(&config.dir, &config.profile) {
                        saved.token = token.clone();
                        if let Err(error) = profile::save(&config.dir, &config.profile, &saved) {
                            eprintln!("babelchat: could not save the sign-in: {error}");
                        }
                    }
                }
            },
        ));
        self.screen = Screen::Chat(chat);
        cx.notify();
    }
}

impl Render for BabelApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        Theme::tick(window, cx);
        let colors = cx.md().colors;
        let content: AnyView = match &self.screen {
            Screen::SignIn(view) => view.clone().into(),
            Screen::Chat(view) => view.clone().into(),
            Screen::Connecting => {
                let t = i18n::strings(&profile::system_locale());
                return div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .items_center()
                    .justify_center()
                    .bg(colors.surface_container)
                    .text_color(colors.on_surface)
                    .md_type(TypeRole::BodyLarge, cx)
                    .child(LoadingIndicator::new("connecting").size(px(64.0)))
                    .child(t.connecting)
                    .into_any_element();
            }
        };
        div()
            .size_full()
            .on_action(|_: &FocusNext, window, cx| window.focus_next(cx))
            .on_action(|_: &FocusPrev, window, cx| window.focus_prev(cx))
            .bg(colors.surface_container)
            .text_color(colors.on_surface)
            .child(content)
            .into_any_element()
    }
}
