use gpui::{
    App, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, Pixels,
    RenderOnce, Styled as _, Window, canvas, div, px,
};

use crate::{
    ActiveTheme as _, child_id,
    interaction::{Interaction, interactive},
    motion::{MotionToken, animate},
    shapes::{MaterialShape, ShapeTransform, paint_morph},
};

/// A library shape that morphs into another shape while hovered.
#[derive(IntoElement)]
pub struct ShapeView {
    id: ElementId,
    shape: MaterialShape,
    hover_shape: Option<MaterialShape>,
    size: Pixels,
    color: Option<Hsla>,
    hover_color: Option<Hsla>,
    rotation: f32,
}

impl ShapeView {
    pub fn new(id: impl Into<ElementId>, shape: MaterialShape) -> Self {
        Self {
            id: id.into(),
            shape,
            hover_shape: None,
            size: px(64.0),
            color: None,
            hover_color: None,
            rotation: 0.0,
        }
    }

    /// Morphs into `shape` (with a quarter turn) while hovered.
    pub fn morph_on_hover(mut self, shape: MaterialShape) -> Self {
        self.hover_shape = Some(shape);
        self
    }

    pub fn size(mut self, size: Pixels) -> Self {
        self.size = size;
        self
    }

    pub fn color(mut self, color: Hsla) -> Self {
        self.color = Some(color);
        self
    }

    pub fn hover_color(mut self, color: Hsla) -> Self {
        self.hover_color = Some(color);
        self
    }

    /// Static rotation in degrees.
    pub fn rotation(mut self, degrees: f32) -> Self {
        self.rotation = degrees;
        self
    }
}

impl RenderOnce for ShapeView {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.md().colors;
        let base = self.color.unwrap_or(colors.primary);
        let hover_color = self.hover_color.unwrap_or(colors.tertiary);
        let interaction = Interaction::use_state(&self.id, window, cx);
        let hovered = interaction.read(cx).hovered && self.hover_shape.is_some();
        let t = animate(
            child_id(&self.id, "morph"),
            if hovered { 1.0 } else { 0.0 },
            MotionToken::DefaultSpatial,
            window,
            cx,
        );
        let color_t = animate(
            child_id(&self.id, "color"),
            if hovered { 1.0 } else { 0.0 },
            MotionToken::DefaultEffects,
            window,
            cx,
        )
        .clamp(0.0, 1.0);
        let color = crate::theme::lerp_color(base, hover_color, color_t);
        let from = self.shape;
        let to = self.hover_shape.unwrap_or(self.shape);
        // The overshoot of the spring shows as a slight swell.
        let overshoot = (t - t.clamp(0.0, 1.0)).abs();
        let transform = ShapeTransform {
            rotation: self.rotation + t * 90.0,
            scale: 1.0 + overshoot * 0.6,
        };

        let element = div().id(self.id.clone()).flex_none().size(self.size).child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    paint_morph(
                        window,
                        from,
                        to,
                        t.clamp(0.0, 1.0),
                        bounds,
                        transform,
                        color,
                    );
                },
            )
            .size_full(),
        );
        interactive(element, &interaction, false, cx)
    }
}
