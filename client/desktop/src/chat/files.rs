//! Files: attached in the composer (button, drag and drop, or a picture
//! pasted), uploaded at once, sent with the next message; shown under messages, images as
//! pictures (opened large on click), anything else as a card to save.

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use babel_sdk::{ApiError, Attachment, Error, MAX_UPLOAD_BYTES};
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, ExternalPaths, Image, ImageFormat, IntoElement, ObjectFit,
    PathPromptOptions, SharedString, Window, div, img, px,
};
use gpui_md3e::prelude::*;

use super::ChatView;
use crate::{
    i18n::fill,
    runtime::{self, describe},
};

/// The largest a picture is shown in a message.
const PICTURE_WIDTH: f32 = 360.0;
const PICTURE_HEIGHT: f32 = 280.0;

/// A file attached in the composer.
pub(super) struct Upload {
    key: u64,
    channel_id: String,
    name: String,
    state: UploadState,
    /// The message on its way that took it (`pending.rs`), if any.
    message: Option<u64>,
}

enum UploadState {
    Uploading,
    Ready(Attachment),
}

/// A picture under a message, as far as it has come.
pub(super) enum Picture {
    Loading,
    Ready(Arc<Image>),
    Failed,
}

/// Files in the composer and pictures fetched, kept by the chat view.
#[derive(Default)]
pub(super) struct Files {
    uploads: Vec<Upload>,
    next_key: u64,
    pictures: HashMap<String, Picture>,
    /// A picture opened large.
    pub viewer: Option<Attachment>,
}

fn image_format(content_type: &str) -> Option<ImageFormat> {
    match content_type {
        "image/png" => Some(ImageFormat::Png),
        "image/jpeg" => Some(ImageFormat::Jpeg),
        "image/gif" => Some(ImageFormat::Gif),
        "image/webp" => Some(ImageFormat::Webp),
        _ => None,
    }
}

fn picture_extension(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Webp => "webp",
        ImageFormat::Gif => "gif",
        ImageFormat::Svg => "svg",
        ImageFormat::Bmp => "bmp",
        ImageFormat::Tiff => "tiff",
        ImageFormat::Ico => "ico",
        ImageFormat::Pnm => "pnm",
    }
}

/// Shown as a picture: an image the server measured.
fn is_picture(file: &Attachment) -> bool {
    image_format(&file.content_type).is_some() && file.width.is_some() && file.height.is_some()
}

/// The size a picture is shown at in a message: as it is, or smaller to
/// fit, never larger.
fn picture_size(file: &Attachment, max_width: f32, max_height: f32) -> (f32, f32) {
    let (width, height) = (
        file.width.unwrap_or(1).max(1) as f32,
        file.height.unwrap_or(1).max(1) as f32,
    );
    let scale = (max_width / width).min(max_height / height).min(1.0);
    ((width * scale).max(24.0), (height * scale).max(24.0))
}

pub(super) fn size_text(bytes: i64) -> String {
    match bytes {
        bytes if bytes >= 1024 * 1024 => format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)),
        bytes if bytes >= 1024 => format!("{} KB", bytes / 1024),
        bytes => format!("{bytes} B"),
    }
}

/// Where downloads go first: the user's Downloads folder, else home.
fn downloads_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let downloads = home.join("Downloads");
    if downloads.is_dir() { downloads } else { home }
}

