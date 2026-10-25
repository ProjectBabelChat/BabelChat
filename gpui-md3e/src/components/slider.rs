use std::rc::Rc;

use gpui::{
    App, Bounds, DispatchPhase, ElementId, Entity, Hsla, InteractiveElement as _, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _,
    Pixels, RenderOnce, Role, StatefulInteractiveElement as _, Styled as _, Window, canvas, div,
    point, px, quad, transparent_black,
};

use crate::{
    ActiveTheme as _, StyledExt as _, child_id,
    motion::{MotionToken, animate},
    theme::TypeRole,
};

const TRACK_HEIGHT: f32 = 16.0;
const HANDLE_HEIGHT: f32 = 44.0;
const HANDLE_GAP: f32 = 6.0;
const INNER_RADIUS: f32 = 2.0;
const STOP_SIZE: f32 = 4.0;

type ChangeHandler = Rc<dyn Fn(&f32, &mut Window, &mut App)>;

struct SliderState {
    dragging: bool,
    hovered: bool,
    bounds: Bounds<Pixels>,
    focus_handle: gpui::FocusHandle,
}

/// The M3 Expressive slider: a thick track split by a slim handle that
/// narrows while dragged, with a value bubble above it.
#[derive(IntoElement)]
pub struct Slider {
    id: ElementId,
    value: f32,
    steps: Option<u32>,
    disabled: bool,
    on_change: Option<ChangeHandler>,
    label: Option<Rc<dyn Fn(f32) -> String>>,
}

impl Slider {
    /// A continuous slider over 0..=1.
    pub fn new(id: impl Into<ElementId>, value: f32) -> Self {
        Self {
            id: id.into(),
            value: value.clamp(0.0, 1.0),
            steps: None,
            disabled: false,
            on_change: None,
            label: None,
        }
    }

    /// Snaps to `steps` equal intervals and shows stop indicators.
    pub fn steps(mut self, steps: u32) -> Self {
        self.steps = Some(steps.max(1));
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change(mut self, handler: impl Fn(&f32, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }

    /// Formats the value bubble shown while dragging. Defaults to a percentage.
    pub fn label(mut self, format: impl Fn(f32) -> String + 'static) -> Self {
        self.label = Some(Rc::new(format));
        self
    }
}

fn quantize(value: f32, steps: Option<u32>) -> f32 {
    let value = value.clamp(0.0, 1.0);
    match steps {
        Some(steps) => (value * steps as f32).round() / steps as f32,
        None => value,
    }
}

fn value_at(bounds: Bounds<Pixels>, x: Pixels, steps: Option<u32>) -> f32 {
    // The handle center can travel the full width minus half its width.
    let start = bounds.left() + px(2.0);
    let width = (bounds.size.width - px(4.0)).max(px(1.0));
    quantize(((x - start) / width).clamp(0.0, 1.0), steps)
}

impl RenderOnce for Slider {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.md().colors;
        let id = self.id.clone();
        let state: Entity<SliderState> =
            window.use_keyed_state(child_id(&id, "slider"), cx, |_, cx| SliderState {
                dragging: false,
                hovered: false,
                bounds: Bounds::default(),
                focus_handle: cx.focus_handle().tab_stop(true),
            });
        let (dragging, focus_handle) = {
            let s = state.read(cx);
            (s.dragging, s.focus_handle.clone())
        };
        let focused = focus_handle.is_focused(window) && window.last_input_was_keyboard();
        let active = dragging || focused;

        let value = animate(
            child_id(&id, "value"),
            self.value,
            MotionToken::FastSpatial,
            window,
            cx,
        );
        let handle_width = animate(
            child_id(&id, "handle-width"),
            if dragging { 2.0 } else { 4.0 },
            MotionToken::FastSpatial,
            window,
            cx,
        );
        let bubble = animate(
            child_id(&id, "bubble"),
            if active { 1.0 } else { 0.0 },
            MotionToken::FastSpatial,
            window,
            cx,
        );

        let (active_color, inactive_color, handle_color) = if self.disabled {
            (
                colors.on_surface.opacity(0.38),
                colors.on_surface.opacity(0.12),
                colors.on_surface.opacity(0.38),
            )
        } else {
            (colors.primary, colors.secondary_container, colors.primary)
        };
        let stop_on_active = colors.secondary_container;
        let stop_on_inactive = colors.primary;
        let steps = self.steps;
        let label_text = match &self.label {
            Some(format) => format(self.value),
            None => format!("{}", (self.value * 100.0).round() as i32),
        };

        let paint_state = state.clone();
        let drag_change = self.on_change.clone();
        let track = canvas(
            move |bounds, _, _| bounds,
            move |bounds, _, window, cx| {
                paint_state.update(cx, |state, _| state.bounds = bounds);
                paint_track(
                    window,
                    bounds,
                    value,
                    handle_width,
                    steps,
                    [
                        active_color,
                        inactive_color,
                        handle_color,
                        stop_on_active,
                        stop_on_inactive,
                    ],
                );

                // Window-level listeners keep the drag alive outside the track.
                let move_state = paint_state.clone();
                let on_change = drag_change.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                    if phase != DispatchPhase::Bubble || !move_state.read(cx).dragging {
                        return;
                    }
                    let bounds = move_state.read(cx).bounds;
                    let value = value_at(bounds, event.position.x, steps);
                    if let Some(on_change) = &on_change {
                        on_change(&value, window, cx);
                    }
                });
                let up_state = paint_state.clone();
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble && up_state.read(cx).dragging {
                        up_state.update(cx, |state, cx| {
                            state.dragging = false;
                            cx.notify();
                        });
                    }
                });
            },
        )
        .size_full();

        // The value bubble rises out of the handle while dragging.
        let bubble_el = (bubble > 0.01).then(|| {
            let scale = bubble.max(0.0);
            let width = 48.0 * scale;
            let height = 44.0 * scale;
            div()
                .absolute()
                .bottom(px(HANDLE_HEIGHT + 4.0))
                .left(gpui::relative(value.clamp(0.0, 1.0)))
                .ml(px(-width / 2.0))
                .w(px(width))
                .h(px(height))
                .rounded(px(height / 2.0))
                .bg(colors.inverse_surface)
                .text_color(colors.inverse_on_surface.opacity(bubble.clamp(0.0, 1.0)))
                .md_type(TypeRole::LabelLarge, cx)
                .flex()
                .items_center()
                .justify_center()
                .overflow_hidden()
                .child(label_text)
        });

        let on_change = self.on_change.clone();
        let down_state = state.clone();
        let hover_state = state;
        let key_change = self.on_change.clone();
        let current = self.value;
        let key_step = steps.map_or(0.05, |s| 1.0 / s as f32);
        let mut root = div()
            .id(id)
            .relative()
            .w_full()
            .min_w(px(120.0))
            .h(px(HANDLE_HEIGHT))
            .role(Role::Slider)
            .aria_numeric_value(self.value as f64)
            .aria_min_numeric_value(0.0)
            .aria_max_numeric_value(1.0)
            .track_focus(&focus_handle)
            .child(track)
            .children(bubble_el);
        if !self.disabled {
            root = root
                .cursor_pointer()
                .on_hover(move |hovered, _, cx| {
                    hover_state.update(cx, |state, cx| {
                        state.hovered = *hovered;
                        cx.notify();
                    })
                })
                .on_mouse_down(
                    MouseButton::Left,
                    move |event: &MouseDownEvent, window, cx| {
                        let bounds = down_state.read(cx).bounds;
                        down_state.update(cx, |state, cx| {
                            state.dragging = true;
                            window.focus(&state.focus_handle, cx);
                            cx.notify();
                        });
                        let value = value_at(bounds, event.position.x, steps);
                        if let Some(on_change) = &on_change {
                            on_change(&value, window, cx);
                        }
                    },
                )
                .on_key_down(move |event: &KeyDownEvent, window, cx| {
                    let delta = match event.keystroke.key.as_str() {
                        "right" | "up" => key_step,
                        "left" | "down" => -key_step,
                        _ => return,
                    };
                    if let Some(on_change) = &key_change {
                        on_change(&quantize(current + delta, steps), window, cx);
                    }
                });
        }
        root
    }
}

