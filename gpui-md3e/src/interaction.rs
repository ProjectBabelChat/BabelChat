//! Hover, press and focus tracking with Material state layers and ripples.

use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    App, Bounds, Corners, Div, ElementId, Entity, FocusHandle, Hsla, InteractiveElement as _,
    IntoElement, KeyDownEvent, KeyUpEvent, MouseButton, Pixels, Point, Stateful,
    StatefulInteractiveElement as _, Styled as _, Window, canvas, div, px, quad, transparent_black,
};
use smallvec::SmallVec;

use crate::{
    StyledExt as _, child_id,
    motion::{MotionToken, animate, fast_out_slow_in, frame_now},
    paint::{distance, paint_clipped_circle},
    theme::ActiveTheme as _,
};

const RIPPLE_EXPAND: Duration = Duration::from_millis(225);
const RIPPLE_FADE_IN: Duration = Duration::from_millis(75);
const RIPPLE_FADE_OUT: Duration = Duration::from_millis(150);

/// Pointer and keyboard state of one interactive component.
pub struct Interaction {
    pub hovered: bool,
    pub pressed: bool,
    pub focus_handle: FocusHandle,
    ripples: SmallVec<[Ripple; 2]>,
    bounds: Rc<Cell<Bounds<Pixels>>>,
}

#[derive(Clone, Copy)]
struct Ripple {
    /// Press position in window coordinates, or `None` for keyboard presses
    /// which ripple from the center.
    origin: Option<Point<Pixels>>,
    start: Instant,
    released: Option<Instant>,
}

impl Ripple {
    fn finished(&self, now: Instant) -> bool {
        self.fade_out_start()
            .is_some_and(|start| now >= start + RIPPLE_FADE_OUT)
    }

    /// The fade-out begins once the press is released and the ripple has
    /// finished expanding, so quick taps still show a full ripple.
    fn fade_out_start(&self) -> Option<Instant> {
        self.released
            .map(|released| released.max(self.start + RIPPLE_EXPAND))
    }
}

/// A snapshot of the interaction state for rendering.
#[derive(Clone, Copy, Debug, Default)]
pub struct InteractionSnapshot {
    pub hovered: bool,
    pub pressed: bool,
    /// Focused through the keyboard, so the focus ring should show.
    pub focus_visible: bool,
}

impl Interaction {
    /// The interaction state of the component `id`, created on first use.
    pub fn use_state(id: &ElementId, window: &mut Window, cx: &mut App) -> Entity<Interaction> {
        window.use_keyed_state(child_id(id, "interaction"), cx, |_, cx| Interaction {
            hovered: false,
            pressed: false,
            focus_handle: cx.focus_handle().tab_stop(true),
            ripples: SmallVec::new(),
            bounds: Rc::default(),
        })
    }

    pub fn snapshot(state: &Entity<Interaction>, window: &Window, cx: &App) -> InteractionSnapshot {
        let state = state.read(cx);
        InteractionSnapshot {
            hovered: state.hovered,
            pressed: state.pressed,
            focus_visible: state.focus_handle.is_focused(window)
                && window.last_input_was_keyboard(),
        }
    }

    /// The bounds the component was painted at last frame.
    pub fn bounds(&self) -> Bounds<Pixels> {
        self.bounds.get()
    }

    fn press(&mut self, origin: Option<Point<Pixels>>) {
        self.pressed = true;
        let now = Instant::now();
        self.ripples.retain(|ripple| !ripple.finished(now));
        self.ripples.push(Ripple {
            origin,
            start: now,
            released: None,
        });
    }

    fn release(&mut self) {
        self.pressed = false;
        let now = Instant::now();
        for ripple in &mut self.ripples {
            ripple.released.get_or_insert(now);
        }
    }

    fn has_active_ripples(&self, now: Instant) -> bool {
        self.ripples.iter().any(|ripple| !ripple.finished(now))
    }
}

/// Wires hover, press, keyboard activation and focus tracking to `element`.
pub fn interactive(
    element: Stateful<Div>,
    state: &Entity<Interaction>,
    disabled: bool,
    cx: &App,
) -> Stateful<Div> {
    if disabled {
        return element;
    }
    let focus_handle = state.read(cx).focus_handle.clone();
    let hover = state.clone();
    let down = state.clone();
    let up = state.clone();
    let up_out = state.clone();
    let key_down = state.clone();
    let key_up = state.clone();
    element
        .track_focus(&focus_handle)
        .on_hover(move |hovered, _, cx| {
            hover.update(cx, |state, cx| {
                state.hovered = *hovered;
                cx.notify();
            });
        })
        .on_mouse_down(MouseButton::Left, move |event, window, cx| {
            let position = event.position;
            down.update(cx, |state, cx| {
                state.press(Some(position));
                window.focus(&state.focus_handle, cx);
                cx.notify();
            });
        })
        .on_mouse_up(MouseButton::Left, move |_, _, cx| {
            up.update(cx, |state, cx| {
                state.release();
                cx.notify();
            });
        })
        .on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
            up_out.update(cx, |state, cx| {
                if state.pressed {
                    state.release();
                    cx.notify();
                }
            });
        })
        .on_key_down(move |event: &KeyDownEvent, _, cx| {
            if is_activation_key(event.keystroke.key.as_str()) && !event.is_held {
                key_down.update(cx, |state, cx| {
                    state.press(None);
                    cx.notify();
                });
            }
        })
        .on_key_up(move |event: &KeyUpEvent, _, cx| {
            if is_activation_key(event.keystroke.key.as_str()) {
                key_up.update(cx, |state, cx| {
                    state.release();
                    cx.notify();
                });
            }
        })
}

