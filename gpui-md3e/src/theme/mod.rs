mod color;
mod elevation;
mod shape;
mod typography;

use std::time::{Duration, Instant};

use gpui::{App, Global, Window};

pub use color::*;
pub use elevation::*;
pub use shape::*;
pub use typography::*;

use crate::motion::{MotionScheme, emphasized_decelerate, frame_now};

/// How long a theme change cross-fades every color role.
const THEME_TRANSITION: Duration = Duration::from_millis(550);

/// The Material theme shared by every component in the app.
pub struct Theme {
    /// The colors to render with this frame (mid-transition while animating).
    pub colors: ColorScheme,
    pub typography: Typography,
    pub shapes: ShapeScale,
    pub motion: MotionScheme,
    pub state: StateLayers,
    pub(crate) epoch: Instant,
    config: SchemeConfig,
    transition: Option<Transition>,
}

struct Transition {
    from: ColorScheme,
    to: ColorScheme,
    start: Instant,
}

impl Global for Theme {}

impl Theme {
    pub fn new(config: SchemeConfig) -> Self {
        Self {
            colors: ColorScheme::from_config(config),
            typography: Typography::default(),
            shapes: ShapeScale::default(),
            motion: MotionScheme::default(),
            state: StateLayers::default(),
            epoch: Instant::now(),
            config,
            transition: None,
        }
    }

    pub fn config(&self) -> SchemeConfig {
        self.config
    }

    pub fn is_dark(&self) -> bool {
        self.config.dark
    }

    /// Regenerates the scheme and cross-fades into it.
    pub fn set_config(&mut self, config: SchemeConfig) {
        if config == self.config {
            return;
        }
        self.config = config;
        self.transition = Some(Transition {
            from: self.colors,
            to: ColorScheme::from_config(config),
            start: Instant::now(),
        });
    }

    /// Advances an in-flight theme transition. Call once per frame from the
    /// root view's render.
    pub fn tick(window: &mut Window, cx: &mut App) {
        if cx.global::<Theme>().transition.is_none() {
            return;
        }
        let now = frame_now(window, cx);
        let theme = cx.global_mut::<Theme>();
        let Some(transition) = &theme.transition else {
            return;
        };
        let t = now
            .saturating_duration_since(transition.start)
            .as_secs_f32()
            / THEME_TRANSITION.as_secs_f32();
        if t >= 1.0 {
            theme.colors = transition.to;
            theme.transition = None;
        } else {
            theme.colors = transition
                .from
                .lerp(&transition.to, emphasized_decelerate(t));
            window.request_animation_frame();
        }
    }
}

/// Opacities of interaction state layers.
#[derive(Clone, Copy, Debug)]
pub struct StateLayers {
    pub hover: f32,
    pub focus: f32,
    pub pressed: f32,
    pub dragged: f32,
    pub disabled_content: f32,
    pub disabled_container: f32,
}

impl Default for StateLayers {
    fn default() -> Self {
        Self {
            hover: 0.08,
            focus: 0.10,
            pressed: 0.10,
            dragged: 0.16,
            disabled_content: 0.38,
            disabled_container: 0.10,
        }
    }
}

/// Access to the Material theme.
pub trait ActiveTheme {
    fn md(&self) -> &Theme;
}

impl ActiveTheme for App {
    fn md(&self) -> &Theme {
        self.global::<Theme>()
    }
}
