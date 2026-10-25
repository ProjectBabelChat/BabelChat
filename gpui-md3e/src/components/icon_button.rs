use std::rc::Rc;

use gpui::{
    App, ClickEvent, Corners, ElementId, IntoElement, Pixels, RenderOnce, Role, SharedString,
    Window, px, transparent_black,
};

use super::{
    ButtonShape, ButtonSize,
    pressable::{ClickHandler, GroupOverrides, Pressable, SurfaceColors},
};
use crate::{
    ActiveTheme as _, Icon, child_id, icon,
    theme::{ColorScheme, Corner},
};

/// The emphasis of an icon button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum IconButtonStyle {
    #[default]
    Standard,
    Filled,
    Tonal,
    Outlined,
}

/// The horizontal padding of an icon button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum IconButtonWidth {
    Narrow,
    #[default]
    Default,
    Wide,
}

impl IconButtonStyle {
    pub const ALL: [IconButtonStyle; 4] = [
        IconButtonStyle::Standard,
        IconButtonStyle::Filled,
        IconButtonStyle::Tonal,
        IconButtonStyle::Outlined,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            IconButtonStyle::Standard => "Standard",
            IconButtonStyle::Filled => "Filled",
            IconButtonStyle::Tonal => "Tonal",
            IconButtonStyle::Outlined => "Outlined",
        }
    }

    pub fn colors(self, c: &ColorScheme, selected: Option<bool>) -> SurfaceColors {
        let none = transparent_black();
        match (self, selected) {
            (IconButtonStyle::Standard, Some(true)) => SurfaceColors::new(none, c.primary),
            (IconButtonStyle::Standard, _) => SurfaceColors::new(none, c.on_surface_variant),
            (IconButtonStyle::Filled, None | Some(true)) => {
                SurfaceColors::new(c.primary, c.on_primary)
            }
            (IconButtonStyle::Filled, Some(false)) => {
                SurfaceColors::new(c.surface_container, c.on_surface_variant)
            }
            (IconButtonStyle::Tonal, None | Some(false)) => {
                SurfaceColors::new(c.secondary_container, c.on_secondary_container)
            }
            (IconButtonStyle::Tonal, Some(true)) => SurfaceColors::new(c.secondary, c.on_secondary),
            (IconButtonStyle::Outlined, None | Some(false)) => {
                SurfaceColors::outlined(c.on_surface_variant, c.outline_variant)
            }
            (IconButtonStyle::Outlined, Some(true)) => {
                SurfaceColors::new(c.inverse_surface, c.inverse_on_surface)
            }
        }
    }
}

/// Metrics from the `*IconButtonTokens`.
#[derive(Clone, Copy, Debug)]
pub struct IconButtonMetrics {
    pub height: Pixels,
    pub icon: Pixels,
    pub narrow: Pixels,
    pub default: Pixels,
    pub wide: Pixels,
    pub outline: Pixels,
    pub square: Corner,
    pub pressed: Corner,
    /// The corner a selected round button morphs to.
    pub selected_round: Corner,
}

pub const fn icon_button_metrics(size: ButtonSize) -> IconButtonMetrics {
    match size {
        ButtonSize::XSmall => IconButtonMetrics {
            height: px(32.0),
            icon: px(20.0),
            narrow: px(4.0),
            default: px(6.0),
            wide: px(10.0),
            outline: px(1.0),
            square: Corner::Medium,
            pressed: Corner::Small,
            selected_round: Corner::Medium,
        },
        ButtonSize::Small => IconButtonMetrics {
            height: px(40.0),
            icon: px(24.0),
            narrow: px(4.0),
            default: px(8.0),
            wide: px(14.0),
            outline: px(1.0),
            square: Corner::Medium,
            pressed: Corner::Small,
            selected_round: Corner::Medium,
        },
        ButtonSize::Medium => IconButtonMetrics {
            height: px(56.0),
            icon: px(24.0),
            narrow: px(12.0),
            default: px(16.0),
            wide: px(24.0),
            outline: px(1.0),
            square: Corner::Large,
            pressed: Corner::Medium,
            selected_round: Corner::Large,
        },
        ButtonSize::Large => IconButtonMetrics {
            height: px(96.0),
            icon: px(32.0),
            narrow: px(16.0),
            default: px(32.0),
            wide: px(48.0),
            outline: px(2.0),
            square: Corner::ExtraLarge,
            pressed: Corner::Large,
            selected_round: Corner::ExtraLarge,
        },
        ButtonSize::XLarge => IconButtonMetrics {
            height: px(136.0),
            icon: px(40.0),
            narrow: px(32.0),
            default: px(48.0),
            wide: px(72.0),
            outline: px(3.0),
            square: Corner::ExtraLarge,
            pressed: Corner::Large,
            selected_round: Corner::ExtraLarge,
        },
    }
}

