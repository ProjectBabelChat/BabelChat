use std::rc::Rc;

use gpui::{
    App, ClickEvent, Corners, ElementId, IntoElement, ParentElement as _, Pixels, RenderOnce, Role,
    SharedString, Window, div, px, transparent_black,
};

use super::pressable::{ClickHandler, GroupOverrides, Pressable, SurfaceColors};
use crate::{
    ActiveTheme as _, Icon, StyledExt as _, child_id, icon,
    theme::{ColorScheme, Corner, TypeRole},
};

/// The emphasis of a button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ButtonStyle {
    Filled,
    Tonal,
    Outlined,
    Elevated,
    Text,
}

/// The five M3 Expressive button sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ButtonSize {
    XSmall,
    Small,
    Medium,
    Large,
    XLarge,
}

/// Round buttons are pills; square buttons use a size-dependent corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ButtonShape {
    #[default]
    Round,
    Square,
}

/// Size-dependent metrics from the `Button*Tokens`.
#[derive(Clone, Copy, Debug)]
pub struct ButtonMetrics {
    pub height: Pixels,
    pub padding: Pixels,
    pub icon: Pixels,
    pub gap: Pixels,
    pub outline: Pixels,
    pub label: TypeRole,
    pub square: Corner,
    pub pressed: Corner,
}

impl ButtonSize {
    pub const ALL: [ButtonSize; 5] = [
        ButtonSize::XSmall,
        ButtonSize::Small,
        ButtonSize::Medium,
        ButtonSize::Large,
        ButtonSize::XLarge,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            ButtonSize::XSmall => "XS",
            ButtonSize::Small => "S",
            ButtonSize::Medium => "M",
            ButtonSize::Large => "L",
            ButtonSize::XLarge => "XL",
        }
    }

    pub const fn metrics(self) -> ButtonMetrics {
        match self {
            ButtonSize::XSmall => ButtonMetrics {
                height: px(32.0),
                padding: px(12.0),
                icon: px(20.0),
                gap: px(4.0),
                outline: px(1.0),
                label: TypeRole::LabelLarge,
                square: Corner::Medium,
                pressed: Corner::Small,
            },
            ButtonSize::Small => ButtonMetrics {
                height: px(40.0),
                padding: px(16.0),
                icon: px(20.0),
                gap: px(8.0),
                outline: px(1.0),
                label: TypeRole::LabelLarge,
                square: Corner::Medium,
                pressed: Corner::Small,
            },
            ButtonSize::Medium => ButtonMetrics {
                height: px(56.0),
                padding: px(24.0),
                icon: px(24.0),
                gap: px(8.0),
                outline: px(1.0),
                label: TypeRole::TitleMedium,
                square: Corner::Large,
                pressed: Corner::Medium,
            },
            ButtonSize::Large => ButtonMetrics {
                height: px(96.0),
                padding: px(48.0),
                icon: px(32.0),
                gap: px(12.0),
                outline: px(2.0),
                label: TypeRole::HeadlineSmall,
                square: Corner::ExtraLarge,
                pressed: Corner::Large,
            },
            ButtonSize::XLarge => ButtonMetrics {
                height: px(136.0),
                padding: px(64.0),
                icon: px(40.0),
                gap: px(16.0),
                outline: px(3.0),
                label: TypeRole::HeadlineLarge,
                square: Corner::ExtraLarge,
                pressed: Corner::Large,
            },
        }
    }
}

