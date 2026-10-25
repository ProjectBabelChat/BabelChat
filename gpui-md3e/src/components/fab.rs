use std::rc::Rc;

use gpui::{
    App, ClickEvent, Corners, ElementId, IntoElement, ParentElement as _, Pixels, RenderOnce, Role,
    SharedString, Styled as _, Window, div, px,
};

use super::pressable::{ClickHandler, GroupOverrides, Pressable, SurfaceColors};
use crate::{
    ActiveTheme as _, Icon, StyledExt as _, child_id, icon, measure_text,
    motion::{MotionToken, animate},
    theme::{ColorScheme, Corner, TypeRole},
};

/// The color pairing of a floating action button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FabColor {
    #[default]
    PrimaryContainer,
    SecondaryContainer,
    TertiaryContainer,
    Primary,
    Secondary,
    Tertiary,
}

impl FabColor {
    pub const ALL: [FabColor; 6] = [
        FabColor::PrimaryContainer,
        FabColor::SecondaryContainer,
        FabColor::TertiaryContainer,
        FabColor::Primary,
        FabColor::Secondary,
        FabColor::Tertiary,
    ];

    pub fn colors(self, c: &ColorScheme) -> SurfaceColors {
        match self {
            FabColor::PrimaryContainer => {
                SurfaceColors::new(c.primary_container, c.on_primary_container)
            }
            FabColor::SecondaryContainer => {
                SurfaceColors::new(c.secondary_container, c.on_secondary_container)
            }
            FabColor::TertiaryContainer => {
                SurfaceColors::new(c.tertiary_container, c.on_tertiary_container)
            }
            FabColor::Primary => SurfaceColors::new(c.primary, c.on_primary),
            FabColor::Secondary => SurfaceColors::new(c.secondary, c.on_secondary),
            FabColor::Tertiary => SurfaceColors::new(c.tertiary, c.on_tertiary),
        }
    }
}

/// FAB sizes. The baseline small FAB is deprecated in M3 Expressive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FabSize {
    #[default]
    Regular,
    Medium,
    Large,
}

#[derive(Clone, Copy, Debug)]
struct FabMetrics {
    height: Pixels,
    icon: Pixels,
    corner: Corner,
    /// Extended FAB padding, gap and label role.
    padding: Pixels,
    gap: Pixels,
    label: TypeRole,
}

impl FabSize {
    pub const ALL: [FabSize; 3] = [FabSize::Regular, FabSize::Medium, FabSize::Large];

    const fn metrics(self) -> FabMetrics {
        match self {
            FabSize::Regular => FabMetrics {
                height: px(56.0),
                icon: px(24.0),
                corner: Corner::Large,
                padding: px(16.0),
                gap: px(8.0),
                label: TypeRole::TitleMedium,
            },
            FabSize::Medium => FabMetrics {
                height: px(80.0),
                icon: px(28.0),
                corner: Corner::LargeIncreased,
                padding: px(26.0),
                gap: px(16.0),
                label: TypeRole::TitleLarge,
            },
            FabSize::Large => FabMetrics {
                height: px(96.0),
                icon: px(32.0),
                corner: Corner::ExtraLarge,
                padding: px(28.0),
                gap: px(20.0),
                label: TypeRole::HeadlineSmall,
            },
        }
    }
}

/// A floating action button, optionally extended with a label that
/// collapses and expands with a spatial spring.
#[derive(IntoElement)]
pub struct Fab {
    id: ElementId,
    icon: Icon,
    label: Option<SharedString>,
    expanded: bool,
    color: FabColor,
    size: FabSize,
    lowered: bool,
    on_click: Option<ClickHandler>,
}

impl Fab {
    pub fn new(id: impl Into<ElementId>, icon: Icon) -> Self {
        Self {
            id: id.into(),
            icon,
            label: None,
            expanded: true,
            color: FabColor::PrimaryContainer,
            size: FabSize::Regular,
            lowered: false,
            on_click: None,
        }
    }

    /// Makes this an extended FAB.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Collapses an extended FAB to its icon, e.g. while scrolling.
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    pub fn color(mut self, color: FabColor) -> Self {
        self.color = color;
        self
    }

    pub fn size(mut self, size: FabSize) -> Self {
        self.size = size;
        self
    }

    /// Uses the lower resting elevation.
    pub fn lowered(mut self, lowered: bool) -> Self {
        self.lowered = lowered;
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

impl RenderOnce for Fab {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let metrics = self.size.metrics();
        let theme = cx.md();
        let radii = Corners::all(theme.shapes.radius(metrics.corner, metrics.height));
        let colors = self.color.colors(&theme.colors);
        let label_style = theme.typography.style(metrics.label);
        let id = self.id.clone();

        // An extended FAB animates between its icon-only width and the width
        // its label needs; the label fades as the container narrows.
        let collapsed_padding = (metrics.height - metrics.icon) / 2.0;
        let (width, padding, label_opacity) = match &self.label {
            Some(label) => {
                let label_width = measure_text(label, label_style, window, cx);
                let expanded_width =
                    metrics.padding * 2.0 + metrics.icon + metrics.gap + label_width.ceil();
                let t = animate(
                    child_id(&id, "expand"),
                    if self.expanded { 1.0 } else { 0.0 },
                    MotionToken::DefaultSpatial,
                    window,
                    cx,
                );
                let opacity = animate(
                    child_id(&id, "label-opacity"),
                    if self.expanded { 1.0 } else { 0.0 },
                    MotionToken::DefaultEffects,
                    window,
                    cx,
                );
                (
                    metrics.height + (expanded_width - metrics.height) * t,
                    collapsed_padding + (metrics.padding - collapsed_padding) * t.clamp(0.0, 1.0),
                    opacity.clamp(0.0, 1.0),
                )
            }
            None => (metrics.height, collapsed_padding, 0.0),
        };
        let elevation = if self.lowered { (1.0, 2.0) } else { (3.0, 4.0) };
        let label = self.label.clone();
        let glyph = self.icon;
        let aria = self
            .label
            .clone()
            .unwrap_or_else(|| self.icon.name().into());

        Pressable {
            id: self.id,
            label: aria,
            role: Role::Button,
            height: metrics.height,
            width: Some(width),
            padding_x: px(0.0),
            gap: px(0.0),
            rest_radii: radii,
            selected_radii: radii,
            pressed_radii: radii,
            colors,
            selected_colors: None,
            selected: None,
            elevation,
            outline_width: px(0.0),
            disabled: false,
            on_click: self.on_click,
            group: GroupOverrides::default(),
        }
        .render(window, cx, move |state, _, cx| {
            let mut row = div()
                .size_full()
                .flex()
                .flex_row()
                .items_center()
                .overflow_hidden()
                .pl(padding)
                .gap(metrics.gap)
                .child(icon(glyph).size(metrics.icon).color(state.content));
            if let Some(label) = label {
                let mut color = state.content;
                color.a *= label_opacity;
                row = row.child(
                    div()
                        .flex_none()
                        .md_type_style(label_style, cx)
                        .text_color(color)
                        .child(label),
                );
            }
            vec![row.into_any_element()]
        })
    }
}
