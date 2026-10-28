//! Phase 0: every BabelChat service in one process.
//!
//! The services are wired exactly as they would be when split up — each one
//! only sees the others through their API traits (types/*.tyml) — and each
//! keeps its own data directory:
//!
//! ```text
//! <data>/account               Account (development stub)
//! <data>/directory             Directory
//! <data>/message-cell-<n>      Message Cells
//! <data>/translation-cell-<n>  Translation Cells
//! ```
//!
//! Splitting a service out later means putting an HTTP client behind the same
//! trait; nothing else changes. This is also how BabelChat can be self-hosted
//! in one piece.
//!
//! See docs/architecture/overview.md.

use std::{collections::HashMap, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use anyhow::Context;
use babel_account::Account;
use babel_api_engine::types::Engine;
use babel_api_message_cell::types::MessageCell as MessageCellApi;
use babel_api_translation_cell::types::{Plan, TranslationCell as TranslationCellApi};
use babel_directory::Directory;
use babel_edge::{Cluster, Edge, EdgeConfig};
use babel_engine::{RemoteEngine, mock::MockConfig};
use babel_message_cell::MessageCell;
use babel_store::{KvStore, RocksStore, Table};
use babel_translation_cell::{TranslationCell, TranslationCellConfig};
use jsonwebtoken::DecodingKey;

#[derive(Debug, Clone)]
pub enum EngineChoice {
    /// The mock engine, in-process.
    Mock { latency: Duration },
    /// An Engine API server elsewhere (`babel-engine`, BabelChat Cloud).
    Remote { url: String },
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub data_dir: PathBuf,
    pub message_cells: usize,
    pub translation_cells: usize,
    pub engine: EngineChoice,
    pub default_plan: Plan,
    pub daily_budget: Option<i64>,
}

/// The assembled services, before the Edge is served.
pub struct Server {
    pub edge: Edge,
}

fn open_store(
    config: &ServerConfig,
    name: &str,
    tables: &[Table],
) -> anyhow::Result<Arc<dyn KvStore>> {
    let path = config.data_dir.join(name);
    std::fs::create_dir_all(&path)?;
    let store =
        RocksStore::open(&path, tables).with_context(|| format!("opening {}", path.display()))?;
    Ok(Arc::new(store))
}

impl Server {
    /// Opens every service. Must be called within a Tokio runtime.
    pub fn open(config: &ServerConfig) -> anyhow::Result<Self> {
        let engine: Arc<dyn Engine> = match &config.engine {
            EngineChoice::Mock { latency } => Arc::new(babel_engine::Engine::mock(MockConfig {
                latency: *latency,
                ..MockConfig::default()
            })),
            EngineChoice::Remote { url } => Arc::new(RemoteEngine::new(url.clone())),
        };

        let mut translation_cells = HashMap::<String, Arc<dyn TranslationCellApi>>::new();
        for n in 0..config.translation_cells.max(1) {
            let name = format!("translation-cell-{n}");
            let store = open_store(config, &name, babel_translation_cell::TABLES)?;
            let cell = TranslationCell::open(
                store,
                engine.clone(),
                TranslationCellConfig {
                    default_plan: config.default_plan,
                    daily_budget: config.daily_budget,
                    ..TranslationCellConfig::default()
                },
            )?;
            translation_cells.insert(name, Arc::new(cell));
        }

        let mut message_cells = Vec::new();
        let mut cell_handles = Vec::new();
        for n in 0..config.message_cells.max(1) {
            let name = format!("message-cell-{n}");
            let store = open_store(config, &name, babel_message_cell::TABLES)?;
            let cell = MessageCell::open(store, translation_cells.clone())?;
            cell_handles.push(cell.clone());
            message_cells.push((name, Arc::new(cell) as Arc<dyn MessageCellApi>));
        }

        let directory = Arc::new(Directory::open(
            open_store(config, "directory", babel_directory::TABLES)?,
            message_cells.clone(),
            translation_cells.keys().cloned().collect(),
        )?);
        for cell in &cell_handles {
            cell.connect_directory(directory.clone());
        }

        let account = Account::open(open_store(config, "account", babel_account::TABLES)?, None)?;
        let token_key = DecodingKey::from_ed_der(account.token_public_key());

        let edge = Edge::start(
            Cluster {
                account: Arc::new(account),
                directory,
                message_cells: message_cells.into_iter().collect(),
                translation_cells,
            },
            EdgeConfig {
                token_keys: vec![token_key],
                writes_per_second: 5.0,
                write_burst: 20.0,
            },
        );

        Ok(Self { edge })
    }

    /// Serves the public Edge API on `listener` until the process ends.
    pub async fn serve(self, listener: tokio::net::TcpListener) -> anyhow::Result<()> {
        let router = self.edge.websocket_router();
        axum::serve(listener, babel_api_edge::router(self.edge).merge(router)).await?;
        Ok(())
    }
}

/// Opens the services and serves them on `address`. Returns the bound
/// address and the serving task.
pub async fn start(
    config: &ServerConfig,
    address: &str,
) -> anyhow::Result<(SocketAddr, tokio::task::JoinHandle<anyhow::Result<()>>)> {
    let server = Server::open(config)?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    let bound = listener.local_addr()?;
    Ok((bound, tokio::spawn(server.serve(listener))))
}
