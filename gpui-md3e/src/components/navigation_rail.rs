use std::rc::Rc;

use gpui::{
    AnyElement, App, ClickEvent, Corners, ElementId, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, RenderOnce, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, Window, div, px,
};

use crate::{
    ActiveTheme as _, Icon, StyledExt as _, child_id, icon,
    interaction::{Interaction, focus_ring, interactive, state_layer},
    measure_text,
    motion::{MotionToken, animate},
    theme::{TypeRole, TypeStyle, lerp_color},
};

const COLLAPSED_WIDTH: f32 = 96.0;
const EXPANDED_WIDTH: f32 = 220.0;
const INDICATOR_X: f32 = 20.0;

/// A destination in a [`NavigationRail`].
pub struct RailItem {
    pub icon: Icon,
    pub label: SharedString,
}

impl RailItem {
    pub fn new(icon: Icon, label: impl Into<SharedString>) -> Self {
        Self {
            icon,
            label: label.into(),
        }
    }
}

type SelectHandler = Rc<dyn Fn(&usize, &mut Window, &mut App)>;
type ToggleHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// The M3 Expressive navigation rail.
///
/// Collapsed, items stack their label under a pill indicator; expanded, the
/// rail widens and every item reflows into a row, all driven by one spatial
/// spring so the transition is continuous and interruptible.
#[derive(IntoElement)]
pub struct NavigationRail {
    id: ElementId,
    items: Vec<RailItem>,
    selected: usize,
    expanded: bool,
    header: Option<AnyElement>,
    on_select: Option<SelectHandler>,
    on_toggle: Option<ToggleHandler>,
}

