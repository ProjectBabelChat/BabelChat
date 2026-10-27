//! What every service binary does on start: read the cluster file, find
//! itself in it, load its certificate, and connect to the services it
//! calls.

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::{Context, bail};
use babel_api_account::types::Account;
use babel_api_directory::types::Directory;
use babel_api_message_cell::types::MessageCell;
use babel_api_translation_cell::types::TranslationCell;
use babel_common::{Identity, Role};
use babel_store::RocksStore;

use crate::{
    config::{Cluster, Service},
    remote::{RemoteAccount, RemoteDirectory, RemoteMessageCell, RemoteTranslationCell},
    serve::{Policy, serve},
    tls::Credentials,
};

/// Command-line options shared by every service binary.
#[derive(Debug, Clone, clap::Args)]
pub struct NodeArgs {
    /// The cluster file listing every service.
    #[arg(long, env = "BABEL_CLUSTER", default_value = "cluster.toml")]
    pub cluster: PathBuf,
    /// This service's name in the cluster file.
    #[arg(long, env = "BABEL_NAME")]
    pub name: String,
    /// Where this service's certificates are, instead of the cluster file's
    /// `pki` (systemd's credentials directory, for instance).
    #[arg(long, env = "BABEL_PKI")]
    pub pki: Option<PathBuf>,
}

/// Command-line options of services that keep data.
#[derive(Debug, Clone, clap::Args)]
pub struct StoreArgs {
    /// Data directory [default: data/<name>]
    #[arg(long)]
    pub data: Option<PathBuf>,
    /// Back the store up into this directory (preferably on another disk).
    #[arg(long)]
    pub backup_dir: Option<PathBuf>,
    /// Minutes between backups.
    #[arg(long, default_value_t = 15)]
    pub backup_every_mins: u64,
    /// Backups kept.
    #[arg(long, default_value_t = 96)]
    pub backup_keep: usize,
}

pub struct Node {
    pub cluster: Cluster,
    pub identity: Identity,
    pub service: Service,
    pub credentials: Credentials,
}

impl Node {
    /// Loads the cluster file and this service's certificate. Fails unless
    /// the cluster lists `args.name` as a `role`.
    pub fn load(args: &NodeArgs, role: Role) -> anyhow::Result<Self> {
        let node = Self::load_any(args)?;
        if node.identity.role != role {
            bail!("{} is a {}, not a {role}", args.name, node.identity.role);
        }
        Ok(node)
    }

    /// Loads whichever service the cluster lists as `args.name` (a standby
    /// copies any of them).
    pub fn load_any(args: &NodeArgs) -> anyhow::Result<Self> {
        let mut cluster = Cluster::load(&args.cluster)?;
        if let Some(pki) = &args.pki {
            cluster.pki = pki.clone();
        }
        let Some((identity, service)) = cluster.find(&args.name) else {
            bail!(
                "{} lists no service {:?}",
                args.cluster.display(),
                args.name
            );
        };
        let service = service.clone();
        let credentials = Credentials::load(
            &cluster.ca_path(),
            &cluster.cert_path(&identity.name),
            &cluster.key_path(&identity.name),
        )?;
        credentials.expect(&identity)?;
        Ok(Self {
            cluster,
            identity,
            service,
            credentials,
        })
    }

    /// An HTTP client that calls `peer` over mutual TLS.
    pub fn http_client(&self, peer: &Identity) -> anyhow::Result<reqwest::Client> {
        self.credentials.http_client(peer)
    }

    fn peer(
        &self,
        role: Role,
        service: &Service,
    ) -> anyhow::Result<(Identity, String, reqwest::Client)> {
        let identity = Identity {
            role,
            name: service.name.clone(),
        };
        let client = self.http_client(&identity)?;
        Ok((identity, service.url.clone(), client))
    }

    pub fn account(&self) -> anyhow::Result<Arc<dyn Account>> {
        let (peer, url, client) = self.peer(Role::Account, &self.cluster.account)?;
        Ok(Arc::new(RemoteAccount::new(peer, url, client)))
    }

    pub fn directory(&self) -> anyhow::Result<Arc<dyn Directory>> {
        let (peer, url, client) = self.peer(Role::Directory, &self.cluster.directory)?;
        Ok(Arc::new(RemoteDirectory::new(peer, url, client)))
    }

    /// Every Message Cell, by name, in cluster file order.
    pub fn message_cells(&self) -> anyhow::Result<Vec<(String, Arc<dyn MessageCell>)>> {
        self.cluster
            .message_cell
            .iter()
            .map(|service| {
                let (peer, url, client) = self.peer(Role::MessageCell, service)?;
                Ok((
                    service.name.clone(),
                    Arc::new(RemoteMessageCell::new(peer, url, client)) as Arc<dyn MessageCell>,
                ))
            })
            .collect()
    }

    /// Every Translation Cell, by name.
    pub fn translation_cells(&self) -> anyhow::Result<HashMap<String, Arc<dyn TranslationCell>>> {
        self.cluster
            .translation_cell
            .iter()
            .map(|service| {
                let (peer, url, client) = self.peer(Role::TranslationCell, service)?;
                Ok((
                    service.name.clone(),
                    Arc::new(RemoteTranslationCell::new(peer, url, client))
                        as Arc<dyn TranslationCell>,
                ))
            })
            .collect()
    }

    /// Opens this service's store with `tables`, and backs it up if asked.
    pub fn open_store(
        &self,
        args: &StoreArgs,
        tables: &[babel_store::Table],
    ) -> anyhow::Result<Arc<RocksStore>> {
        let data = args
            .data
            .clone()
            .unwrap_or_else(|| PathBuf::from("data").join(&self.identity.name));
        std::fs::create_dir_all(&data)?;
        let store = Arc::new(
            RocksStore::open(&data, tables)
                .with_context(|| format!("opening {}", data.display()))?,
        );
        if let Some(dir) = &args.backup_dir {
            crate::backup::schedule(
                store.clone(),
                dir.clone(),
                std::time::Duration::from_secs(args.backup_every_mins.max(1) * 60),
                args.backup_keep.max(1),
            );
        }
        Ok(store)
    }

    /// Serves `router` like [`Self::serve`], with the replication routes for
    /// standbys of `store` next to it.
    pub async fn serve_replicated(
        &self,
        router: axum::Router,
        policy: Policy,
        store: Arc<RocksStore>,
    ) -> anyhow::Result<()> {
        self.serve(
            router.merge(crate::replication::router(store)),
            policy.with_replication(&self.identity),
        )
        .await
    }

    /// Serves `router` over mutual TLS at this service's address until the
    /// process ends.
    pub async fn serve(&self, router: axum::Router, policy: Policy) -> anyhow::Result<()> {
        let address = self.service.listen_address()?;
        let listener = tokio::net::TcpListener::bind(&address)
            .await
            .with_context(|| format!("listening on {address}"))?;
        tracing::info!(service = %self.identity, %address, "serving");
        serve(router, listener, &self.credentials, policy).await
    }
}

/// Logs to stderr, `info` unless `RUST_LOG` says otherwise. With
/// `BABEL_LOG_FORMAT=json`, one JSON object per line (for journald and log
/// collectors) instead of text.
pub fn init_tracing() {
    use std::io::IsTerminal;

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal());
    if std::env::var("BABEL_LOG_FORMAT").is_ok_and(|format| format == "json") {
        builder.json().flatten_event(true).init();
    } else {
        builder.init();
    }
}
