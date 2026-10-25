//! A mock BabelChat conversation that exercises the components together.
//! Translations are canned: there is no translation backend yet.

use std::time::{Duration, Instant};

use gpui::{
    AnyElement, Context, InteractiveElement as _, IntoElement, ParentElement as _, SharedString,
    StatefulInteractiveElement as _, Styled as _, Window, div, px,
};
use gpui_md3e::{motion::animate_from, prelude::*};

use crate::Gallery;

/// How long the mock translation takes.
const TRANSLATION_DELAY: Duration = Duration::from_millis(1400);

pub struct Message {
    pub author: SharedString,
    pub original: SharedString,
    pub original_language: &'static str,
    pub translation: Option<SharedString>,
    pub mine: bool,
    pub sent_at: Instant,
    pub show_original: bool,
}

pub fn seed_messages() -> Vec<Message> {
    let long_ago = Instant::now() - Duration::from_secs(60);
    let message =
        |author: &str, original: &str, language, translation: Option<&str>, mine| Message {
            author: author.to_owned().into(),
            original: original.to_owned().into(),
            original_language: language,
            translation: translation.map(|t| t.to_owned().into()),
            mine,
            sent_at: long_ago,
            show_original: false,
        };
    vec![
        message(
            "Ben",
            "Hey! Did the new build of the client land?",
            "English",
            Some("やあ！クライアントの新しいビルドはもう入った？"),
            false,
        ),
        message("You", "うん、さっきマージしたよ。", "日本語", None, true),
        message(
            "Carla",
            "¡Genial! La animación del botón se ve increíble.",
            "Español",
            Some("最高！ボタンのアニメーションがすごく良く見える。"),
            false,
        ),
        message(
            "Ben",
            "The loading indicator morphing between shapes is my favorite part.",
            "English",
            Some("図形の間を変形するローディング表示が一番のお気に入り。"),
            false,
        ),
    ]
}

impl Gallery {
    /// Appends the composer's text as a new outgoing message.
    pub fn send(&mut self, text: &SharedString, cx: &mut Context<Self>) -> bool {
        let text = text.trim();
        if text.is_empty() {
            return false;
        }
        self.messages.push(Message {
            author: "You".into(),
            original: text.to_owned().into(),
            original_language: "日本語",
            translation: None,
            mine: true,
            sent_at: Instant::now(),
            show_original: false,
        });
        // A friend answers, and the reply goes through "translation".
        let reply = Message {
            author: "Ben".into(),
            original: "Nice, that reads perfectly on my side too!".into(),
            original_language: "English",
            translation: Some("いいね、こっちでも完璧に読めるよ！".into()),
            mine: false,
            sent_at: Instant::now() + Duration::from_millis(900),
            show_original: false,
        };
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(900))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.messages.push(reply);
                cx.notify();
            });
            // Keep rendering until the mock translation lands.
            cx.background_executor()
                .timer(TRANSLATION_DELAY + Duration::from_millis(50))
                .await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        cx.notify();
        true
    }
}

