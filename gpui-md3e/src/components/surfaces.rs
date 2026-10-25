//! Cards, list items, the floating toolbar and snackbars.

use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, Corners, ElementId, Hsla, InteractiveElement as _, IntoElement,
    ParentElement, Pixels, RenderOnce, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div, px,
};
use smallvec::SmallVec;

use super::pressable::ClickHandler;
use crate::{
    ActiveTheme as _, Icon, StyledExt as _, child_id, icon,
    interaction::{Interaction, focus_ring, interactive, state_layer},
    motion::{MotionToken, animate},
    theme::TypeRole,
};

/// The three card treatments.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum CardStyle {
    #[default]
    Elevated,
    Filled,
    Outlined,
}

/// A container for related content; optionally clickable.
#[derive(IntoElement)]
pub struct Card {
    id: ElementId,
    style: CardStyle,
    children: SmallVec<[AnyElement; 4]>,
    on_click: Option<ClickHandler>,
    padding: Pixels,
}

impl Card {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            style: CardStyle::Elevated,
            children: SmallVec::new(),
            on_click: None,
            padding: px(16.0),
        }
    }

    pub fn style(mut self, style: CardStyle) -> Self {
        self.style = style;
        self
    }

    pub fn padding(mut self, padding: Pixels) -> Self {
        self.padding = padding;
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl ParentElement for Card {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Card {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = cx.md().colors;
        let radii = Corners::all(px(12.0));
        let clickable = self.on_click.is_some();
        let interaction = Interaction::use_state(&self.id, window, cx);
        let snapshot = Interaction::snapshot(&interaction, window, cx);
        let (bg, rest, hover) = match self.style {
            CardStyle::Elevated => (c.surface_container_low, 1.0, 2.0),
            CardStyle::Filled => (c.surface_container_highest, 0.0, 1.0),
            CardStyle::Outlined => (c.surface, 0.0, 1.0),
        };
        let elevation = animate(
            child_id(&self.id, "elevation"),
            if clickable && snapshot.hovered && !snapshot.pressed {
                hover
            } else {
                rest
            },
            MotionToken::DefaultEffects,
            window,
            cx,
        );
        let focus = animate(
            child_id(&self.id, "focus"),
            if snapshot.focus_visible { 1.0 } else { 0.0 },
            MotionToken::FastEffects,
            window,
            cx,
        );

        let mut card = div()
            .id(self.id.clone())
            .relative()
            .flex()
            .flex_col()
            .p(self.padding)
            .gap_2()
            .md_corners(radii)
            .bg(bg)
            .text_color(c.on_surface)
            .md_elevation(elevation, c.shadow);
        if self.style == CardStyle::Outlined {
            card = card.border_1().border_color(c.outline_variant);
        }
        if clickable {
            let layer = state_layer(&self.id, &interaction, radii, c.on_surface, window, cx);
            card = card.cursor_pointer().role(Role::Button).child(layer);
            if let Some(handler) = self.on_click {
                card = card.on_click(move |event, window, cx| handler(event, window, cx));
            }
            card = interactive(card, &interaction, false, cx);
        }
        card.children(self.children)
            .child(focus_ring(radii, focus, c.secondary))
    }
}

/// A row in a list with a leading icon or avatar, a headline, optional
/// supporting text and trailing content.
#[derive(IntoElement)]
pub struct ListItem {
    id: ElementId,
    headline: SharedString,
    supporting: Option<SharedString>,
    leading: Option<AnyElement>,
    trailing: Option<AnyElement>,
    selected: bool,
    segmented: Option<(bool, bool)>,
    on_click: Option<ClickHandler>,
}

impl ListItem {
    pub fn new(id: impl Into<ElementId>, headline: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            headline: headline.into(),
            supporting: None,
            leading: None,
            trailing: None,
            selected: false,
            segmented: None,
            on_click: None,
        }
    }

    pub fn supporting(mut self, text: impl Into<SharedString>) -> Self {
        self.supporting = Some(text.into());
        self
    }

    pub fn leading_icon(mut self, glyph: Icon) -> Self {
        self.leading = Some(icon(glyph).into_any_element());
        self
    }

    pub fn leading(mut self, element: impl IntoElement) -> Self {
        self.leading = Some(element.into_any_element());
        self
    }

    pub fn trailing(mut self, element: impl IntoElement) -> Self {
        self.trailing = Some(element.into_any_element());
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Renders the item as one segment of an M3 Expressive grouped list,
    /// with large outer corners on the first and last items.
    pub fn segmented(mut self, first: bool, last: bool) -> Self {
        self.segmented = Some((first, last));
        self
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for ListItem {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = cx.md().colors;
        let interaction = Interaction::use_state(&self.id, window, cx);
        let snapshot = Interaction::snapshot(&interaction, window, cx);
        let selected_t = animate(
            child_id(&self.id, "selected"),
            if self.selected { 1.0 } else { 0.0 },
            MotionToken::DefaultEffects,
            window,
            cx,
        )
        .clamp(0.0, 1.0);
        // Selected segments round out to a pill-like shape.
        let shape_t = animate(
            child_id(&self.id, "shape"),
            if self.selected || snapshot.pressed {
                1.0
            } else {
                0.0
            },
            MotionToken::FastSpatial,
            window,
            cx,
        );
        let focus = animate(
            child_id(&self.id, "focus"),
            if snapshot.focus_visible { 1.0 } else { 0.0 },
            MotionToken::FastEffects,
            window,
            cx,
        );
        let height = if self.supporting.is_some() {
            72.0
        } else {
            56.0
        };
        let radii = match self.segmented {
            Some((first, last)) => {
                let small = 4.0 + (16.0 - 4.0) * shape_t;
                let large = 16.0;
                Corners {
                    top_left: px(if first { large } else { small }),
                    top_right: px(if first { large } else { small }),
                    bottom_left: px(if last { large } else { small }),
                    bottom_right: px(if last { large } else { small }),
                }
            }
            None => Corners::all(px(0.0)),
        };
        let base_bg = if self.segmented.is_some() {
            c.surface_container_high
        } else {
            gpui::transparent_black()
        };
        let bg = crate::theme::lerp_color(base_bg, c.secondary_container, selected_t);
        let content = crate::theme::lerp_color(c.on_surface, c.on_secondary_container, selected_t);
        let typography = cx.md().typography.clone();

        let mut text = div().flex().flex_col().flex_1().min_w_0().child(
            div()
                .md_type_style(typography.style(TypeRole::BodyLarge), cx)
                .text_color(content)
                .truncate()
                .child(self.headline.clone()),
        );
        if let Some(supporting) = self.supporting {
            text = text.child(
                div()
                    .md_type_style(typography.style(TypeRole::BodyMedium), cx)
                    .text_color(c.on_surface_variant)
                    .truncate()
                    .child(supporting),
            );
        }

        let mut row = div()
            .id(self.id.clone())
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .gap_4()
            .px_4()
            .h(px(height))
            .w_full()
            .md_corners(radii)
            .bg(bg)
            .text_color(c.on_surface_variant)
            .role(Role::ListItem)
            .aria_label(self.headline);
        let layer = state_layer(&self.id, &interaction, radii, content, window, cx);
        row = row.child(layer);
        if let Some(leading) = self.leading {
            row = row.child(leading);
        }
        row = row.child(text);
        if let Some(trailing) = self.trailing {
            row = row.child(trailing);
        }
        row = row.child(focus_ring(radii, focus, c.secondary));
        if let Some(handler) = self.on_click {
            row = row
                .cursor_pointer()
                .on_click(move |event, window, cx| handler(event, window, cx));
        }
        interactive(row, &interaction, false, cx)
    }
}

/// A floating toolbar: a pill of actions hovering over content.
#[derive(IntoElement)]
pub struct Toolbar {
    children: SmallVec<[AnyElement; 6]>,
    vibrant: bool,
    vertical: bool,
}

impl Toolbar {
    pub fn new() -> Self {
        Self {
            children: SmallVec::new(),
            vibrant: false,
            vertical: false,
        }
    }

    /// Uses the primary container colors for extra emphasis.
    pub fn vibrant(mut self, vibrant: bool) -> Self {
        self.vibrant = vibrant;
        self
    }

    pub fn vertical(mut self, vertical: bool) -> Self {
        self.vertical = vertical;
        self
    }
}

impl Default for Toolbar {
    fn default() -> Self {
        Self::new()
    }
}

impl ParentElement for Toolbar {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl RenderOnce for Toolbar {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = cx.md().colors;
        let (bg, fg) = if self.vibrant {
            (c.primary_container, c.on_primary_container)
        } else {
            (c.surface_container, c.on_surface)
        };
        let bar = div()
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .p_2()
            .rounded_full()
            .bg(bg)
            .text_color(fg)
            .md_elevation(3.0, c.shadow);
        let bar = if self.vertical {
            bar.flex_col()
        } else {
            bar.flex_row()
        };
        bar.children(self.children)
    }
}

/// A brief message at the bottom of the screen. Its appearance is animated:
/// render it every frame and toggle `visible`.
#[derive(IntoElement)]
pub struct Snackbar {
    id: ElementId,
    message: SharedString,
    action: Option<(SharedString, ClickHandler)>,
    visible: bool,
}

impl Snackbar {
    pub fn new(id: impl Into<ElementId>, message: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            message: message.into(),
            action: None,
            visible: true,
        }
    }

    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some((label.into(), Rc::new(handler)));
        self
    }

    pub fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }
}

