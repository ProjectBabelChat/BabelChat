//! Choosing an emoji to react with: a few common ones, then every emoji,
//! searchable by name or shortcode.
//!
//! Only Unicode emoji for now. A guild's own emoji (`custom:<id>`, usable
//! in other guilds too) would come first here once they exist.

use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, SharedString,
    Subscription, Window, div, px, uniform_list,
};
use gpui_md3e::{
    components::{TextField, TextFieldEvent},
    prelude::*,
};

/// Offered before searching.
pub const COMMON: [&str; 8] = ["👍", "❤️", "😂", "😮", "😢", "🙏", "🎉", "👀"];
const COLUMNS: usize = 8;
const CELL: f32 = 40.0;

pub enum EmojiEvent {
    Picked(String),
}

pub struct EmojiPicker {
    search: Entity<TextField>,
    /// What the search leaves, in the order emoji are listed.
    shown: Vec<&'static emojis::Emoji>,
    hovered: Option<&'static emojis::Emoji>,
    empty_text: SharedString,
    _subscription: Subscription,
}

impl EventEmitter<EmojiEvent> for EmojiPicker {}

impl EmojiPicker {
    pub fn new(
        placeholder: &str,
        empty_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let this = cx.entity().downgrade();
        let placeholder = placeholder.to_string();
        let search = cx.new(|cx| {
            TextField::new(cx)
                .placeholder(placeholder)
                .leading_icon(Icon::Search)
                .style(TextFieldStyle::Outlined)
                // Enter takes the first match.
                .on_submit(move |_, _, cx| {
                    let _ = this.update(cx, |this, cx| {
                        if let Some(emoji) = this.shown.first() {
                            cx.emit(EmojiEvent::Picked(emoji.as_str().to_string()));
                        }
                    });
                    false
                })
        });
        let subscription = cx.subscribe(&search, |this, _, event: &TextFieldEvent, cx| {
            if *event == TextFieldEvent::Edited {
                this.filter(cx);
            }
        });
        window.focus(&search.focus_handle(cx), cx);
        Self {
            search,
            shown: emojis::iter().collect(),
            hovered: None,
            empty_text: empty_text.to_string().into(),
            _subscription: subscription,
        }
    }

    fn filter(&mut self, cx: &mut Context<Self>) {
        let query = self.search.read(cx).text().trim().to_lowercase();
        let query = query.trim_matches(':');
        self.shown = emojis::iter()
            .filter(|emoji| {
                query.is_empty()
                    || emoji.name().contains(query)
                    || emoji.shortcodes().any(|code| code.contains(query))
            })
            .collect();
        cx.notify();
    }

    fn cell(
        &self,
        emoji: &'static emojis::Emoji,
        id: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.md().colors;
        div()
            .id(id)
            .size(px(CELL))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(12.0))
            .text_size(px(24.0))
            .cursor_pointer()
            .hover(|style| style.bg(colors.surface_container_highest))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.hovered = Some(emoji);
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |_, _, _, cx| {
                cx.emit(EmojiEvent::Picked(emoji.as_str().to_string()))
            }))
            .child(emoji.as_str())
            .into_any_element()
    }
}

impl Focusable for EmojiPicker {
    fn focus_handle(&self, cx: &gpui::App) -> FocusHandle {
        self.search.focus_handle(cx)
    }
}

impl Render for EmojiPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.md().colors;
        let searching = !self.search.read(cx).text().trim().is_empty();
        let mut column = div()
            .w(px(CELL * COLUMNS as f32 + 24.0))
            .h(px(400.0))
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .rounded(px(20.0))
            .bg(colors.surface_container)
            .md_elevation(3.0, colors.shadow)
            .child(self.search.clone());
        if !searching {
            let common = COMMON
                .iter()
                .filter_map(|text| emojis::get(text))
                .enumerate()
                .map(|(ix, emoji)| self.cell(emoji, SharedString::from(format!("common-{ix}")), cx))
                .collect::<Vec<_>>();
            column = column.child(
                div()
                    .flex()
                    .flex_row()
                    .pb_2()
                    .border_b_1()
                    .border_color(colors.outline_variant)
                    .children(common),
            );
        }
        let rows = self.shown.len().div_ceil(COLUMNS);
        if rows == 0 {
            column = column.child(
                div()
                    .flex_1()
                    .p_4()
                    .md_type(TypeRole::BodyMedium, cx)
                    .text_color(colors.on_surface_variant)
                    .child(self.empty_text.clone()),
            );
        } else {
            column = column.child(
                uniform_list(
                    "emoji-rows",
                    rows,
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        range
                            .map(|row| {
                                let cells = this
                                    .shown
                                    .iter()
                                    .skip(row * COLUMNS)
                                    .take(COLUMNS)
                                    .enumerate()
                                    .map(|(col, emoji)| {
                                        let id = format!("emoji-{}", row * COLUMNS + col);
                                        this.cell(emoji, id.into(), cx)
                                    })
                                    .collect::<Vec<_>>();
                                div().flex().flex_row().children(cells)
                            })
                            .collect()
                    }),
                )
                .flex_1(),
            );
        }
        let name = self
            .hovered
            .map(|emoji| {
                format!(
                    "{}  :{}:",
                    emoji.as_str(),
                    emoji.shortcode().unwrap_or(emoji.name())
                )
            })
            .unwrap_or_default();
        column.child(
            div()
                .h(px(20.0))
                .md_type(TypeRole::LabelMedium, cx)
                .text_color(colors.on_surface_variant)
                .truncate()
                .child(name),
        )
    }
}
