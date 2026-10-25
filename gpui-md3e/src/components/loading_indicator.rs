use std::time::Duration;

use gpui::{
    App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, RenderOnce,
    Role, StatefulInteractiveElement as _, Styled as _, Window, canvas, div, px,
};

use crate::{
    ActiveTheme as _,
    motion::{Spring, clock},
    shapes::{MaterialShape, ShapeTransform, max_radius, paint_morph},
};

/// The shapes an indeterminate loading indicator morphs through.
pub const LOADING_SHAPES: [MaterialShape; 7] = [
    MaterialShape::SoftBurst,
    MaterialShape::Cookie9Sided,
    MaterialShape::Pentagon,
    MaterialShape::Pill,
    MaterialShape::Sunny,
    MaterialShape::Cookie4Sided,
    MaterialShape::Oval,
];

const MORPH_INTERVAL: Duration = Duration::from_millis(650);
const GLOBAL_ROTATION: Duration = Duration::from_millis(4666);
/// The morph spring from Compose's `LoadingIndicator`.
const MORPH_SPRING: Spring = Spring::new(0.6, 200.0);
const CONTAINER: f32 = 48.0;
const INDICATOR: f32 = 38.0;

/// The M3 Expressive loading indicator: a shape that morphs and spins.
///
/// Meant for waits under five seconds. Without a progress value it loops
/// through [`LOADING_SHAPES`]; with one it morphs from a circle to a burst.
#[derive(IntoElement)]
pub struct LoadingIndicator {
    id: ElementId,
    contained: bool,
    progress: Option<f32>,
    size: Pixels,
}

impl LoadingIndicator {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            contained: false,
            progress: None,
            size: px(CONTAINER),
        }
    }

    /// Draws the indicator on a circular container.
    pub fn contained(mut self, contained: bool) -> Self {
        self.contained = contained;
        self
    }

    /// Shows determinate progress from 0 to 1.
    pub fn progress(mut self, progress: f32) -> Self {
        self.progress = Some(progress.clamp(0.0, 1.0));
        self
    }

    /// The container size; the indicator scales with it.
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }
}

impl RenderOnce for LoadingIndicator {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.md().colors;
        let (container, color) = if self.contained {
            (Some(colors.primary_container), colors.on_primary_container)
        } else {
            (None, colors.primary)
        };

        // Keep rotating shapes inside the container.
        let fit = 0.5 / max_radius(&LOADING_SHAPES);
        let scale = fit * INDICATOR / CONTAINER;

        let (from, to, progress, rotation) = match self.progress {
            Some(progress) => (
                MaterialShape::Circle,
                MaterialShape::SoftBurst,
                progress,
                -progress * 180.0,
            ),
            None => {
                window.request_animation_frame();
                let elapsed = clock(cx);
                let interval = MORPH_INTERVAL.as_secs_f32();
                let step = (elapsed.as_secs_f32() / interval).floor();
                let local = elapsed.as_secs_f32() - step * interval;
                let morph = MORPH_SPRING
                    .config()
                    .step(Default::default(), 1.0, local)
                    .position;
                let index = step as usize;
                let global = (elapsed.as_secs_f32() % GLOBAL_ROTATION.as_secs_f32())
                    / GLOBAL_ROTATION.as_secs_f32()
                    * 360.0;
                (
                    LOADING_SHAPES[index % LOADING_SHAPES.len()],
                    LOADING_SHAPES[(index + 1) % LOADING_SHAPES.len()],
                    morph,
                    // Each morph adds a quarter turn, overshooting with the spring.
                    (index as f32 + 1.0) * 90.0 + morph * 90.0 + global,
                )
            }
        };

        let mut root = div()
            .id(self.id)
            .flex_none()
            .size(self.size)
            .rounded_full()
            .role(Role::ProgressIndicator)
            .aria_label("Loading");
        if let Some(container) = container {
            root = root.bg(container);
        }
        if let Some(progress) = self.progress {
            root = root.aria_numeric_value(progress as f64);
        }
        root.child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    paint_morph(
                        window,
                        from,
                        to,
                        progress.clamp(0.0, 1.0),
                        bounds,
                        ShapeTransform { rotation, scale },
                        color,
                    );
                },
            )
            .size_full(),
        )
    }
}
