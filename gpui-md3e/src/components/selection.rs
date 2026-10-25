//! Switches, checkboxes and radio buttons.

use std::rc::Rc;

use gpui::{
    App, Bounds, ClickEvent, Corners, ElementId, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Point, RenderOnce, Role, StatefulInteractiveElement as _,
    Styled as _, Toggled, Window, canvas, div, point, px, quad, transparent_black,
};

use super::pressable::ClickHandler;
use crate::{
    ActiveTheme as _, Icon, child_id, icon,
    interaction::{Interaction, focus_ring, interactive, state_layer},
    motion::{MotionToken, animate},
    paint::round_stroke,
    theme::lerp_color,
};

/// The 40px circular target that carries a control's state layer.
const TARGET: f32 = 40.0;

/// Shared click handling for the selection controls.
fn clickable(
    element: gpui::Stateful<gpui::Div>,
    interaction: &gpui::Entity<Interaction>,
    disabled: bool,
    on_click: Option<ClickHandler>,
    cx: &App,
) -> gpui::Stateful<gpui::Div> {
    let mut element = element;
    if !disabled {
        element = element.cursor_pointer();
        if let Some(handler) = on_click {
            element = element.on_click(move |event, window, cx| handler(event, window, cx));
        }
    }
    interactive(element, interaction, disabled, cx)
}

