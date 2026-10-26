use std::f32::consts::{PI, TAU};

use gpui::{
    App, Bounds, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    Point, RenderOnce, Role, StatefulInteractiveElement as _, Styled as _, Window, canvas, div,
    point, px, quad, transparent_black,
};

use crate::{
    ActiveTheme as _, child_id,
    motion::{Spring, animate_with, clock, fast_out_slow_in},
    paint::round_stroke,
};

/// Progress values glide toward their target on this spring.
const PROGRESS_SPRING: Spring = Spring::new(1.0, 50.0);
const THICKNESS: f32 = 4.0;
const GAP: f32 = 4.0;

/// Whether the active indicator is drawn as a straight line or a wave.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ProgressStyle {
    #[default]
    Flat,
    Wavy,
}

/// Shared state of linear and circular indicators.
struct Indicator {
    id: ElementId,
    progress: Option<f32>,
    style: ProgressStyle,
}

impl Indicator {
    /// The animated determinate progress and wave amplitude (0..1).
    fn resolve(&self, window: &mut Window, cx: &mut App) -> (Option<f32>, f32) {
        let progress = self.progress.map(|target| {
            animate_with(
                child_id(&self.id, "progress"),
                0.0,
                target,
                PROGRESS_SPRING,
                window,
                cx,
            )
            .clamp(0.0, 1.0)
        });
        let wave_target = match (self.style, progress) {
            (ProgressStyle::Flat, _) => 0.0,
            // Waves calm down at the very start and end of determinate progress.
            (ProgressStyle::Wavy, Some(p)) if p <= 0.1 || p >= 0.95 => 0.0,
            (ProgressStyle::Wavy, _) => 1.0,
        };
        let amplitude = animate_with(
            child_id(&self.id, "amplitude"),
            wave_target,
            wave_target,
            Spring::new(1.0, 60.0),
            window,
            cx,
        );
        if self.style == ProgressStyle::Wavy || self.progress.is_none() {
            window.request_animation_frame();
        }
        (progress, amplitude.clamp(0.0, 1.0))
    }
}

/// A horizontal progress bar with a gap between the indicator and the track,
/// and a stop dot marking the end.
#[derive(IntoElement)]
pub struct LinearProgress {
    indicator: Indicator,
}

impl LinearProgress {
    /// An indeterminate indicator.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            indicator: Indicator {
                id: id.into(),
                progress: None,
                style: ProgressStyle::Flat,
            },
        }
    }

    pub fn progress(mut self, progress: f32) -> Self {
        self.indicator.progress = Some(progress.clamp(0.0, 1.0));
        self
    }

    pub fn style(mut self, style: ProgressStyle) -> Self {
        self.indicator.style = style;
        self
    }
}

impl RenderOnce for LinearProgress {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.md().colors;
        let (progress, amplitude) = self.indicator.resolve(window, cx);
        let style = self.indicator.style;
        let height = if style == ProgressStyle::Wavy {
            10.0
        } else {
            4.0
        };
        let elapsed = clock(window, cx).as_secs_f32();
        let active = colors.primary;
        let track = colors.secondary_container;

        let mut root = div()
            .id(self.indicator.id.clone())
            .w_full()
            .h(px(height))
            .role(Role::ProgressIndicator);
        if let Some(progress) = progress {
            root = root.aria_numeric_value(progress as f64);
        }
        root.child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let width = bounds.size.width.as_f32();
                    let wavelength = if progress.is_some() { 40.0 } else { 20.0 };
                    let wave = WaveLine {
                        bounds,
                        amplitude: 3.0 * amplitude,
                        wavelength,
                        // One wavelength per second.
                        phase: elapsed * wavelength,
                    };
                    let mut segments = match progress {
                        Some(p) => vec![(0.0, p * width)],
                        None => linear_indeterminate(elapsed)
                            .into_iter()
                            .map(|(a, b)| (a * width, b * width))
                            .collect(),
                    };
                    // The second indeterminate line enters while the first is
                    // still leaving, so it is the left one for part of a cycle.
                    segments.sort_by(|a, b| a.0.total_cmp(&b.0));

                    // The track fills the gaps between indicator segments,
                    // which are laid out left to right.
                    let mut cursor = 0.0;
                    for &(start, end) in &segments {
                        if end - start < 0.5 {
                            continue;
                        }
                        let track_end = start - GAP - THICKNESS / 2.0;
                        if track_end > cursor {
                            wave.paint_flat(window, cursor, track_end, track);
                        }
                        wave.paint(window, start, end, active);
                        cursor = end + GAP + THICKNESS / 2.0;
                    }
                    if cursor < width {
                        wave.paint_flat(window, cursor, width, track);
                    }

