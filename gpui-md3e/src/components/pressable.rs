//! The shared machinery of every button-like component: a container whose
//! corners, colors and elevation spring between rest, toggled and pressed
//! states, with a state layer, ripples and a focus ring.

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, Corners, Div, ElementId, Entity, Hsla, InteractiveElement as _,
    ParentElement as _, Pixels, Role, SharedString, Stateful, StatefulInteractiveElement as _,
    Styled as _, Toggled, Window, div, px, transparent_black,
};

use crate::{
    ActiveTheme as _, StyledExt as _, child_id,
    interaction::{Interaction, focus_ring, interactive, state_layer},
    motion::{MotionToken, animate},
    theme::lerp_color,
};

pub(crate) type ClickHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// Container, content and outline colors in one state.
#[derive(Clone, Copy, Debug)]
pub struct SurfaceColors {
    pub container: Hsla,
    pub content: Hsla,
    pub outline: Hsla,
}

impl SurfaceColors {
    pub fn new(container: Hsla, content: Hsla) -> Self {
        Self {
            container,
            content,
            outline: transparent_black(),
        }
    }

    pub fn outlined(content: Hsla, outline: Hsla) -> Self {
        Self {
            container: transparent_black(),
            content,
            outline,
        }
    }

    fn lerp(self, other: Self, t: f32) -> Self {
        Self {
            container: lerp_color(self.container, other.container, t),
            content: lerp_color(self.content, other.content, t),
            outline: lerp_color(self.outline, other.outline, t),
        }
    }
}

/// Overrides a parent component (e.g. a button group) can apply.
#[derive(Clone, Default)]
pub(crate) struct GroupOverrides {
    pub interaction: Option<Entity<Interaction>>,
    /// Extra width split between both sides of the content.
    pub extra_width: Pixels,
    /// Replaces the computed corner radii.
    pub radii: Option<Corners<Pixels>>,
    pub fill_width: bool,
}

/// Everything needed to render a pressable surface.
pub(crate) struct Pressable {
    pub id: ElementId,
    pub label: SharedString,
    pub role: Role,
    pub height: Pixels,
    /// Fixed width, or `None` to hug the content.
    pub width: Option<Pixels>,
    pub padding_x: Pixels,
    pub gap: Pixels,
    pub rest_radii: Corners<Pixels>,
    pub selected_radii: Corners<Pixels>,
    pub pressed_radii: Corners<Pixels>,
    pub colors: SurfaceColors,
    pub selected_colors: Option<SurfaceColors>,
    pub selected: Option<bool>,
    /// Resting and hovered elevation levels.
    pub elevation: (f32, f32),
    pub outline_width: Pixels,
    pub disabled: bool,
    pub on_click: Option<ClickHandler>,
    pub group: GroupOverrides,
}

/// Values a component needs to draw its content.
#[derive(Clone, Copy)]
pub(crate) struct PressableState {
    pub content: Hsla,
    pub selected: bool,
}

