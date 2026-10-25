# BabelChat

Chat without language barriers: everyone writes and reads in their own
language, with zero-click translation.

This repository holds the open-source parts of the project.

| Path | What |
|---|---|
| [`gpui-reloaded/`](gpui-reloaded) | Fork of Zed's GPUI UI framework |
| [`gpui-md3e/`](gpui-md3e) | Material Design 3 Expressive components for GPUI, with a gallery app |
| [`types/`](types) | API definitions in [TYML](https://tyml-org.github.io/tyml-lang.org/) |

## Building

The workspace builds with plain `cargo` (Rust 1.98+, see `rust-toolchain.toml`)
on Linux, macOS and Windows. On Nix, `nix develop` provides the toolchain and
the Linux libraries GPUI loads at runtime (Wayland, X11, Vulkan, fontconfig).

```sh
nix develop
cargo run -p md3e-gallery
```

On other Linux distributions install the development packages for
`libxkbcommon`, `wayland`, `vulkan-loader`, `fontconfig`, `freetype` and
`libxcb`.
