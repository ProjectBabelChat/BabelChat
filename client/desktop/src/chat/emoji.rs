//! Choosing an emoji, to react with or to write: the user's guilds' own
//! first (the open guild's leading), then a few common ones, then every
//! Unicode emoji, all searchable by name or shortcode.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Entity, EventEmitter, FocusHandle, Focusable, Image, IntoElement,
    ObjectFit, SharedString, Subscription, Window, div, img, px, uniform_list,
};
use gpui_md3e::{
    components::{TextField, TextFieldEvent},
    prelude::*,
};

/// Offered before searching.
pub const COMMON: [&str; 8] = ["👍", "❤️", "😂", "😮", "😢", "🙏", "🎉", "👀"];
const COLUMNS: usize = 8;
const CELL: f32 = 40.0;
/// Rows of a guild's emoji shown before the list scrolls past them.
const CUSTOM_ROWS: usize = 2;

pub enum EmojiEvent {
    /// A Unicode emoji, or a guild's emoji's code.
    Picked(String),
}

/// A guild's emoji the user may use.
#[derive(Clone)]
pub struct CustomEmoji {
    pub code: String,
    pub name: String,
    /// Its guild's name.
    pub guild: SharedString,
    /// None while loading.
    pub image: Option<Arc<Image>>,
}

pub struct EmojiPicker {
    search: Entity<TextField>,
    custom: Vec<CustomEmoji>,
    /// What the search leaves, in the order emoji are listed.
    custom_shown: Vec<CustomEmoji>,
    shown: Vec<&'static emojis::Emoji>,
    /// What the pointer is on, said under the list.
    hovered: SharedString,
    empty_text: SharedString,
    _subscription: Subscription,
}

impl EventEmitter<EmojiEvent> for EmojiPicker {}

impl EmojiPicker {
    pub fn new(
        placeholder: &str,
        empty_text: &str,
        custom: Vec<CustomEmoji>,
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
                        let first = this
                            .custom_shown
                            .first()
                            .map(|custom| custom.code.clone())
                            .or_else(|| this.shown.first().map(|emoji| emoji.as_str().to_string()));
                        if let Some(first) = first {
                            cx.emit(EmojiEvent::Picked(first));
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
            custom_shown: custom.clone(),
            custom,
            shown: emojis::iter().collect(),
            hovered: SharedString::default(),
            empty_text: empty_text.to_string().into(),
            _subscription: subscription,
        }
    }

    /// The guild's emoji changed, or their pictures arrived.
    pub fn set_custom(&mut self, custom: Vec<CustomEmoji>, cx: &mut Context<Self>) {
        self.custom = custom;
        self.filter(cx);
    }

    fn query(&self, cx: &Context<Self>) -> String {
        let query = self.search.read(cx).text().trim().to_lowercase();
        query.trim_matches(':').to_string()
    }

    fn filter(&mut self, cx: &mut Context<Self>) {
        let query = self.query(cx);
        self.custom_shown = self
            .custom
            .iter()
            .filter(|custom| custom.name.to_lowercase().contains(&query))
            .cloned()
            .collect();
        self.shown = emojis::iter()
            .filter(|emoji| {
                query.is_empty()
                    || emoji.name().contains(query.as_str())
                    || emoji.shortcodes().any(|code| code.contains(query.as_str()))
            })
            .collect();
        cx.notify();
    }

    fn cell(
        &self,
        id: SharedString,
        face: AnyElement,
        label: SharedString,
        picked: String,
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
                    this.hovered = label.clone();
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(EmojiEvent::Picked(picked.clone()))))
            .child(face)
            .into_any_element()
    }

    fn unicode_cell(
        &self,
        emoji: &'static emojis::Emoji,
        id: SharedString,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = format!(
            "{}  :{}:",
            emoji.as_str(),
            emoji.shortcode().unwrap_or(emoji.name())
        );
        self.cell(
            id,
            div().child(emoji.as_str()).into_any_element(),
            label.into(),
            emoji.as_str().to_string(),
            cx,
        )
    }

    fn custom_cell(&self, custom: &CustomEmoji, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let face = match &custom.image {
            Some(image) => img(image.clone())
                .size(px(28.0))
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            None => div().size(px(28.0)).into_any_element(),
        };
        let label = format!(":{}:  {}", custom.name, custom.guild);
        self.cell(
            SharedString::from(format!("custom-{ix}")),
            face,
            label.into(),
            custom.code.clone(),
            cx,
        )
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
        let searching = !self.query(cx).is_empty();
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
        if !self.custom_shown.is_empty() {
            let cells = self
                .custom_shown
                .iter()
                .enumerate()
                .map(|(ix, custom)| self.custom_cell(custom, ix, cx))
                .collect::<Vec<_>>();
            column = column.child(
                div()
                    .id("custom-emoji")
                    .flex_none()
                    .max_h(px(CELL * CUSTOM_ROWS as f32))
                    .overflow_y_scroll()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .pb_2()
                    .border_b_1()
                    .border_color(colors.outline_variant)
                    .children(cells),
            );
        }
        if !searching {
            let common = COMMON
                .iter()
                .filter_map(|text| emojis::get(text))
                .enumerate()
                .map(|(ix, emoji)| {
                    self.unicode_cell(emoji, SharedString::from(format!("common-{ix}")), cx)
                })
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
        if rows == 0 && self.custom_shown.is_empty() {
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
                                        this.unicode_cell(emoji, id.into(), cx)
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
        column.child(
            div()
                .h(px(20.0))
                .md_type(TypeRole::LabelMedium, cx)
                .text_color(colors.on_surface_variant)
                .truncate()
                .child(self.hovered.clone()),
        )
    }
}