impl ChatView {
    /// Asks for files to attach.
    pub(super) fn pick_files(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(self.t().attach_files.into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await {
                let _ = this.update(cx, |this, cx| this.attach_paths(&paths, cx));
            }
        })
        .detach();
    }

    pub(super) fn on_drop_files(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        self.attach_paths(paths.paths(), cx);
    }

    /// Uploads files for the next message in the open channel.
    fn attach_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        for path in paths {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if path.is_dir() || name.is_empty() {
                continue;
            }
            if std::fs::metadata(path).is_ok_and(|meta| meta.len() as i64 > MAX_UPLOAD_BYTES) {
                let text = fill(self.t().too_large, &[&name]);
                self.show_toast(text, None, cx);
                continue;
            }
            let path = path.clone();
            self.attach(name, move || std::fs::read(path), cx);
        }
    }

    /// Attaches a picture pasted into the composer, as Discord does
    /// (`image.png`).
    pub(super) fn attach_picture(&mut self, picture: &Image, cx: &mut Context<Self>) {
        let name = format!("image.{}", picture_extension(picture.format));
        if picture.bytes.len() as i64 > MAX_UPLOAD_BYTES {
            let text = fill(self.t().too_large, &[&name]);
            self.show_toast(text, None, cx);
            return;
        }
        let bytes = picture.bytes.clone();
        self.attach(name, move || Ok(bytes), cx);
    }

    /// Uploads the file `read` gives, as `name`, for the next message in
    /// the open channel.
    fn attach(
        &mut self,
        name: String,
        read: impl FnOnce() -> std::io::Result<Vec<u8>> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(channel) = self.channel.clone() else {
            return;
        };
        let key = self.files.next_key;
        self.files.next_key += 1;
        self.files.uploads.push(Upload {
            key,
            channel_id: channel.clone(),
            name: name.clone(),
            state: UploadState::Uploading,
            message: None,
        });
        let session = self.session.clone();
        let task = runtime::spawn(cx, async move {
            let read = tokio::task::spawn_blocking(read).await;
            match read {
                Ok(Ok(bytes)) => session.upload(&channel, &name, bytes).await,
                Ok(Err(error)) => Err(Error::Transport(error.to_string())),
                Err(error) => Err(Error::Transport(error.to_string())),
            }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                let Some(index) = this.files.uploads.iter().position(|up| up.key == key) else {
                    return;
                };
                match result {
                    Ok(file) => this.files.uploads[index].state = UploadState::Ready(file),
                    Err(error) => {
                        this.files.uploads.remove(index);
                        let text = describe(&error, this.t());
                        this.show_toast(text, None, cx);
                    }
                }
                // A message may have been waiting for it.
                this.send_pending(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Hands the composer's files in `channel` to `message`; returns how
    /// many there were.
    pub(super) fn take_uploads(&mut self, channel: &str, message: u64) -> usize {
        let mut taken = 0;
        for upload in &mut self.files.uploads {
            if upload.channel_id == channel && upload.message.is_none() {
                upload.message = Some(message);
                taken += 1;
            }
        }
        taken
    }

    /// The files of `message`, or `None` while some are still uploading.
    pub(super) fn message_files(&self, message: u64) -> Option<Vec<Attachment>> {
        let mut ready = Vec::new();
        for upload in self.message_uploads(message) {
            match &upload.state {
                UploadState::Uploading => return None,
                UploadState::Ready(file) => ready.push(file.clone()),
            }
        }
        Some(ready)
    }

    /// The names of `message`'s files, and whether each is still uploading.
    pub(super) fn message_file_names(&self, message: u64) -> Vec<(String, bool)> {
        self.message_uploads(message)
            .map(|upload| {
                let uploading = matches!(upload.state, UploadState::Uploading);
                (upload.name.clone(), uploading)
            })
            .collect()
    }

    fn message_uploads(&self, message: u64) -> impl Iterator<Item = &Upload> {
        self.files
            .uploads
            .iter()
            .filter(move |upload| upload.message == Some(message))
    }

    /// Forgets the files of a message sent or thrown away.
    pub(super) fn forget_uploads(&mut self, message: u64) {
        self.files
            .uploads
            .retain(|upload| upload.message != Some(message));
    }

    /// The composer's attached files, if any.
    pub(super) fn render_uploads(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let channel = self.channel.as_deref()?;
        let colors = cx.md().colors;
        let t = self.t();
        let uploads = self
            .files
            .uploads
            .iter()
            .filter(|upload| upload.channel_id == channel && upload.message.is_none())
            .collect::<Vec<_>>();
        if uploads.is_empty() {
            return None;
        }
        let mut row = div().flex().flex_row().flex_wrap().gap_2().pb_2();
        for upload in uploads {
            let key = upload.key;
            let detail = match &upload.state {
                UploadState::Uploading => t.uploading.to_string(),
                UploadState::Ready(file) => size_text(file.size),
            };
            row = row.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .pl_3()
                    .pr_1()
                    .py_1()
                    .max_w(px(280.0))
                    .rounded(px(12.0))
                    .bg(colors.surface_container_high)
                    .child(
                        icon(Icon::AttachFile)
                            .size(px(18.0))
                            .color(colors.on_surface_variant),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(
                                div()
                                    .md_type(TypeRole::LabelLarge, cx)
                                    .truncate()
                                    .child(upload.name.clone()),
                            )
                            .child(
                                div()
                                    .md_type(TypeRole::LabelSmall, cx)
                                    .text_color(colors.on_surface_variant)
                                    .child(detail),
                            ),
                    )
                    .child(
                        IconButton::new(
                            SharedString::from(format!("remove-upload-{key}")),
                            Icon::Close,
                        )
                        .size(ButtonSize::XSmall)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.files.uploads.retain(|upload| upload.key != key);
                            cx.notify();
                        })),
                    ),
            );
        }
        Some(row.into_any_element())
    }

    /// A picture's image, fetched the first time it is asked for.
    fn picture(&mut self, file: &Attachment, cx: &mut Context<Self>) -> Option<Arc<Image>> {
        match self.files.pictures.get(&file.id) {
            Some(Picture::Ready(image)) => return Some(image.clone()),
            Some(_) => return None,
            None => {}
        }
        self.files
            .pictures
            .insert(file.id.clone(), Picture::Loading);
        let format = image_format(&file.content_type)?;
        let (session, file) = (self.session.clone(), file.clone());
        let id = file.id.clone();
        let task = runtime::spawn(cx, async move { session.download(&file).await });
        cx.spawn(async move |this, cx| {
            let picture = match task.await {
                Ok(bytes) => Picture::Ready(Arc::new(Image::from_bytes(format, bytes.to_vec()))),
                Err(_) => Picture::Failed,
            };
            let _ = this.update(cx, |this, cx| {
                this.files.pictures.insert(id, picture);
                cx.notify();
            });
        })
        .detach();
        None
    }

    /// A message's files, under its text.
    pub(super) fn render_attachments(
        &mut self,
        message_id: &str,
        files: &[Attachment],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let colors = cx.md().colors;
        let mut column = div()
            .w_full()
            .flex()
            .flex_col()
            .items_start()
            .gap_2()
            .pt_1();
        for (n, file) in files.iter().enumerate() {
            let id = SharedString::from(format!("file-{message_id}-{n}"));
            if is_picture(file) {
                let (width, height) = picture_size(file, PICTURE_WIDTH, PICTURE_HEIGHT);
                let failed = matches!(self.files.pictures.get(&file.id), Some(Picture::Failed));
                let mut frame = div()
                    .id(id)
                    .w(px(width))
                    .max_w_full()
                    .h(px(height))
                    .rounded(px(12.0))
                    .overflow_hidden()
                    .bg(colors.surface_container_high)
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer();
                frame = match self.picture(file, cx) {
                    Some(image) => frame.child(img(image).size_full().object_fit(ObjectFit::Cover)),
                    None => frame.child(
                        icon(if failed { Icon::Error } else { Icon::Image })
                            .color(colors.on_surface_variant),
                    ),
                };
                let opened = file.clone();
                column = column.child(frame.on_click(cx.listener(move |this, _, _, cx| {
                    this.files.viewer = Some(opened.clone());
                    cx.notify();
                })));
            } else {
                let saved = file.clone();
                column =
                    column.child(
                        div()
                            .id(id)
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_3()
                            .pl_3()
                            .pr_1()
                            .py_2()
                            .w_full()
                            .max_w(px(320.0))
                            .rounded(px(12.0))
                            .border_1()
                            .border_color(colors.outline_variant)
                            .bg(colors.surface_container_low)
                            .child(icon(Icon::AttachFile).color(colors.primary))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(
                                        div()
                                            .md_type(TypeRole::TitleSmall, cx)
                                            .truncate()
                                            .child(file.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .md_type(TypeRole::LabelSmall, cx)
                                            .text_color(colors.on_surface_variant)
                                            .child(size_text(file.size)),
                                    ),
                            )
                            .child(
                                IconButton::new(
                                    SharedString::from(format!("save-{message_id}-{n}")),
                                    Icon::Download,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| this.save_file(saved.clone(), cx),
                                )),
                            ),
                    );
            }
        }
        column.into_any_element()
    }

    /// Asks where to save a file, then downloads it there.
    pub(super) fn save_file(&mut self, file: Attachment, cx: &mut Context<Self>) {
        let chosen = cx.prompt_for_new_path(&downloads_dir(), Some(&file.name));
        let session = self.session.clone();
        let tokio = cx.global::<runtime::Tokio>().0.clone();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(path))) = chosen.await else {
                return;
            };
            let target = path.clone();
            let task = tokio.spawn(async move {
                let bytes = session.download(&file).await?;
                tokio::task::spawn_blocking(move || std::fs::write(&target, &bytes))
                    .await
                    .map_err(|error| Error::Transport(error.to_string()))?
                    .map_err(|error| Error::Transport(error.to_string()))
            });
            let result = task
                .await
                .unwrap_or_else(|error| Err(Error::Transport(error.to_string())));
            let _ = this.update(cx, |this, cx| {
                let text = match result {
                    Ok(()) => fill(this.t().saved_to, &[&path.display().to_string()]),
                    Err(Error::Api(ApiError::NotFound)) => this.t().file_unavailable.to_string(),
                    Err(error) => describe(&error, this.t()),
                };
                this.show_toast(text, None, cx);
            });
        })
        .detach();
    }

    /// A picture opened large, over everything; a click or Escape closes it.
    pub(super) fn render_viewer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let file = self.files.viewer.clone()?;
        let colors = cx.md().colors;
        let t = self.t();
        let viewport = window.viewport_size();
        let (width, height) = picture_size(
            &file,
            (f32::from(viewport.width) - 96.0).max(64.0),
            (f32::from(viewport.height) - 160.0).max(64.0),
        );
        let picture = match self.picture(&file, cx) {
            Some(image) => img(image)
                .w(px(width))
                .h(px(height))
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            None => div()
                .w(px(width))
                .h(px(height))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    icon(Icon::Image)
                        .size(px(48.0))
                        .color(colors.on_surface_variant),
                )
                .into_any_element(),
        };
        let saved = file.clone();
        Some(
            div()
                .id("viewer")
                .occlude()
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .bg(gpui::black().opacity(0.85))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.files.viewer = None;
                    cx.notify();
                }))
                .child(picture)
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .text_color(gpui::white())
                        .child(
                            div()
                                .md_type(TypeRole::TitleSmall, cx)
                                .max_w(px(480.0))
                                .truncate()
                                .child(file.name.clone()),
                        )
                        .child(
                            div()
                                .md_type(TypeRole::LabelMedium, cx)
                                .child(size_text(file.size)),
                        )
                        .child(
                            Button::new("viewer-save", t.save_file)
                                .style(ButtonStyle::Tonal)
                                .icon(Icon::Download)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.save_file(saved.clone(), cx)
                                })),
                        ),
                )
                .into_any_element(),
        )
    }
}
