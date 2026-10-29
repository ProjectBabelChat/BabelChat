//! Emoji in the composer, as on Discord: `:pa` offers the emoji whose names
//! match (the user's guilds' own first, then Unicode emoji by shortcode),
//! and a `:name:` typed in full becomes its emoji at once. A guild's emoji
//! goes into the text as its code, which the composer shows as its picture.

use std::ops::Range;

use babel_sdk::emoji_code;
use gpui::prelude::*;
use gpui::{AnyElement, App, Context, Focusable, ObjectFit, SharedString, Window, div, img, px};
use gpui_md3e::prelude::*;

use super::ChatView;
use crate::i18n::fill;

/// Letters a query needs before emoji are offered.
const MIN_QUERY: usize = 2;
/// Emoji offered at once.
const SHOWN: usize = 10;

/// Emoji offered for the name being typed.
pub(super) struct Completion {
    /// Where `:query` is in the composer's text.
    range: Range<usize>,
    query: String,
    items: Vec<Suggestion>,
    selected: usize,
}

struct Suggestion {
    /// What goes into the text: a Unicode emoji, or a guild's emoji's code.
    text: String,
    /// The name that matched: a shortcode, or a guild's emoji's name.
    name: String,
    /// A guild's emoji: its guild's ID and name, and its ID.
    custom: Option<(String, SharedString, String)>,
}

/// Letters of emoji names and shortcodes (`+1`, `t-rex`).
fn name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-')
}

/// The `:name` being typed at the end of `before`, with where its colon is:
/// a colon at the start, after a space or after punctuation (not in `10:30`,
/// nor in a code's `<:`).
fn typing_name(before: &str) -> Option<(usize, &str)> {
    let start = before.trim_end_matches(name_char).len();
    let colon = start.checked_sub(1)?;
    if !before[..start].ends_with(':') {
        return None;
    }
    let ok = before[..colon]
        .chars()
        .next_back()
        .is_none_or(|c| !c.is_alphanumeric() && !matches!(c, ':' | '<'));
    ok.then(|| (colon, &before[start..]))
        .filter(|(_, name)| !name.is_empty())
}

/// The `:name:` just finished at the end of `before`, with where it starts.
fn typed_name(before: &str) -> Option<(usize, &str)> {
    typing_name(before.strip_suffix(':')?)
}

/// How well `name` matches `query` (lower case): 0 exactly, 1 at its
/// start, 2 within it.
fn rank(name: &str, query: &str) -> Option<u8> {
    let name = name.to_lowercase();
    if name == query {
        Some(0)
    } else if name.starts_with(query) {
        Some(1)
    } else {
        name.contains(query).then_some(2)
    }
}

/// The Unicode emoji whose shortcodes match `query`, with the best match.
fn unicode_matches(query: &str) -> impl Iterator<Item = (u8, Suggestion)> + '_ {
    emojis::iter().filter_map(move |emoji| {
        let (rank, name) = emoji
            .shortcodes()
            .filter_map(|code| Some((rank(code, query)?, code)))
            .min_by_key(|(rank, _)| *rank)?;
        Some((
            rank,
            Suggestion {
                text: emoji.as_str().to_string(),
                name: name.to_string(),
                custom: None,
            },
        ))
    })
}

impl ChatView {
    /// What `:name:` means here: the user's guilds' emoji first, as when
    /// sending, then a Unicode emoji's shortcode.
    fn emoji_for_name(&self, name: &str, cx: &App) -> Option<String> {
        let custom = self
            .guild
            .as_deref()
            .and_then(|guild| self.session.find_emoji(guild, name))
            .filter(|custom| !self.emoji_gone(&custom.id, cx));
        match custom {
            Some(custom) => Some(emoji_code(&custom)),
            None => emojis::get_by_shortcode(name).map(|emoji| emoji.as_str().to_string()),
        }
    }

    /// The composer changed: a `:name:` just finished becomes its emoji,
    /// and a `:name` being typed is offered emoji.
    pub(super) fn composer_edited(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.fetch_composer_pictures(cx);
        let composer = self.composer.read(cx);
        let (text, caret) = (composer.text().clone(), composer.caret());
        if composer.is_composing() {
            self.completion = None;
            return;
        }
        if let Some((start, name)) = typed_name(&text[..caret])
            && let Some(emoji) = self.emoji_for_name(name, cx)
        {
            self.completion = None;
            self.composer.update(cx, |field, cx| {
                field.replace(start..caret, &emoji, window, cx)
            });
            return;
        }
        let Some((start, query)) =
            typing_name(&text[..caret]).filter(|(_, query)| query.chars().count() >= MIN_QUERY)
        else {
            self.completion = None;
            cx.notify();
            return;
        };
        let query = query.to_lowercase();
        let selected = match &self.completion {
            Some(open) if open.query == query => open.selected,
            _ => 0,
        };
        let items = self.suggestions(&query, cx);
        self.completion = (!items.is_empty()).then(|| Completion {
            range: start..caret,
            selected: selected.min(items.len() - 1),
            query,
            items,
        });
        cx.notify();
    }