impl RenderOnce for Snackbar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let c = cx.md().colors;
        let t = animate(
            child_id(&self.id, "visible"),
            if self.visible { 1.0 } else { 0.0 },
            MotionToken::DefaultSpatial,
            window,
            cx,
        );
        let alpha = animate(
            child_id(&self.id, "alpha"),
            if self.visible { 1.0 } else { 0.0 },
            MotionToken::DefaultEffects,
            window,
            cx,
        )
        .clamp(0.0, 1.0);
        let mut bar = div()
            .id(self.id.clone())
            .relative()
            .top(px((1.0 - t) * 32.0))
            .opacity(alpha)
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .min_h(px(48.0))
            .pl_4()
            .pr_2()
            .rounded(px(4.0))
            .bg(c.inverse_surface)
            .text_color(c.inverse_on_surface)
            .md_type(TypeRole::BodyMedium, cx)
            .md_elevation(3.0 * alpha, c.shadow)
            .role(Role::Alert)
            .child(div().flex_1().py_3().child(self.message));
        if let Some((label, handler)) = self.action {
            bar = bar.child(snackbar_action(
                &self.id,
                label,
                handler,
                c.inverse_primary,
                window,
                cx,
            ));
        }
        bar
    }
}

fn snackbar_action(
    id: &ElementId,
    label: SharedString,
    handler: ClickHandler,
    color: Hsla,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let action_id = child_id(id, "action");
    let interaction = Interaction::use_state(&action_id, window, cx);
    let radii = Corners::all(px(20.0));
    let layer = state_layer(&action_id, &interaction, radii, color, window, cx);
    let element = div()
        .id(action_id)
        .relative()
        .h(px(40.0))
        .px_3()
        .flex()
        .items_center()
        .rounded(px(20.0))
        .text_color(color)
        .md_type(TypeRole::LabelLarge, cx)
        .cursor_pointer()
        .child(layer)
        .child(label)
        .on_click(move |event, window, cx| handler(event, window, cx));
    interactive(element, &interaction, false, cx).into_any_element()
}