                    // The stop indicator marks where determinate progress ends.
                    if progress.is_some() {
                        let center = point(bounds.right() - px(THICKNESS / 2.0), bounds.center().y);
                        paint_dot(window, center, THICKNESS / 2.0, active);
                    }
                },
            )
            .size_full(),
        )
    }
}

/// The two moving segments of the indeterminate linear indicator, as
/// fractions of the width.
fn linear_indeterminate(elapsed: f32) -> [(f32, f32); 2] {
    const DURATION: f32 = 1.75;
    let t = (elapsed % DURATION) * 1000.0;
    let line =
        |delay: f32, duration: f32| fast_out_slow_in(((t - delay) / duration).clamp(0.0, 1.0));
    [
        (line(250.0, 1000.0), line(0.0, 1000.0)),
        (line(900.0, 850.0), line(650.0, 850.0)),
    ]
}

struct WaveLine {
    bounds: Bounds<Pixels>,
    amplitude: f32,
    wavelength: f32,
    phase: f32,
}

impl WaveLine {
    fn y_at(&self, x: f32) -> f32 {
        self.bounds.center().y.as_f32()
            + self.amplitude * ((x - self.phase) / self.wavelength * TAU).sin()
    }

    fn paint(&self, window: &mut Window, start: f32, end: f32, color: Hsla) {
        let origin = self.bounds.origin.x.as_f32();
        // Round caps extend past the ends, so inset by half the thickness.
        let (a, b) = (start + THICKNESS / 2.0, end - THICKNESS / 2.0);
        if b <= a {
            paint_dot(
                window,
                point(px(origin + (start + end) / 2.0), self.bounds.center().y),
                ((end - start) / 2.0).max(0.0),
                color,
            );
            return;
        }
        let mut builder = round_stroke(px(THICKNESS));
        let steps = ((b - a) / 2.0).ceil().max(1.0) as usize;
        for i in 0..=steps {
            let x = a + (b - a) * i as f32 / steps as f32;
            let p = point(px(origin + x), px(self.y_at(x)));
            if i == 0 {
                builder.move_to(p);
            } else {
                builder.line_to(p);
            }
        }
        if let Ok(path) = builder.build() {
            window.paint_path(path, color);
        }
    }

    fn paint_flat(&self, window: &mut Window, start: f32, end: f32, color: Hsla) {
        let origin = self.bounds.origin.x;
        let y = self.bounds.center().y - px(THICKNESS / 2.0);
        window.paint_quad(quad(
            Bounds::new(
                point(origin + px(start), y),
                gpui::size(px(end - start), px(THICKNESS)),
            ),
            px(THICKNESS / 2.0),
            color,
            px(0.0),
            transparent_black(),
            Default::default(),
        ));
    }
}

fn paint_dot(window: &mut Window, center: Point<Pixels>, radius: f32, color: Hsla) {
    if radius <= 0.0 {
        return;
    }
    window.paint_quad(quad(
        Bounds::new(
            point(center.x - px(radius), center.y - px(radius)),
            gpui::size(px(radius * 2.0), px(radius * 2.0)),
        ),
        px(radius),
        color,
        px(0.0),
        transparent_black(),
        Default::default(),
    ));
}

/// A circular progress indicator, optionally with a wavy active arc.
#[derive(IntoElement)]
pub struct CircularProgress {
    indicator: Indicator,
}

impl CircularProgress {
    /// An indeterminate indicator.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            indicator: Indicator {
                id: id.into(),
                progress: None,
                style: ProgressStyle::Flat,
            },
        }
    }

    pub fn progress(mut self, progress: f32) -> Self {
        self.indicator.progress = Some(progress.clamp(0.0, 1.0));
        self
    }

    pub fn style(mut self, style: ProgressStyle) -> Self {
        self.indicator.style = style;
        self
    }
}

impl RenderOnce for CircularProgress {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.md().colors;
        let (progress, amplitude) = self.indicator.resolve(window, cx);
        let style = self.indicator.style;
        let size = if style == ProgressStyle::Wavy {
            48.0
        } else {
            40.0
        };
        let elapsed = clock(window, cx).as_secs_f32();
        let active = colors.primary;
        let track = colors.secondary_container;

        let mut root = div()
            .id(self.indicator.id.clone())
            .flex_none()
            .size(px(size))
            .role(Role::ProgressIndicator);
        if let Some(progress) = progress {
            root = root.aria_numeric_value(progress as f64);
        }
        root.child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    let center = bounds.center();
                    let wave_amplitude = 1.6 * amplitude;
                    let radius =
                        bounds.size.width.as_f32() / 2.0 - THICKNESS / 2.0 - wave_amplitude;
                    let circumference = TAU * radius;
                    let waves = (circumference / 15.0).round().max(1.0);
                    // Gap between arcs, as an angle, including the round caps.
                    let gap = (GAP + THICKNESS) / radius;

