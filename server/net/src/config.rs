//! The cluster file: every service of one deployment and where it is
//! reached. All processes read the same file and find themselves in it by
//! name.
//!
//! ```toml
//! pki = "pki"                       # ca.crt, <name>.crt/.key, token.pub
//!
//! [account]
//! name = "account"
//! url = "https://10.8.0.2:7101"
//!
//! [directory]
//! name = "directory"
//! url = "https://10.8.0.2:7102"
//!
//! [[message_cell]]
//! name = "message-cell-0"
//! url = "https://10.8.0.3:7110"
//!
//! [[translation_cell]]
//! name = "translation-cell-0"
//! url = "https://10.8.0.4:7120"
//!
//! [[engine]]                        # optional; without one, cells use a mock
//! name = "engine-0"
//! url = "https://10.8.0.5:7140"
//!
//! [[edge]]                          # url: where clients reach it
//! name = "edge-0"
//! url = "http://203.0.113.10:7000"
//! listen = "0.0.0.0:7000"
//! ```
//!
//! `listen` defaults to the host and port of `url`. Relative paths are
//! relative to the cluster file.

use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use babel_common::{Identity, Role};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Service {
    pub name: String,
    pub url: String,
    pub listen: Option<String>,
}

impl Service {
    /// The address to bind: `listen`, or the host and port of `url`.
    pub fn listen_address(&self) -> anyhow::Result<String> {
        if let Some(listen) = &self.listen {
            return Ok(listen.clone());
        }
        let rest = self
            .url
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or(&self.url);
        let authority = rest.split('/').next().unwrap_or(rest);
        if !authority.contains(':') {
            bail!("{}: give the url a port, or set listen", self.name);
        }
        Ok(authority.to_string())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cluster {
    pub pki: PathBuf,
    pub account: Service,
    pub directory: Service,
    #[serde(default)]
    pub message_cell: Vec<Service>,
    #[serde(default)]
    pub translation_cell: Vec<Service>,
    #[serde(default)]
    pub engine: Vec<Service>,
    #[serde(default)]
    pub edge: Vec<Service>,
}

impl Cluster {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut cluster: Cluster =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        if cluster.pki.is_relative() {
            let base = path.parent().unwrap_or(Path::new("."));
            cluster.pki = base.join(&cluster.pki);
        }
        cluster.check()?;
        Ok(cluster)
    }

    fn check(&self) -> anyhow::Result<()> {
        let mut seen = std::collections::HashSet::new();
        for (_, service) in self.services() {
            if !valid_name(&service.name) {
                bail!(
                    "{:?}: names are lowercase letters, digits and dashes",
                    service.name
                );
            }
            if !seen.insert(service.name.as_str()) {
                bail!("{:?} is in the cluster file twice", service.name);
            }
        }
        if self.message_cell.is_empty() || self.translation_cell.is_empty() {
            bail!("a cluster needs a message_cell and a translation_cell");
        }
        Ok(())
    }

    /// Every service with its role.
    pub fn services(&self) -> impl Iterator<Item = (Role, &Service)> {
        [
            (Role::Account, &self.account),
            (Role::Directory, &self.directory),
        ]
        .into_iter()
        .chain(self.message_cell.iter().map(|s| (Role::MessageCell, s)))
        .chain(
            self.translation_cell
                .iter()
                .map(|s| (Role::TranslationCell, s)),
        )
        .chain(self.engine.iter().map(|s| (Role::Engine, s)))
        .chain(self.edge.iter().map(|s| (Role::Edge, s)))
    }

    /// The service called `name`, as an identity.
    pub fn find(&self, name: &str) -> Option<(Identity, &Service)> {
        self.services()
            .find(|(_, service)| service.name == name)
            .map(|(role, service)| {
                (
                    Identity {
                        role,
                        name: service.name.clone(),
                    },
                    service,
                )
            })
    }

    pub fn ca_path(&self) -> PathBuf {
        self.pki.join("ca.crt")
    }

    pub fn cert_path(&self, name: &str) -> PathBuf {
        self.pki.join(format!("{name}.crt"))
    }

    pub fn key_path(&self, name: &str) -> PathBuf {
        self.pki.join(format!("{name}.key"))
    }

    /// The public key Edges verify user tokens with.
    pub fn token_public_key_path(&self) -> PathBuf {
        self.pki.join("token.pub")
    }

    /// The private key the Account service signs user tokens with.
    pub fn token_private_key_path(&self) -> PathBuf {
        self.pki.join("token.key")
    }
}

/// Names end up in certificates and file names.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 63
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_cluster_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cluster.toml");
        std::fs::write(
            &path,
            r#"
            pki = "pki"
            [account]
            name = "account"
            url = "https://127.0.0.1:7101"
            [directory]
            name = "directory"
            url = "https://127.0.0.1:7102"
            [[message_cell]]
            name = "message-cell-0"
            url = "https://127.0.0.1:7110"
            listen = "0.0.0.0:7110"
            [[translation_cell]]
            name = "translation-cell-0"
            url = "https://127.0.0.1:7120"
            [[edge]]
            name = "edge-0"
            url = "http://127.0.0.1:7000/"
            "#,
        )
        .unwrap();
        let cluster = Cluster::load(&path).unwrap();
        assert_eq!(cluster.pki, dir.path().join("pki"));
        let (identity, service) = cluster.find("message-cell-0").unwrap();
        assert_eq!(identity.role, Role::MessageCell);
        assert_eq!(service.listen_address().unwrap(), "0.0.0.0:7110");
        let (_, edge) = cluster.find("edge-0").unwrap();
        assert_eq!(edge.listen_address().unwrap(), "127.0.0.1:7000");
        assert!(cluster.find("edge-1").is_none());

        std::fs::write(
            &path,
            std::fs::read_to_string(&path)
                .unwrap()
                .replace("edge-0", "account"),
        )
        .unwrap();
        assert!(Cluster::load(&path).is_err());
    }
}
