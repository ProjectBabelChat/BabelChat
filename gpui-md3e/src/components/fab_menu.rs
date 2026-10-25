use std::{rc::Rc, time::Instant};

use gpui::{
    App, ClickEvent, Corners, ElementId, IntoElement, ParentElement as _, RenderOnce, Role,
    SharedString, Styled as _, Window, div, px,
};

use super::pressable::{ClickHandler, GroupOverrides, Pressable, SurfaceColors};
use crate::{
    ActiveTheme as _, Icon, StyledExt as _, child_id, icon,
    motion::{MotionToken, animate, frame_now},
    theme::{TypeRole, lerp_color},
};

/// Delay between successive menu items appearing.
const STAGGER: f32 = 0.045;

struct FabMenuState {
    changed_at: Instant,
    open: bool,
}

/// One action in a [`FabMenu`].
pub struct FabMenuItem {
    pub icon: Icon,
    pub label: SharedString,
    pub on_click: Option<ClickHandler>,
}

impl FabMenuItem {
    pub fn new(icon: Icon, label: impl Into<SharedString>) -> Self {
        Self {
            icon,
            label: label.into(),
            on_click: None,
        }
    }

    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

/// A FAB that morphs into a close button and fans out a column of actions,
/// replacing the speed dial.
#[derive(IntoElement)]
pub struct FabMenu {
    id: ElementId,
    icon: Icon,
    open: bool,
    items: Vec<FabMenuItem>,
    on_toggle: Option<ClickHandler>,
}

impl FabMenu {
    pub fn new(id: impl Into<ElementId>, icon: Icon) -> Self {
        Self {
            id: id.into(),
            icon,
            open: false,
            items: Vec::new(),
            on_toggle: None,
        }
    }

    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    pub fn item(mut self, item: FabMenuItem) -> Self {
        self.items.push(item);
        self
    }

    pub fn on_toggle(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_toggle = Some(Rc::new(handler));
        self
    }
}

impl RenderOnce for FabMenu {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.md();
        let colors = theme.colors;
        let label_style = theme.typography.style(TypeRole::TitleMedium);
        let id = self.id.clone();

        let now = frame_now(window, cx);
        let state = window.use_keyed_state(child_id(&id, "menu"), cx, |_, _| FabMenuState {
            changed_at: now,
            open: self.open,
        });
        if state.read(cx).open != self.open {
            let open = self.open;
            state.update(cx, |state, _| {
                state.open = open;
                state.changed_at = now;
            });
        }
        let since = now
            .saturating_duration_since(state.read(cx).changed_at)
            .as_secs_f32();
        let count = self.items.len();

        let open_t = animate(
            child_id(&id, "open"),
            if self.open { 1.0 } else { 0.0 },
            MotionToken::FastSpatial,
            window,
            cx,
        );
        let color_t = animate(
            child_id(&id, "open-color"),
            if self.open { 1.0 } else { 0.0 },
            MotionToken::DefaultEffects,
            window,
            cx,
        )
        .clamp(0.0, 1.0);

        let mut column = div().flex().flex_col().items_end().gap(px(4.0));
        for (ix, item) in self.items.into_iter().enumerate() {
            // Items closest to the FAB move first when opening and last when
            // closing.
            let order = if self.open { count - 1 - ix } else { ix };
            let delay = order as f32 * STAGGER;
            let visible = if self.open {
                since >= delay
            } else {
                since < delay
            };
            if since < delay {
                window.request_animation_frame();
            }
            let item_id = child_id(&id, format!("item-{ix}"));
            let t = animate(
                child_id(&item_id, "appear"),
                if visible { 1.0 } else { 0.0 },
                MotionToken::DefaultSpatial,
                window,
                cx,
            );
            let alpha = animate(
                child_id(&item_id, "alpha"),
                if visible { 1.0 } else { 0.0 },
                MotionToken::DefaultEffects,
                window,
                cx,
            )
            .clamp(0.0, 1.0);
            if alpha <= 0.001 && t.abs() <= 0.001 {
                continue;
            }
            let label = item.label.clone();
            let glyph = item.icon;
            let pill = Pressable {
                id: item_id,
                label: item.label,
                role: Role::MenuItem,
                height: px(56.0),
                width: None,
                padding_x: px(24.0),
                gap: px(8.0),
                rest_radii: Corners::all(px(28.0)),
                selected_radii: Corners::all(px(28.0)),
                pressed_radii: Corners::all(px(16.0)),
                colors: SurfaceColors::new(colors.primary_container, colors.on_primary_container),
                selected_colors: None,
                selected: None,
                elevation: (3.0 * alpha, 4.0 * alpha),
                outline_width: px(0.0),
                disabled: alpha < 0.5,
                on_click: item.on_click,
                group: GroupOverrides::default(),
            }
            .render(window, cx, move |state, _, cx| {
                vec![
                    icon(glyph).color(state.content).into_any_element(),
                    div()
                        .md_type_style(label_style, cx)
                        .child(label)
                        .into_any_element(),
                ]
            });
            // Slide up from the FAB while fading and growing in.
            column = column.child(
                div()
                    .relative()
                    .top(px((1.0 - t) * 24.0))
                    .opacity(alpha)
                    .child(pill),
            );
        }

        // The FAB itself morphs into a round close button.
        let corner = px(16.0) + (px(28.0) - px(16.0)) * open_t;
        let fab_colors = SurfaceColors::new(
            lerp_color(colors.primary_container, colors.primary, color_t),
            lerp_color(colors.on_primary_container, colors.on_primary, color_t),
        );
        let base_icon = self.icon;
        let toggle = Pressable {
            id: child_id(&id, "toggle"),
            label: if self.open { "Close menu" } else { "Open menu" }.into(),
            role: Role::Button,
            height: px(56.0),
            width: Some(px(56.0)),
            padding_x: px(0.0),
            gap: px(0.0),
            rest_radii: Corners::all(corner),
            selected_radii: Corners::all(corner),
            pressed_radii: Corners::all(corner),
            colors: fab_colors,
            selected_colors: None,
            selected: Some(self.open),
            elevation: (3.0, 4.0),
            outline_width: px(0.0),
            disabled: false,
            on_click: self.on_toggle,
            group: GroupOverrides {
                radii: Some(Corners::all(corner)),
                ..Default::default()
            },
        }
        .render(window, cx, move |state, _, _| {
            let mut base = state.content;
            base.a *= 1.0 - color_t;
            let mut close = state.content;
            close.a *= color_t;
            vec![
                div()
                    .relative()
                    .size(px(24.0))
                    .child(
                        div()
                            .absolute()
                            .child(icon(base_icon).color(base).rotation(open_t * 90.0)),
                    )
                    .child(
                        div().absolute().child(
                            icon(Icon::Close)
                                .size(px(24.0))
                                .color(close)
                                .rotation((open_t - 1.0) * 90.0),
                        ),
                    )
                    .into_any_element(),
            ]
        });

        // The actions float above the FAB without affecting layout.
        div()
            .relative()
            .flex_none()
            .size(px(56.0))
            .child(div().absolute().bottom(px(64.0)).right_0().child(column))
            .child(toggle)
    }
}
