//! A single-line Material text field with IME support.
//!
//! Text editing follows GPUI's `input` example: the field implements
//! [`EntityInputHandler`], so composition from fcitx5, IBus, macOS and
//! Windows input methods flows through the platform's text input protocol.

use std::{ops::Range, rc::Rc, time::Duration};

use gpui::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, FocusHandle, Focusable, GlobalElementId, Hsla, InteractiveElement,
    IntoElement, KeyBinding, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement as _, Pixels, Point, Render, ShapedLine, SharedString, Style, Styled as _, Task,
    TextRun, UTF16Selection, UnderlineStyle, Window, actions, div, fill, point, px, relative, size,
};
use unicode_segmentation::UnicodeSegmentation as _;

use crate::{
    ActiveTheme as _, Icon, StyledExt as _, icon,
    motion::{MotionToken, animate},
    theme::{TypeRole, TypeStyle, lerp_color},
};

actions!(
    md3e_text_field,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
        Submit
    ]
);

const KEY_CONTEXT: &str = "Md3eTextField";
const BLINK: Duration = Duration::from_millis(530);
/// What a masked field shows for each character.
const MASK: char = '\u{2022}';

/// Installs the editing key bindings. Called by [`crate::init`].
pub(crate) fn bind_keys(cx: &mut App) {
    let ctx = Some(KEY_CONTEXT);
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, ctx),
        KeyBinding::new("delete", Delete, ctx),
        KeyBinding::new("left", Left, ctx),
        KeyBinding::new("right", Right, ctx),
        KeyBinding::new("shift-left", SelectLeft, ctx),
        KeyBinding::new("shift-right", SelectRight, ctx),
        KeyBinding::new("home", Home, ctx),
        KeyBinding::new("end", End, ctx),
        KeyBinding::new("enter", Submit, ctx),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-a", SelectAll, ctx),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-v", Paste, ctx),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-c", Copy, ctx),
        #[cfg(target_os = "macos")]
        KeyBinding::new("cmd-x", Cut, ctx),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-a", SelectAll, ctx),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-v", Paste, ctx),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-c", Copy, ctx),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-x", Cut, ctx),
    ]);
}

/// Filled fields sit on a tinted container with an underline; outlined
/// fields draw a border.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TextFieldStyle {
    #[default]
    Filled,
    Outlined,
}

/// Returns whether the field should clear itself.
type SubmitHandler = Rc<dyn Fn(&SharedString, &mut Window, &mut App) -> bool>;

/// A text field view. Create it with `cx.new(|cx| TextField::new(cx))` and
/// render the entity.
pub struct TextField {
    focus_handle: FocusHandle,
    content: SharedString,
    label: Option<SharedString>,
    placeholder: Option<SharedString>,
    supporting: Option<SharedString>,
    error: Option<SharedString>,
    leading: Option<Icon>,
    trailing: Option<Icon>,
    style: TextFieldStyle,
    /// Shows a bullet per character (passwords).
    masked: bool,
    /// How far the line is scrolled left to keep the caret in view.
    scroll_x: Pixels,
    /// The color behind an outlined field, used to cut the label's notch.
    surface: Option<Hsla>,
    on_submit: Option<SubmitHandler>,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
    cursor_visible: bool,
    /// Whether the field was focused at the last render; the caret only
    /// blinks (and re-renders) while it is.
    was_focused: bool,
    _blink: Task<()>,
}

