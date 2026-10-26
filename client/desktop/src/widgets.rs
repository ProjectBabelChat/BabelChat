//! Small pieces shared by the screens: avatars, badges and dialogs.

use gpui::prelude::*;
use gpui::{AnyElement, App, ElementId, Hsla, IntoElement, SharedString, div, hsla, px};
use gpui_md3e::prelude::*;

/// A stable color for a person or guild, from their name.
pub fn name_color(name: &str, dark: bool) -> Hsla {
    let hash = name.bytes().fold(2166136261u32, |hash, byte| {
        (hash ^ byte as u32).wrapping_mul(16777619)
    });
    let hue = (hash % 360) as f32 / 360.0;
    if dark {
        hsla(hue, 0.45, 0.32, 1.0)
    } else {
        hsla(hue, 0.55, 0.78, 1.0)
    }
}

/// The first letter of a name, for avatars.
pub fn initial(name: &str) -> String {
    name.chars()
        .next()
        .map(|ch| ch.to_uppercase().collect())
        .unwrap_or_default()
}

/// A round avatar with the name's initial.
pub fn avatar(name: &str, size: f32, cx: &App) -> gpui::Div {
    let theme = cx.md();
    let role = if size >= 36.0 {
        TypeRole::TitleMedium
    } else {
        TypeRole::LabelMedium
    };
    div()
        .flex_none()
        .size(px(size))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(name_color(name, theme.is_dark()))
        .text_color(theme.colors.on_surface)
        .md_type(role, cx)
        .child(initial(name))
}

/// A count in an error-colored pill (unread mentions).
pub fn badge(count: i64, cx: &App) -> gpui::Div {
    let colors = cx.md().colors;
    let label = if count > 99 {
        "99+".to_string()
    } else {
        count.to_string()
    };
    div()
        .flex_none()
        .min_w(px(18.0))
        .h(px(18.0))
        .px_1()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(colors.error)
        .text_color(colors.on_error)
        .md_type(TypeRole::LabelSmall, cx)
        .child(label)
}

/// A modal dialog over a scrim. Clicking the scrim calls `on_dismiss`.
pub fn dialog(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    body: impl IntoElement,
    actions: Vec<AnyElement>,
    on_dismiss: impl Fn(&mut gpui::Window, &mut App) + 'static,
    cx: &App,
) -> AnyElement {
    let colors = cx.md().colors;
    let id = id.into();
    let card = div()
        .id(gpui_md3e::child_id(&id, "card"))
        .occlude()
        .w(px(440.0))
        .max_w_full()
        .flex()
        .flex_col()
        .gap_4()
        .p_6()
        .rounded(px(28.0))
        .bg(colors.surface_container_high)
        .text_color(colors.on_surface)
        .md_elevation(3.0, colors.shadow)
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            div()
                .md_type(TypeRole::HeadlineSmall, cx)
                .child(title.into()),
        )
        .child(body)
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .children(actions),
        );
    div()
        .id(id)
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .bg(colors.scrim.opacity(0.32))
        .on_mouse_down(gpui::MouseButton::Left, move |_, window, cx| {
            on_dismiss(window, cx)
        })
        .child(card)
        .into_any_element()
}

/// A line of secondary text.
pub fn supporting(text: impl Into<SharedString>, cx: &App) -> gpui::Div {
    div()
        .md_type(TypeRole::BodyMedium, cx)
        .text_color(cx.md().colors.on_surface_variant)
        .child(text.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatars_show_the_first_letter() {
        assert_eq!(initial("alice"), "A");
        assert_eq!(initial("はるか"), "は");
        assert_eq!(initial(""), "");
    }

    #[test]
    fn names_keep_their_color() {
        assert_eq!(name_color("alice", false), name_color("alice", false));
        assert_ne!(name_color("alice", false), name_color("bob", false));
    }
}
