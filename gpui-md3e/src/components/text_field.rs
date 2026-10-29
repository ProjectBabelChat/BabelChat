//! A Material text field with IME support: one line, or several
//! ([`TextField::multiline`]) that wrap and grow.
//!
//! Text editing follows GPUI's `input` example: the field implements
//! [`EntityInputHandler`], so composition from fcitx5, IBus, macOS and
//! Windows input methods flows through the platform's text input protocol.
//!
//! Runs of the text may show as pictures ([`TextField::inline_pictures`], a
//! chat's emoji codes): each stands on a placeholder, painted over, and the
//! caret moves past it and deletes it as one character.

use std::{ops::Range, rc::Rc, sync::Arc, time::Duration};

use gpui::{
    App, AvailableSpace, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, EventEmitter, ExternalText, FocusHandle,
    Focusable, Font, GlobalElementId, Hsla, Image, InteractiveElement, IntoElement, KeyBinding,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _,
    Pixels, Point, Render, ScrollEvent, SharedString, StatefulInteractiveElement as _, Style,
    Styled as _, Task, TextRun, UTF16Selection, UnderlineStyle, Window, WrappedLine, actions, div,
    fill, point, px, relative, size,
};
use unicode_segmentation::UnicodeSegmentation as _;

use super::{
    DraggedText,
    selectable_text::{IMAGE_PLACEHOLDER, paint_picture},
};
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
        Submit,
        Newline,
        Up,
        Down,
        SelectUp,
        SelectDown
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
        KeyBinding::new("shift-enter", Newline, ctx),
        KeyBinding::new("up", Up, ctx),
        KeyBinding::new("down", Down, ctx),
        KeyBinding::new("shift-up", SelectUp, ctx),
        KeyBinding::new("shift-down", SelectDown, ctx),
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

/// What a text field tells its subscribers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextFieldEvent {
    /// The user changed the text: typed, composed, pasted, cut or dropped
    /// (not [`TextField::set_text`]).
    Edited,
}

impl EventEmitter<TextFieldEvent> for TextField {}

/// A run of a field's text shown as a picture.
#[derive(Clone)]
pub struct InlinePicture {
    /// Where the run is in the text.
    pub range: Range<usize>,
    /// None while loading: the run takes its room and shows nothing.
    pub image: Option<Arc<Image>>,
}

/// Finds the runs of a text shown as pictures.
type FindPictures = Rc<dyn Fn(&str, &App) -> Vec<InlinePicture>>;

/// The field's text as shaped: one wrapped line per line of the text.
#[derive(Clone)]
struct Shaped {
    text: SharedString,
    /// Each line, with where it starts in `text`.
    lines: Rc<Vec<(usize, WrappedLine)>>,
    line_height: Pixels,
}

