//! Read-only text that can be selected with the mouse, copied and dragged
//! into a [`TextField`](super::TextField).
//!
//! There is one selection outside text fields at a time, like on a web
//! page: pressing on another [`SelectableText`] or anywhere else replaces
//! or clears it. [`CopySelection`] (Ctrl+C, ⌘C) copies it, and so does a
//! focused text field's copy when the field has nothing selected itself.

use std::ops::Range;

use gpui::{
    App, AppContext as _, Bounds, ClipboardItem, Context, CursorStyle, DispatchPhase, Element,
    ElementId, Entity, EntityId, ExternalDragPayload, Global, GlobalElementId, Hitbox,
    HitboxBehavior, InteractiveElement as _, IntoElement, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point, Render, RenderOnce,
    SharedString, StatefulInteractiveElement as _, Styled as _, StyledText, TextLayout, Window,
    actions, div, fill, point, px, size,
};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::{ActiveTheme as _, StyledExt as _, child_id, theme::TypeRole};

actions!(md3e, [CopySelection]);

/// Text being dragged. Text fields accept it as a drop.
#[derive(Clone, Debug)]
pub struct DraggedText {
    pub text: SharedString,
    /// The text field and range it came from, so a drop back into the same
    /// field moves the text instead of copying it.
    pub(crate) source: Option<(EntityId, Range<usize>)>,
}

impl DraggedText {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            source: None,
        }
    }
}

/// What follows the pointer while text is dragged.
pub struct DragPreview {
    text: SharedString,
}

impl Render for DragPreview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = cx.md().colors;
        let mut text: String = self.text.chars().take(40).collect();
        if text.len() < self.text.len() {
            text.push('…');
        }
        div()
            .max_w(px(320.0))
            .px_3()
            .py_1()
            .rounded(px(8.0))
            .bg(colors.inverse_surface)
            .text_color(colors.inverse_on_surface)
            .md_type(TypeRole::BodyMedium, cx)
            .md_elevation(2.0, colors.shadow)
            .truncate()
            .child(text.replace('\n', " "))
    }
}

/// What a text drag hands to other applications when it leaves the window.
pub(crate) fn external_payload(
    dragged: &DraggedText,
    _: &mut Window,
    _: &mut App,
) -> Option<ExternalDragPayload> {
    Some(ExternalDragPayload::Text(dragged.text.clone()))
}

/// The view a drag of `text` shows.
pub(crate) fn drag_preview(text: SharedString, cx: &mut App) -> Entity<DragPreview> {
    cx.new(|_| DragPreview { text })
}

/// The app's selection outside text fields.
#[derive(Default)]
struct TextSelection {
    owner: Option<ElementId>,
    text: SharedString,
    range: Range<usize>,
    anchor: usize,
    /// The mouse button is down and the selection follows it.
    selecting: bool,
    /// The selection's rectangles relative to the text, from the last paint;
    /// drags start on them.
    rects: Vec<Bounds<Pixels>>,
}

impl Global for TextSelection {}

/// The selected text outside text fields, if any.
pub fn selected_text(cx: &App) -> Option<SharedString> {
    let selection = cx.try_global::<TextSelection>()?;
    (selection.owner.is_some() && !selection.range.is_empty())
        .then(|| selection.text[selection.range.clone()].to_string().into())
}

/// Drops the selection outside text fields.
pub fn clear_selection(cx: &mut App) {
    if cx
        .try_global::<TextSelection>()
        .is_some_and(|selection| selection.owner.is_some())
    {
        cx.set_global(TextSelection::default());
    }
}

/// Copies the selection outside text fields; returns whether there was one.
pub(crate) fn copy_selection(cx: &mut App) -> bool {
    match selected_text(cx) {
        Some(text) => {
            cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
            true
        }
        None => false,
    }
}

pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        #[cfg(target_os = "macos")]
        gpui::KeyBinding::new("cmd-c", CopySelection, None),
        #[cfg(not(target_os = "macos"))]
        gpui::KeyBinding::new("ctrl-c", CopySelection, None),
    ]);
    cx.on_action(|_: &CopySelection, cx| {
        copy_selection(cx);
    });
}

