use gpui::{Pixels, px};

/// A step on the Material corner scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Corner {
    None,
    ExtraSmall,
    Small,
    Medium,
    Large,
    LargeIncreased,
    ExtraLarge,
    ExtraLargeIncreased,
    ExtraExtraLarge,
    /// Half of the shorter side: a pill or a circle.
    Full,
}

/// Corner radii of the shape scale, including the M3 Expressive additions.
#[derive(Clone, Copy, Debug)]
pub struct ShapeScale {
    pub none: Pixels,
    pub extra_small: Pixels,
    pub small: Pixels,
    pub medium: Pixels,
    pub large: Pixels,
    pub large_increased: Pixels,
    pub extra_large: Pixels,
    pub extra_large_increased: Pixels,
    pub extra_extra_large: Pixels,
}

impl Default for ShapeScale {
    fn default() -> Self {
        Self {
            none: px(0.0),
            extra_small: px(4.0),
            small: px(8.0),
            medium: px(12.0),
            large: px(16.0),
            large_increased: px(20.0),
            extra_large: px(28.0),
            extra_large_increased: px(32.0),
            extra_extra_large: px(48.0),
        }
    }
}

impl ShapeScale {
    /// Resolves a corner for a container whose shorter side is `min_side`.
    pub fn radius(&self, corner: Corner, min_side: Pixels) -> Pixels {
        let radius = match corner {
            Corner::None => self.none,
            Corner::ExtraSmall => self.extra_small,
            Corner::Small => self.small,
            Corner::Medium => self.medium,
            Corner::Large => self.large,
            Corner::LargeIncreased => self.large_increased,
            Corner::ExtraLarge => self.extra_large,
            Corner::ExtraLargeIncreased => self.extra_large_increased,
            Corner::ExtraExtraLarge => self.extra_extra_large,
            Corner::Full => min_side / 2.0,
        };
        radius.min(min_side / 2.0)
    }
}