impl ButtonStyle {
    pub const ALL: [ButtonStyle; 5] = [
        ButtonStyle::Elevated,
        ButtonStyle::Filled,
        ButtonStyle::Tonal,
        ButtonStyle::Outlined,
        ButtonStyle::Text,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            ButtonStyle::Filled => "Filled",
            ButtonStyle::Tonal => "Tonal",
            ButtonStyle::Outlined => "Outlined",
            ButtonStyle::Elevated => "Elevated",
            ButtonStyle::Text => "Text",
        }
    }

    /// Colors for a plain button, or for a toggle button's selected and
    /// unselected states.
    pub fn colors(self, c: &ColorScheme, selected: Option<bool>) -> SurfaceColors {
        match (self, selected) {
            (ButtonStyle::Filled, None | Some(true)) => SurfaceColors::new(c.primary, c.on_primary),
            (ButtonStyle::Filled, Some(false)) => {
                SurfaceColors::new(c.surface_container, c.on_surface_variant)
            }
            (ButtonStyle::Tonal, None | Some(false)) => {
                SurfaceColors::new(c.secondary_container, c.on_secondary_container)
            }
            (ButtonStyle::Tonal, Some(true)) => SurfaceColors::new(c.secondary, c.on_secondary),
            (ButtonStyle::Outlined, None | Some(false)) => {
                SurfaceColors::outlined(c.on_surface_variant, c.outline_variant)
            }
            (ButtonStyle::Outlined, Some(true)) => {
                SurfaceColors::new(c.inverse_surface, c.inverse_on_surface)
            }
            (ButtonStyle::Elevated, None | Some(false)) => {
                SurfaceColors::new(c.surface_container_low, c.primary)
            }
            (ButtonStyle::Elevated, Some(true)) => SurfaceColors::new(c.primary, c.on_primary),
            (ButtonStyle::Text, _) => SurfaceColors::new(transparent_black(), c.primary),
        }
    }

    /// Resting and hovered elevation levels.
    pub const fn elevation(self) -> (f32, f32) {
        match self {
            ButtonStyle::Elevated => (1.0, 2.0),
            ButtonStyle::Filled | ButtonStyle::Tonal => (0.0, 1.0),
            ButtonStyle::Outlined | ButtonStyle::Text => (0.0, 0.0),
        }
    }
}

/// A common button, optionally acting as a toggle.
///
/// Pressing morphs the container toward a squarer shape with a fast spatial
/// spring; toggling morphs between the round and square shapes.
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    style: ButtonStyle,
    size: ButtonSize,
    shape: ButtonShape,
    icon: Option<Icon>,
    selected: Option<bool>,
    disabled: bool,
    on_click: Option<ClickHandler>,
    pub(crate) group: GroupOverrides,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            style: ButtonStyle::Filled,
            size: ButtonSize::Small,
            shape: ButtonShape::Round,
            icon: None,
            selected: None,
            disabled: false,
            on_click: None,
            group: GroupOverrides::default(),
        }
    }

    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn shape(mut self, shape: ButtonShape) -> Self {
        self.shape = shape;
        self
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Makes this a toggle button in the given state.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = Some(selected);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }

    /// Width without group adjustments, height and horizontal padding.
    pub(crate) fn natural_metrics(&self, window: &Window, cx: &App) -> (Pixels, Pixels, Pixels) {
        let metrics = self.size.metrics();
        let style = cx.md().typography.style(metrics.label);
        let text = crate::measure_text(&self.label, style, window, cx).ceil();
        let icon = self.icon.map_or(px(0.0), |_| metrics.icon + metrics.gap);
        (
            metrics.padding * 2.0 + icon + text,
            metrics.height,
            metrics.padding,
        )
    }

    pub(crate) fn id(&self) -> &ElementId {
        &self.id
    }

    pub(crate) fn is_selected(&self) -> bool {
        self.selected == Some(true)
    }
}

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let metrics = self.size.metrics();
        let theme = cx.md();
        let shapes = theme.shapes;
        let colors = theme.colors;
        let label_style = theme.typography.style(metrics.label);

        let round = Corners::all(shapes.radius(Corner::Full, metrics.height));
        let square = Corners::all(shapes.radius(metrics.square, metrics.height));
        let (rest, selected_radii) = match self.shape {
            ButtonShape::Round => (round, square),
            ButtonShape::Square => (square, round),
        };
        let toggle = self.selected.is_some();
        let id = self.id.clone();
        let label = self.label.clone();
        let icon_glyph = self.icon;
        let icon_size = metrics.icon;

        Pressable {
            id: self.id,
            label: self.label,
            role: Role::Button,
            height: metrics.height,
            width: None,
            padding_x: metrics.padding,
            gap: metrics.gap,
            rest_radii: rest,
            selected_radii: if toggle { selected_radii } else { rest },
            pressed_radii: Corners::all(shapes.radius(metrics.pressed, metrics.height)),
            colors: self.style.colors(&colors, self.selected.map(|_| false)),
            selected_colors: toggle.then(|| self.style.colors(&colors, Some(true))),
            selected: self.selected,
            elevation: self.style.elevation(),
            outline_width: metrics.outline,
            disabled: self.disabled,
            on_click: self.on_click,
            group: self.group,
        }
        .render(window, cx, move |state, _, cx| {
            let mut children = Vec::with_capacity(2);
            if let Some(glyph) = icon_glyph {
                children.push(
                    icon(glyph)
                        .size(icon_size)
                        .color(state.content)
                        .filled(state.selected)
                        .animated(child_id(&id, "icon"))
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
        })
    }
}
