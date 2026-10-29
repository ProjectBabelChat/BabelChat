//! A guild's own emoji: their pictures (fetched once a run), in message
//! text and reactions, offered in the emoji picker, and added and deleted
//! by owners in the guild's emoji dialog: an image first, then its name.
//! Codes of emoji that are gone show their `:name:`.

use std::{collections::HashMap, path::Path, sync::Arc};

use gpui_md3e::components::InlinePicture;

use babel_sdk::{ApiError, Error, MAX_EMOJI_BYTES, emoji, emoji_code};
use gpui::prelude::*;
use gpui::{
    AnyElement, App, Context, Focusable, Image, ImageFormat, IntoElement, ObjectFit,
    PathPromptOptions, SharedString, Window, div, img, px,
};
use gpui_md3e::{components::TextPart, prelude::*};

use super::{ChatView, Dialog, emoji::CustomEmoji};
use crate::runtime::{self, describe};

/// An emoji's picture, as far as it has come.
enum Picture {
    Loading,
    Ready(Arc<Image>),
    /// Deleted, or not a picture: its name stands in.
    Gone,
}

/// Pictures of emoji by ID, with their guild, kept by the chat view (an
/// entity of its own, which the composer reads as it shows codes).
#[derive(Default)]
pub(super) struct EmojiPictures(HashMap<String, (String, Picture)>);

impl EmojiPictures {
    pub(super) fn ready(&self, id: &str) -> Option<Arc<Image>> {
        match self.0.get(id) {
            Some((_, Picture::Ready(image))) => Some(image.clone()),
            _ => None,
        }
    }

    pub(super) fn gone(&self, id: &str) -> bool {
        matches!(self.0.get(id), Some((_, Picture::Gone)))
    }

    /// The codes in `text` whose pictures may show, for the composer.
    pub(super) fn in_text(&self, text: &str) -> Vec<InlinePicture> {
        emoji::find(text)
            .into_iter()
            .filter(|(_, custom)| !self.gone(custom.id))
            .map(|(range, custom)| InlinePicture {
                range,
                image: self.ready(custom.id),
            })
            .collect()
    }
}

/// The format of an image, from its first bytes.
fn image_format(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(b"\x89PNG") {
        Some(ImageFormat::Png)
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(ImageFormat::Jpeg)
    } else if bytes.starts_with(b"GIF8") {
        Some(ImageFormat::Gif)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(ImageFormat::Webp)
    } else {
        None
    }
}

/// A name offered for an emoji from its file's: letters, digits and
/// underscores, or nothing when the file's name has too few of them.
fn name_from_file(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>();
    let name = name
        .trim_matches('_')
        .chars()
        .take(emoji::MAX_NAME_CHARS)
        .collect::<String>();
    if emoji::valid_name(&name) {
        name
    } else {
        String::new()
    }
}

/// An image chosen for a new emoji, waiting for its name.
pub struct NewEmoji {
    bytes: Vec<u8>,
    pub(super) image: Arc<Image>,
}

/// Why an image cannot be an emoji.
enum Unfit {
    TooLarge,
    NotImage,
    Unreadable(String),
}

/// Reads an image for an emoji (off the UI thread: it blocks).
fn read_emoji_image(path: &Path) -> Result<NewEmoji, Unfit> {
    if let Ok(metadata) = std::fs::metadata(path)
        && metadata.len() as i64 > MAX_EMOJI_BYTES
    {
        return Err(Unfit::TooLarge);
    }
    let bytes = std::fs::read(path).map_err(|error| Unfit::Unreadable(error.to_string()))?;
    if bytes.len() as i64 > MAX_EMOJI_BYTES {
        return Err(Unfit::TooLarge);
    }
    let format = image_format(&bytes).ok_or(Unfit::NotImage)?;
    let image = Arc::new(Image::from_bytes(format, bytes.clone()));
    Ok(NewEmoji { bytes, image })
}