    /// The emoji matching `query`: the best matches first, guilds' emoji
    /// before Unicode ones that match as well.
    fn suggestions(&mut self, query: &str, cx: &mut Context<Self>) -> Vec<Suggestion> {
        let mut usable = self.session.usable_emoji(self.guild.as_deref());
        usable.retain(|custom| !self.emoji_gone(&custom.id, cx));
        let mut found = usable
            .into_iter()
            .filter_map(|custom| {
                let rank = rank(&custom.name, query)?;
                let guild = self
                    .session
                    .guild(&custom.guild_id)
                    .map(|view| view.guild.name)
                    .unwrap_or_default();
                Some((
                    rank,
                    Suggestion {
                        text: emoji_code(&custom),
                        name: custom.name,
                        custom: Some((custom.guild_id, guild.into(), custom.id)),
                    },
                ))
            })
            .chain(unicode_matches(query))
            .collect::<Vec<_>>();
        found.sort_by_key(|(rank, _)| *rank);
        found
            .into_iter()
            .take(SHOWN)
            .map(|(_, suggestion)| suggestion)
            .collect()
    }

    /// Whether emoji are offered for what the composer holds now (it may
    /// have been cleared or changed since).
    pub(super) fn completing(&self, cx: &App) -> bool {
        let Some(open) = &self.completion else {
            return false;
        };
        let composer = self.composer.read(cx);
        composer.caret() == open.range.end
            && composer
                .text()
                .get(open.range.clone())
                .is_some_and(|typed| typed[1..].eq_ignore_ascii_case(&open.query))
    }

    /// Up or Down in the offered emoji.
    pub(super) fn move_completion(&mut self, by: isize, cx: &mut Context<Self>) {
        if let Some(open) = &mut self.completion {
            let count = open.items.len() as isize;
            open.selected = (open.selected as isize + by).rem_euclid(count) as usize;
            cx.notify();
        }
    }

    /// Writes the offered emoji `ix` (the selected one if None) in place of
    /// the name typed.
    pub(super) fn accept_completion(
        &mut self,
        ix: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.completing(cx) {
            self.completion = None;
            return;
        }
        let Some(open) = self.completion.take() else {
            return;
        };
        let Some(chosen) = open.items.get(ix.unwrap_or(open.selected)) else {
            return;
        };
        let text = format!("{} ", chosen.text);
        // A closing colon typed first (`::`, then the name) goes too.
        let mut range = open.range.clone();
        if self.composer.read(cx).text()[range.end..].starts_with(':') {
            range.end += 1;
        }
        self.composer
            .update(cx, |field, cx| field.replace(range, &text, window, cx));
        window.focus(&self.composer.focus_handle(cx), cx);
        cx.notify();
    }

    /// The offered emoji, over the composer.
    pub(super) fn render_completion(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.completing(cx) {
            return None;
        }
        let colors = cx.md().colors;
        let t = self.t();
        let open = self.completion.as_ref()?;
        let (query, selected) = (open.query.clone(), open.selected);
        let items = open
            .items
            .iter()
            .map(|item| (item.text.clone(), item.name.clone(), item.custom.clone()))
            .collect::<Vec<_>>();
        let mut list = div()
            .flex()
            .flex_col()
            .p_2()
            .rounded(px(12.0))
            .bg(colors.surface_container)
            .shadow_md()
            .child(
                div()
                    .px_2()
                    .pb_1()
                    .md_type(TypeRole::LabelMedium, cx)
                    .text_color(colors.on_surface_variant)
                    .child(fill(t.emoji_matching, &[&format!(":{query}")])),
            );
        for (ix, (text, name, custom)) in items.into_iter().enumerate() {
            let face = match &custom {
                Some((guild_id, _, id)) => match self.emoji_picture(guild_id, id, cx) {
                    Some(image) => img(image)
                        .size(px(24.0))
                        .object_fit(ObjectFit::Contain)
                        .into_any_element(),
                    None => div().size(px(24.0)).into_any_element(),
                },
                None => div()
                    .w(px(24.0))
                    .text_size(px(20.0))
                    .child(text)
                    .into_any_element(),
            };
            let mut row = div()
                .id(("emoji-completion", ix))
                .h(px(36.0))
                .px_2()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .rounded(px(8.0))
                .cursor_pointer()
                .hover(|style| style.bg(colors.on_surface.opacity(0.08)))
                .child(face)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .md_type(TypeRole::BodyLarge, cx)
                        .text_color(colors.on_surface)
                        .child(format!(":{name}:")),
                )
                .children(custom.map(|(_, guild, _)| {
                    div()
                        .md_type(TypeRole::LabelMedium, cx)
                        .text_color(colors.on_surface_variant)
                        .child(guild)
                }))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.accept_completion(Some(ix), window, cx)
                }));
            if ix == selected {
                row = row.bg(colors.secondary_container);
            }
            list = list.child(row);
        }
        Some(list.into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_found_where_they_are_typed() {
        assert_eq!(typing_name(":pa"), Some((0, "pa")));
        assert_eq!(typing_name("hi :party_1"), Some((3, "party_1")));
        assert_eq!(typing_name("(:+1"), Some((1, "+1")));
        assert_eq!(typing_name("10:30"), None);
        assert_eq!(typing_name("<:party"), None);
        assert_eq!(typing_name("hi :"), None);
        assert_eq!(typing_name("hi"), None);
        assert_eq!(typed_name("so :smile:"), Some((3, "smile")));
        assert_eq!(typed_name("so :smile"), None);
        assert_eq!(typed_name("at 10:30:"), None);
    }

    #[test]
    fn shortcodes_rank_by_how_they_match() {
        let found = unicode_matches("smile")
            .map(|(rank, suggestion)| (rank, suggestion.name))
            .collect::<Vec<_>>();
        assert!(found.contains(&(0, "smile".to_string())));
        assert!(found.iter().any(|(rank, _)| *rank == 2));
        assert_eq!(rank("Party", "par"), Some(1));
        assert_eq!(rank("cat", "dog"), None);
    }
}
