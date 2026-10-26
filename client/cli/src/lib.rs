//! BabelChat in a terminal: a line-mode client on top of the SDK, for
//! chatting before the desktop app exists and for poking at servers.
//!
//! On-device translation uses the SDK's mock until real models are wired
//! in, so device translations show up as `[device fr-FR] …`.

pub mod profile;
pub mod repl;