/// An icon-only button, optionally a toggle.
#[derive(IntoElement)]
pub struct IconButton {
    id: ElementId,
    icon: Icon,
    label: SharedString,
    style: IconButtonStyle,
    size: ButtonSize,
    width: IconButtonWidth,
    shape: ButtonShape,
    selected: Option<bool>,
    disabled: bool,
    on_click: Option<ClickHandler>,
    pub(crate) group: GroupOverrides,
}

impl IconButton {
    pub fn new(id: impl Into<ElementId>, icon: Icon) -> Self {
        Self {
            id: id.into(),
            icon,
            label: icon.name().into(),
            style: IconButtonStyle::Standard,
            size: ButtonSize::Small,
            width: IconButtonWidth::Default,
            shape: ButtonShape::Round,
            selected: None,
            disabled: false,
            on_click: None,
            group: GroupOverrides::default(),
        }
    }

    /// The accessible name; defaults to the icon's name.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    pub fn style(mut self, style: IconButtonStyle) -> Self {
        self.style = style;
        self
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn width(mut self, width: IconButtonWidth) -> Self {
        self.width = width;
        self
    }

    pub fn shape(mut self, shape: ButtonShape) -> Self {
        self.shape = shape;
        self
    }

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
}

impl RenderOnce for IconButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let metrics = icon_button_metrics(self.size);
        let theme = cx.md();
        let shapes = theme.shapes;
        let colors = theme.colors;
        let padding = match self.width {
            IconButtonWidth::Narrow => metrics.narrow,
            IconButtonWidth::Default => metrics.default,
            IconButtonWidth::Wide => metrics.wide,
        };
        let radius = |corner| Corners::all(shapes.radius(corner, metrics.height));
        let (rest, selected_radii) = match self.shape {
            ButtonShape::Round => (radius(Corner::Full), radius(metrics.selected_round)),
            ButtonShape::Square => (radius(metrics.square), radius(Corner::Full)),
        };
        let toggle = self.selected.is_some();
        let id = self.id.clone();
        let glyph = self.icon;
        let icon_size = metrics.icon;

        Pressable {
            id: self.id,
            label: self.label,
            role: Role::Button,
            height: metrics.height,
            width: Some(metrics.icon + padding * 2.0),
            padding_x: padding,
            gap: px(0.0),
            rest_radii: rest,
            selected_radii: if toggle { selected_radii } else { rest },
            pressed_radii: radius(metrics.pressed),
            colors: self.style.colors(&colors, self.selected.map(|_| false)),
            selected_colors: toggle.then(|| self.style.colors(&colors, Some(true))),
            selected: self.selected,
            elevation: (0.0, 0.0),
            outline_width: metrics.outline,
            disabled: self.disabled,
            on_click: self.on_click,
            group: self.group,
        }
        .render(window, cx, move |state, _, _| {
            vec![
                icon(glyph)
                    .size(icon_size)
                    .color(state.content)
                    .filled(state.selected)
                    .animated(child_id(&id, "icon"))
                    .into_any_element(),
            ]
        })
    }
}

impl IconButton {
    /// Width without group adjustments, height and horizontal padding.
    pub(crate) fn natural_metrics(&self) -> (Pixels, Pixels, Pixels) {
        let metrics = icon_button_metrics(self.size);
        let padding = match self.width {
            IconButtonWidth::Narrow => metrics.narrow,
            IconButtonWidth::Default => metrics.default,
            IconButtonWidth::Wide => metrics.wide,
        };
        (metrics.icon + padding * 2.0, metrics.height, padding)
    }

    pub(crate) fn id(&self) -> &ElementId {
        &self.id
    }

    pub(crate) fn is_selected(&self) -> bool {
        self.selected == Some(true)
    }
}