fn is_activation_key(key: &str) -> bool {
    matches!(key, "enter" | "space")
}

/// Paints the hover/focus state layer and press ripples over a container.
///
/// Returns an absolutely positioned element that fills its parent; add it as
/// the first child so content is painted above it.
pub fn state_layer(
    id: &ElementId,
    state: &Entity<Interaction>,
    radii: Corners<Pixels>,
    color: Hsla,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement + use<> {
    let snapshot = Interaction::snapshot(state, window, cx);
    let layers = cx.md().state;
    let hover = animate(
        child_id(id, "hover-layer"),
        if snapshot.hovered { 1.0 } else { 0.0 },
        MotionToken::FastEffects,
        window,
        cx,
    );
    let focus = animate(
        child_id(id, "focus-layer"),
        if snapshot.focus_visible { 1.0 } else { 0.0 },
        MotionToken::FastEffects,
        window,
        cx,
    );
    let opacity = (hover * layers.hover)
        .max(focus * layers.focus)
        .clamp(0.0, 1.0);

    let now = frame_now(window, cx);
    let (ripples, bounds_cell) = state.update(cx, |state, _| {
        state.ripples.retain(|ripple| !ripple.finished(now));
        (state.ripples.clone(), state.bounds.clone())
    });
    if state.read(cx).has_active_ripples(now) {
        window.request_animation_frame();
    }
    let pressed_opacity = layers.pressed;

    canvas(
        move |bounds, _, _| bounds_cell.set(bounds),
        move |bounds, _, window, _| {
            if opacity > 0.001 {
                let mut layer = color;
                layer.a *= opacity;
                window.paint_quad(quad(
                    bounds,
                    radii,
                    layer,
                    px(0.0),
                    transparent_black(),
                    Default::default(),
                ));
            }
            for ripple in ripples {
                paint_ripple(window, bounds, radii, color, pressed_opacity, ripple, now);
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

fn paint_ripple(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    radii: Corners<Pixels>,
    color: Hsla,
    opacity: f32,
    ripple: Ripple,
    now: Instant,
) {
    let elapsed = now.saturating_duration_since(ripple.start).as_secs_f32();
    let expand = fast_out_slow_in((elapsed / RIPPLE_EXPAND.as_secs_f32()).min(1.0));
    let fade_in = (elapsed / RIPPLE_FADE_IN.as_secs_f32()).min(1.0);
    let fade_out = ripple.fade_out_start().map_or(1.0, |start| {
        1.0 - (now.saturating_duration_since(start).as_secs_f32() / RIPPLE_FADE_OUT.as_secs_f32())
            .min(1.0)
    });
    let alpha = fade_in * fade_out * opacity;
    if alpha <= 0.0 {
        return;
    }

    let center = bounds.center();
    let origin = ripple.origin.unwrap_or(center);
    let position = origin + (center - origin) * expand;
    let end_radius = distance(bounds.origin, center) + px(10.0);
    let start_radius = bounds.size.width.max(bounds.size.height) * 0.3;
    let radius = start_radius + (end_radius - start_radius) * expand;

    let mut tint = color;
    tint.a *= alpha;
    paint_clipped_circle(window, bounds, radii, position, radius, tint);
}

/// The Material focus indicator: a 3px ring offset 2px outside the container.
pub fn focus_ring(radii: Corners<Pixels>, visible: f32, color: Hsla) -> impl IntoElement {
    const OFFSET: f32 = 2.0;
    const WIDTH: f32 = 3.0;
    let grow = px(OFFSET + WIDTH);
    let mut ring = color;
    ring.a *= visible.clamp(0.0, 1.0);
    div()
        .absolute()
        .top(-grow)
        .left(-grow)
        .right(-grow)
        .bottom(-grow)
        .border(px(WIDTH))
        .border_color(ring)
        .md_corners(Corners {
            top_left: radii.top_left + grow,
            top_right: radii.top_right + grow,
            bottom_right: radii.bottom_right + grow,
            bottom_left: radii.bottom_left + grow,
        })
}
