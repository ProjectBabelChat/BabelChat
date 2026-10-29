use gpui::{Font, FontFallbacks, FontFeatures, FontStyle, FontWeight, Pixels, SharedString, px};

/// A role in the Material 3 type scale.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TypeRole {
    DisplayLarge,
    DisplayMedium,
    DisplaySmall,
    HeadlineLarge,
    HeadlineMedium,
    HeadlineSmall,
    TitleLarge,
    TitleMedium,
    TitleSmall,
    BodyLarge,
    BodyMedium,
    BodySmall,
    LabelLarge,
    LabelMedium,
    LabelSmall,
}

impl TypeRole {
    pub const ALL: [TypeRole; 15] = [
        TypeRole::DisplayLarge,
        TypeRole::DisplayMedium,
        TypeRole::DisplaySmall,
        TypeRole::HeadlineLarge,
        TypeRole::HeadlineMedium,
        TypeRole::HeadlineSmall,
        TypeRole::TitleLarge,
        TypeRole::TitleMedium,
        TypeRole::TitleSmall,
        TypeRole::BodyLarge,
        TypeRole::BodyMedium,
        TypeRole::BodySmall,
        TypeRole::LabelLarge,
        TypeRole::LabelMedium,
        TypeRole::LabelSmall,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            TypeRole::DisplayLarge => "Display Large",
            TypeRole::DisplayMedium => "Display Medium",
            TypeRole::DisplaySmall => "Display Small",
            TypeRole::HeadlineLarge => "Headline Large",
            TypeRole::HeadlineMedium => "Headline Medium",
            TypeRole::HeadlineSmall => "Headline Small",
            TypeRole::TitleLarge => "Title Large",
            TypeRole::TitleMedium => "Title Medium",
            TypeRole::TitleSmall => "Title Small",
            TypeRole::BodyLarge => "Body Large",
            TypeRole::BodyMedium => "Body Medium",
            TypeRole::BodySmall => "Body Small",
            TypeRole::LabelLarge => "Label Large",
            TypeRole::LabelMedium => "Label Medium",
            TypeRole::LabelSmall => "Label Small",
        }
    }
}

/// The metrics of one type scale entry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypeStyle {
    pub size: Pixels,
    pub line_height: Pixels,
    pub weight: FontWeight,
    /// Letter spacing in pixels. GPUI cannot apply tracking yet, so this is
    /// informational until the text system supports it.
    pub tracking: f32,
}

const fn style(size: f32, line_height: f32, weight: f32, tracking: f32) -> TypeStyle {
    TypeStyle {
        size: px(size),
        line_height: px(line_height),
        weight: FontWeight(weight),
        tracking,
    }
}

const REGULAR: f32 = 400.0;
const MEDIUM: f32 = 500.0;
const BOLD: f32 = 700.0;

/// Baseline and emphasized type scales from the M3 Expressive tokens.
#[derive(Clone, Debug)]
pub struct Typography {
    pub family: SharedString,
    pub fallbacks: FontFallbacks,
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            family: "Roboto".into(),
            // Color emoji before CJK: CJK fonts carry plain glyphs for
            // many emoji (❤, 😀), and emoji fonts no CJK, so each script
            // still finds its own font.
            fallbacks: FontFallbacks::from_fonts(vec![
                "Noto Color Emoji".into(),
                "Apple Color Emoji".into(),
                "Segoe UI Emoji".into(),
                "Noto Sans CJK JP".into(),
                "Noto Sans JP".into(),
                "Hiragino Sans".into(),
                "Yu Gothic UI".into(),
            ]),
        }
    }
}

impl Typography {
    /// The baseline style of a role.
    pub const fn style(&self, role: TypeRole) -> TypeStyle {
        match role {
            TypeRole::DisplayLarge => style(57.0, 64.0, REGULAR, -0.25),
            TypeRole::DisplayMedium => style(45.0, 52.0, REGULAR, 0.0),
            TypeRole::DisplaySmall => style(36.0, 44.0, REGULAR, 0.0),
            TypeRole::HeadlineLarge => style(32.0, 40.0, REGULAR, 0.0),
            TypeRole::HeadlineMedium => style(28.0, 36.0, REGULAR, 0.0),
            TypeRole::HeadlineSmall => style(24.0, 32.0, REGULAR, 0.0),
            TypeRole::TitleLarge => style(22.0, 28.0, REGULAR, 0.0),
            TypeRole::TitleMedium => style(16.0, 24.0, MEDIUM, 0.15),
            TypeRole::TitleSmall => style(14.0, 20.0, MEDIUM, 0.1),
            TypeRole::BodyLarge => style(16.0, 24.0, REGULAR, 0.5),
            TypeRole::BodyMedium => style(14.0, 20.0, REGULAR, 0.25),
            TypeRole::BodySmall => style(12.0, 16.0, REGULAR, 0.4),
            TypeRole::LabelLarge => style(14.0, 20.0, MEDIUM, 0.1),
            TypeRole::LabelMedium => style(12.0, 16.0, MEDIUM, 0.5),
            TypeRole::LabelSmall => style(11.0, 16.0, MEDIUM, 0.5),
        }
    }

    /// The emphasized style of a role, which only changes the weight.
    pub const fn emphasized(&self, role: TypeRole) -> TypeStyle {
        let mut base = self.style(role);
        base.weight = FontWeight(match role {
            TypeRole::LabelLarge
            | TypeRole::LabelMedium
            | TypeRole::LabelSmall
            | TypeRole::TitleMedium
            | TypeRole::TitleSmall => BOLD,
            _ => MEDIUM,
        });
        base
    }

    /// The font to render a style with, including CJK and emoji fallbacks.
    pub fn font(&self, style: TypeStyle) -> Font {
        Font {
            family: self.family.clone(),
            features: FontFeatures::default(),
            fallbacks: Some(self.fallbacks.clone()),
            weight: style.weight,
            style: FontStyle::Normal,
        }
    }
}