impl ChatView {
    /// The picture of emoji `id` of `guild_id`, fetched the first time it
    /// is asked for.
    pub(super) fn emoji_picture(
        &mut self,
        guild_id: &str,
        id: &str,
        cx: &mut Context<Self>,
    ) -> Option<Arc<Image>> {
        match self.emoji_pictures.read(cx).0.get(id) {
            Some((_, Picture::Ready(image))) => return Some(image.clone()),
            Some(_) => return None,
            None => {}
        }
        self.emoji_pictures.update(cx, |pictures, _| {
            pictures
                .0
                .insert(id.to_string(), (guild_id.to_string(), Picture::Loading))
        });
        let session = self.session.clone();
        let (guild_id, id) = (guild_id.to_string(), id.to_string());
        let key = id.clone();
        let task = runtime::spawn(
            cx,
            async move { session.emoji_image(&guild_id, &key).await },
        );
        cx.spawn(async move |this, cx| {
            let picture = match task.await {
                Ok(bytes) => match image_format(&bytes) {
                    Some(format) => {
                        Picture::Ready(Arc::new(Image::from_bytes(format, bytes.to_vec())))
                    }
                    None => Picture::Gone,
                },
                Err(_) => Picture::Gone,
            };
            let _ = this.update(cx, |this, cx| {
                this.emoji_pictures.update(cx, |pictures, _| {
                    if let Some((_, known)) = pictures.0.get_mut(&id) {
                        *known = picture;
                    }
                });
                this.refresh_picker(cx);
                this.composer.update(cx, |_, cx| cx.notify());
                cx.notify();
            });
        })
        .detach();
        None
    }

    pub(super) fn emoji_gone(&self, id: &str, cx: &App) -> bool {
        self.emoji_pictures.read(cx).gone(id)
    }

    /// Fetches the pictures of the codes in the composer.
    pub(super) fn fetch_composer_pictures(&mut self, cx: &mut Context<Self>) {
        let text = self.composer.read(cx).text().clone();
        for (_, custom) in emoji::find(&text) {
            self.emoji_picture(custom.guild_id, custom.id, cx);
        }
    }

    /// A guild's emoji changed: those it deleted show their names from
    /// now on (the device may still have their pictures).
    pub(super) fn forget_deleted_emoji(&mut self, guild_id: &str, cx: &mut Context<Self>) {
        let Some(view) = self.session.guild(guild_id) else {
            return;
        };
        self.emoji_pictures.update(cx, |pictures, _| {
            for (id, (guild, picture)) in &mut pictures.0 {
                if guild == guild_id && !view.emoji.iter().any(|custom| custom.id == *id) {
                    *picture = Picture::Gone;
                }
            }
        });
    }

    /// `text` for a [`SelectableText`](gpui_md3e::components::SelectableText):
    /// a guild's emoji as their pictures, or as `:name:` once gone.
    pub(super) fn text_parts(&mut self, text: &str, cx: &mut Context<Self>) -> Vec<TextPart> {
        emoji::segments(text)
            .into_iter()
            .map(|segment| match segment {
                emoji::Segment::Text(run) => TextPart::Text(run.to_string().into()),
                emoji::Segment::Emoji(custom) => {
                    let name = SharedString::from(format!(":{}:", custom.name));
                    if self.emoji_gone(custom.id, cx) {
                        TextPart::Text(name)
                    } else {
                        TextPart::Image {
                            image: self.emoji_picture(custom.guild_id, custom.id, cx),
                            text: name,
                        }
                    }
                }
            })
            .collect()
    }

    /// What a reaction shows: its Unicode emoji, or a guild's emoji's
    /// picture (its name once gone).
    pub(super) fn reaction_face(&mut self, reaction: &str, cx: &mut Context<Self>) -> AnyElement {
        let Some(custom) = emoji::parse(reaction) else {
            return div()
                .text_size(px(16.0))
                .child(reaction.to_string())
                .into_any_element();
        };
        if self.emoji_gone(custom.id, cx) {
            return div()
                .md_type(TypeRole::LabelMedium, cx)
                .child(format!(":{}:", custom.name))
                .into_any_element();
        }
        match self.emoji_picture(custom.guild_id, custom.id, cx) {
            Some(image) => img(image)
                .size(px(18.0))
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            None => div().size(px(18.0)).into_any_element(),
        }
    }