                    let (start, sweep, rotation) = match progress {
                        Some(p) => (0.0, p * TAU, 0.0),
                        None => circular_indeterminate(elapsed),
                    };
                    let start = start - PI / 2.0 + rotation;
                    let arc = Arc {
                        center,
                        radius,
                        amplitude: wave_amplitude,
                        waves,
                        phase: elapsed * TAU / waves * 1.0,
                    };
                    if sweep > 0.001 {
                        arc.paint(window, start, sweep, active, true);
                    }
                    let track_sweep = TAU - sweep - gap * 2.0;
                    if track_sweep > 0.0 {
                        let track_start = if sweep > 0.001 {
                            start + sweep + gap
                        } else {
                            start
                        };
                        let track_sweep = if sweep > 0.001 { track_sweep } else { TAU };
                        arc.paint(window, track_start, track_sweep, track, false);
                    }
                },
            )
            .size_full(),
        )
    }
}

/// Start angle, sweep and extra rotation of the indeterminate arc.
fn circular_indeterminate(elapsed: f32) -> (f32, f32, f32) {
    const CYCLE: f32 = 1.5;
    let cycle = (elapsed / CYCLE).floor();
    let t = (elapsed % CYCLE) / CYCLE;
    let (min, max) = (0.1 * TAU, 0.87 * TAU);
    // The head leads during the first half, the tail catches up in the second.
    let head = fast_out_slow_in((t * 2.0).min(1.0));
    let tail = fast_out_slow_in((t * 2.0 - 1.0).max(0.0));
    let sweep = min + (max - min) * (head - tail);
    // Each cycle starts where the previous one's tail stopped, so the loop is
    // seamless. Wrapping keeps the angle small enough for f32 over long runs.
    let start = ((max - min) * (tail + cycle)).rem_euclid(TAU);
    let rotation = elapsed * TAU / 6.0 * 3.0;
    (start, sweep, rotation)
}

struct Arc {
    center: Point<Pixels>,
    radius: f32,
    amplitude: f32,
    waves: f32,
    phase: f32,
}

impl Arc {
    fn paint(&self, window: &mut Window, start: f32, sweep: f32, color: Hsla, wavy: bool) {
        let steps = ((sweep * self.radius) / 2.0).ceil().max(2.0) as usize;
        let mut builder = round_stroke(px(THICKNESS));
        for i in 0..=steps {
            let angle = start + sweep * i as f32 / steps as f32;
            let r = if wavy {
                self.radius + self.amplitude * (angle * self.waves + self.phase).sin()
            } else {
                self.radius
            };
            let p = point(
                self.center.x + px(r * angle.cos()),
                self.center.y + px(r * angle.sin()),
            );
            if i == 0 {
                builder.move_to(p);
            } else {
                builder.line_to(p);
            }
        }
        if let Ok(path) = builder.build() {
            window.paint_path(path, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: f32 = 0.001;

    /// The smallest angle between two directions.
    fn angle_between(a: f32, b: f32) -> f32 {
        let d = (a - b).rem_euclid(TAU);
        d.min(TAU - d)
    }

    #[test]
    fn circular_indeterminate_loops_without_jumps() {
        let ends = |t: f32| {
            let (start, sweep, rotation) = circular_indeterminate(t);
            (start + rotation, start + rotation + sweep)
        };
        let mut previous = ends(0.0);
        for i in 1..=6000 {
            let current = ends(i as f32 * STEP);
            assert!(angle_between(current.0, previous.0) < 0.05, "tail jumps at {i} ms");
            assert!(angle_between(current.1, previous.1) < 0.05, "head jumps at {i} ms");
            previous = current;
        }
    }

    #[test]
    fn linear_indeterminate_loops_without_jumps() {
        let mut previous = linear_indeterminate(0.0);
        for i in 1..=6000 {
            let current = linear_indeterminate(i as f32 * STEP);
            for (a, b) in previous.iter().zip(&current) {
                // A segment may only move instantly while it is invisible.
                let empty = a.1 - a.0 < 1e-3 && b.1 - b.0 < 1e-3;
                let moved = (a.0 - b.0).abs().max((a.1 - b.1).abs());
                assert!(empty || moved < 0.01, "segment jumps at {i} ms");
            }
            previous = current;
        }
    }

    #[test]
    fn linear_indeterminate_segments_never_overlap() {
        for i in 0..1750 {
            let [first, second] = linear_indeterminate(i as f32 * STEP);
            let visible = |s: (f32, f32)| s.1 - s.0 > 1e-3;
            if visible(first) && visible(second) {
                assert!(second.1 < first.0, "segments overlap at {i} ms");
            }
        }
    }
}
