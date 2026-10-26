//! Material Design 3 Expressive for GPUI.
//!
//! Call [`init`] once at startup, register [`Md3eAssets`] as (or inside) the
//! application's asset source, and call [`Theme::tick`] at the start of the
//! root view's render so theme changes animate.

pub mod components;
mod icon;
pub mod interaction;
pub mod motion;
pub mod paint;
pub mod script;
pub mod shapes;
pub mod theme;

use std::{borrow::Cow, sync::Arc};

use gpui::{App, Corners, ElementId, Hsla, Pixels, Styled};

pub use icon::{Icon, IconView, Md3eAssets, icon};
pub use theme::{ActiveTheme, Theme};

use theme::{SchemeConfig, TypeRole, TypeStyle, elevation_shadows};

/// Commonly used items.
pub mod prelude {
    pub use crate::{
        ActiveTheme as _, Icon, StyledExt as _, Theme,
        components::*,
        icon,
        motion::{MotionToken, animate},
        shapes::MaterialShape,
        theme::{ColorScheme, Corner, SchemeConfig, SchemeVariant, TypeRole},
    };
}

const ROBOTO: [&[u8]; 3] = [
    include_bytes!("../assets/fonts/roboto/Roboto-Regular.ttf"),
    include_bytes!("../assets/fonts/roboto/Roboto-Medium.ttf"),
    include_bytes!("../assets/fonts/roboto/Roboto-Bold.ttf"),
];

/// Registers the bundled fonts and installs the theme.
pub fn init(config: SchemeConfig, cx: &mut App) {
    let fonts = ROBOTO.iter().map(|bytes| Cow::Borrowed(*bytes)).collect();
    if let Err(error) = cx.text_system().add_fonts(fonts) {
        eprintln!("gpui-md3e: failed to load Roboto: {error:#}");
    }
    cx.set_global(Theme::new(config));
    components::bind_keys(cx);
}

/// Derives the id of a sub-part of a component.
pub fn child_id(id: &ElementId, name: impl Into<gpui::SharedString>) -> ElementId {
    ElementId::NamedChild(Arc::new(id.clone()), name.into())
}

/// Material styling shortcuts for any [`Styled`] element.
pub trait StyledExt: Styled + Sized {
    /// Sets each corner radius.
    fn md_corners(mut self, radii: Corners<Pixels>) -> Self {
        let style = self.style();
        style.corner_radii.top_left = Some(radii.top_left.into());
        style.corner_radii.top_right = Some(radii.top_right.into());
        style.corner_radii.bottom_right = Some(radii.bottom_right.into());
        style.corner_radii.bottom_left = Some(radii.bottom_left.into());
        self
    }

    /// Applies a type scale role from the theme.
    fn md_type(self, role: TypeRole, cx: &App) -> Self {
        let style = cx.md().typography.style(role);
        self.md_type_style(style, cx)
    }

    /// Applies the emphasized variant of a type scale role.
    fn md_type_emphasized(self, role: TypeRole, cx: &App) -> Self {
        let style = cx.md().typography.emphasized(role);
        self.md_type_style(style, cx)
    }

    fn md_type_style(self, style: TypeStyle, cx: &App) -> Self {
        let font = cx.md().typography.font(style);
        self.font(font)
            .text_size(style.size)
            .line_height(style.line_height)
    }

    /// Applies the shadows of an elevation level (0..=5, may be fractional).
    fn md_elevation(mut self, level: f32, shadow: Hsla) -> Self {
        self.style().box_shadow = Some(elevation_shadows(level, shadow));
        self
    }
}

impl<E: Styled> StyledExt for E {}

/// Measures the width of a single line of text set in `style`.
pub fn measure_text(
    text: &gpui::SharedString,
    style: TypeStyle,
    window: &gpui::Window,
    cx: &App,
) -> Pixels {
    let run = gpui::TextRun {
        len: text.len(),
        font: cx.md().typography.font(style),
        ..Default::default()
    };
    window
        .text_system()
        .shape_line(text.clone(), style.size, &[run], None)
        .width()
}