    /// The user's emoji for the picker, the open guild's first.
    pub(super) fn picker_emoji(&mut self, cx: &mut Context<Self>) -> Vec<CustomEmoji> {
        let mut usable = self.session.usable_emoji(self.guild.as_deref());
        usable.retain(|custom| !self.emoji_gone(&custom.id, cx));
        usable
            .into_iter()
            .map(|custom| {
                let guild = self
                    .session
                    .guild(&custom.guild_id)
                    .map(|view| view.guild.name)
                    .unwrap_or_default();
                CustomEmoji {
                    code: emoji_code(&custom),
                    image: self.emoji_picture(&custom.guild_id, &custom.id, cx),
                    name: custom.name,
                    guild: guild.into(),
                }
            })
            .collect()
    }

    /// Gives an open picker the pictures that arrived and the emoji that
    /// came or went.
    pub(super) fn refresh_picker(&mut self, cx: &mut Context<Self>) {
        let Some(view) = self.picker.as_ref().map(|picker| picker.view.clone()) else {
            return;
        };
        let custom = self.picker_emoji(cx);
        view.update(cx, |picker, cx| picker.set_custom(custom, cx));
    }

    /// Asks for an image for a new emoji.
    pub(super) fn pick_emoji_image(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(self.t().add_emoji.into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            if let Some(path) = paths.into_iter().next() {
                let _ = this.update_in(cx, |this, window, cx| {
                    this.choose_emoji_image(path, window, cx)
                });
            }
        })
        .detach();
    }

    /// Shows the image at `path` (picked or dropped) for a new emoji,
    /// waiting for its name (offered from the file's).
    pub(super) fn choose_emoji_image(
        &mut self,
        path: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.spawn_in(window, async move |this, cx| {
            let offered = name_from_file(&path);
            let read = cx
                .background_executor()
                .spawn(async move { read_emoji_image(&path) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                let t = this.t();
                let Some(Dialog::Emoji {
                    name, new, error, ..
                }) = &mut this.dialog
                else {
                    return;
                };
                match read {
                    Ok(image) => {
                        *new = Some(image);
                        *error = None;
                        name.update(cx, |field, cx| {
                            field.set_error(None, cx);
                            field.set_text(offered, window, cx)
                        });
                        window.focus(&name.focus_handle(cx), cx);
                    }
                    Err(unfit) => {
                        *error = Some(match unfit {
                            Unfit::TooLarge => t.emoji_limit.to_string(),
                            Unfit::NotImage => t.emoji_not_image.to_string(),
                            Unfit::Unreadable(reason) => reason,
                        })
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Puts the image chosen back.
    pub(super) fn discard_emoji_image(&mut self, cx: &mut Context<Self>) {
        if let Some(Dialog::Emoji { new, error, .. }) = &mut self.dialog {
            *new = None;
            *error = None;
        }
        cx.notify();
    }

    /// Adds the image chosen to the guild, under the name typed.
    pub(super) fn upload_emoji(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let t = self.t();
        let Some(Dialog::Emoji {
            guild_id,
            name,
            new: Some(new),
            busy: false,
            ..
        }) = &self.dialog
        else {
            return;
        };
        let typed = name.read(cx).text().trim().trim_matches(':').to_string();
        if !emoji::valid_name(&typed) {
            name.update(cx, |field, cx| {
                field.set_error(Some(t.emoji_name_hint.into()), cx)
            });
            return;
        }
        name.update(cx, |field, cx| field.set_error(None, cx));
        let (guild_id, bytes) = (guild_id.clone(), new.bytes.clone());
        self.set_emoji_status(true, None, cx);
        let session = self.session.clone();
        let task = runtime::spawn(cx, async move {
            session.create_emoji(&guild_id, &typed, bytes).await
        });
        cx.spawn_in(window, async move |this, cx| {
            let result = task.await;
            let _ = this.update_in(cx, |this, window, cx| match result {
                Ok(_) => {
                    if let Some(Dialog::Emoji { name, new, .. }) = &mut this.dialog {
                        *new = None;
                        name.update(cx, |field, cx| field.set_text("", window, cx));
                    }
                    this.set_emoji_status(false, None, cx);
                }
                Err(error) => {
                    let text = match error {
                        Error::Api(ApiError::LimitExceeded) => this.t().emoji_limit.to_string(),
                        error => describe(&error, this.t()),
                    };
                    this.set_emoji_status(false, Some(text), cx);
                }
            });
        })
        .detach();
    }

    /// The image chosen for a new emoji, beside its name.
    pub(super) fn new_emoji_view(
        image: Arc<Image>,
        name: gpui::Entity<gpui_md3e::components::TextField>,
        cx: &App,
    ) -> AnyElement {
        let colors = cx.md().colors;
        div()
            .flex()
            .flex_row()
            .items_start()
            .gap_4()
            .child(
                div()
                    .flex_none()
                    .size(px(72.0))
                    .rounded(px(12.0))
                    .bg(colors.surface_container_highest)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(img(image).size(px(56.0)).object_fit(ObjectFit::Contain)),
            )
            .child(div().flex_1().min_w_0().child(name))
            .into_any_element()
    }

    pub(super) fn delete_emoji(&mut self, guild_id: String, id: String, cx: &mut Context<Self>) {
        self.set_emoji_status(true, None, cx);
        let session = self.session.clone();
        let task = runtime::spawn(
            cx,
            async move { session.delete_emoji(&guild_id, &id).await },
        );
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let error = result.err().map(|error| describe(&error, this.t()));
                this.set_emoji_status(false, error, cx);
            });
        })
        .detach();
    }

    fn set_emoji_status(
        &mut self,
        now_busy: bool,
        failure: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if let Some(Dialog::Emoji { busy, error, .. }) = &mut self.dialog {
            *busy = now_busy;
            *error = failure;
        }
        cx.notify();
    }

    /// The guild's emoji, with their names; owners add and delete them.
    pub(super) fn emoji_list(
        &mut self,
        guild_id: &str,
        owner: bool,
        busy: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.md().colors;
        let t = self.t();
        let list = self
            .session
            .guild(guild_id)
            .map(|view| view.emoji)
            .unwrap_or_default();
        if list.is_empty() {
            return div()
                .md_type(TypeRole::BodyMedium, cx)
                .text_color(colors.on_surface_variant)
                .child(t.emoji_none)
                .into_any_element();
        }
        let mut column = div()
            .id("emoji-list")
            .max_h(px(240.0))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1();
        for custom in list {
            let face = match self.emoji_picture(&custom.guild_id, &custom.id, cx) {
                Some(image) => img(image)
                    .size(px(28.0))
                    .object_fit(ObjectFit::Contain)
                    .into_any_element(),
                None => div().size(px(28.0)).into_any_element(),
            };
            let mut row = div()
                .h(px(40.0))
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .child(face)
                .child(
                    div()
                        .flex_1()
                        .md_type(TypeRole::BodyLarge, cx)
                        .child(format!(":{}:", custom.name)),
                );
            if owner {
                let (guild_id, id) = (custom.guild_id.clone(), custom.id.clone());
                row = row.child(
                    IconButton::new(
                        SharedString::from(format!("delete-emoji-{}", custom.id)),
                        Icon::Delete,
                    )
                    .size(ButtonSize::XSmall)
                    .label(crate::i18n::fill(t.delete_emoji, &[&custom.name]))
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.delete_emoji(guild_id.clone(), id.clone(), cx)
                    })),
                );
            }
            column = column.child(row);
        }
        column.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_come_from_file_names() {
        assert_eq!(
            name_from_file(Path::new("/tmp/party-parrot.gif")),
            "party_parrot"
        );
        assert_eq!(name_from_file(Path::new("_cat_.png")), "cat");
        assert_eq!(name_from_file(Path::new("x.png")), "");
        assert_eq!(name_from_file(Path::new("お祝い.png")), "");
        assert!(image_format(b"GIF89a....").is_some());
        assert!(image_format(b"hello").is_none());
    }
}
