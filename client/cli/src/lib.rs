//! BabelChat in a terminal: a line-mode client on top of the SDK, for
//! chatting before the desktop app exists and for poking at servers.
//!
//! On-device translation uses the Firefox Translations (Bergamot) models
//! through `babel-bergamot`, fetched on first use; `--device mock` swaps in
//! the SDK's mock, which shows device translations as `[device fr-FR] …`.

pub use babel_sdk::profile;
pub mod repl;
