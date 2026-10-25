//! Material Symbols icons and the bundled asset source.

use std::borrow::Cow;

use anyhow::Result;
use gpui::{
    App, AssetSource, ElementId, Hsla, IntoElement, ParentElement as _, Pixels, RenderOnce,
    SharedString, Styled as _, Transformation, Window, div, px, radians, svg,
};

use crate::{
    child_id,
    motion::{MotionToken, animate},
};

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

/// Serves the icons bundled with gpui-md3e under `md3e/icons/`.
///
/// Apps with their own assets can call [`Md3eAssets::get`] from their
/// [`AssetSource`] before falling back to their own files.
pub struct Md3eAssets;

impl Md3eAssets {
    pub fn get(path: &str) -> Option<&'static [u8]> {
        ICON_FILES
            .iter()
            .find(|(name, _)| *name == path)
            .map(|(_, bytes)| *bytes)
    }
}

impl AssetSource for Md3eAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(Self::get(path).map(Cow::Borrowed))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICON_FILES
            .iter()
            .filter(|(name, _)| name.starts_with(path))
            .map(|(name, _)| SharedString::from(*name))
            .collect())
    }
}

/// An icon that cross-fades between its outlined and filled forms.
#[derive(IntoElement)]
pub struct IconView {
    icon: Icon,
    size: Pixels,
    color: Option<Hsla>,
    filled: bool,
    rotation: f32,
    id: Option<ElementId>,
}

/// Creates an outlined icon at the default 24px size.
pub fn icon(icon: Icon) -> IconView {
    IconView {
        icon,
        size: px(24.0),
        color: None,
        filled: false,
        rotation: 0.0,
        id: None,
    }
}

impl IconView {
    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    /// Uses the filled glyph. With [`IconView::animated`] the change fades.
    pub fn filled(mut self, filled: bool) -> Self {
        self.filled = filled;
        self
    }

    /// Rotation in degrees, clockwise.
    pub fn rotation(mut self, degrees: f32) -> Self {
        self.rotation = degrees;
        self
    }

    /// Gives the icon persistent state so fill changes animate.
    pub fn animated(mut self, id: impl Into<ElementId>) -> Self {
        self.id = Some(id.into());
        self
    }
}

impl RenderOnce for IconView {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (outlined, filled) = self.icon.paths();
        let fill = match &self.id {
            Some(id) => animate(
                child_id(id, "fill"),
                if self.filled { 1.0 } else { 0.0 },
                MotionToken::FastEffects,
                window,
                cx,
            )
            .clamp(0.0, 1.0),
            None => {
                if self.filled {
                    1.0
                } else {
                    0.0
                }
            }
        };
        let color = self.color.unwrap_or_else(|| gpui::black());
        let transformation = Transformation::rotate(radians(self.rotation.to_radians()));
        let glyph = |path: &'static str, opacity: f32| {
            let mut tint = color;
            tint.a *= opacity;
            svg()
                .absolute()
                .top_0()
                .left_0()
                .size(self.size)
                .path(path)
                .text_color(tint)
                .with_transformation(transformation)
        };

        let mut container = div().relative().flex_none().size(self.size);
        if fill < 1.0 {
            container = container.child(glyph(outlined, 1.0 - fill));
        }
        if fill > 0.0 {
            container = container.child(glyph(filled, fill));
        }
        container
    }
}