impl TextField {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let blink = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(BLINK).await;
                let Ok(()) = this.update(cx, |this, cx| {
                    if this.was_focused {
                        this.cursor_visible = !this.cursor_visible;
                        cx.notify();
                    }
                }) else {
                    break;
                };
            }
        });
        Self {
            focus_handle: cx.focus_handle().tab_stop(true),
            content: SharedString::default(),
            label: None,
            placeholder: None,
            supporting: None,
            error: None,
            leading: None,
            trailing: None,
            style: TextFieldStyle::Filled,
            masked: false,
            scroll_x: px(0.0),
            surface: None,
            on_submit: None,
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
            cursor_visible: true,
            was_focused: false,
            _blink: blink,
        }
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    pub fn supporting_text(mut self, text: impl Into<SharedString>) -> Self {
        self.supporting = Some(text.into());
        self
    }

    pub fn leading_icon(mut self, icon: Icon) -> Self {
        self.leading = Some(icon);
        self
    }

    pub fn trailing_icon(mut self, icon: Icon) -> Self {
        self.trailing = Some(icon);
        self
    }

    pub fn style(mut self, style: TextFieldStyle) -> Self {
        self.style = style;
        self
    }

    /// Hides the text behind bullets, for passwords. Copying is disabled.
    pub fn masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }

    /// The background behind an outlined field.
    pub fn surface(mut self, color: Hsla) -> Self {
        self.surface = Some(color);
        self
    }

    /// Called with the text when Enter is pressed; return `true` to clear
    /// the field.
    pub fn on_submit(
        mut self,
        handler: impl Fn(&SharedString, &mut Window, &mut App) -> bool + 'static,
    ) -> Self {
        self.on_submit = Some(Rc::new(handler));
        self
    }

    pub fn text(&self) -> &SharedString {
        &self.content
    }

    pub fn set_text(
        &mut self,
        text: impl Into<SharedString>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        self.content = text.into();
        self.selected_range = self.content.len()..self.content.len();
        self.marked_range = None;
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    /// Changes the placeholder of a field already on screen.
    pub fn set_placeholder(
        &mut self,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.placeholder = Some(placeholder.into());
        cx.notify();
    }

    /// Shows an error message in place of the supporting text.
    pub fn set_error(&mut self, error: Option<SharedString>, cx: &mut Context<Self>) {
        self.error = error;
        cx.notify();
    }

    fn left(&mut self, _: &Left, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), window, cx);
        } else {
            self.move_to(self.selected_range.start, window, cx)
        }
    }

    fn right(&mut self, _: &Right, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), window, cx);
        } else {
            self.move_to(self.selected_range.end, window, cx)
        }
    }

    fn select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), window, cx);
    }

    fn select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), window, cx);
    }

    fn select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, window, cx);
        self.select_to(self.content.len(), window, cx)
    }

    fn home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, window, cx);
    }

    fn end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), window, cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let prev = self.previous_boundary(self.cursor_offset());
            if self.cursor_offset() == prev {
                return;
            }
            self.select_to(prev, window, cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            let next = self.next_boundary(self.cursor_offset());
            if self.cursor_offset() == next {
                return;
            }
            self.select_to(next, window, cx)
        }
        self.replace_text_in_range(None, "", window, cx)
    }

    fn submit(&mut self, _: &Submit, window: &mut Window, cx: &mut Context<Self>) {
        // Enter confirms an IME composition before it submits anything.
        if self.marked_range.is_some() {
            return;
        }
        if let Some(handler) = self.on_submit.clone() {
            let text = self.content.clone();
            if handler(&text, window, cx) {
                self.set_text("", window, cx);
            }
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_selecting = true;
        window.focus(&self.focus_handle, cx);
        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), window, cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), window, cx)
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), window, cx);
        }
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text.replace('\n', " "), window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() && !self.masked {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() && !self.masked {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx)
        }
    }

    fn move_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        self.cursor_visible = true;
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify()
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        self.content_offset(line.closest_index_for_x(position.x - bounds.left() + self.scroll_x))
    }

    fn select_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset
        } else {
            self.selected_range.end = offset
        };
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        self.cursor_visible = true;
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify()
    }

    /// The text as painted: the content, or a bullet per character.
    fn display_text(&self) -> SharedString {
        if self.masked {
            MASK.to_string().repeat(self.content.chars().count()).into()
        } else {
            self.content.clone()
        }
    }

    /// Where a content offset falls in the painted text.
    fn display_offset(&self, offset: usize) -> usize {
        if self.masked {
            self.content[..offset].chars().count() * MASK.len_utf8()
        } else {
            offset
        }
    }

    /// The content offset of an offset in the painted text.
    fn content_offset(&self, display: usize) -> usize {
        if self.masked {
            self.content
                .char_indices()
                .nth(display / MASK.len_utf8())
                .map_or(self.content.len(), |(ix, _)| ix)
        } else {
            display
        }
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }
}

impl EntityInputHandler for TextField {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_range = self.range_from_utf16(&range_utf16);
        self.selection_reversed = false;
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(self.content.encode_utf16().count())
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.selection_reversed = false;
        self.marked_range.take();
        self.cursor_visible = true;
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.marked_range =
            (!new_text.is_empty()).then(|| range.start..range.start + new_text.len());
        let insertion_start_utf16 = self.offset_to_utf16(range.start);
        let inserted_length_utf16 = new_text.encode_utf16().count();
        self.selected_range = new_selected_range_utf16
            .map(|selected| {
                self.range_from_utf16(
                    &(insertion_start_utf16 + selected.start.min(inserted_length_utf16)
                        ..insertion_start_utf16 + selected.end.min(inserted_length_utf16)),
                )
            })
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.selection_reversed = false;
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let last_layout = self.last_layout.as_ref()?;
        if last_layout.text != self.display_text() {
            return None;
        }
        let range = self.range_from_utf16(&range_utf16);
        let left = bounds.left() - self.scroll_x;
        Some(Bounds::from_corners(
            point(
                left + last_layout.x_for_index(self.display_offset(range.start)),
                bounds.top(),
            ),
            point(
                left + last_layout.x_for_index(self.display_offset(range.end)),
                bounds.bottom(),
            ),
        ))
    }

    fn selection_bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Bounds<Pixels>> {
        self.bounds_for_range(range_utf16, bounds, window, cx)
            .into_iter()
            .collect()
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let line_point = self.last_bounds?.localize(&point)?;
        let last_layout = self.last_layout.as_ref()?;
        if last_layout.text != self.display_text() {
            return None;
        }
        let index = last_layout.closest_index_for_x(line_point.x + self.scroll_x);
        Some(self.offset_to_utf16(self.content_offset(index)))
    }
}

