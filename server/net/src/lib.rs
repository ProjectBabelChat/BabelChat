//! How BabelChat services reach each other when they run apart.
//!
//! ```text
//! cluster.toml ──▶ Node ──┬─▶ Remote* clients   (the API traits, over HTTPS)
//!                         └─▶ serve()           (the generated router, over HTTPS)
//! pki/ ──────────▶ Credentials: mutual TLS, a certificate per service
//! ```
//!
//! - [`config`]: the cluster file every process reads.
//! - [`tls`]: certificates that name services, and checking them both ways.
//! - [`serve`]: serving an internal API to the services its [`Policy`] allows;
//!   [`policies`] says who may call what.
//! - [`remote`]: the API traits implemented by HTTP clients, so a service
//!   cannot tell an in-process peer from a remote one.
//! - [`pki`]: the cluster's CA and the user token key pair (`babel-pki`).
//! - [`node`]: what every service binary does on start.
//! - [`egress`]: the proxy outbound requests leave through (`babel-egress`).
//! - [`replication`], [`backup`]: standbys and backups of any service's
//!   store (`babel-standby`, `babel-admin restore`).
//!
//! See docs/deployment.md.

pub mod backup;
pub mod config;
pub mod egress;
pub mod node;
pub mod pki;
pub mod policies;
pub mod remote;
pub mod replication;
pub mod serve;
pub mod tls;

pub use node::{Node, NodeArgs, StoreArgs, init_tracing};
pub use serve::Policy;
