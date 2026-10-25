use gpui::{
    AnyElement, App, Corners, ElementId, IntoElement, ParentElement as _, Pixels, RenderOnce,
    Styled as _, Window, div, px,
};

use super::{Button, IconButton};
use crate::{
    ActiveTheme as _, child_id,
    interaction::Interaction,
    motion::{MotionToken, animate},
    theme::Corner,
};

/// The width a pressed button borrows from its neighbors, as a fraction of
/// its own width.
const EXPANDED_RATIO: f32 = 0.15;

/// A button or icon button inside a [`ButtonGroup`].
pub enum GroupItem {
    Button(Button),
    IconButton(IconButton),
}

impl From<Button> for GroupItem {
    fn from(button: Button) -> Self {
        GroupItem::Button(button)
    }
}

impl From<IconButton> for GroupItem {
    fn from(button: IconButton) -> Self {
        GroupItem::IconButton(button)
    }
}

impl GroupItem {
    fn id(&self) -> &ElementId {
        match self {
            GroupItem::Button(button) => button.id(),
            GroupItem::IconButton(button) => button.id(),
        }
    }

    fn natural_metrics(&self, window: &Window, cx: &App) -> (Pixels, Pixels, Pixels) {
        match self {
            GroupItem::Button(button) => button.natural_metrics(window, cx),
            GroupItem::IconButton(button) => button.natural_metrics(),
        }
    }

    fn is_selected(&self) -> bool {
        match self {
            GroupItem::Button(button) => button.is_selected(),
            GroupItem::IconButton(button) => button.is_selected(),
        }
    }

    fn overrides(&mut self) -> &mut super::pressable::GroupOverrides {
        match self {
            GroupItem::Button(button) => &mut button.group,
            GroupItem::IconButton(button) => &mut button.group,
        }
    }

    fn into_any(self) -> AnyElement {
        match self {
            GroupItem::Button(button) => button.into_any_element(),
            GroupItem::IconButton(button) => button.into_any_element(),
        }
    }
}

/// A row of buttons that react to each other.
///
/// In a standard group, the pressed button grows and squeezes its neighbors.
/// In a connected group the buttons sit 2px apart with small inner corners
/// that round out fully when a button is selected.
#[derive(IntoElement)]
pub struct ButtonGroup {
    id: ElementId,
    items: Vec<GroupItem>,
    connected: bool,
    fill: bool,
}

impl ButtonGroup {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            items: Vec::new(),
            connected: false,
            fill: false,
        }
    }

    /// Uses the connected layout, typically for single or multi selection.
    pub fn connected(mut self, connected: bool) -> Self {
        self.connected = connected;
        self
    }

    /// Stretches the buttons to share the available width equally.
    pub fn fill(mut self, fill: bool) -> Self {
        self.fill = fill;
        self
    }

    pub fn child(mut self, item: impl Into<GroupItem>) -> Self {
        self.items.push(item.into());
        self
    }

    pub fn children(mut self, items: impl IntoIterator<Item = impl Into<GroupItem>>) -> Self {
        self.items.extend(items.into_iter().map(Into::into));
        self
    }
}

impl RenderOnce for ButtonGroup {
    fn render(mut self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let count = self.items.len();
        let shapes = cx.md().shapes;

        // Interaction state lives here so the group can see every press.
        let interactions: Vec<_> = self
            .items
            .iter()
            .map(|item| {
                let key = child_id(&self.id, "item");
                Interaction::use_state(&child_id_of(&key, item.id()), window, cx)
            })
            .collect();
        let metrics: Vec<_> = self
            .items
            .iter()
            .map(|item| item.natural_metrics(window, cx))
            .collect();
        let pressed: Vec<f32> = interactions
            .iter()
            .enumerate()
            .map(|(ix, interaction)| {
                let pressed = interaction.read(cx).pressed;
                animate(
                    child_id(&self.id, format!("press-{ix}")),
                    if pressed { 1.0 } else { 0.0 },
                    MotionToken::FastSpatial,
                    window,
                    cx,
                )
            })
            .collect();

        let mut extra = vec![px(0.0); count];
        if !self.connected && count > 1 {
            for ix in 0..count {
                let t = pressed[ix];
                if t.abs() < 0.0001 {
                    continue;
                }
                let (width, _, _) = metrics[ix];
                let padding = |j: usize| metrics[j].2;
                if ix > 0 && ix + 1 < count {
                    let growth = (width * (EXPANDED_RATIO / 2.0))
                        .min(padding(ix - 1))
                        .min(padding(ix + 1))
                        * t;
                    extra[ix - 1] -= growth;
                    extra[ix + 1] -= growth;
                    extra[ix] += growth * 2.0;
                } else {
                    let neighbor = if ix == 0 { 1 } else { ix - 1 };
                    let growth = (width * EXPANDED_RATIO).min(padding(neighbor)) * t;
                    extra[neighbor] -= growth;
                    extra[ix] += growth;
                }
            }
        }

        let mut row = div().flex().flex_row().items_center();
        row = if self.connected {
            row.gap(px(2.0))
        } else {
            row.gap(px(12.0))
        };
        if self.fill {
            row = row.w_full();
        }

        let items = std::mem::take(&mut self.items);
        for (ix, (mut item, interaction)) in items.into_iter().zip(interactions).enumerate() {
            let (_, height, _) = metrics[ix];
            let selected = item.is_selected();
            let overrides = item.overrides();
            overrides.interaction = Some(interaction);
            overrides.extra_width = extra[ix];
            overrides.fill_width = self.fill;
            if self.connected {
                let full = height / 2.0;
                let inner = shapes.radius(Corner::Small, height);
                let inner_pressed = shapes.radius(Corner::ExtraSmall, height);
                let selected_t = animate(
                    child_id(&self.id, format!("selected-{ix}")),
                    if selected { 1.0 } else { 0.0 },
                    MotionToken::FastSpatial,
                    window,
                    cx,
                );
                let base = inner + (inner_pressed - inner) * pressed[ix];
                let inner_radius = base + (full - base) * selected_t;
                let first = ix == 0;
                let last = ix + 1 == count;
                overrides.radii = Some(Corners {
                    top_left: if first { full } else { inner_radius },
                    bottom_left: if first { full } else { inner_radius },
                    top_right: if last { full } else { inner_radius },
                    bottom_right: if last { full } else { inner_radius },
                });
            }
            row = row.child(item.into_any());
        }
        row
    }
}

fn child_id_of(parent: &ElementId, id: &ElementId) -> ElementId {
    child_id(parent, id.to_string())
}