impl Focusable for TextField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

/// Colors and metrics the editable line needs to paint.
#[derive(Clone, Copy)]
struct LineStyle {
    text: TypeStyle,
    color: Hsla,
    placeholder: Hsla,
    caret: Hsla,
    selection: Hsla,
    show_placeholder: bool,
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.md();
        let c = theme.colors;
        let typography = theme.typography.clone();
        let focused = self.focus_handle.is_focused(window);
        if focused != self.was_focused {
            self.was_focused = focused;
            self.cursor_visible = true;
        }
        let has_text = !self.content.is_empty() || self.marked_range.is_some();
        let error = self.error.is_some();
        let id = ElementId::View(cx.entity_id());

        let float_t = animate(
            crate::child_id(&id, "float"),
            if focused || has_text || self.label.is_none() {
                1.0
            } else {
                0.0
            },
            MotionToken::FastSpatial,
            window,
            cx,
        );
        let focus_t = animate(
            crate::child_id(&id, "focus"),
            if focused { 1.0 } else { 0.0 },
            MotionToken::DefaultEffects,
            window,
            cx,
        )
        .clamp(0.0, 1.0);

        let accent = if error { c.error } else { c.primary };
        let idle_line = if error { c.error } else { c.on_surface_variant };
        let line_color = lerp_color(idle_line, accent, focus_t);
        let label_color = lerp_color(
            if error { c.error } else { c.on_surface_variant },
            accent,
            focus_t,
        );
        let body = typography.style(TypeRole::BodyLarge);
        let small = typography.style(TypeRole::BodySmall);
        let float = float_t.clamp(0.0, 1.05);
        let label_style = TypeStyle {
            size: body.size + (small.size - body.size) * float,
            line_height: body.line_height + (small.line_height - body.line_height) * float,
            ..body
        };

        let leading_pad = if self.leading.is_some() { 52.0 } else { 16.0 };
        let filled = self.style == TextFieldStyle::Filled;
        // Resting labels sit on the text baseline; floating labels move to
        // the top of a filled container or onto an outlined border.
        let label_top = if filled {
            18.0 + (8.0 - 18.0) * float
        } else {
            18.0 + (-8.0 - 18.0) * float
        };
        let text_top = if filled && self.label.is_some() {
            24.0
        } else {
            16.0
        };
        let surface = self.surface.unwrap_or(c.surface);

        let line_style = LineStyle {
            text: body,
            color: c.on_surface,
            placeholder: c.on_surface_variant,
            caret: accent,
            selection: c.primary.opacity(0.3),
            show_placeholder: float_t > 0.9,
        };

        let mut field = div()
            .id("text-field")
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::submit))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .relative()
            .w_full()
            .h(px(56.0));

        if filled {
            field = field
                .rounded_t(px(4.0))
                .bg(c.surface_container_highest)
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .h(px(1.0 + focus_t))
                        .bg(line_color),
                );
        } else {
            let width = 1.0 + focus_t;
            field = field.child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .rounded(px(4.0))
                    .border(px(width))
                    .border_color(if focused || error {
                        line_color
                    } else {
                        c.outline
                    }),
            );
        }

        if let Some(glyph) = self.leading {
            field = field.child(
                div()
                    .absolute()
                    .left(px(12.0))
                    .top(px(16.0))
                    .child(icon(glyph).color(c.on_surface_variant)),
            );
        }
        if let Some(glyph) = self.trailing {
            field =
                field.child(
                    div().absolute().right(px(12.0)).top(px(16.0)).child(
                        icon(glyph).color(if error { c.error } else { c.on_surface_variant }),
                    ),
                );
        }
        if let Some(label) = self.label.clone() {
            let mut label_el = div()
                .absolute()
                .left(px(if filled || float < 0.5 {
                    leading_pad
                } else {
                    12.0
                }))
                .top(px(label_top))
                .px(px(if filled { 0.0 } else { 4.0 * float.min(1.0) }))
                .md_type_style(label_style, cx)
                .text_color(label_color)
                .whitespace_nowrap();
            if !filled && float > 0.01 {
                // Cut the border behind the floating label.
                label_el = label_el.bg(surface);
            }
            field = field.child(label_el.child(label));
        }
        let trailing_pad = if self.trailing.is_some() { 52.0 } else { 16.0 };
        field = field.child(
            div()
                .absolute()
                .left(px(leading_pad))
                .right(px(trailing_pad))
                .top(px(text_top))
                .h(body.line_height)
                .overflow_hidden()
                .md_type_style(body, cx)
                .child(TextLine {
                    field: cx.entity(),
                    style: line_style,
                    focused,
                }),
        );

        let supporting = self.error.clone().or(self.supporting.clone());
        let mut root = div().flex().flex_col().gap_1().w_full().child(field);
        if let Some(text) = supporting {
            root = root.child(
                div()
                    .px_4()
                    .md_type(TypeRole::BodySmall, cx)
                    .text_color(if error { c.error } else { c.on_surface_variant })
                    .child(text),
            );
        }
        root
    }
}

