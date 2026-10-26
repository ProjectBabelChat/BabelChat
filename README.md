# BabelChat

Chat without language barriers: everyone writes and reads in their own
language, with zero-click translation.

This repository holds the open-source parts of the project.

| Path | What |
|---|---|
| [`docs/`](docs) | Architecture and development documentation |
| [`server/`](server) | The servers: Edge, Directory, Message and Translation Cells, translation engine |
| [`client/`](client) | The client SDK, on-device translation models (`bergamot`) and `babel`, a terminal chat client |
| [`types/`](types) | API definitions in [TYML](https://tyml-org.github.io/tyml-lang.org/), one file per boundary |
| [`crates/babel-lang`](crates/babel-lang) | Language tag normalization, shared by clients and servers |
| [`gpui-reloaded/`](gpui-reloaded) | Fork of Zed's GPUI UI framework |
| [`gpui-md3e/`](gpui-md3e) | Material Design 3 Expressive components for GPUI, with a gallery app |

Start with the [architecture overview](docs/architecture/overview.md).

## Building

The workspace builds with plain `cargo` (Rust 1.98+, see `rust-toolchain.toml`)
on Linux, macOS and Windows. On Nix, `nix develop` provides the toolchain, the
Linux libraries GPUI loads at runtime (Wayland, X11, Vulkan, fontconfig) and
libclang for RocksDB.

```sh
nix develop
cargo run -p md3e-gallery                        # the component gallery
cargo run -p babel-server -- --plan paid3        # every server in one process
cargo run -p babel-cli -- register alice         # then `cargo run -p babel-cli` to chat
```

On other Linux distributions install the development packages for
`libxkbcommon`, `wayland`, `vulkan-loader`, `fontconfig`, `freetype` and
`libxcb`, and clang for the servers. See [development](docs/development.md).
