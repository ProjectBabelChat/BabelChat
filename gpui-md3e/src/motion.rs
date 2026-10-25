//! Spring physics following the M3 Expressive motion scheme.
//!
//! Material motion replaces durations and easing curves with springs. Spatial
//! springs move, resize and reshape things and may overshoot; effects springs
//! drive color and opacity and never bounce.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use gpui::{App, ElementId, Global, Hsla, SpringConfig, SpringState, Window, WindowId};

use crate::theme::{ActiveTheme as _, lerp_color};

/// One of the six motion scheme springs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MotionToken {
    FastSpatial,
    DefaultSpatial,
    SlowSpatial,
    FastEffects,
    DefaultEffects,
    SlowEffects,
}

/// A damped spring described the way the Material tokens describe it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub damping_ratio: f32,
    pub stiffness: f32,
}

impl Spring {
    pub const fn new(damping_ratio: f32, stiffness: f32) -> Self {
        Self {
            damping_ratio,
            stiffness,
        }
    }

    /// Converts to GPUI's physical parameters with a unit mass.
    pub fn config(self) -> SpringConfig {
        SpringConfig::new(
            self.stiffness,
            2.0 * self.damping_ratio * self.stiffness.sqrt(),
            1.0,
        )
    }
}

/// Which family of spring tokens the theme uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MotionScheme {
    /// Bouncier springs for hero moments. The M3 Expressive default.
    #[default]
    Expressive,
    /// Calmer springs for utilitarian products.
    Standard,
}

impl MotionScheme {
    /// Values from `ExpressiveMotionTokens` and `StandardMotionTokens`.
    pub const fn spring(self, token: MotionToken) -> Spring {
        match (self, token) {
            (MotionScheme::Expressive, MotionToken::FastSpatial) => Spring::new(0.6, 800.0),
            (MotionScheme::Expressive, MotionToken::DefaultSpatial) => Spring::new(0.8, 380.0),
            (MotionScheme::Expressive, MotionToken::SlowSpatial) => Spring::new(0.8, 200.0),
            (MotionScheme::Standard, MotionToken::FastSpatial) => Spring::new(0.9, 1400.0),
            (MotionScheme::Standard, MotionToken::DefaultSpatial) => Spring::new(0.9, 700.0),
            (MotionScheme::Standard, MotionToken::SlowSpatial) => Spring::new(0.9, 300.0),
            (_, MotionToken::FastEffects) => Spring::new(1.0, 3800.0),
            (_, MotionToken::DefaultEffects) => Spring::new(1.0, 1600.0),
            (_, MotionToken::SlowEffects) => Spring::new(1.0, 800.0),
        }
    }
}

/// A scalar driven by a spring that can be retargeted at any time without
/// losing its velocity.
#[derive(Clone, Debug)]
pub struct SpringValue {
    state: SpringState,
    target: f32,
    config: SpringConfig,
    epsilon: f32,
    last_tick: Instant,
}

/// Frames longer than this are treated as a stall instead of letting the
/// spring jump to its rest position.
const MAX_FRAME: f32 = 1.0 / 20.0;

impl SpringValue {
    pub fn new(value: f32, spring: Spring) -> Self {
        Self {
            state: SpringState {
                position: value,
                velocity: 0.0,
            },
            target: value,
            config: spring.config(),
            epsilon: 0.001,
            last_tick: Instant::now(),
        }
    }

    /// Sets the settling tolerance in the value's own units.
    pub fn with_epsilon(mut self, epsilon: f32) -> Self {
        self.epsilon = epsilon;
        self
    }

    pub fn value(&self) -> f32 {
        self.state.position
    }

