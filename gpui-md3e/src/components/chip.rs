use std::rc::Rc;

use gpui::{
    App, ClickEvent, Corners, ElementId, IntoElement, ParentElement as _, RenderOnce, Role,
    SharedString, Styled as _, Window, div, px,
};

use super::pressable::{ClickHandler, GroupOverrides, Pressable, SurfaceColors};
use crate::{
    ActiveTheme as _, Icon, StyledExt as _, child_id, icon,
    motion::{MotionToken, animate},
    theme::TypeRole,
};

/// The four chip types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ChipKind {
    /// A smart suggestion of an action.
    #[default]
    Assist,
    /// A toggle that shows a check mark when selected.
    Filter,
    /// A piece of user input, with a trailing remove icon.
    Input,
    /// A dynamically generated suggestion.
    Suggestion,
}

/// A compact element for actions, filters and input.
#[derive(IntoElement)]
pub struct Chip {
    id: ElementId,
    label: SharedString,
    kind: ChipKind,
    icon: Option<Icon>,
    selected: bool,
    elevated: bool,
    disabled: bool,
    on_click: Option<ClickHandler>,
}

impl Chip {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind: ChipKind::Assist,
            icon: None,
            selected: false,
            elevated: false,
            disabled: false,
            on_click: None,
        }
    }

    pub fn kind(mut self, kind: ChipKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// The selected state of a filter or input chip.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn elevated(mut self, elevated: bool) -> Self {
        self.elevated = elevated;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
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

impl RenderOnce for Chip {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.md();
        let c = theme.colors;
        let label_style = theme.typography.style(TypeRole::LabelLarge);
        let radius = Corners::all(px(8.0));
        let selectable = matches!(self.kind, ChipKind::Filter | ChipKind::Input);
        let selected = selectable && self.selected;
        let id = self.id.clone();

        let content = match self.kind {
            ChipKind::Assist => c.on_surface,
            _ => c.on_surface_variant,
        };
        let unselected = if self.elevated {
            SurfaceColors::new(c.surface_container_low, content)
        } else {
            SurfaceColors::outlined(content, c.outline_variant)
        };
        let selected_colors = SurfaceColors::new(c.secondary_container, c.on_secondary_container);
        let icon_color = match self.kind {
            ChipKind::Assist | ChipKind::Suggestion => c.primary,
            _ => content,
        };

        // A filter chip's check mark slides in, widening the chip.
        let check = animate(
            child_id(&id, "check"),
            if self.kind == ChipKind::Filter && selected {
                1.0
            } else {
                0.0
            },
            MotionToken::FastSpatial,
            window,
            cx,
        );
        let leading = self.icon;
        let label = self.label.clone();
        let kind = self.kind;
        let has_leading = leading.is_some() || kind == ChipKind::Filter;

        Pressable {
            id: self.id,
            label: self.label,
            role: if selectable {
                Role::CheckBox
            } else {
                Role::Button
            },
            height: px(32.0),
            width: None,
            padding_x: px(0.0),
            gap: px(0.0),
            rest_radii: radius,
            selected_radii: radius,
            pressed_radii: radius,
            colors: unselected,
            selected_colors: selectable.then_some(selected_colors),
            selected: selectable.then_some(selected),
            elevation: if self.elevated {
                (1.0, 2.0)
            } else {
                (0.0, 1.0)
            },
            outline_width: px(1.0),
            disabled: self.disabled,
            on_click: self.on_click,
            group: GroupOverrides::default(),
        }
        .render(window, cx, move |state, _, cx| {
            let check_t = check.clamp(0.0, 1.2);
            let leading_pad = match kind {
                ChipKind::Filter => 16.0 - 8.0 * check_t.min(1.0),
                _ if has_leading => 8.0,
                _ => 16.0,
            };
            let trailing_pad = if kind == ChipKind::Input { 8.0 } else { 16.0 };
            let mut row = div()
                .flex()
                .flex_row()
                .items_center()
                .pl(px(leading_pad))
                .pr(px(trailing_pad));
            if kind == ChipKind::Filter {
                // 18px check plus 8px spacing, revealed as the chip widens.
                row = row.child(
                    div()
                        .flex_none()
                        .w(px(26.0 * check_t))
                        .overflow_hidden()
                        .child(icon(Icon::Check).size(px(18.0)).color(state.content)),
                );
            } else if let Some(glyph) = leading {
                let color = if state.selected {
                    state.content
                } else {
                    icon_color
                };
                row = row.child(
                    div()
                        .mr(px(8.0))
                        .child(icon(glyph).size(px(18.0)).color(color)),
                );
            }
            row = row.child(div().md_type_style(label_style, cx).child(label));
            if kind == ChipKind::Input {
                row = row.child(
                    div()
                        .ml(px(8.0))
                        .child(icon(Icon::Close).size(px(18.0)).color(state.content)),
                );
            }
            vec![row.into_any_element()]
        })
    }
}