impl Pressable {
    pub fn render(
        self,
        window: &mut Window,
        cx: &mut App,
        content: impl FnOnce(PressableState, &mut Window, &mut App) -> Vec<AnyElement>,
    ) -> Stateful<Div> {
        let id = self.id.clone();
        let theme = cx.md();
        let scheme = theme.colors;
        let states = theme.state;

        let interaction = self
            .group
            .interaction
            .clone()
            .unwrap_or_else(|| Interaction::use_state(&id, window, cx));
        let snapshot = Interaction::snapshot(&interaction, window, cx);
        let pressed = snapshot.pressed && !self.disabled;
        let selected = self.selected == Some(true);

        let selected_t = animate(
            child_id(&id, "selected-shape"),
            if selected { 1.0 } else { 0.0 },
            MotionToken::FastSpatial,
            window,
            cx,
        );
        let press_t = animate(
            child_id(&id, "press-shape"),
            if pressed { 1.0 } else { 0.0 },
            MotionToken::FastSpatial,
            window,
            cx,
        );
        let radii = self.group.radii.unwrap_or_else(|| {
            let toggled = lerp_corners(self.rest_radii, self.selected_radii, selected_t);
            lerp_corners(toggled, self.pressed_radii, press_t)
        });
        let radii = clamp_corners(radii, self.height / 2.0);

        let color_t = animate(
            child_id(&id, "selected-color"),
            if selected { 1.0 } else { 0.0 },
            MotionToken::DefaultEffects,
            window,
            cx,
        )
        .clamp(0.0, 1.0);
        let mut palette = match self.selected_colors {
            Some(on) => self.colors.lerp(on, color_t),
            None => self.colors,
        };
        if self.disabled {
            palette.content = scheme.on_surface.opacity(states.disabled_content);
            if palette.container.a > 0.0 {
                palette.container = scheme.on_surface.opacity(states.disabled_container);
            }
            if palette.outline.a > 0.0 {
                palette.outline = scheme.outline_variant;
            }
        }

        let (rest_elevation, hover_elevation) = self.elevation;
        let elevation = animate(
            child_id(&id, "elevation"),
            if self.disabled {
                0.0
            } else if snapshot.hovered && !pressed {
                hover_elevation
            } else {
                rest_elevation
            },
            MotionToken::DefaultEffects,
            window,
            cx,
        );
        let focus = animate(
            child_id(&id, "focus-ring"),
            if snapshot.focus_visible { 1.0 } else { 0.0 },
            MotionToken::FastEffects,
            window,
            cx,
        );

        let mut surface = div()
            .id(id.clone())
            .relative()
            .flex()
            .flex_none()
            .flex_row()
            .items_center()
            .justify_center()
            .gap(self.gap)
            .h(self.height)
            .px(self.padding_x + self.group.extra_width / 2.0)
            .md_corners(radii)
            .bg(palette.container)
            .text_color(palette.content)
            .whitespace_nowrap()
            .md_elevation(elevation, scheme.shadow)
            .role(self.role)
            .aria_label(self.label.clone());
        if let Some(width) = self.width {
            surface = surface.w(width + self.group.extra_width).px_0();
        }
        if self.group.fill_width {
            surface = surface.flex_1().min_w_0();
        }
        if let Some(selected) = self.selected {
            surface = surface.aria_toggled(if selected {
                Toggled::True
            } else {
                Toggled::False
            });
        }
        if palette.outline.a > 0.0 {
            surface = surface
                .border(self.outline_width)
                .border_color(palette.outline);
        }
        if !self.disabled {
            surface = surface.cursor_pointer();
            if let Some(handler) = self.on_click.clone() {
                surface = surface.on_click(move |event, window, cx| handler(event, window, cx));
            }
        }
        let surface = interactive(surface, &interaction, self.disabled, cx);

        let layer = state_layer(&id, &interaction, radii, palette.content, window, cx);
        let children = content(
            PressableState {
                content: palette.content,
                selected,
            },
            window,
            cx,
        );
        surface
            .child(layer)
            .children(children)
            .child(focus_ring(radii, focus, scheme.secondary))
    }
}

pub(crate) fn lerp_corners(a: Corners<Pixels>, b: Corners<Pixels>, t: f32) -> Corners<Pixels> {
    let mix = |x: Pixels, y: Pixels| x + (y - x) * t;
    Corners {
        top_left: mix(a.top_left, b.top_left),
        top_right: mix(a.top_right, b.top_right),
        bottom_right: mix(a.bottom_right, b.bottom_right),
        bottom_left: mix(a.bottom_left, b.bottom_left),
    }
}

pub(crate) fn clamp_corners(radii: Corners<Pixels>, max: Pixels) -> Corners<Pixels> {
    let clamp = |r: Pixels| r.max(px(0.0)).min(max);
    Corners {
        top_left: clamp(radii.top_left),
        top_right: clamp(radii.top_right),
        bottom_right: clamp(radii.bottom_right),
        bottom_left: clamp(radii.bottom_left),
    }
}
