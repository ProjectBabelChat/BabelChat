//! The M3 Expressive shape library and shape morphing.
//!
//! Shapes are [`RoundedPolygon`]s normalized to the unit square, ported to
//! Rust by the `polymorpher` crate from androidx `graphics-shapes`.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use gpui::{Background, Bounds, Path, PathBuilder, Pixels, Window, point, px};
use polymorpher::{Cubic, Morph, RoundedPolygon, shapes};

macro_rules! material_shapes {
    ($($variant:ident => $label:literal, $ctor:expr;)*) => {
        /// One of the 35 shapes of the M3 Expressive shape library.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum MaterialShape {
            $($variant,)*
        }

        impl MaterialShape {
            pub const ALL: &[MaterialShape] = &[$(MaterialShape::$variant,)*];

            pub const fn label(self) -> &'static str {
                match self {
                    $(MaterialShape::$variant => $label,)*
                }
            }

            fn build(self) -> RoundedPolygon {
                match self {
                    $(MaterialShape::$variant => $ctor,)*
                }
            }
        }
    };
}

material_shapes! {
    Circle => "Circle", shapes::circle(None);
    Square => "Square", shapes::square();
    Slanted => "Slanted", shapes::slanted();
    Arch => "Arch", shapes::arch();
    Fan => "Fan", shapes::fan();
    Arrow => "Arrow", shapes::arrow();
    SemiCircle => "Semicircle", shapes::semi_circle();
    Oval => "Oval", shapes::oval();
    Pill => "Pill", shapes::pill();
    Triangle => "Triangle", shapes::triangle();
    Diamond => "Diamond", shapes::diamond();
    ClamShell => "Clam shell", shapes::clam_shell();
    Pentagon => "Pentagon", shapes::pentagon();
    Gem => "Gem", shapes::gem();
    Sunny => "Sunny", shapes::sunny();
    VerySunny => "Very sunny", shapes::very_sunny();
    Cookie4Sided => "4-sided cookie", shapes::cookie4();
    Cookie6Sided => "6-sided cookie", shapes::cookie6();
    Cookie7Sided => "7-sided cookie", shapes::cookie7();
    Cookie9Sided => "9-sided cookie", shapes::cookie9();
    Cookie12Sided => "12-sided cookie", shapes::cookie12();
    Ghostish => "Ghost-ish", shapes::ghostish();
    Clover4Leaf => "4-leaf clover", shapes::clover4();
    Clover8Leaf => "8-leaf clover", shapes::clover8();
    Burst => "Burst", shapes::burst();
    SoftBurst => "Soft burst", shapes::soft_burst();
    Boom => "Boom", shapes::boom();
    SoftBoom => "Soft boom", shapes::soft_boom();
    Flower => "Flower", shapes::flower();
    Puffy => "Puffy", shapes::puffy();
    PuffyDiamond => "Puffy diamond", shapes::puffy_diamond();
    PixelCircle => "Pixel circle", shapes::pixel_circle();
    PixelTriangle => "Pixel triangle", shapes::pixel_triangle();
    Bun => "Bun", shapes::bun();
    Heart => "Heart", shapes::heart();
}

thread_local! {
    static POLYGONS: RefCell<HashMap<MaterialShape, Rc<RoundedPolygon>>> = RefCell::default();
    static MORPHS: RefCell<HashMap<(MaterialShape, MaterialShape), Rc<Morph>>> = RefCell::default();
}

impl MaterialShape {
    /// The shape's polygon, normalized to the unit square.
    pub fn polygon(self) -> Rc<RoundedPolygon> {
        POLYGONS.with(|polygons| {
            polygons
                .borrow_mut()
                .entry(self)
                .or_insert_with(|| Rc::new(self.build()))
                .clone()
        })
    }

    /// A cached morph from this shape to `other`. Building a morph matches
    /// features between the outlines, which is too slow to do every frame.
    pub fn morph_to(self, other: MaterialShape) -> Rc<Morph> {
        MORPHS.with(|morphs| {
            morphs
                .borrow_mut()
                .entry((self, other))
                .or_insert_with(|| {
                    Rc::new(Morph::new(
                        (*self.polygon()).clone(),
                        (*other.polygon()).clone(),
                    ))
                })
                .clone()
        })
    }
}

