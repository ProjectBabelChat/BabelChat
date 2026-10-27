# gpui-reloaded

A fork of [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui),
the GPU-accelerated UI framework behind the Zed editor, maintained for
BabelChat and [gpui-md3e](../gpui-md3e).

Upstream GPUI is developed inside the Zed monorepo, is not published on
crates.io in its current form and rarely takes changes that Zed itself does
not need. This directory holds the GPUI crates plus the few Zed crates they
depend on, so the workspace builds with plain `cargo` on every platform.

## Upstream

- Imported from `zed-industries/zed@347f171a3fd00a3b672382d78348b37bd969d17c`
  (2026-10-24) with [`scripts/import_upstream.py`](scripts/import_upstream.py).
- The script copies the crate closure of `gpui_platform`, skipping Zed's HTTP
  stack (`reqwest_client`, `http_client_tls`), and prints the
  `[workspace.dependencies]` entries for the root `Cargo.toml`. The gpui
  dev-dependency on `reqwest_client` and the two examples that used it are
  removed by hand.
- Every crate here is Apache-2.0, as upstream.

To update, run the script against a newer Zed checkout and re-apply the
patches below.

## Patches

| Area | Change |
|---|---|
| Text (Linux) | Variable fonts render at the requested weight. Upstream loads every face at `Weight::NORMAL` and rasterizes the default instance, so Noto Sans CJK VF (NixOS, Arch) drew all Japanese text in *Thin*. Instances are now created per `(face, weight)` and rasterized with the `wght` axis set (`gpui_wgpu/src/cosmic_text_system.rs`). |
| Frame timing | `Window::frame_number()` counts drawn frames and `Window::frame_timestamp()` exposes the compositor's frame-callback time (Wayland; `None` elsewhere), so animations can advance on the display's vblank grid instead of the jittery render time (`gpui/src/window.rs`, `gpui/src/platform.rs`, `gpui_linux/src/linux/wayland`). |
| Paths | `LineCap` and `LineJoin` are re-exported next to `StrokeOptions` so round-capped strokes can be built (`gpui/src/path_builder.rs`). |
| Drag and drop (text) | Text crosses application boundaries. `ExternalDragPayload::Text` lets an in-app drag leave the window as text (Wayland offers `text/plain;charset=utf-8`, `UTF8_STRING` and `text/plain`, copy only), and `FileDropEvent::TextEntered` brings text dragged in from other programs as an `ExternalText` drag value; upstream accepts only `text/uri-list` and drops text offers on purpose. Wayland only: macOS declines text payloads and stays in-app (`gpui/src/interactive.rs`, `gpui/src/window.rs`, `gpui_linux/src/linux/wayland`, `gpui_macos/src/window.rs`). |
| Lints | `phi()` allows `clippy::approx_constant` so newer toolchains lint cleanly (`gpui/src/geometry.rs`). |
| Examples | The `reqwest_client` dev-dependency and the `image` and `image_gallery` examples that need it are removed (`gpui/Cargo.toml`). |

## Planned

- Letter spacing in `TextStyle` (Material type tokens specify tracking).
- Element transforms (scale and rotation of whole subtrees).
- Rounded clipping masks for children.
- Japanese line breaking (kinsoku) in the line wrapper.
- Wayland `text-input-v3` preedit cursor ranges for fcitx5/Mozc.
