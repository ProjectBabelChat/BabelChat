use gpui::{Hsla, Rgba};
use material_colors::{
    color::Rgb,
    dynamic_color::{DynamicScheme, Platform, SpecVersion, Variant},
    hct::Hct,
};

/// The palette style used to derive a [`ColorScheme`] from a seed color.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SchemeVariant {
    TonalSpot,
    Neutral,
    Vibrant,
    Expressive,
    Fidelity,
    Content,
    Rainbow,
    FruitSalad,
    Monochrome,
}

impl SchemeVariant {
    pub const ALL: [SchemeVariant; 9] = [
        SchemeVariant::TonalSpot,
        SchemeVariant::Neutral,
        SchemeVariant::Vibrant,
        SchemeVariant::Expressive,
        SchemeVariant::Fidelity,
        SchemeVariant::Content,
        SchemeVariant::Rainbow,
        SchemeVariant::FruitSalad,
        SchemeVariant::Monochrome,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            SchemeVariant::TonalSpot => "Tonal spot",
            SchemeVariant::Neutral => "Neutral",
            SchemeVariant::Vibrant => "Vibrant",
            SchemeVariant::Expressive => "Expressive",
            SchemeVariant::Fidelity => "Fidelity",
            SchemeVariant::Content => "Content",
            SchemeVariant::Rainbow => "Rainbow",
            SchemeVariant::FruitSalad => "Fruit salad",
            SchemeVariant::Monochrome => "Monochrome",
        }
    }

    fn to_material(self) -> Variant {
        match self {
            SchemeVariant::TonalSpot => Variant::TonalSpot,
            SchemeVariant::Neutral => Variant::Neutral,
            SchemeVariant::Vibrant => Variant::Vibrant,
            SchemeVariant::Expressive => Variant::Expressive,
            SchemeVariant::Fidelity => Variant::Fidelity,
            SchemeVariant::Content => Variant::Content,
            SchemeVariant::Rainbow => Variant::Rainbow,
            SchemeVariant::FruitSalad => Variant::FruitSalad,
            SchemeVariant::Monochrome => Variant::Monochrome,
        }
    }
}

/// The inputs a [`ColorScheme`] is generated from.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SchemeConfig {
    /// The seed color as `0xRRGGBB`.
    pub seed: u32,
    pub variant: SchemeVariant,
    pub dark: bool,
    /// From -1 (reduced) through 0 (standard) to 1 (high contrast).
    pub contrast: f64,
}

impl Default for SchemeConfig {
    fn default() -> Self {
        Self {
            seed: 0x6750A4,
            variant: SchemeVariant::TonalSpot,
            dark: false,
            contrast: 0.0,
        }
    }
}

macro_rules! color_roles {
    ($($role:ident),* $(,)?) => {
        /// Every color role of a Material 3 color scheme.
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct ColorScheme {
            $(pub $role: Hsla,)*
        }

        impl ColorScheme {
            /// Generates the scheme with the 2025 dynamic color spec.
            pub fn from_config(config: SchemeConfig) -> Self {
                let scheme = DynamicScheme::from_spec(
                    Hct::new(Rgb::from_u32(config.seed)),
                    config.variant.to_material(),
                    config.dark,
                    Some(config.contrast),
                    Platform::Phone,
                    SpecVersion::Spec2025,
                );
                Self {
                    $($role: to_hsla(scheme.$role()),)*
                }
            }

            /// Linearly interpolates every role in sRGB space.
            pub fn lerp(&self, other: &Self, t: f32) -> Self {
                Self {
                    $($role: lerp_color(self.$role, other.$role, t),)*
                }
            }
        }
    };
}

color_roles!(
    primary,
    on_primary,
    primary_container,
    on_primary_container,
    inverse_primary,
    primary_fixed,
    primary_fixed_dim,
    on_primary_fixed,
    on_primary_fixed_variant,
    secondary,
    on_secondary,
    secondary_container,
    on_secondary_container,
    secondary_fixed,
    secondary_fixed_dim,
    on_secondary_fixed,
    on_secondary_fixed_variant,
    tertiary,
    on_tertiary,
    tertiary_container,
    on_tertiary_container,
    tertiary_fixed,
    tertiary_fixed_dim,
    on_tertiary_fixed,
    on_tertiary_fixed_variant,
    error,
    on_error,
    error_container,
    on_error_container,
    background,
    on_background,
    surface,
    on_surface,
    surface_variant,
    on_surface_variant,
    surface_dim,
    surface_bright,
    surface_container_lowest,
    surface_container_low,
    surface_container,
    surface_container_high,
    surface_container_highest,
    inverse_surface,
    inverse_on_surface,
    outline,
    outline_variant,
    shadow,
    scrim,
    surface_tint,
);

fn to_hsla(rgb: Rgb) -> Hsla {
    gpui::rgb(rgb.as_u32()).into()
}

/// Interpolates two colors in sRGB space, which avoids the hue sweeps an HSL
/// interpolation produces between unrelated colors.
pub fn lerp_color(from: Hsla, to: Hsla, t: f32) -> Hsla {
    if from == to {
        return from;
    }
    let a = Rgba::from(from);
    let b = Rgba::from(to);
    let mix = |x: f32, y: f32| x + (y - x) * t;
    Rgba {
        r: mix(a.r, b.r),
        g: mix(a.g, b.g),
        b: mix(a.b, b.b),
        a: mix(a.a, b.a),
    }
    .into()
}

/// Composites `overlay` at `opacity` over `base`, like a Material state layer.
pub fn blend_over(base: Hsla, overlay: Hsla, opacity: f32) -> Hsla {
    let mut over = overlay;
    over.a *= opacity;
    base.blend(over)
}