pub fn chat(gallery: &mut Gallery, window: &mut Window, cx: &mut Context<Gallery>) -> AnyElement {
    let colors = cx.md().colors;
    let mut log = div().flex().flex_col().gap_3().px_8().py_6();
    let count = gallery.messages.len();
    for ix in 0..count {
        log = log.child(bubble(gallery, ix, window, cx));
    }

    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_4()
        .px_8()
        .py_4()
        .border_b_1()
        .border_color(colors.outline_variant)
        .child(
            div().size(px(48.0)).child(
                ShapeView::new("channel-avatar", MaterialShape::Cookie9Sided)
                    .size(px(48.0))
                    .color(colors.tertiary_container)
                    .hover_color(colors.primary_container)
                    .morph_on_hover(MaterialShape::Flower),
            ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .child(div().md_type(TypeRole::TitleLarge, cx).child("#babel-dev"))
                .child(
                    div()
                        .md_type(TypeRole::BodyMedium, cx)
                        .text_color(colors.on_surface_variant)
                        .child("3 languages · everyone reads in their own"),
                ),
        )
        .child(
            Chip::new("chat-language", "日本語で表示")
                .kind(ChipKind::Filter)
                .selected(true),
        );

    let composer = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .px_8()
        .py_4()
        .child(
            Toolbar::new()
                .child(IconButton::new("attach", Icon::AttachFile))
                .child(IconButton::new("emoji", Icon::AddReaction)),
        )
        .child(div().flex_1().child(gallery.composer.clone()))
        .child({
            let composer = gallery.composer.clone();
            IconButton::new("send", Icon::Send)
                .label("Send")
                .style(IconButtonStyle::Filled)
                .size(ButtonSize::Medium)
                .width(IconButtonWidth::Wide)
                .on_click(cx.listener(move |this, _, window, cx| {
                    let text = composer.read(cx).text().clone();
                    if this.send(&text, cx) {
                        composer.update(cx, |field, cx| field.set_text("", window, cx));
                    }
                }))
        });

    div()
        .size_full()
        .flex()
        .flex_col()
        .child(header)
        .child(div().id("chat-log").flex_1().overflow_y_scroll().child(log))
        .child(composer)
        .into_any_element()
}

fn bubble(
    gallery: &mut Gallery,
    ix: usize,
    window: &mut Window,
    cx: &mut Context<Gallery>,
) -> AnyElement {
    let colors = cx.md().colors;
    let message = &gallery.messages[ix];
    let id = SharedString::from(format!("message-{ix}"));
    let elapsed = Instant::now().saturating_duration_since(message.sent_at);
    let translating = !message.mine && message.translation.is_some() && elapsed < TRANSLATION_DELAY;
    if translating {
        window.request_animation_frame();
    }

    // New messages rise into place on a spatial spring.
    let appear = animate_from(
        (id.clone(), 0usize),
        0.0,
        1.0,
        MotionToken::DefaultSpatial,
        window,
        cx,
    );
    let show_original = message.show_original || message.translation.is_none();
    let body = if translating {
        message.original.clone()
    } else if show_original {
        message.original.clone()
    } else {
        message.translation.clone().unwrap_or_default()
    };
    let (bg, fg) = if message.mine {
        (colors.primary_container, colors.on_primary_container)
    } else {
        (colors.surface_container_high, colors.on_surface)
    };
    let radius = px(20.0);
    let small = px(6.0);

    let mut meta = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .md_type(TypeRole::LabelSmall, cx)
        .text_color(colors.on_surface_variant);
    if translating {
        meta = meta
            .child(LoadingIndicator::new((id, 1usize)).size(px(20.0)))
            .child("Translating…");
    } else if message.translation.is_some() {
        let label = if show_original {
            format!(
                "Original ({}) · Show translation",
                message.original_language
            )
        } else {
            format!(
                "Translated from {} · Show original",
                message.original_language
            )
        };
        meta = meta
            .child(icon(Icon::Translate).size(px(14.0)).color(colors.primary))
            .child(
                div()
                    .id((id, 2usize))
                    .cursor_pointer()
                    .text_color(colors.primary)
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.messages[ix].show_original = !this.messages[ix].show_original;
                        cx.notify();
                    })),
            );
    }

    let mut column = div()
        .flex()
        .flex_col()
        .gap_1()
        .max_w(px(520.0))
        .relative()
        .top(px((1.0 - appear) * 24.0))
        .opacity(appear.clamp(0.0, 1.0));
    if !message.mine {
        column = column.child(
            div()
                .md_type(TypeRole::LabelMedium, cx)
                .text_color(colors.on_surface_variant)
                .child(message.author.clone()),
        );
    }
    column = column
        .child(
            div()
                .px_4()
                .py_3()
                .bg(bg)
                .text_color(fg)
                .md_type(TypeRole::BodyLarge, cx)
                .md_corners(gpui::Corners {
                    top_left: if message.mine { radius } else { small },
                    top_right: if message.mine { small } else { radius },
                    bottom_left: radius,
                    bottom_right: radius,
                })
                .child(body),
        )
        .child(meta);

    let mut row = div().flex().flex_row().w_full();
    if message.mine {
        row = row.justify_end();
    }
    row.child(column).into_any_element()
}
