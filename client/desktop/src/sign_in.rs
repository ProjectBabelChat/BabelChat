//! Signing in, or creating an account.

use babel_sdk::{Client, profile};
use gpui::prelude::*;
use gpui::{
    Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render, SharedString,
    Window, div, px,
};
use gpui_md3e::prelude::*;

use crate::{
    i18n::{self, Strings},
    runtime::{self, Config, describe},
    widgets,
};

pub enum SignInEvent {
    /// Signed in; the app saves the sign-in and connects.
    SignedIn { client: Client, name: String },
    /// Try the saved sign-in again (the server was unreachable).
    Retry,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    SignIn,
    Register,
}

pub struct SignIn {
    t: &'static Strings,
    mode: Mode,
    server: Entity<TextField>,
    name: Entity<TextField>,
    password: Entity<TextField>,
    locale: Entity<TextField>,
    signup_code: Entity<TextField>,
    busy: bool,
    error: Option<String>,
    /// A saved sign-in could not connect: offer to try it again.
    can_retry: bool,
}

impl EventEmitter<SignInEvent> for SignIn {}

impl SignIn {
    pub fn new(
        error: Option<String>,
        can_retry: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let system_locale = profile::system_locale();
        let t = i18n::strings(&system_locale);
        let (server, saved_name) = {
            let config = cx.global::<Config>();
            let saved = profile::load(&config.dir, &config.profile).ok().flatten();
            (
                saved
                    .as_ref()
                    .map_or(config.server.clone(), |saved| saved.server.clone()),
                saved.map(|saved| saved.name),
            )
        };
        let this = cx.entity().downgrade();
        let submit = move |_: &SharedString, window: &mut Window, cx: &mut gpui::App| {
            // The field submitting is still being updated; read it afterwards.
            let this = this.clone();
            window.defer(cx, move |window, cx| {
                let _ = this.update(cx, |this: &mut SignIn, cx| this.submit(window, cx));
            });
            false
        };
        let field = |label: &str, cx: &mut Context<Self>| {
            let label = label.to_string();
            let submit = submit.clone();
            cx.new(|cx| TextField::new(cx).label(label).on_submit(submit))
        };
        let server_field = field(t.server, cx);
        let name = cx.new(|cx| {
            TextField::new(cx)
                .label(t.name)
                .supporting_text(t.name_hint)
                .leading_icon(Icon::Person)
                .on_submit(submit.clone())
        });
        let password = cx.new(|cx| {
            TextField::new(cx)
                .label(t.password)
                .masked(true)
                .leading_icon(Icon::Lock)
                .on_submit(submit.clone())
        });
        let locale = cx.new(|cx| {
            TextField::new(cx)
                .label(t.locale)
                .supporting_text(t.locale_hint)
                .leading_icon(Icon::Translate)
                .on_submit(submit.clone())
        });
        let signup_code = cx.new(|cx| {
            TextField::new(cx)
                .label(t.signup_code)
                .supporting_text(t.signup_code_hint)
                .leading_icon(Icon::Mail)
                .on_submit(submit.clone())
        });
        server_field.update(cx, |field, cx| field.set_text(server, window, cx));
        locale.update(cx, |field, cx| field.set_text(system_locale, window, cx));
        let has_name = saved_name.is_some();
        if let Some(saved_name) = saved_name {
            name.update(cx, |field, cx| field.set_text(saved_name, window, cx));
        }
        let first = if has_name { &password } else { &name };
        window.focus(&first.focus_handle(cx), cx);
        Self {
            t,
            mode: Mode::SignIn,
            server: server_field,
            name,
            password,
            locale,
            signup_code,
            busy: false,
            error,
            can_retry,
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let server = self.server.read(cx).text().trim().to_string();
        let name = self.name.read(cx).text().trim().to_string();
        let password = self.password.read(cx).text().to_string();
        let locale = self.locale.read(cx).text().trim().to_string();
        let signup_code = self.signup_code.read(cx).text().trim().to_string();
        if server.is_empty() || name.is_empty() || password.is_empty() {
            self.error = Some(self.t.invalid_request.to_string());
            cx.notify();
            return;
        }
        self.busy = true;
        self.error = None;
        let register = self.mode == Mode::Register;
        let task = runtime::spawn(cx, {
            let name = name.clone();
            async move {
                if register {
                    let code = Some(signup_code.as_str()).filter(|code| !code.is_empty());
                    Client::register(&server, &name, &password, &locale, code).await
                } else {
                    Client::login(&server, &name, &password).await
                }
            }
        });
        let t = self.t;
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.busy = false;
                match result {
                    Ok(client) => cx.emit(SignInEvent::SignedIn { client, name }),
                    Err(error) => this.error = Some(describe(&error, t)),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn switch_mode(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.mode = match self.mode {
            Mode::SignIn => Mode::Register,
            Mode::Register => Mode::SignIn,
        };
        self.error = None;
        window.focus(&self.name.focus_handle(cx), cx);
        cx.notify();
    }
}

impl Focusable for SignIn {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.name.focus_handle(cx)
    }
}

impl Render for SignIn {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.t;
        let colors = cx.md().colors;
        let register = self.mode == Mode::Register;

        let logo = div()
            .relative()
            .size(px(72.0))
            .child(
                ShapeView::new("logo", MaterialShape::Cookie9Sided)
                    .size(px(72.0))
                    .color(colors.primary_container)
                    .hover_color(colors.tertiary_container)
                    .morph_on_hover(MaterialShape::Sunny),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        icon(Icon::Translate)
                            .size(px(32.0))
                            .color(colors.on_primary_container),
                    ),
            );

        let mut form = div()
            .flex()
            .flex_col()
            .gap_4()
            .w(px(400.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_3()
                    .pb_2()
                    .child(logo)
                    .child(div().md_type(TypeRole::DisplaySmall, cx).child(t.app_name))
                    .child(widgets::supporting(t.tagline, cx)),
            )
            .child(self.server.clone())
            .child(self.name.clone())
            .child(self.password.clone());
        if register {
            form = form
                .child(self.locale.clone())
                .child(self.signup_code.clone());
        }
        if let Some(error) = &self.error {
            form = form.child(
                div()
                    .md_type(TypeRole::BodyMedium, cx)
                    .text_color(colors.error)
                    .child(error.clone()),
            );
        }

        let primary = if self.busy {
            div()
                .flex()
                .justify_center()
                .h(px(56.0))
                .child(LoadingIndicator::new("signing-in").size(px(48.0)))
                .into_any_element()
        } else {
            Button::new(
                "submit",
                if register {
                    t.create_account
                } else {
                    t.sign_in
                },
            )
            .size(ButtonSize::Medium)
            .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx)))
            .into_any_element()
        };
        let mut actions = div()
            .flex()
            .flex_col()
            .items_stretch()
            .gap_2()
            .pt_2()
            .child(primary)
            .child(
                Button::new(
                    "switch-mode",
                    if register {
                        t.have_account
                    } else {
                        t.need_account
                    },
                )
                .style(ButtonStyle::Text)
                .on_click(cx.listener(|this, _, window, cx| this.switch_mode(window, cx))),
            );
        if self.can_retry && !self.busy {
            actions = actions.child(
                Button::new("retry", t.retry)
                    .style(ButtonStyle::Outlined)
                    .icon(Icon::Refresh)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SignInEvent::Retry))),
            );
        }

        div()
            .id("sign-in")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .items_center()
            .justify_center()
            .bg(colors.surface_container)
            .text_color(colors.on_surface)
            .md_type(TypeRole::BodyMedium, cx)
            .child(
                div()
                    .p_10()
                    .rounded(px(28.0))
                    .bg(colors.surface)
                    .child(form.child(actions)),
            )
    }
}