/// Text the reader can select, copy and drag. Its `id` must be unique in
/// the window; the text takes the surrounding text style and wraps.
#[derive(IntoElement)]
pub struct SelectableText {
    id: ElementId,
    text: SharedString,
}

impl SelectableText {
    pub fn new(id: impl Into<ElementId>, text: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
        }
    }
}

impl RenderOnce for SelectableText {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        // Handles over the current selection start a drag; a press elsewhere
        // on the text starts a new selection.
        let mut handles = Vec::new();
        if let Some(selection) = cx.try_global::<TextSelection>()
            && selection.owner.as_ref() == Some(&self.id)
            && !selection.range.is_empty()
            && !selection.selecting
            && selection.text == self.text
        {
            let dragged = DraggedText::new(selection.text[selection.range.clone()].to_string());
            for (ix, rect) in selection.rects.iter().enumerate() {
                handles.push(
                    div()
                        .id(child_id(&self.id, format!("drag-{ix}")))
                        .absolute()
                        .left(rect.origin.x)
                        .top(rect.origin.y)
                        .w(rect.size.width)
                        .h(rect.size.height)
                        .cursor(CursorStyle::Arrow)
                        .on_drag(dragged.clone(), |dragged, _, _, cx| {
                            drag_preview(dragged.text.clone(), cx)
                        })
                        .external_drag_payload(external_payload)
                        // A click without a drag drops the selection.
                        .on_click(|_, window, cx| {
                            clear_selection(cx);
                            window.refresh();
                        }),
                );
            }
        }
        div()
            .relative()
            .child(SelectableTextElement {
                id: self.id,
                text: self.text.clone(),
                styled: StyledText::new(self.text),
            })
            .children(handles)
    }
}

struct SelectableTextElement {
    id: ElementId,
    text: SharedString,
    styled: StyledText,
}

impl IntoElement for SelectableTextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SelectableTextElement {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.styled.request_layout(None, None, window, cx)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.styled.prepaint(None, None, bounds, state, window, cx);
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let layout = self.styled.layout().clone();

        // The selection, behind the text.
        let selection = cx
            .try_global::<TextSelection>()
            .filter(|selection| selection.owner.as_ref() == Some(&self.id))
            .map(|selection| selection.range.clone())
            .filter(|range| !range.is_empty() && range.end <= self.text.len());
        let mut relative = Vec::new();
        if let Some(range) = selection {
            let color = cx.md().colors.primary.opacity(0.32);
            for rect in selection_rects(&layout, range) {
                window.paint_quad(fill(rect, color));
                relative.push(Bounds::new(rect.origin - bounds.origin, rect.size));
            }
        }
        let stale = cx.try_global::<TextSelection>().is_some_and(|global| {
            global.owner.as_ref() == Some(&self.id) && global.rects != relative
        });
        if stale {
            cx.global_mut::<TextSelection>().rects = relative;
            // The drag handles are laid out from the rectangles.
            window.refresh();
        }

        self.styled
            .paint(None, None, bounds, state, &mut (), window, cx);
        window.set_cursor_style(CursorStyle::IBeam, hitbox);