/// Paints the split track, stop indicators and handle.
fn paint_track(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    value: f32,
    handle_width: f32,
    steps: Option<u32>,
    [active, inactive, handle, stop_on_active, stop_on_inactive]: [Hsla; 5],
) {
    let left = bounds.left().as_f32();
    let width = bounds.size.width.as_f32();
    let center_y = bounds.center().y.as_f32();
    let track_top = center_y - TRACK_HEIGHT / 2.0;
    let handle_x = left + 2.0 + (width - 4.0) * value.clamp(0.0, 1.0);
    let outer = TRACK_HEIGHT / 2.0;

    let segment = |window: &mut Window,
                   from: f32,
                   to: f32,
                   color: Hsla,
                   left_outer: bool,
                   right_outer: bool| {
        if to - from <= 0.5 {
            return;
        }
        let max_radius = ((to - from) / 2.0).min(outer);
        let r = |is_outer: bool| {
            px(if is_outer {
                max_radius
            } else {
                INNER_RADIUS.min(max_radius)
            })
        };
        window.paint_quad(quad(
            Bounds::new(
                point(px(from), px(track_top)),
                gpui::size(px(to - from), px(TRACK_HEIGHT)),
            ),
            gpui::Corners {
                top_left: r(left_outer),
                bottom_left: r(left_outer),
                top_right: r(right_outer),
                bottom_right: r(right_outer),
            },
            color,
            px(0.0),
            transparent_black(),
            Default::default(),
        ));
    };

    let gap = HANDLE_GAP + handle_width / 2.0;
    segment(window, left, handle_x - gap, active, true, false);
    segment(window, handle_x + gap, left + width, inactive, false, true);

    let dot = |window: &mut Window, x: f32, color: Hsla| {
        let r = STOP_SIZE / 2.0;
        window.paint_quad(quad(
            Bounds::new(
                point(px(x - r), px(center_y - r)),
                gpui::size(px(STOP_SIZE), px(STOP_SIZE)),
            ),
            px(r),
            color,
            px(0.0),
            transparent_black(),
            Default::default(),
        ));
    };
    match steps {
        Some(steps) => {
            for step in 0..=steps {
                let x = left + 2.0 + (width - 4.0) * step as f32 / steps as f32;
                let x = x.clamp(left + outer, left + width - outer);
                if (x - handle_x).abs() < gap + STOP_SIZE {
                    continue;
                }
                dot(
                    window,
                    x,
                    if x < handle_x {
                        stop_on_active
                    } else {
                        stop_on_inactive
                    },
                );
            }
        }
        None => {
            let x = left + width - outer;
            if x - handle_x > gap + STOP_SIZE {
                dot(window, x, stop_on_inactive);
            }
        }
    }

    window.paint_quad(quad(
        Bounds::new(
            point(
                px(handle_x - handle_width / 2.0),
                px(center_y - HANDLE_HEIGHT / 2.0),
            ),
            gpui::size(px(handle_width), px(HANDLE_HEIGHT)),
        ),
        px(handle_width / 2.0),
        handle,
        px(0.0),
        transparent_black(),
        Default::default(),
    ));
}
