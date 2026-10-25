//! Geometry helpers for things GPUI's quads cannot express: ripples clipped to
//! a rounded container, morphing shapes, and stroked progress tracks.

use std::f32::consts::{PI, TAU};

use gpui::{
    Background, Bounds, Corners, LineCap, LineJoin, Path, PathBuilder, PathStyle, Pixels, Point,
    StrokeOptions, Window, point, px,
};

/// Points along the outline of a rounded rectangle, clockwise from the top
/// left corner. The polygon is convex, so it can be used as a clip region.
pub fn rounded_rect_polygon(bounds: Bounds<Pixels>, radii: Corners<Pixels>) -> Vec<Point<Pixels>> {
    let max = bounds.size.width.min(bounds.size.height) / 2.0;
    let clamp = |r: Pixels| r.max(px(0.0)).min(max);
    let (tl, tr, br, bl) = (
        clamp(radii.top_left),
        clamp(radii.top_right),
        clamp(radii.bottom_right),
        clamp(radii.bottom_left),
    );
    let left = bounds.left();
    let right = bounds.right();
    let top = bounds.top();
    let bottom = bounds.bottom();

    let mut points = Vec::with_capacity(4 * 12);
    let mut corner = |center: Point<Pixels>, radius: Pixels, start_angle: f32| {
        if radius <= px(0.01) {
            points.push(center);
            return;
        }
        let segments = ((radius.as_f32() / 2.0).ceil() as usize).clamp(3, 16);
        for i in 0..=segments {
            let angle = start_angle + (i as f32 / segments as f32) * (PI / 2.0);
            points.push(point(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            ));
        }
    };
    corner(point(left + tl, top + tl), tl, PI);
    corner(point(right - tr, top + tr), tr, 1.5 * PI);
    corner(point(right - br, bottom - br), br, 0.0);
    corner(point(left + bl, bottom - bl), bl, 0.5 * PI);
    points
}

/// A circle as a polygon, clockwise in screen coordinates.
pub fn circle_polygon(center: Point<Pixels>, radius: Pixels) -> Vec<Point<Pixels>> {
    let segments = ((radius.as_f32() * 0.6).ceil() as usize).clamp(16, 96);
    (0..segments)
        .map(|i| {
            let angle = i as f32 / segments as f32 * TAU;
            point(
                center.x + radius * angle.cos(),
                center.y + radius * angle.sin(),
            )
        })
        .collect()
}

/// Clips `subject` to the convex polygon `clip` (Sutherland–Hodgman).
/// Both polygons must wind clockwise in screen coordinates.
pub fn clip_convex(subject: &[Point<Pixels>], clip: &[Point<Pixels>]) -> Vec<Point<Pixels>> {
    let cross = |a: Point<Pixels>, b: Point<Pixels>, p: Point<Pixels>| {
        (b.x - a.x).as_f32() * (p.y - a.y).as_f32() - (b.y - a.y).as_f32() * (p.x - a.x).as_f32()
    };
    let intersect = |a: Point<Pixels>, b: Point<Pixels>, p: Point<Pixels>, q: Point<Pixels>| {
        let (x1, y1, x2, y2) = (a.x.as_f32(), a.y.as_f32(), b.x.as_f32(), b.y.as_f32());
        let (x3, y3, x4, y4) = (p.x.as_f32(), p.y.as_f32(), q.x.as_f32(), q.y.as_f32());
        let denom = (x1 - x2) * (y3 - y4) - (y1 - y2) * (x3 - x4);
        if denom.abs() < f32::EPSILON {
            return p;
        }
        let t = ((x1 - x3) * (y3 - y4) - (y1 - y3) * (x3 - x4)) / denom;
        point(px(x1 + t * (x2 - x1)), px(y1 + t * (y2 - y1)))
    };

    let mut output = subject.to_vec();
    for i in 0..clip.len() {
        if output.is_empty() {
            break;
        }
        let a = clip[i];
        let b = clip[(i + 1) % clip.len()];
        let input = std::mem::take(&mut output);
        for j in 0..input.len() {
            let current = input[j];
            let previous = input[(j + input.len() - 1) % input.len()];
            let current_inside = cross(a, b, current) >= 0.0;
            let previous_inside = cross(a, b, previous) >= 0.0;
            if current_inside {
                if !previous_inside {
                    output.push(intersect(previous, current, a, b));
                }
                output.push(current);
            } else if previous_inside {
                output.push(intersect(previous, current, a, b));
            }
        }
    }
    output
}

/// Builds a filled path from a polygon.
pub fn polygon_path(points: &[Point<Pixels>]) -> Option<Path<Pixels>> {
    if points.len() < 3 {
        return None;
    }
    let mut builder = PathBuilder::fill();
    builder.add_polygon(points, true);
    builder.build().ok()
}

/// Fills `bounds` shaped by `radii` with a circle of `radius` centered at
/// `center`, clipped to the container. Used for ripples.
pub fn paint_clipped_circle(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    radii: Corners<Pixels>,
    center: Point<Pixels>,
    radius: Pixels,
    color: impl Into<Background>,
) {
    let container = rounded_rect_polygon(bounds, radii);
    // A circle that covers every corner of the container is the container.
    let covers = [
        bounds.origin,
        bounds.top_right(),
        bounds.bottom_right(),
        bounds.bottom_left(),
    ]
    .iter()
    .all(|corner| distance(*corner, center) <= radius);
    let clipped = if covers {
        container
    } else {
        clip_convex(&circle_polygon(center, radius), &container)
    };
    if let Some(path) = polygon_path(&clipped) {
        window.paint_path(path, color);
    }
}

pub fn distance(a: Point<Pixels>, b: Point<Pixels>) -> Pixels {
    px((a.x - b.x).as_f32().hypot((a.y - b.y).as_f32()))
}

/// A stroke builder with round caps, as Material progress tracks use.
pub fn round_stroke(width: Pixels) -> PathBuilder {
    PathBuilder::stroke(width).with_style(PathStyle::Stroke(
        StrokeOptions::default()
            .with_line_width(width.as_f32())
            .with_line_cap(LineCap::Round)
            .with_line_join(LineJoin::Round),
    ))
}