        let id = self.id.clone();
        let text = self.text.clone();
        let hitbox = hitbox.clone();
        {
            let layout = layout.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                    return;
                }
                let current = cx.default_global::<TextSelection>();
                let owned = current.owner.as_ref() == Some(&id);
                if !hitbox.is_hovered(window) {
                    // A press anywhere else drops this text's selection.
                    if owned && !current.selecting {
                        clear_selection(cx);
                        window.refresh();
                    }
                    return;
                }
                let on_selection = owned
                    && !current.range.is_empty()
                    && current
                        .rects
                        .iter()
                        .any(|rect| rect.contains(&(event.position - hitbox.bounds.origin)));
                if on_selection && event.click_count == 1 && !event.modifiers.shift {
                    // The drag handle on top takes this press.
                    return;
                }
                let ix = index_at(&layout, event.position);
                let (anchor, range) = match event.click_count {
                    2 => {
                        let word = word_at(&text, ix);
                        (word.start, word)
                    }
                    n if n >= 3 => (0, 0..text.len()),
                    _ if event.modifiers.shift && owned => {
                        let anchor = current.anchor;
                        (anchor, anchor.min(ix)..anchor.max(ix))
                    }
                    _ => (ix, ix..ix),
                };
                cx.set_global(TextSelection {
                    owner: Some(id.clone()),
                    text: text.clone(),
                    range,
                    anchor,
                    selecting: event.click_count == 1,
                    rects: Vec::new(),
                });
                window.refresh();
            });
        }
        {
            let id = self.id.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble {
                    return;
                }
                let Some(selection) = cx.try_global::<TextSelection>() else {
                    return;
                };
                if selection.owner.as_ref() != Some(&id) || !selection.selecting {
                    return;
                }
                if event.pressed_button != Some(MouseButton::Left) {
                    cx.global_mut::<TextSelection>().selecting = false;
                    return;
                }
                let ix = index_at(&layout, event.position);
                let anchor = selection.anchor;
                let range = anchor.min(ix)..anchor.max(ix);
                if range != selection.range {
                    cx.global_mut::<TextSelection>().range = range;
                    window.refresh();
                }
            });
        }
        let id = self.id.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble || event.button != MouseButton::Left {
                return;
            }
            if let Some(selection) = cx.try_global::<TextSelection>()
                && selection.owner.as_ref() == Some(&id)
                && selection.selecting
            {
                cx.global_mut::<TextSelection>().selecting = false;
                window.refresh();
            }
        });
    }
}

/// The text index nearest to `position`, above the text meaning its start
/// and below it its end.
fn index_at(layout: &TextLayout, position: Point<Pixels>) -> usize {
    let bounds = layout.bounds();
    if position.y < bounds.top() {
        return 0;
    }
    if position.y > bounds.bottom() {
        return layout.len();
    }
    layout
        .index_for_position(position)
        .unwrap_or_else(|ix| ix)
        .min(layout.len())
}

/// The word (or run of punctuation or space) around byte `ix`.
fn word_at(text: &str, ix: usize) -> Range<usize> {
    text.split_word_bound_indices()
        .map(|(start, word)| start..start + word.len())
        .find(|range| range.contains(&ix))
        .or_else(|| {
            text.split_word_bound_indices()
                .next_back()
                .map(|(start, word)| start..start + word.len())
        })
        .unwrap_or(0..0)
}

/// The rectangles covering `range` on screen, one per visual line.
fn selection_rects(layout: &TextLayout, range: Range<usize>) -> Vec<Bounds<Pixels>> {
    let (Some(start), Some(end)) = (
        layout.position_for_index(range.start),
        layout.position_for_index(range.end),
    ) else {
        return Vec::new();
    };
    let bounds = layout.bounds();
    let line_height = layout.line_height();
    let row = |y: Pixels, from: Pixels, to: Pixels| {
        Bounds::new(point(from, y), size((to - from).max(px(0.0)), line_height))
    };
    if (start.y - end.y).abs() < px(0.5) {
        return vec![row(start.y, start.x, end.x)];
    }
    let mut rects = vec![row(start.y, start.x, bounds.right())];
    let mut y = start.y + line_height;
    while y + px(0.5) < end.y {
        rects.push(row(y, bounds.left(), bounds.right()));
        y += line_height;
    }
    rects.push(row(end.y, bounds.left(), end.x));
    rects.retain(|rect| rect.size.width > px(0.0));
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn double_clicks_pick_the_word() {
        let text = "hello wide world";
        assert_eq!(word_at(text, 1), 0..5);
        assert_eq!(word_at(text, 7), 6..10);
        assert_eq!(word_at(text, 16), 11..16);
        assert_eq!(&"今日は晴れ"[word_at("今日は晴れ", 0)], "今");
    }
}