/// An on/off switch whose handle grows when selected and swells on press.
#[derive(IntoElement)]
pub struct Switch {
    id: ElementId,
    selected: bool,
    icons: bool,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl Switch {
    pub fn new(id: impl Into<ElementId>, selected: bool) -> Self {
        Self {
            id: id.into(),
            selected,
            icons: false,
            disabled: false,
            on_click: None,
        }
    }

    /// Shows a check or close icon in the handle.
    pub fn icons(mut self, icons: bool) -> Self {
        self.icons = icons;
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

impl RenderOnce for Switch {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        const WIDTH: f32 = 52.0;
        const HEIGHT: f32 = 32.0;
        let colors = cx.md().colors;
        let id = self.id.clone();
        let interaction = Interaction::use_state(&id, window, cx);
        let snapshot = Interaction::snapshot(&interaction, window, cx);
        let pressed = snapshot.pressed && !self.disabled;

        let position = animate(
            child_id(&id, "position"),
            if self.selected { 1.0 } else { 0.0 },
            MotionToken::FastSpatial,
            window,
            cx,
        );
        let selected_t = animate(
            child_id(&id, "color"),
            if self.selected { 1.0 } else { 0.0 },
            MotionToken::DefaultEffects,
            window,
            cx,
        )
        .clamp(0.0, 1.0);
        let handle_target = if pressed {
            28.0
        } else if self.selected || self.icons {
            24.0
        } else {
            16.0
        };
        let handle = animate(
            child_id(&id, "handle"),
            handle_target,
            MotionToken::FastSpatial,
            window,
            cx,
        );
        let focus = animate(
            child_id(&id, "focus"),
            if snapshot.focus_visible { 1.0 } else { 0.0 },
            MotionToken::FastEffects,
            window,
            cx,
        );

        let mut track = lerp_color(colors.surface_container_highest, colors.primary, selected_t);
        let mut outline = lerp_color(colors.outline, colors.primary, selected_t);
        let handle_color = if self.selected && (pressed || snapshot.hovered) {
            colors.primary_container
        } else if !self.selected && (pressed || snapshot.hovered) {
            colors.on_surface_variant
        } else {
            lerp_color(colors.outline, colors.on_primary, selected_t)
        };
        let icon_color = lerp_color(
            colors.surface_container_highest,
            colors.on_primary_container,
            selected_t,
        );
        let mut handle_color = handle_color;
        if self.disabled {
            track = track.opacity(0.12);
            outline = outline.opacity(0.12);
            handle_color = handle_color.opacity(0.38);
        }

        let center_x = 16.0 + (WIDTH - 32.0) * position;
        let layer_color = if self.selected {
            colors.primary
        } else {
            colors.on_surface
        };
        let target = Corners::all(px(TARGET / 2.0));

        let mut handle_el = div()
            .absolute()
            .left(px(center_x - handle / 2.0 - 2.0))
            .top(px(HEIGHT / 2.0 - handle / 2.0 - 2.0))
            .size(px(handle))
            .rounded_full()
            .bg(handle_color)
            .flex()
            .items_center()
            .justify_center();
        if self.icons {
            handle_el = handle_el.child(
                icon(if self.selected {
                    Icon::Check
                } else {
                    Icon::Close
                })
                .size(px(16.0))
                .color(icon_color),
            );
        }

        let root = div()
            .id(id.clone())
            .relative()
            .flex_none()
            .w(px(WIDTH))
            .h(px(HEIGHT))
            .rounded_full()
            .bg(track)
            .border_2()
            .border_color(outline)
            .role(Role::Switch)
            .aria_toggled(if self.selected {
                Toggled::True
            } else {
                Toggled::False
            })
            .child(
                div()
                    .absolute()
                    .left(px(center_x - TARGET / 2.0 - 2.0))
                    .top(px(HEIGHT / 2.0 - TARGET / 2.0 - 2.0))
                    .size(px(TARGET))
                    .child(state_layer(
                        &id,
                        &interaction,
                        target,
                        layer_color,
                        window,
                        cx,
                    )),
            )
            .child(handle_el)
            .child(focus_ring(
                Corners::all(px(HEIGHT / 2.0)),
                focus,
                colors.secondary,
            ));
        clickable(root, &interaction, self.disabled, self.on_click, cx)
    }
}

/// The checkbox value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CheckState {
    Unchecked,
    Checked,
    Indeterminate,
}

/// A checkbox whose mark draws itself in.
#[derive(IntoElement)]
pub struct Checkbox {
    id: ElementId,
    state: CheckState,
    error: bool,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl Checkbox {
    pub fn new(id: impl Into<ElementId>, state: CheckState) -> Self {
        Self {
            id: id.into(),
            state,
            error: false,
            disabled: false,
            on_click: None,
        }
    }

    pub fn error(mut self, error: bool) -> Self {
        self.error = error;
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

impl RenderOnce for Checkbox {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        const BOX: f32 = 18.0;
        let colors = cx.md().colors;
        let id = self.id.clone();
        let interaction = Interaction::use_state(&id, window, cx);
        let snapshot = Interaction::snapshot(&interaction, window, cx);
        let on = self.state != CheckState::Unchecked;

        let fill = animate(
            child_id(&id, "fill"),
            if on { 1.0 } else { 0.0 },
            MotionToken::FastEffects,
            window,
            cx,
        )
        .clamp(0.0, 1.0);
        let check = animate(
            child_id(&id, "check"),
            if self.state == CheckState::Checked {
                1.0
            } else {
                0.0
            },
            MotionToken::DefaultSpatial,
            window,
            cx,
        )
        .clamp(0.0, 1.0);
        let dash = animate(
            child_id(&id, "dash"),
            if self.state == CheckState::Indeterminate {
                1.0
            } else {
                0.0
            },
            MotionToken::DefaultSpatial,
            window,
            cx,
        )
        .clamp(0.0, 1.0);
        let focus = animate(
            child_id(&id, "focus"),
            if snapshot.focus_visible { 1.0 } else { 0.0 },
            MotionToken::FastEffects,
            window,
            cx,
        );

        let (accent, on_accent) = if self.error {
            (colors.error, colors.on_error)
        } else {
            (colors.primary, colors.on_primary)
        };
        let unchecked_outline = if self.error {
            colors.error
        } else {
            colors.on_surface_variant
        };
        let mut container = lerp_color(transparent_black(), accent, fill);
        let mut outline = lerp_color(unchecked_outline, accent, fill);
        let mut mark = on_accent;
        if self.disabled {
            container = container.opacity(0.38);
            outline = colors.on_surface.opacity(0.38);
            mark = colors.surface;
        }
        let layer_color = if on { accent } else { colors.on_surface };
        let target = Corners::all(px(TARGET / 2.0));
        let inset = (TARGET - BOX) / 2.0;

        let root = div()
            .id(id.clone())
            .relative()
            .flex_none()
            .size(px(TARGET))
            .role(Role::CheckBox)
            .aria_toggled(match self.state {
                CheckState::Checked => Toggled::True,
                CheckState::Unchecked => Toggled::False,
                CheckState::Indeterminate => Toggled::Mixed,
            })
            .child(state_layer(
                &id,
                &interaction,
                target,
                layer_color,
                window,
                cx,
            ))
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let origin = bounds.origin + point(px(inset), px(inset));
                        let rect = Bounds::new(origin, gpui::size(px(BOX), px(BOX)));
                        window.paint_quad(quad(
                            rect,
                            px(2.0),
                            container,
                            px(2.0 * (1.0 - fill)).max(px(0.0)),
                            outline,
                            Default::default(),
                        ));
                        if check > 0.0 {
                            let points = [point(4.0, 9.5), point(7.5, 13.0), point(14.0, 6.0)];
                            paint_partial_polyline(window, origin, &points, check, mark);
                        }
                        if dash > 0.0 {
                            let half = 5.0 * dash;
                            let points = [point(9.0 - half, 9.0), point(9.0 + half, 9.0)];
                            paint_partial_polyline(window, origin, &points, 1.0, mark);
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(focus_ring(target, focus, colors.secondary));
        clickable(root, &interaction, self.disabled, self.on_click, cx)
    }
}

/// Strokes the first `progress` fraction (by length) of a polyline given in
/// box coordinates.
fn paint_partial_polyline(
    window: &mut Window,
    origin: Point<Pixels>,
    points: &[Point<f32>],
    progress: f32,
    color: Hsla,
) {
    let lengths: Vec<f32> = points
        .windows(2)
        .map(|pair| (pair[1].x - pair[0].x).hypot(pair[1].y - pair[0].y))
        .collect();
    let total: f32 = lengths.iter().sum();
    let mut remaining = total * progress.clamp(0.0, 1.0);
    if remaining <= 0.01 {
        return;
    }
    let to_px = |p: Point<f32>| point(origin.x + px(p.x), origin.y + px(p.y));
    let mut builder = round_stroke(px(2.0));
    builder.move_to(to_px(points[0]));
    for (ix, length) in lengths.iter().enumerate() {
        let (a, b) = (points[ix], points[ix + 1]);
        if remaining >= *length {
            builder.line_to(to_px(b));
            remaining -= length;
        } else {
            let t = remaining / length;
            builder.line_to(to_px(point(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)));
            break;
        }
    }
    if let Ok(path) = builder.build() {
        window.paint_path(path, color);
    }
}

/// A radio button whose dot springs in.
#[derive(IntoElement)]
pub struct RadioButton {
    id: ElementId,
    selected: bool,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl RadioButton {
    pub fn new(id: impl Into<ElementId>, selected: bool) -> Self {
        Self {
            id: id.into(),
            selected,
            disabled: false,
            on_click: None,
        }
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

impl RenderOnce for RadioButton {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.md().colors;
        let id = self.id.clone();
        let interaction = Interaction::use_state(&id, window, cx);
        let snapshot = Interaction::snapshot(&interaction, window, cx);
        let dot = animate(
            child_id(&id, "dot"),
            if self.selected { 5.0 } else { 0.0 },
            MotionToken::FastSpatial,
            window,
            cx,
        )
        .max(0.0);
        let color_t = animate(
            child_id(&id, "color"),
            if self.selected { 1.0 } else { 0.0 },
            MotionToken::DefaultEffects,
            window,
            cx,
        )
        .clamp(0.0, 1.0);
        let focus = animate(
            child_id(&id, "focus"),
            if snapshot.focus_visible { 1.0 } else { 0.0 },
            MotionToken::FastEffects,
            window,
            cx,
        );
        let mut color = lerp_color(colors.on_surface_variant, colors.primary, color_t);
        if self.disabled {
            color = colors.on_surface.opacity(0.38);
        }
        let layer_color = if self.selected {
            colors.primary
        } else {
            colors.on_surface
        };
        let target = Corners::all(px(TARGET / 2.0));

        let root = div()
            .id(id.clone())
            .relative()
            .flex_none()
            .size(px(TARGET))
            .role(Role::RadioButton)
            .aria_toggled(if self.selected {
                Toggled::True
            } else {
                Toggled::False
            })
            .child(state_layer(
                &id,
                &interaction,
                target,
                layer_color,
                window,
                cx,
            ))
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let center = bounds.center();
                        let ring = Bounds::new(
                            point(center.x - px(10.0), center.y - px(10.0)),
                            gpui::size(px(20.0), px(20.0)),
                        );
                        window.paint_quad(quad(
                            ring,
                            px(10.0),
                            transparent_black(),
                            px(2.0),
                            color,
                            Default::default(),
                        ));
                        if dot > 0.05 {
                            let dot_bounds = Bounds::new(
                                point(center.x - px(dot), center.y - px(dot)),
                                gpui::size(px(dot * 2.0), px(dot * 2.0)),
                            );
                            window.paint_quad(quad(
                                dot_bounds,
                                px(dot),
                                color,
                                px(0.0),
                                transparent_black(),
                                Default::default(),
                            ));
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(focus_ring(target, focus, colors.secondary));
        clickable(root, &interaction, self.disabled, self.on_click, cx)
    }
}
