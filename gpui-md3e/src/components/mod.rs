mod button;
mod button_group;
mod chip;
mod fab;
mod fab_menu;
mod icon_button;
mod loading_indicator;
mod navigation_rail;
mod pressable;
mod progress;
mod selectable_text;
mod selection;
mod shape_view;
mod slider;
mod split_button;
mod surfaces;
mod text_field;

pub use button::*;
pub use button_group::*;
pub use chip::*;
pub use fab::*;
pub use fab_menu::*;
pub use icon_button::*;
pub use loading_indicator::*;
pub use navigation_rail::*;
pub use pressable::SurfaceColors;
pub use progress::*;
pub use selectable_text::{
    CopySelection, DragPreview, DraggedText, SelectableText, clear_selection, selected_text,
};
pub use selection::*;
pub use shape_view::*;
pub use slider::*;
pub use split_button::*;
pub use surfaces::*;
pub use text_field::{TextField, TextFieldStyle};

/// Installs key bindings used by gpui-md3e components.
pub(crate) fn bind_keys(cx: &mut gpui::App) {
    text_field::bind_keys(cx);
    selectable_text::bind_keys(cx);
}