impl Shaped {
    fn new(
        text: SharedString,
        runs: &[TextRun],
        font_size: Pixels,
        line_height: Pixels,
        wrap_width: Option<Pixels>,
        window: &Window,
    ) -> Self {
        let mut start = 0;
        let lines = window
            .text_system()
            .shape_text(text.clone(), font_size, runs, wrap_width, None)
            .map(|lines| {
                lines
                    .into_iter()
                    .map(|line| {
                        let at = start;
                        start += line.len() + 1;
                        (at, line)
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self {
            text,
            lines: Rc::new(lines),
            line_height,
        }
    }

    fn line_rows(line: &WrappedLine) -> usize {
        line.wrap_boundaries().len() + 1
    }

    /// Rows on screen, wrapped ones included.
    fn rows(&self) -> usize {
        self.lines
            .iter()
            .map(|(_, line)| Self::line_rows(line))
            .sum::<usize>()
            .max(1)
    }

    fn width(&self) -> Pixels {
        self.lines
            .iter()
            .map(|(_, line)| line.width())
            .fold(px(0.0), Pixels::max)
    }

    /// Where the caret before `ix` goes, from the text's top left.
    fn position(&self, ix: usize) -> Point<Pixels> {
        let mut top = px(0.0);
        for (start, line) in self.lines.iter() {
            if ix <= start + line.len() {
                let at = line
                    .position_for_index(ix.saturating_sub(*start), self.line_height)
                    .unwrap_or_default();
                return point(at.x, top + at.y);
            }
            top += self.line_height * Self::line_rows(line) as f32;
        }
        point(px(0.0), (top - self.line_height).max(px(0.0)))
    }

    /// The index closest to a point from the text's top left.
    fn index(&self, position: Point<Pixels>) -> usize {
        let mut top = px(0.0);
        let last = self.lines.len().saturating_sub(1);
        for (n, (start, line)) in self.lines.iter().enumerate() {
            let height = self.line_height * Self::line_rows(line) as f32;
            if position.y < top + height || n == last {
                // Inside this line's rows, so it never answers for another.
                let y = (position.y - top).max(px(0.0)).min(height - px(0.5));
                let x = position.x.max(px(0.0));
                let (Ok(ix) | Err(ix)) =
                    line.closest_index_for_position(point(x, y), self.line_height);
                return start + ix;
            }
            top += height;
        }
        0
    }
}

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
    /// Several lines, wrapped, showing at most this many before scrolling.
    multiline: Option<usize>,
    /// How far the line is scrolled left to keep the caret in view.
    scroll_x: Pixels,
    /// How far several lines are scrolled up.
    scroll_y: Pixels,
    /// The caret moved or the text changed: scroll it into view at the
    /// next frame (and not otherwise, so the wheel can scroll away).
    reveal_caret: bool,
    /// The color behind an outlined field, used to cut the label's notch.
    surface: Option<Hsla>,
    on_submit: Option<SubmitHandler>,
    find_pictures: Option<FindPictures>,
    /// The pictures in the text, in order, found when it last changed.
    pictures: Vec<InlinePicture>,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_layout: Option<Shaped>,
    last_bounds: Option<Bounds<Pixels>>,
    /// The font, size and wrap width the text was last shaped with.
    last_font: Option<(Font, Pixels, Option<Pixels>)>,
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
            multiline: None,
            scroll_x: px(0.0),
            scroll_y: px(0.0),
            reveal_caret: false,
            surface: None,
            on_submit: None,
            find_pictures: None,
            pictures: Vec::new(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            last_font: None,
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

    /// Several lines: the text wraps, Shift+Enter starts a new line, and the
    /// field grows to `max_lines` before it scrolls. Enter still submits.
    pub fn multiline(mut self, max_lines: usize) -> Self {
        self.multiline = Some(max_lines.max(1));
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

    /// Shows runs of the text as pictures: `find` gives them, in order and
    /// apart, for the text, and is asked again whenever the text changes or
    /// the field renders (when a picture may have arrived).
    pub fn inline_pictures(
        mut self,
        find: impl Fn(&str, &App) -> Vec<InlinePicture> + 'static,
    ) -> Self {
        self.find_pictures = Some(Rc::new(find));
        self
    }

    pub fn text(&self) -> &SharedString {
        &self.content
    }

    /// Where the caret is in the text.
    pub fn caret(&self) -> usize {
        self.cursor_offset()
    }

    /// Whether an input method is composing text in the field.
    pub fn is_composing(&self) -> bool {
        self.marked_range.is_some()
    }

    /// Replaces `range` of the text, as the user would, and puts the caret
    /// after the new text.
    pub fn replace(
        &mut self,
        range: Range<usize>,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = self.range_to_utf16(&range);
        self.replace_text_in_range(Some(range), text, window, cx);
    }

    fn find_pictures(&mut self, cx: &App) {
        let Some(find) = self.find_pictures.clone() else {
            return;
        };
        let content = self.content.clone();
        let mut end = 0;
        self.pictures = find(&content, cx)
            .into_iter()
            .filter(|picture| {
                let ok = end <= picture.range.start
                    && picture.range.start < picture.range.end
                    && content.get(picture.range.clone()).is_some();
                if ok {
                    end = picture.range.end;
                }
                ok
            })
            .collect();
    }

    /// The picture `offset` falls inside of, if any.
    fn picture_around(&self, offset: usize) -> Option<Range<usize>> {
        self.pictures
            .iter()
            .map(|picture| picture.range.clone())
            .find(|range| range.start < offset && offset < range.end)
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
        self.find_pictures(cx);
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

    /// The start of the caret's line (of the text, not of the wrapping).
    fn home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        let cursor = self.cursor_offset();
        let start = self.content[..cursor].rfind('\n').map_or(0, |at| at + 1);
        self.move_to(start, window, cx);
    }

    fn end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        let cursor = self.cursor_offset();
        let end = self.content[cursor..]
            .find('\n')
            .map_or(self.content.len(), |at| cursor + at);
        self.move_to(end, window, cx);
    }

    fn newline(&mut self, _: &Newline, window: &mut Window, cx: &mut Context<Self>) {
        if self.multiline.is_some() {
            self.replace_text_in_range(None, "\n", window, cx);
        } else {
            self.submit(&Submit, window, cx);
        }
    }

    /// The index a row above (`rows` < 0) or below the caret; the start or
    /// end of the text past the first or last row. None for one line.
    fn vertical(&self, rows: f32, window: &Window) -> Option<usize> {
        self.multiline?;
        let shaped = self.current_layout(window)?;
        let at = shaped.position(self.display_offset(self.cursor_offset()));
        let y = at.y + shaped.line_height * (rows + 0.5);
        let total = shaped.line_height * shaped.rows() as f32;
        Some(if y < px(0.0) {
            0
        } else if y >= total {
            self.content.len()
        } else {
            self.content_offset(shaped.index(point(at.x, y)))
        })
    }

    fn up(&mut self, _: &Up, window: &mut Window, cx: &mut Context<Self>) {
        match self.vertical(-1.0, window) {
            Some(ix) => self.move_to(ix, window, cx),
            None => cx.propagate(),
        }
    }

    fn down(&mut self, _: &Down, window: &mut Window, cx: &mut Context<Self>) {
        match self.vertical(1.0, window) {
            Some(ix) => self.move_to(ix, window, cx),
            None => cx.propagate(),
        }
    }

    fn select_up(&mut self, _: &SelectUp, window: &mut Window, cx: &mut Context<Self>) {
        match self.vertical(-1.0, window) {
            Some(ix) => self.select_to(ix, window, cx),
            None => cx.propagate(),
        }
    }

    fn select_down(&mut self, _: &SelectDown, window: &mut Window, cx: &mut Context<Self>) {
        match self.vertical(1.0, window) {
            Some(ix) => self.select_to(ix, window, cx),
            None => cx.propagate(),
        }
    }

    /// Newlines as this field takes them: kept with several lines (as
    /// `\n`), spaces otherwise.
    fn accept(&self, text: &str) -> String {
        let text = text.replace("\r\n", "\n");
        if self.multiline.is_some() {
            text.replace('\r', "\n")
        } else {
            text.replace(['\n', '\r'], " ")
        }
    }

    /// The user changed the text.
    fn edited(&mut self, cx: &mut Context<Self>) {
        self.reveal_caret = true;
        cx.emit(TextFieldEvent::Edited);
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
        super::clear_selection(cx);
        window.focus(&self.focus_handle, cx);
        let ix = self.index_for_mouse_position(event.position);
        match event.click_count {
            2 => {
                let word = self.word_at(ix);
                self.move_to(word.start, window, cx);
                self.select_to(word.end, window, cx);
            }
            n if n >= 3 => {
                self.move_to(0, window, cx);
                self.select_to(self.content.len(), window, cx);
            }
            _ if event.modifiers.shift => {
                self.is_selecting = true;
                self.select_to(ix, window, cx);
            }
            // A press on the selection may start dragging it; the drag
            // handle over the selection takes care of that.
            _ if self.drag_handle().is_some()
                && self.selected_range.start < ix
                && ix < self.selected_range.end => {}
            _ => {
                self.is_selecting = true;
                self.move_to(ix, window, cx)
            }
        }
    }

    /// The word (or run of punctuation or space) around `ix`; a picture
    /// is one.
    fn word_at(&self, ix: usize) -> Range<usize> {
        if let Some(picture) = self
            .pictures
            .iter()
            .find(|picture| picture.range.start <= ix && ix < picture.range.end)
        {
            return picture.range.clone();
        }
        self.content
            .split_word_bound_indices()
            .map(|(start, word)| start..start + word.len())
            .find(|range| range.contains(&ix))
            .or_else(|| {
                self.content
                    .split_word_bound_indices()
                    .next_back()
                    .map(|(start, word)| start..start + word.len())
            })
            .unwrap_or(0..0)
    }

    /// Where the selection can be grabbed and dragged, in the text's
    /// coordinates (before scrolling). Only a selection within one row.
    fn drag_handle(&self) -> Option<Bounds<Pixels>> {
        if self.masked || self.selected_range.is_empty() || self.marked_range.is_some() {
            return None;
        }
        let shaped = self.last_layout.as_ref()?;
        if shaped.text != self.display_text() {
            return None;
        }
        let start = shaped.position(self.display_offset(self.selected_range.start));
        let end = shaped.position(self.display_offset(self.selected_range.end));
        (start.y == end.y)
            .then(|| Bounds::from_corners(start, point(end.x, end.y + shaped.line_height)))
    }

    /// Inserts dropped text where it was dropped. Text dragged from this
    /// field moves; text from anywhere else is copied in.
    fn drop_text(&mut self, dragged: &DraggedText, window: &mut Window, cx: &mut Context<Self>) {
        let mut ix = self.index_for_mouse_position(window.mouse_position());
        let text = self.accept(&dragged.text);
        if let Some((source, range)) = &dragged.source
            && *source == cx.entity_id()
        {
            if ix >= range.start && ix <= range.end {
                return;
            }
            let moved = self.content[range.clone()].to_string();
            self.content =
                (self.content[..range.start].to_owned() + &self.content[range.end..]).into();
            if ix > range.end {
                ix -= range.len();
            }
            self.content = (self.content[..ix].to_owned() + &moved + &self.content[ix..]).into();
            self.selected_range = ix..ix + moved.len();
        } else {
            self.content = (self.content[..ix].to_owned() + &text + &self.content[ix..]).into();
            self.selected_range = ix..ix + text.len();
        }
        self.selection_reversed = false;
        self.marked_range = None;
        self.find_pictures(cx);
        window.focus(&self.focus_handle, cx);
        window.notify_text_input_changed(&self.focus_handle);
        self.edited(cx);
        cx.notify();
    }

    /// The shaped text: the last painted, or shaped anew when the text
    /// changed since (the platform asks for character positions between an
    /// edit and the next frame).
    fn current_layout(&self, window: &Window) -> Option<Shaped> {
        let display = self.display_text();
        let last = self.last_layout.as_ref()?;
        if last.text == display {
            return Some(last.clone());
        }
        let (font, size, wrap) = self.last_font.clone()?;
        let run = TextRun {
            len: display.len(),
            font,
            ..Default::default()
        };
        Some(Shaped::new(
            display,
            &[run],
            size,
            last.line_height,
            wrap,
            window,
        ))
    }

    /// Where the text's top left is on screen, after scrolling.
    fn text_origin(&self, bounds: &Bounds<Pixels>) -> Point<Pixels> {
        point(bounds.left() - self.scroll_x, bounds.top() - self.scroll_y)
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
            let text = self.accept(&text);
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() && !self.masked {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        } else if self.selected_range.is_empty() {
            // Nothing selected here: copy what is selected on the page.
            super::selectable_text::copy_selection(cx);
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
        self.reveal_caret = true;
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
        let (Some(bounds), Some(shaped)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        let origin = self.text_origin(bounds);
        self.content_offset(shaped.index(point(position.x - origin.x, position.y - origin.y)))
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
        self.reveal_caret = true;
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify()
    }

    /// The text as painted: the content with placeholders for pictures,
    /// or a bullet per character.
    fn display_text(&self) -> SharedString {
        if self.masked {
            return MASK.to_string().repeat(self.content.chars().count()).into();
        }
        if self.pictures.is_empty() {
            return self.content.clone();
        }
        let mut display = String::with_capacity(self.content.len());
        let mut at = 0;
        for picture in &self.pictures {
            display.push_str(&self.content[at..picture.range.start]);
            display.push_str(IMAGE_PLACEHOLDER);
            at = picture.range.end;
        }
        display.push_str(&self.content[at..]);
        display.into()
    }

    /// Where the pictures' placeholders are in the painted text.
    fn placeholders(&self) -> Vec<Range<usize>> {
        if self.masked {
            return Vec::new();
        }
        self.pictures
            .iter()
            .map(|picture| {
                let start = self.display_offset(picture.range.start);
                start..start + IMAGE_PLACEHOLDER.len()
            })
            .collect()
    }

    /// Where a content offset falls in the painted text (inside a picture:
    /// before it).
    fn display_offset(&self, offset: usize) -> usize {
        if self.masked {
            return self.content[..offset].chars().count() * MASK.len_utf8();
        }
        let mut display = offset;
        for picture in &self.pictures {
            if offset <= picture.range.start {
                break;
            }
            if offset < picture.range.end {
                return display - (offset - picture.range.start);
            }
            display = display - picture.range.len() + IMAGE_PLACEHOLDER.len();
        }
        display
    }

    /// The content offset of an offset in the painted text (inside a
    /// picture's placeholder: after the picture).
    fn content_offset(&self, display: usize) -> usize {
        if self.masked {
            return self
                .content
                .char_indices()
                .nth(display / MASK.len_utf8())
                .map_or(self.content.len(), |(ix, _)| ix);
        }
        // How far the content runs ahead of the painted text.
        let mut ahead = 0;
        for picture in &self.pictures {
            let start = picture.range.start - ahead;
            if display <= start {
                break;
            }
            if display < start + IMAGE_PLACEHOLDER.len() {
                return picture.range.end;
            }
            ahead += picture.range.len() - IMAGE_PLACEHOLDER.len();
        }
        display + ahead
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
        let previous = self
            .content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0);
        self.picture_around(previous)
            .map_or(previous, |picture| picture.start)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        let next = self
            .content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len());
        self.picture_around(next)
            .map_or(next, |picture| picture.end)
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
        self.find_pictures(cx);
        window.notify_text_input_changed(&self.focus_handle);
        self.edited(cx);
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
        self.find_pictures(cx);
        window.notify_text_input_changed(&self.focus_handle);
        self.edited(cx);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let shaped = self.current_layout(window)?;
        let range = self.range_from_utf16(&range_utf16);
        let origin = self.text_origin(&bounds);
        let start = shaped.position(self.display_offset(range.start));
        let end = shaped.position(self.display_offset(range.end));
        // On the start's row; to its end when the range wraps.
        let end_x = if end.y == start.y {
            end.x
        } else {
            shaped.width()
        };
        Some(Bounds::from_corners(
            point(origin.x + start.x, origin.y + start.y),
            point(origin.x + end_x, origin.y + start.y + shaped.line_height),
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
        window: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        bounds.localize(&point)?;
        let shaped = self.current_layout(window)?;
        let origin = self.text_origin(&bounds);
        let index = shaped.index(gpui::point(point.x - origin.x, point.y - origin.y));
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
        // A picture may have arrived.
        self.find_pictures(cx);
        let focused = self.focus_handle.is_focused(window);
        if focused != self.was_focused {
            self.was_focused = focused;
            self.cursor_visible = true;
            if focused {
                // Move the input method's candidate window to this field.
                window.invalidate_character_coordinates();
            }
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
        let drop_target = c.secondary_container;

        let line_style = LineStyle {
            text: body,
            color: c.on_surface,
            placeholder: c.on_surface_variant,
            caret: accent,
            selection: c.primary.opacity(0.32),
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
            .on_action(cx.listener(Self::newline))
            .on_action(cx.listener(Self::up))
            .on_action(cx.listener(Self::down))
            .on_action(cx.listener(Self::select_up))
            .on_action(cx.listener(Self::select_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_drop(cx.listener(Self::drop_text))
            .drag_over::<DraggedText>(move |style, _, _, _| style.bg(drop_target))
            .on_drop(cx.listener(|this, text: &ExternalText, window, cx| {
                this.drop_text(&DraggedText::new(text.text().clone()), window, cx)
            }))
            .drag_over::<ExternalText>(move |style, _, _, _| style.bg(drop_target))
            .relative()
            .w_full()
            .min_h(px(56.0));

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
        // In the flow, so several lines grow the field; one line keeps it
        // at 56 px.
        let below = (px(56.0 - text_top) - body.line_height).max(px(0.0));
        let mut text_area = div()
            .relative()
            .ml(px(leading_pad))
            .mr(px(trailing_pad))
            .mt(px(text_top))
            .mb(below)
            .overflow_hidden()
            .md_type_style(body, cx)
            .child(TextLine {
                field: cx.entity(),
                style: line_style,
                focused,
                max_lines: self.multiline,
            });
        if self.multiline.is_some() {
            text_area = text_area.on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
                let line_height = this
                    .last_layout
                    .as_ref()
                    .map_or(px(24.0), |shaped| shaped.line_height);
                let delta = event.delta.pixel_delta(line_height).y;
                this.scroll_y = (this.scroll_y - delta).max(px(0.0));
                cx.notify();
            }));
        }
        if let Some(handle) = self.drag_handle() {
            // The selection can be dragged out (or elsewhere in the field);
            // a click on it without a drag places the caret.
            let range = self.selected_range.clone();
            let dragged = DraggedText {
                text: self.content[range.clone()].to_string().into(),
                source: Some((cx.entity_id(), range)),
            };
            text_area = text_area.child(
                div()
                    .id("drag-selection")
                    .absolute()
                    .top(handle.top() - self.scroll_y)
                    .h(handle.size.height)
                    .left(handle.left() - self.scroll_x)
                    .w(handle.size.width)
                    .cursor(CursorStyle::Arrow)
                    .on_drag(dragged, |dragged, _, _, cx| {
                        super::selectable_text::drag_preview(dragged.text.clone(), cx)
                    })
                    .external_drag_payload(super::selectable_text::external_payload)
                    .on_click(cx.listener(|this, _, window, cx| {
                        let ix = this.index_for_mouse_position(window.mouse_position());
                        this.move_to(ix, window, cx);
                    })),
            );
        }
        field = field.child(text_area);

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

/// The editable text, painted directly so the caret, selection and IME
/// composition can be drawn and reported to the platform.
struct TextLine {
    field: Entity<TextField>,
    style: LineStyle,
    focused: bool,
    /// Several lines: at most this many show.
    max_lines: Option<usize>,
}

struct LinePrepaint {
    shaped: Option<Shaped>,
    font: Font,
    wrap_width: Option<Pixels>,
    caret: Option<gpui::PaintQuad>,
    selection: Vec<gpui::PaintQuad>,
    scroll: Point<Pixels>,
}

impl TextLine {
    const CARET_WIDTH: Pixels = px(2.0);

    /// The field's text (or placeholder) as it is painted, wrapped at
    /// `wrap_width`.
    fn shape(
        field: &TextField,
        style: &LineStyle,
        wrap_width: Option<Pixels>,
        window: &Window,
        cx: &App,
    ) -> Shaped {
        let content = field.display_text();
        let marked_range = field
            .marked_range
            .as_ref()
            .map(|marked| field.display_offset(marked.start)..field.display_offset(marked.end));
        let font = cx.md().typography.font(style.text);
        let (display, color, hidden) = match (&field.placeholder, content.is_empty()) {
            (Some(placeholder), true) if style.show_placeholder => {
                (placeholder.clone(), style.placeholder, Vec::new())
            }
            _ => (content, style.color, field.placeholders()),
        };
        // Runs end wherever the composition or a picture's placeholder
        // starts or ends.
        let mut cuts = vec![0, display.len()];
        cuts.extend(
            marked_range
                .iter()
                .flat_map(|marked| [marked.start, marked.end]),
        );
        cuts.extend(hidden.iter().flat_map(|range| [range.start, range.end]));
        cuts.sort_unstable();
        cuts.dedup();
        let runs = cuts
            .windows(2)
            .map(|cut| {
                let within = |range: &Range<usize>| range.start <= cut[0] && cut[1] <= range.end;
                TextRun {
                    len: cut[1] - cut[0],
                    font: font.clone(),
                    color: if hidden.iter().any(within) {
                        color.opacity(0.0)
                    } else {
                        color
                    },
                    underline: marked_range
                        .as_ref()
                        .filter(|marked| within(marked))
                        .map(|_| UnderlineStyle {
                            color: Some(color),
                            thickness: px(1.0),
                            wavy: false,
                        }),
                    ..Default::default()
                }
            })
            .collect::<Vec<_>>();
        Shaped::new(
            display,
            &runs,
            style.text.size,
            style.text.line_height,
            wrap_width,
            window,
        )
    }
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
        let line_height = self.style.text.line_height;
        let Some(max_lines) = self.max_lines else {
            style.size.height = line_height.into();
            return (window.request_layout(style, [], cx), ());
        };
        // As many rows as the text wraps into at the width given, up to
        // `max_lines`.
        let (field, line_style) = (self.field.clone(), self.style);
        let layout = window.request_measured_layout(style, move |known, available, window, cx| {
            let width = known.width.or(match available.width {
                AvailableSpace::Definite(width) => Some(width),
                _ => None,
            });
            let wrap_width = width.map(|width| (width - Self::CARET_WIDTH).max(px(1.0)));
            let rows = Self::shape(field.read(cx), &line_style, wrap_width, window, cx).rows();
            size(
                width.unwrap_or_default(),
                line_height * rows.clamp(1, max_lines) as f32,
            )
        });
        (layout, ())
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
        let wrap_width = self
            .max_lines
            .map(|_| (bounds.size.width - Self::CARET_WIDTH).max(px(1.0)));
        let shaped = Self::shape(field, &self.style, wrap_width, window, cx);
        let font = cx.md().typography.font(self.style.text);
        let line_height = self.style.text.line_height;
        let selected_range = field.display_offset(field.selected_range.start)
            ..field.display_offset(field.selected_range.end);
        let caret = shaped.position(field.display_offset(field.cursor_offset()));

        // Scroll just enough to keep the caret in view, and no further than
        // the end of the text: sideways for one line, up and down for
        // several (only when the caret moved, so the wheel can scroll).
        let mut scroll = point(field.scroll_x, field.scroll_y);
        if self.max_lines.is_some() {
            let total = line_height * shaped.rows() as f32;
            if field.reveal_caret {
                if caret.y < scroll.y {
                    scroll.y = caret.y;
                } else if caret.y + line_height > scroll.y + bounds.size.height {
                    scroll.y = caret.y + line_height - bounds.size.height;
                }
            }
            scroll.y = scroll
                .y
                .min((total - bounds.size.height).max(px(0.0)))
                .max(px(0.0));
            scroll.x = px(0.0);
        } else {
            let room = (bounds.size.width - Self::CARET_WIDTH).max(px(0.0));
            if caret.x < scroll.x {
                scroll.x = caret.x;
            } else if caret.x > scroll.x + room {
                scroll.x = caret.x - room;
            }
            scroll.x = scroll
                .x
                .min((shaped.width() - room).max(px(0.0)))
                .max(px(0.0));
            scroll.y = px(0.0);
        }
        let origin = point(bounds.left() - scroll.x, bounds.top() - scroll.y);

        let (selection, caret) = if selected_range.is_empty() {
            (
                Vec::new(),
                Some(fill(
                    Bounds::new(origin + caret, size(Self::CARET_WIDTH, line_height)),
                    self.style.caret,
                )),
            )
        } else {
            // From the start to the end of its row, whole rows between, and
            // the last row up to the end.
            let start = shaped.position(selected_range.start);
            let end = shaped.position(selected_range.end);
            let right = shaped.width().max(bounds.size.width);
            let mut quads = Vec::new();
            let mut top = start.y;
            while top <= end.y {
                let left = if top == start.y { start.x } else { px(0.0) };
                let right = if top == end.y { end.x } else { right };
                quads.push(fill(
                    Bounds::from_corners(
                        origin + point(left, top),
                        origin + point(right, top + line_height),
                    ),
                    self.style.selection,
                ));
                top += line_height;
            }
            (quads, None)
        };
        LinePrepaint {
            shaped: Some(shaped),
            font,
            wrap_width,
            caret,
            selection,
            scroll,
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
        for selection in prepaint.selection.drain(..) {
            window.paint_quad(selection);
        }
        let Some(shaped) = prepaint.shaped.take() else {
            return;
        };
        let scroll = prepaint.scroll;
        let line_height = self.style.text.line_height;
        let mut top = bounds.top() - scroll.y;
        for (_, line) in shaped.lines.iter() {
            let height = line_height * Shaped::line_rows(line) as f32;
            // Lines scrolled out of view are not painted.
            if top + height >= bounds.top() && top <= bounds.bottom() {
                let _ = line.paint(
                    point(bounds.left() - scroll.x, top),
                    line_height,
                    gpui::TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }
            top += height;
        }
        let field = self.field.read(cx);
        let pictures = field
            .pictures
            .iter()
            .zip(field.placeholders())
            .filter_map(|(picture, range)| Some((picture.image.clone()?, range)))
            .collect::<Vec<_>>();
        let origin = point(bounds.left() - scroll.x, bounds.top() - scroll.y);
        for (image, range) in pictures {
            let (start, end) = (shaped.position(range.start), shaped.position(range.end));
            paint_picture(
                &image,
                origin + start,
                origin + end,
                line_height,
                window,
                cx,
            );
        }
        let blink_on = self.field.read(cx).cursor_visible;
        if self.focused
            && blink_on
            && let Some(caret) = prepaint.caret.take()
        {
            window.paint_quad(caret);
        }
        let (font, size, wrap_width) = (
            prepaint.font.clone(),
            self.style.text.size,
            prepaint.wrap_width,
        );
        self.field.update(cx, |field, _| {
            field.last_layout = Some(shaped);
            field.last_bounds = Some(bounds);
            field.scroll_x = scroll.x;
            field.scroll_y = scroll.y;
            field.reveal_caret = false;
            field.last_font = Some((font, size, wrap_width));
        });
    }
}

#[cfg(test)]
mod tests {
    use gpui::{
        Bounds, EntityInputHandler as _, ExternalText, FileDropEvent, TestAppContext, point, px,
        size,
    };

    use super::TextField;
    use crate::theme::SchemeConfig;

    /// Input methods ask where the composition is right after changing it,
    /// before the next frame; the answer must follow the new text.
    #[gpui::test]
    fn composition_is_located_before_the_next_frame(cx: &mut TestAppContext) {
        cx.update(|cx| crate::init(SchemeConfig::default(), cx));
        let (field, cx) = cx.add_window_view(|_, cx| TextField::new(cx));
        let element = Bounds::new(point(px(100.0), px(50.0)), size(px(400.0), px(24.0)));
        cx.update(|window, cx| {
            field.update(cx, |field, cx| {
                field.replace_and_mark_text_in_range(None, "にほんご", None, window, cx);
                let start = field.bounds_for_range(0..0, element, window, cx);
                let end = field.bounds_for_range(4..4, element, window, cx);
                let (start, end) = (start.expect("start"), end.expect("end"));
                assert_eq!(start.origin.x, px(100.0));
                assert!(end.origin.x > start.origin.x);
                assert_eq!(start.origin.y, px(50.0));
            });
        });
    }

    /// Several lines take Shift+Enter as a new line, and the caret moves
    /// between lines with Up and Down; one line submits instead.
    #[gpui::test]
    fn several_lines_take_newlines_and_move_between_them(cx: &mut TestAppContext) {
        cx.update(|cx| crate::init(SchemeConfig::default(), cx));
        let (field, cx) = cx.add_window_view(|_, cx| TextField::new(cx).multiline(4));
        cx.update(|window, cx| {
            field.update(cx, |field, cx| {
                field.replace_text_in_range(None, "one", window, cx);
                field.newline(&super::Newline, window, cx);
                field.replace_text_in_range(None, "two", window, cx);
                assert_eq!(field.text(), "one\ntwo");
            });
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            field.update(cx, |field, cx| {
                field.up(&super::Up, window, cx);
                assert!(field.cursor_offset() <= 3, "{}", field.cursor_offset());
                field.down(&super::Down, window, cx);
                assert!(field.cursor_offset() >= 4, "{}", field.cursor_offset());
                field.home(&super::Home, window, cx);
                assert_eq!(field.cursor_offset(), 4);
            });
        });

        let submitted = std::rc::Rc::new(std::cell::Cell::new(false));
        let (line, cx) = cx.add_window_view({
            let submitted = submitted.clone();
            move |_, cx| {
                TextField::new(cx).on_submit(move |_, _, _| {
                    submitted.set(true);
                    false
                })
            }
        });
        cx.update(|window, cx| {
            line.update(cx, |line, cx| {
                line.replace_text_in_range(None, "a", window, cx);
                line.newline(&super::Newline, window, cx);
                assert_eq!(line.text(), "a");
                assert_eq!(line.accept("b\nc"), "b c");
            });
        });
        assert!(submitted.get());
    }

    /// A run shown as a picture stands on a placeholder, and the caret
    /// moves past it and deletes it as one character.
    #[gpui::test]
    fn pictures_move_and_delete_as_one_character(cx: &mut TestAppContext) {
        cx.update(|cx| crate::init(SchemeConfig::default(), cx));
        let (field, cx) = cx.add_window_view(|_, cx| {
            TextField::new(cx).inline_pictures(|text, _| {
                text.match_indices("<cat>")
                    .map(|(at, run)| super::InlinePicture {
                        range: at..at + run.len(),
                        image: None,
                    })
                    .collect()
            })
        });
        cx.update(|window, cx| {
            field.update(cx, |field, cx| {
                field.replace_text_in_range(None, "a<cat>b", window, cx);
                assert_eq!(field.display_text(), "aMMb");
                assert_eq!(field.display_offset(6), 3);
                assert_eq!(field.display_offset(3), 1);
                assert_eq!(field.content_offset(2), 6);
                assert_eq!(field.content_offset(4), 7);
                field.left(&super::Left, window, cx);
                field.left(&super::Left, window, cx);
                assert_eq!(field.caret(), 1);
                field.right(&super::Right, window, cx);
                assert_eq!(field.caret(), 6);
                field.backspace(&super::Backspace, window, cx);
                assert_eq!(field.text(), "ab");
                field.replace(1..1, "<cat>", window, cx);
                assert_eq!(field.text(), "a<cat>b");
                assert_eq!(field.caret(), 6);
            });
        });
    }

    /// Text dragged in from another application arrives as the platform's
    /// drop events and lands in the field under the pointer.
    #[gpui::test]
    fn text_from_other_applications_drops_in(cx: &mut TestAppContext) {
        cx.update(|cx| crate::init(SchemeConfig::default(), cx));
        let (field, cx) = cx.add_window_view(|_, cx| TextField::new(cx));
        let position = point(px(40.0), px(28.0));
        cx.simulate_event(FileDropEvent::TextEntered {
            position,
            text: ExternalText("こんにちは".into()),
        });
        cx.simulate_event(FileDropEvent::Submit { position });
        let text = field.read_with(cx, |field, _| field.text().clone());
        assert_eq!(text, "こんにちは");
    }
}