    pub fn velocity(&self) -> f32 {
        self.state.velocity
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    /// Moves the rest position, keeping the current velocity.
    pub fn set_target(&mut self, target: f32, spring: Spring) {
        self.set_target_at(target, spring, Instant::now());
    }

    /// Like [`SpringValue::set_target`], starting a settled spring at `now`.
    pub fn set_target_at(&mut self, target: f32, spring: Spring, now: Instant) {
        if self.is_settled() {
            self.last_tick = now;
        }
        self.target = target;
        self.config = spring.config();
    }

    /// Adds an instantaneous velocity, e.g. to make a value wobble.
    pub fn kick(&mut self, velocity: f32) {
        if self.is_settled() {
            self.last_tick = Instant::now();
        }
        self.state.velocity += velocity;
    }

    /// Jumps to a value and stops.
    pub fn snap(&mut self, value: f32) {
        self.state = SpringState {
            position: value,
            velocity: 0.0,
        };
        self.target = value;
    }

    pub fn is_settled(&self) -> bool {
        self.state.position == self.target && self.state.velocity == 0.0
    }

    /// Advances the simulation to `now` and returns the new value.
    pub fn tick(&mut self, now: Instant) -> f32 {
        if self.is_settled() {
            self.last_tick = now;
            return self.state.position;
        }
        let elapsed = now
            .saturating_duration_since(self.last_tick)
            .as_secs_f32()
            .min(MAX_FRAME);
        self.last_tick = now;
        self.state = self.config.step(self.state, self.target, elapsed);
        if self
            .config
            .is_settled(self.state, self.target, self.epsilon)
        {
            self.snap(self.target);
        }
        self.state.position
    }
}

struct AnimatedScalar {
    value: SpringValue,
}

/// Drives a value toward `target` with a spring whose state persists across
/// frames under `id`, and keeps the window animating until it settles.
///
/// A newly mounted value starts at its target.
pub fn animate(
    id: impl Into<ElementId>,
    target: f32,
    token: MotionToken,
    window: &mut Window,
    cx: &mut App,
) -> f32 {
    animate_from(id, target, target, token, window, cx)
}

/// Like [`animate`], but a newly mounted value starts at `initial`.
pub fn animate_from(
    id: impl Into<ElementId>,
    initial: f32,
    target: f32,
    token: MotionToken,
    window: &mut Window,
    cx: &mut App,
) -> f32 {
    let spring = cx.md().motion.spring(token);
    animate_with(id, initial, target, spring, window, cx)
}

/// Like [`animate_from`], with an explicit spring.
pub fn animate_with(
    id: impl Into<ElementId>,
    initial: f32,
    target: f32,
    spring: Spring,
    window: &mut Window,
    cx: &mut App,
) -> f32 {
    let state = window.use_keyed_state(id, cx, |_, _| AnimatedScalar {
        value: SpringValue::new(initial, spring),
    });
    let reduce_motion = cx.reduce_motion();
    let now = frame_now(window, cx);
    let (value, settled) = state.update(cx, |state, _| {
        if state.value.target() != target {
            state.value.set_target_at(target, spring, now);
        }
        if reduce_motion {
            state.value.snap(target);
        }
        let value = state.value.tick(now);
        (value, state.value.is_settled())
    });
    if !settled {
        window.request_animation_frame();
    }
    value
}

/// Animates between two colors with an effects spring keyed on a 0..1 phase.
pub fn animate_color(
    id: impl Into<ElementId>,
    from: Hsla,
    to: Hsla,
    active: bool,
    window: &mut Window,
    cx: &mut App,
) -> Hsla {
    let phase = animate(
        id,
        if active { 1.0 } else { 0.0 },
        MotionToken::DefaultEffects,
        window,
        cx,
    );
    lerp_color(from, to, phase.clamp(0.0, 1.0))
}

/// Animation time since a shared epoch, for looping animations that should
/// stay in phase across elements. Advances with [`frame_now`].
pub fn clock(window: &Window, cx: &mut App) -> Duration {
    let epoch = cx.md().epoch;
    frame_now(window, cx).saturating_duration_since(epoch)
}

/// Per-window animation clocks; see [`frame_now`].
#[derive(Default)]
struct FrameClocks(HashMap<WindowId, FrameClock>);

impl Global for FrameClocks {}

struct FrameClock {
    frame: u64,
    /// The animation time of `frame`.
    now: Instant,
    /// When `frame` was sampled.
    sampled_at: Instant,
    /// The compositor's timestamp for `frame`, if any.
    timestamp: Option<Duration>,
}

/// Fallback when the platform does not report the display's refresh rate.
const DEFAULT_REFRESH: Duration = Duration::from_micros(16_667);

/// The time to evaluate animations at in the frame being drawn.
///
/// Frames are rendered at irregular moments (whenever the compositor's frame
/// callback is handled) but shown on the display's fixed vblank grid. Sampling
/// the wall clock would make motion speed up and slow down with that jitter,
/// so this clock advances by whole refresh periods instead: once per frame, by
/// the number of periods that passed since the previous frame. Those are
/// counted from the compositor's frame timestamps when the platform reports
/// them (Wayland), and from the render times otherwise. The clock resyncs with
/// the wall clock whenever the two drift more than a period apart.
pub fn frame_now(window: &Window, cx: &mut App) -> Instant {
    let frame = window.frame_number();
    let period = window
        .refresh_interval(cx)
        .filter(|period| !period.is_zero())
        .unwrap_or(DEFAULT_REFRESH);
    let window_id = window.window_handle().window_id();
    let timestamp = window.frame_timestamp();
    let wall = Instant::now();
    let clocks = cx.default_global::<FrameClocks>();
    let clock = clocks.0.entry(window_id).or_insert(FrameClock {
        frame,
        now: wall,
        sampled_at: wall,
        timestamp,
    });
    if clock.frame != frame {
        *clock = FrameClock {
            frame,
            now: clock.next(wall, timestamp, period),
            sampled_at: wall,
            timestamp,
        };
    }
    clock.now
}

impl FrameClock {
    /// The animation time of a new frame sampled at `wall`.
    fn next(&self, wall: Instant, timestamp: Option<Duration>, period: Duration) -> Instant {
        let periods_in = |elapsed: Duration| elapsed.as_secs_f64() / period.as_secs_f64();
        let by_wall = periods_in(wall.saturating_duration_since(self.sampled_at));
        // Trust the compositor when its timestamp advanced and roughly agrees
        // with the wall clock (it may be stale for locally scheduled frames).
        let by_timestamp = match (self.timestamp, timestamp) {
            (Some(previous), Some(current)) if current > previous => {
                Some(periods_in(current - previous))
                    .filter(|periods| (periods - by_wall).abs() < 1.5)
            }
            _ => None,
        };
        let periods = by_timestamp.unwrap_or(by_wall).round().max(1.0);
        let now = self.now + period.mul_f64(periods);
        if now + period < wall || now > wall + period {
            wall
        } else {
            now
        }
    }
}

/// Material's "emphasized decelerate" curve, approximated with a cubic
/// Bézier (0.05, 0.7, 0.1, 1.0), for the few time-based animations.
pub fn emphasized_decelerate(t: f32) -> f32 {
    cubic_bezier(0.05, 0.7, 0.1, 1.0, t)
}

/// The standard "fast out, slow in" curve (0.4, 0, 0.2, 1).
pub fn fast_out_slow_in(t: f32) -> f32 {
    cubic_bezier(0.4, 0.0, 0.2, 1.0, t)
}

/// Evaluates a CSS-style cubic Bézier timing function.
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let bezier = |a: f32, b: f32, s: f32| {
        let inv = 1.0 - s;
        3.0 * inv * inv * s * a + 3.0 * inv * s * s * b + s * s * s
    };
    // Solve x(s) = t with Newton iterations, falling back to bisection.
    let mut s = t;
    for _ in 0..8 {
        let x = bezier(x1, x2, s) - t;
        let inv = 1.0 - s;
        let dx = 3.0 * inv * inv * x1 + 6.0 * inv * s * (x2 - x1) + 3.0 * s * s * (1.0 - x2);
        if dx.abs() < 1e-6 {
            break;
        }
        s = (s - x / dx).clamp(0.0, 1.0);
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..20 {
        let x = bezier(x1, x2, s);
        if (x - t).abs() < 1e-5 {
            break;
        }
        if x < t {
            lo = s;
        } else {
            hi = s;
        }
        s = (lo + hi) / 2.0;
    }
    bezier(y1, y2, s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_tokens_convert_to_expected_damping() {
        let spring = Spring::new(0.8, 380.0).config();
        let (_, ratio) = spring.canonical();
        assert!((ratio - 0.8).abs() < 1e-4);
    }

    #[test]
    fn spring_value_settles_on_target() {
        let mut value = SpringValue::new(0.0, Spring::new(0.8, 380.0));
        value.set_target(1.0, Spring::new(0.8, 380.0));
        let mut now = Instant::now();
        for _ in 0..200 {
            now += Duration::from_millis(16);
            value.tick(now);
        }
        assert!(value.is_settled());
        assert_eq!(value.value(), 1.0);
    }

    fn clock_at(now: Instant, timestamp: Option<Duration>) -> FrameClock {
        FrameClock {
            frame: 0,
            now,
            sampled_at: now,
            timestamp,
        }
    }

    #[test]
    fn frame_clock_steps_whole_periods_despite_render_jitter() {
        let period = Duration::from_micros(15_151);
        let start = Instant::now();
        let clock = clock_at(start, None);
        for jittered in [11_000, 13_500, 15_151, 17_800, 19_900] {
            let wall = start + Duration::from_micros(jittered);
            assert_eq!(clock.next(wall, None, period), start + period);
        }
        // A dropped frame shows up as two periods.
        let wall = start + Duration::from_micros(29_000);
        assert_eq!(clock.next(wall, None, period), start + period * 2);
    }

    #[test]
    fn frame_clock_prefers_agreeing_compositor_timestamps() {
        let period = Duration::from_micros(15_151);
        let start = Instant::now();
        let clock = clock_at(start, Some(Duration::from_millis(1_000)));
        // Ambiguous render gap, but the compositor says two vblanks passed.
        let wall = start + Duration::from_micros(22_000);
        let timestamp = Some(Duration::from_millis(1_030));
        assert_eq!(clock.next(wall, timestamp, period), start + period * 2);
        // A stale timestamp (locally scheduled frame) falls back to the wall clock.
        let stale = Some(Duration::from_millis(1_000));
        assert_eq!(clock.next(wall, stale, period), start + period);
    }

    #[test]
    fn frame_clock_resyncs_after_drift() {
        let period = Duration::from_micros(15_151);
        let start = Instant::now();
        let mut clock = clock_at(start, None);
        clock.now = start - period * 3;
        let wall = start + period;
        assert_eq!(clock.next(wall, None, period), wall);
    }

    #[test]
    fn cubic_bezier_hits_endpoints() {
        assert!((fast_out_slow_in(0.0)).abs() < 1e-4);
        assert!((fast_out_slow_in(1.0) - 1.0).abs() < 1e-4);
        assert!(emphasized_decelerate(0.5) > 0.5);
    }
}
