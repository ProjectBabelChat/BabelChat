use gpui::{BoxShadow, Hsla, point, px};

/// Key and ambient shadow parameters per elevation level:
/// (key y, key blur, ambient y, ambient blur, ambient spread).
const LEVELS: [(f32, f32, f32, f32, f32); 6] = [
    (0.0, 0.0, 0.0, 0.0, 0.0),
    (1.0, 2.0, 1.0, 3.0, 1.0),
    (1.0, 2.0, 2.0, 6.0, 2.0),
    (1.0, 3.0, 4.0, 8.0, 3.0),
    (2.0, 3.0, 6.0, 10.0, 4.0),
    (4.0, 4.0, 8.0, 12.0, 6.0),
];

/// Shadows for a (possibly fractional, i.e. animating) elevation level 0..=5.
pub fn elevation_shadows(level: f32, shadow: Hsla) -> Vec<BoxShadow> {
    let level = level.clamp(0.0, 5.0);
    if level <= 0.001 {
        return Vec::new();
    }
    let lower = level.floor() as usize;
    let upper = (lower + 1).min(5);
    let t = level - lower as f32;
    let mix = |a: f32, b: f32| a + (b - a) * t;
    let (a, b) = (LEVELS[lower], LEVELS[upper]);
    // Fade the whole shadow in over the first level so 0 -> 1 is continuous.
    let strength = level.min(1.0);
    vec![
        BoxShadow {
            color: shadow.opacity(0.3 * strength),
            offset: point(px(0.0), px(mix(a.0, b.0))),
            blur_radius: px(mix(a.1, b.1)),
            spread_radius: px(0.0),
            inset: false,
        },
        BoxShadow {
            color: shadow.opacity(0.15 * strength),
            offset: point(px(0.0), px(mix(a.2, b.2))),
            blur_radius: px(mix(a.3, b.3)),
            spread_radius: px(mix(a.4, b.4)),
            inset: false,
        },
    ]
}
