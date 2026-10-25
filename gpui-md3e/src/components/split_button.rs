use std::rc::Rc;

use gpui::{
    App, ClickEvent, Corners, ElementId, IntoElement, ParentElement as _, Pixels, RenderOnce, Role,
    SharedString, Styled as _, Window, div, px,
};

use super::{
    ButtonSize, ButtonStyle,
    pressable::{ClickHandler, GroupOverrides, Pressable},
};
use crate::{
    ActiveTheme as _, Icon, StyledExt as _, child_id, icon,
    interaction::Interaction,
    motion::{MotionToken, animate},
    theme::Corner,
};

struct SplitMetrics {
    leading: (Pixels, Pixels),
    trailing: Pixels,
    trailing_icon: Pixels,
    inner: Corner,
    inner_active: Corner,
}

const fn split_metrics(size: ButtonSize) -> SplitMetrics {
    match size {
        ButtonSize::XSmall => SplitMetrics {
            leading: (px(12.0), px(10.0)),
            trailing: px(13.0),
            trailing_icon: px(22.0),
            inner: Corner::ExtraSmall,
            inner_active: Corner::Small,
        },
        ButtonSize::Small => SplitMetrics {
            leading: (px(16.0), px(12.0)),
            trailing: px(13.0),
            trailing_icon: px(22.0),
            inner: Corner::ExtraSmall,
            inner_active: Corner::Medium,
        },
        ButtonSize::Medium => SplitMetrics {
            leading: (px(24.0), px(24.0)),
            trailing: px(15.0),
            trailing_icon: px(26.0),
            inner: Corner::ExtraSmall,
            inner_active: Corner::Medium,
        },
        ButtonSize::Large => SplitMetrics {
            leading: (px(48.0), px(48.0)),
            trailing: px(29.0),
            trailing_icon: px(38.0),
            inner: Corner::Small,
            inner_active: Corner::LargeIncreased,
        },
        ButtonSize::XLarge => SplitMetrics {
            leading: (px(64.0), px(64.0)),
            trailing: px(43.0),
            trailing_icon: px(50.0),
            inner: Corner::Medium,
            inner_active: Corner::LargeIncreased,
        },
    }
}

/// A primary action joined to a menu button. The inner corners soften on
/// hover and press; opening the menu turns the trailing button into a circle
/// and flips its arrow.
#[derive(IntoElement)]
pub struct SplitButton {
    id: ElementId,
    label: SharedString,
    icon: Option<Icon>,
    style: ButtonStyle,
    size: ButtonSize,
    open: bool,
    on_click: Option<ClickHandler>,
    on_toggle: Option<ClickHandler>,
}

impl SplitButton {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            style: ButtonStyle::Filled,
            size: ButtonSize::Small,
            open: false,
            on_click: None,
            on_toggle: None,
        }
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    /// Whether the attached menu is showing.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    pub fn on_toggle(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_toggle = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for SplitButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let base = self.size.metrics();
        let metrics = split_metrics(self.size);
        let theme = cx.md();
        let shapes = theme.shapes;
        let colors = self.style.colors(&theme.colors, None);
        let label_style = theme.typography.style(base.label);
        let height = base.height;
        let full = height / 2.0;
        let inner = shapes.radius(metrics.inner, height);
        let inner_active = shapes.radius(metrics.inner_active, height);

        let leading_id = child_id(&self.id, "leading");
        let trailing_id = child_id(&self.id, "trailing");
        let leading_state = Interaction::use_state(&leading_id, window, cx);
        let trailing_state = Interaction::use_state(&trailing_id, window, cx);
        let leading_snapshot = Interaction::snapshot(&leading_state, window, cx);
        let trailing_snapshot = Interaction::snapshot(&trailing_state, window, cx);

        let active = |snapshot: crate::interaction::InteractionSnapshot| {
            if snapshot.pressed || snapshot.hovered {
                1.0
            } else {
                0.0
            }
        };
        let leading_t = animate(
            child_id(&self.id, "leading-inner"),
            active(leading_snapshot),
            MotionToken::FastSpatial,
            window,
            cx,
        );
        let trailing_t = animate(
            child_id(&self.id, "trailing-inner"),
            active(trailing_snapshot),
            MotionToken::FastSpatial,
            window,
            cx,
        );
        let open_t = animate(
            child_id(&self.id, "open"),
            if self.open { 1.0 } else { 0.0 },
            MotionToken::FastSpatial,
            window,
            cx,
        );

        let leading_inner = inner + (inner_active - inner) * leading_t;
        let trailing_inner = inner + (inner_active - inner) * trailing_t;
        let trailing_inner = trailing_inner + (full - trailing_inner) * open_t;

        let label = self.label.clone();
        let glyph = self.icon;
        let icon_size = base.icon;
        let leading = Pressable {
            id: leading_id.clone(),
            label: self.label.clone(),
            role: Role::Button,
            height,
            width: None,
            padding_x: (metrics.leading.0 + metrics.leading.1) / 2.0,
            gap: base.gap,
            rest_radii: Corners::all(full),
            selected_radii: Corners::all(full),
            pressed_radii: Corners::all(full),
            colors,
            selected_colors: None,
            selected: None,
            elevation: self.style.elevation(),
            outline_width: base.outline,
            disabled: false,
            on_click: self.on_click,
            group: GroupOverrides {
                interaction: Some(leading_state),
                radii: Some(Corners {
                    top_left: full,
                    bottom_left: full,
                    top_right: leading_inner,
                    bottom_right: leading_inner,
                }),
                ..Default::default()
            },
        }
        .render(window, cx, move |state, _, cx| {
            let mut children = Vec::new();
            if let Some(glyph) = glyph {
                children.push(
                    icon(glyph)
                        .size(icon_size)
                        .color(state.content)
                        .into_any_element(),
                );
            }
            children.push(
                div()
                    .md_type_style(label_style, cx)
                    .child(label)
                    .into_any_element(),
            );
            children
        });

        let trailing_icon = metrics.trailing_icon;
        let trailing = Pressable {
            id: trailing_id,
            label: "More options".into(),
            role: Role::Button,
            height,
            width: Some(trailing_icon + metrics.trailing * 2.0),
            padding_x: metrics.trailing,
            gap: px(0.0),
            rest_radii: Corners::all(full),
            selected_radii: Corners::all(full),
            pressed_radii: Corners::all(full),
            colors,
            selected_colors: None,
            selected: Some(self.open),
            elevation: self.style.elevation(),
            outline_width: base.outline,
            disabled: false,
            on_click: self.on_toggle,
            group: GroupOverrides {
                interaction: Some(trailing_state),
                radii: Some(Corners {
                    top_left: trailing_inner,
                    bottom_left: trailing_inner,
                    top_right: full,
                    bottom_right: full,
                }),
                ..Default::default()
            },
        }
        .render(window, cx, move |state, _, _| {
            vec![
                icon(Icon::KeyboardArrowDown)
                    .size(trailing_icon)
                    .color(state.content)
                    .rotation(open_t * 180.0)
                    .into_any_element(),
            ]
        });

        div()
            .flex()
            .flex_row()
            .flex_none()
            .gap(px(2.0))
            .child(leading)
            .child(trailing)
    }
}