/// The editable text line, painted directly so the caret, selection and IME
/// composition can be drawn and reported to the platform.
struct TextLine {
    field: Entity<TextField>,
    style: LineStyle,
    focused: bool,
}

struct LinePrepaint {
    line: Option<ShapedLine>,
    caret: Option<gpui::PaintQuad>,
    selection: Option<gpui::PaintQuad>,
    scroll_x: Pixels,
}

impl IntoElement for TextLine {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextLine {
    type RequestLayoutState = ();
    type PrepaintState = LinePrepaint;

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
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = self.style.text.line_height.into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let field = self.field.read(cx);
        let content = field.display_text();
        let selected_range = field.display_offset(field.selected_range.start)
            ..field.display_offset(field.selected_range.end);
        let cursor = field.display_offset(field.cursor_offset());
        let marked_range = field
            .marked_range
            .as_ref()
            .map(|marked| field.display_offset(marked.start)..field.display_offset(marked.end));
        let font = cx.md().typography.font(self.style.text);

        let (display, color) = match (&field.placeholder, content.is_empty()) {
            (Some(placeholder), true) if self.style.show_placeholder => {
                (placeholder.clone(), self.style.placeholder)
            }
            _ => (content, self.style.color),
        };
        let run = TextRun {
            len: display.len(),
            font,
            color,
            ..Default::default()
        };
        let runs = match marked_range.as_ref() {
            Some(marked) => [
                TextRun {
                    len: marked.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked.end - marked.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display.len() - marked.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect(),
            None => vec![run],
        };
        let line = window
            .text_system()
            .shape_line(display, self.style.text.size, &runs, None);

        // Scroll just enough to keep the caret in view, and no further than
        // the end of the text.
        let caret_width = px(2.0);
        let room = (bounds.size.width - caret_width).max(px(0.0));
        let caret_x = line.x_for_index(cursor);
        let mut scroll_x = field.scroll_x;
        if caret_x < scroll_x {
            scroll_x = caret_x;
        } else if caret_x > scroll_x + room {
            scroll_x = caret_x - room;
        }
        scroll_x = scroll_x
            .min((line.width() - room).max(px(0.0)))
            .max(px(0.0));
        let left = bounds.left() - scroll_x;

        let (selection, caret) = if selected_range.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(left + caret_x, bounds.top()),
                        size(caret_width, bounds.size.height),
                    ),
                    self.style.caret,
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(left + line.x_for_index(selected_range.start), bounds.top()),
                        point(left + line.x_for_index(selected_range.end), bounds.bottom()),
                    ),
                    self.style.selection,
                )),
                None,
            )
        };
        LinePrepaint {
            line: Some(line),
            caret,
            selection,
            scroll_x,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.field.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.field.clone()),
            cx,
        );
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        let Some(line) = prepaint.line.take() else {
            return;
        };
        let scroll_x = prepaint.scroll_x;
        let _ = line.paint(
            point(bounds.left() - scroll_x, bounds.top()),
            self.style.text.line_height,
            gpui::TextAlign::Left,
            None,
            window,
            cx,
        );
        let blink_on = self.field.read(cx).cursor_visible;
        if self.focused
            && blink_on
            && let Some(caret) = prepaint.caret.take()
        {
            window.paint_quad(caret);
        }
        self.field.update(cx, |field, _| {
            field.last_layout = Some(line);
            field.last_bounds = Some(bounds);
            field.scroll_x = scroll_x;
        });
    }
}