impl NavigationRail {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            items: Vec::new(),
            selected: 0,
            expanded: false,
            header: None,
            on_select: None,
            on_toggle: None,
        }
    }

    pub fn item(mut self, item: RailItem) -> Self {
        self.items.push(item);
        self
    }

    pub fn selected(mut self, index: usize) -> Self {
        self.selected = index;
        self
    }

    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    /// Content placed under the menu button, typically a FAB.
    pub fn header(mut self, header: impl IntoElement) -> Self {
        self.header = Some(header.into_any_element());
        self
    }

    pub fn on_select(mut self, handler: impl Fn(&usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }

    /// Shows a menu button that toggles between collapsed and expanded.
    pub fn on_toggle(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_toggle = Some(Rc::new(handler));
        self
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

impl RenderOnce for NavigationRail {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.md().colors;
        let typography = cx.md().typography.clone();
        let t = animate(
            child_id(&self.id, "expand"),
            if self.expanded { 1.0 } else { 0.0 },
            MotionToken::DefaultSpatial,
            window,
            cx,
        );
        // Layout interpolation must not overshoot into negative geometry.
        let layout_t = t.clamp(0.0, 1.05);
        let width = lerp(COLLAPSED_WIDTH, EXPANDED_WIDTH, t);

        let mut rail = div()
            .id(self.id.clone())
            .flex_none()
            .flex()
            .flex_col()
            .h_full()
            .w(px(width))
            .pt(px(16.0))
            .gap(px(4.0))
            .bg(colors.surface_container)
            .role(Role::Navigation);

        let mut top = div()
            .flex()
            .flex_col()
            .gap(px(16.0))
            .pl(px(INDICATOR_X + 8.0))
            .pb(px(24.0));
        if let Some(on_toggle) = self.on_toggle.clone() {
            top = top.child(
                super::IconButton::new(child_id(&self.id, "menu"), Icon::Menu)
                    .label("Toggle navigation")
                    .size(super::ButtonSize::Small)
                    .on_click(move |event, window, cx| on_toggle(event, window, cx)),
            );
        }
        if let Some(header) = self.header {
            top = top.child(header);
        }
        rail = rail.child(top);

        let small = typography.style(TypeRole::LabelMedium);
        let large = typography.style(TypeRole::LabelLarge);
        let label_style = TypeStyle {
            size: lerp_px(small.size, large.size, layout_t),
            line_height: lerp_px(small.line_height, large.line_height, layout_t),
            ..small
        };

        for (ix, item) in self.items.into_iter().enumerate() {
            let item_id = child_id(&self.id, format!("item-{ix}"));
            let selected = ix == self.selected;
            let interaction = Interaction::use_state(&item_id, window, cx);
            let snapshot = Interaction::snapshot(&interaction, window, cx);
            let select_t = animate(
                child_id(&item_id, "selected"),
                if selected { 1.0 } else { 0.0 },
                MotionToken::FastSpatial,
                window,
                cx,
            );
            let color_t = animate(
                child_id(&item_id, "selected-color"),
                if selected { 1.0 } else { 0.0 },
                MotionToken::DefaultEffects,
                window,
                cx,
            )
            .clamp(0.0, 1.0);
            let focus = animate(
                child_id(&item_id, "focus"),
                if snapshot.focus_visible { 1.0 } else { 0.0 },
                MotionToken::FastEffects,
                window,
                cx,
            );

            let label_width = measure_text(&item.label, label_style, window, cx).as_f32();
            // Vertical layout: 56x32 pill, label centered underneath.
            // Horizontal layout: 56px tall pill holding icon and label.
            let indicator_w = lerp(56.0, 16.0 + 24.0 + 8.0 + label_width + 16.0, layout_t);
            let indicator_h = lerp(32.0, 56.0, layout_t);
            let icon_x = INDICATOR_X + lerp(16.0, 16.0, layout_t);
            let icon_y = lerp(4.0, 16.0, layout_t);
            let label_x = lerp(
                COLLAPSED_WIDTH / 2.0 - label_width / 2.0,
                INDICATOR_X + 16.0 + 24.0 + 8.0,
                layout_t,
            );
            let label_y = lerp(
                36.0,
                (56.0 - label_style.line_height.as_f32()) / 2.0,
                layout_t,
            );
            let item_h = lerp(56.0, 56.0, layout_t);

            let radius = px(indicator_h / 2.0);
            let radii = Corners::all(radius);
            let icon_color = lerp_color(
                colors.on_surface_variant,
                colors.on_secondary_container,
                color_t,
            );
            let label_color = lerp_color(colors.on_surface_variant, colors.secondary, color_t);

            // The active indicator grows outward from its center.
            let visible_w = indicator_w * select_t.max(0.0);
            let mut indicator_color = colors.secondary_container;
            indicator_color.a *= select_t.clamp(0.0, 1.0);

            let pill = div()
                .absolute()
                .left(px(INDICATOR_X))
                .top_0()
                .w(px(indicator_w))
                .h(px(indicator_h))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left(px((indicator_w - visible_w) / 2.0))
                        .w(px(visible_w))
                        .h_full()
                        .rounded(radius)
                        .bg(indicator_color),
                )
                .child(state_layer(
                    &item_id,
                    &interaction,
                    radii,
                    icon_color,
                    window,
                    cx,
                ))
                .child(focus_ring(radii, focus, colors.secondary));

            let mut row = div()
                .id(item_id.clone())
                .relative()
                .flex_none()
                .w_full()
                .h(px(item_h))
                .cursor_pointer()
                .role(Role::Tab)
                .aria_label(item.label.clone())
                .aria_selected(selected)
                .child(pill)
                .child(
                    div().absolute().left(px(icon_x)).top(px(icon_y)).child(
                        icon(item.icon)
                            .color(icon_color)
                            .filled(selected)
                            .animated(child_id(&item_id, "icon")),
                    ),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(label_x))
                        .top(px(label_y))
                        .whitespace_nowrap()
                        .md_type_style(label_style, cx)
                        .text_color(label_color)
                        .child(item.label),
                );
            if let Some(on_select) = self.on_select.clone() {
                row = row.on_click(move |_, window, cx| on_select(&ix, window, cx));
            }
            rail = rail.child(interactive(row, &interaction, false, cx));
        }
        rail
    }
}

fn lerp_px(a: Pixels, b: Pixels, t: f32) -> Pixels {
    a + (b - a) * t
}