/// How a unit-square outline is placed into pixel bounds.
#[derive(Clone, Copy, Debug)]
pub struct ShapeTransform {
    /// Clockwise rotation in degrees around the center of the bounds.
    pub rotation: f32,
    /// Scale around the center; 1.0 fills the bounds.
    pub scale: f32,
}

impl Default for ShapeTransform {
    fn default() -> Self {
        Self {
            rotation: 0.0,
            scale: 1.0,
        }
    }
}

/// Converts unit-square cubics into a filled GPUI path inside `bounds`.
pub fn cubics_path(
    cubics: &[Cubic],
    bounds: Bounds<Pixels>,
    transform: ShapeTransform,
) -> Option<Path<Pixels>> {
    let first = cubics.first()?;
    let size = bounds.size.width.min(bounds.size.height).as_f32() * transform.scale;
    let center = bounds.center();
    let (sin, cos) = transform.rotation.to_radians().sin_cos();
    let map = |p: polymorpher::geometry::Point| {
        let x = (p.x - 0.5) * size;
        let y = (p.y - 0.5) * size;
        point(
            center.x + px(x * cos - y * sin),
            center.y + px(x * sin + y * cos),
        )
    };

    let mut builder = PathBuilder::fill();
    builder.move_to(map(first.anchor0()));
    for cubic in cubics {
        builder.cubic_bezier_to(
            map(cubic.anchor1()),
            map(cubic.control0()),
            map(cubic.control1()),
        );
    }
    builder.close();
    builder.build().ok()
}

/// Paints a library shape filling `bounds`.
pub fn paint_shape(
    window: &mut Window,
    shape: MaterialShape,
    bounds: Bounds<Pixels>,
    transform: ShapeTransform,
    color: impl Into<Background>,
) {
    if let Some(path) = cubics_path(&shape.polygon().cubics, bounds, transform) {
        window.paint_path(path, color);
    }
}

/// Paints the in-between outline of `from` morphing into `to`.
pub fn paint_morph(
    window: &mut Window,
    from: MaterialShape,
    to: MaterialShape,
    progress: f32,
    bounds: Bounds<Pixels>,
    transform: ShapeTransform,
    color: impl Into<Background>,
) {
    let cubics = if progress <= 0.0 {
        from.polygon().cubics.clone()
    } else if progress >= 1.0 && from != to {
        to.polygon().cubics.clone()
    } else {
        from.morph_to(to).as_cubics(progress)
    };
    if let Some(path) = cubics_path(&cubics, bounds, transform) {
        window.paint_path(path, color);
    }
}

/// The largest distance from the unit-square center to any point of the
/// shapes' outlines, used to keep rotating shapes inside their bounds.
pub fn max_radius(shapes: &[MaterialShape]) -> f32 {
    shapes
        .iter()
        .flat_map(|shape| {
            let polygon = shape.polygon();
            polygon
                .cubics
                .iter()
                .flat_map(|cubic| [cubic.anchor0(), cubic.control0(), cubic.control1()])
                .map(|p| (p.x - 0.5).hypot(p.y - 0.5))
                .collect::<Vec<_>>()
        })
        .fold(0.5, f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_builds_a_closed_outline() {
        for shape in MaterialShape::ALL {
            let polygon = shape.polygon();
            assert!(!polygon.cubics.is_empty(), "{shape:?}");
            let first = polygon.cubics.first().unwrap().anchor0();
            let last = polygon.cubics.last().unwrap().anchor1();
            assert!((first - last).length() < 1e-3, "{shape:?} is not closed");
        }
    }

    #[test]
    fn loading_indicator_morphs_build() {
        use MaterialShape::*;
        let sequence = [
            SoftBurst,
            Cookie9Sided,
            Pentagon,
            Pill,
            Sunny,
            Cookie4Sided,
            Oval,
        ];
        for (i, from) in sequence.iter().enumerate() {
            let to = sequence[(i + 1) % sequence.len()];
            let morph = from.morph_to(to);
            assert!(!morph.as_cubics(0.5).is_empty());
        }
    }
}
