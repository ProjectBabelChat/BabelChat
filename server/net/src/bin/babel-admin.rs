//! Operator commands for a BabelChat cluster.
//!
//! ```sh
//! babel-pki issue --dir pki --role admin --name ops        # once
//! babel-admin --cluster cluster.toml --name ops placements
//! babel-admin --cluster cluster.toml --name ops move-guild <guild id> message-cell-1
//! babel-admin --cluster cluster.toml --name ops set-plan <guild id> paid1
//! babel-admin restore --backup-dir /backups/message-cell-0 --data data/message-cell-0
//! ```

use std::path::PathBuf;

use anyhow::{Context, bail};
use babel_api_directory::types::Directory;
use babel_api_translation_cell::types::{Plan, TranslationCell};
use babel_common::{Identity, Role};
use babel_net::{
    config::Cluster,
    remote::{RemoteDirectory, RemoteTranslationCell},
    tls::Credentials,
};
use babel_store::RocksStore;
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(about = "Operator commands for a BabelChat cluster")]
struct Args {
    /// The cluster file.
    #[arg(long, env = "BABEL_CLUSTER", default_value = "cluster.toml")]
    cluster: PathBuf,
    /// The operator's certificate (`pki/<name>.crt`, role `admin`).
    #[arg(long, env = "BABEL_ADMIN", default_value = "admin")]
    name: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
enum PlanArg {
    Free,
    Paid1,
    Paid2,
    Paid3,
}

#[derive(Subcommand)]
enum Command {
    /// Lists where every guild lives.
    Placements,
    /// Moves a guild to another Message Cell. Its writes wait while it is
    /// copied (a moment for a small guild); readers notice nothing.
    MoveGuild { guild: String, message_cell: String },
    /// Sets a guild's plan: how many languages it may declare and how many
    /// the LLM translates. Languages that no longer fit are dropped.
    SetPlan { guild: String, plan: PlanArg },
    /// Puts the newest backup back into an empty data directory. The
    /// service must be stopped.
    Restore {
        #[arg(long)]
        backup_dir: PathBuf,
        #[arg(long)]
        data: PathBuf,
    },
}

/// The operator's credentials.
fn operator(args: &Args) -> anyhow::Result<(Cluster, Credentials)> {
    let cluster = Cluster::load(&args.cluster)?;
    let credentials = Credentials::load(
        &cluster.ca_path(),
        &cluster.cert_path(&args.name),
        &cluster.key_path(&args.name),
    )
    .with_context(|| {
        format!(
            "no operator certificate; `babel-pki issue --dir {} --role admin --name {}`",
            cluster.pki.display(),
            args.name
        )
    })?;
    if credentials.identity.role != Role::Admin {
        bail!("{} is not an operator's certificate", args.name);
    }
    Ok((cluster, credentials))
}

fn directory(args: &Args) -> anyhow::Result<RemoteDirectory> {
    let (cluster, credentials) = operator(args)?;
    let peer = Identity {
        role: Role::Directory,
        name: cluster.directory.name.clone(),
    };
    let client = credentials.http_client(&peer)?;
    Ok(RemoteDirectory::new(peer, cluster.directory.url, client))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    babel_net::init_tracing();
    let args = Args::parse();
    match &args.command {
        Command::Placements => {
            let directory = directory(&args)?;
            let mut after = 0;
            loop {
                let batch = directory
                    .watch_placements(after, 0)
                    .await
                    .map_err(|error| anyhow::anyhow!("{error:?}"))?;
                for placement in &batch.placements {
                    println!(
                        "{}  {}  {}  (epoch {})",
                        placement.guild_id,
                        placement.message_cell,
                        placement.translation_cell,
                        placement.epoch
                    );
                }
                if batch.placements.is_empty() || batch.version <= after {
                    break;
                }
                after = batch.version;
            }
        }
        Command::MoveGuild {
            guild,
            message_cell,
        } => {
            let placement = directory(&args)?
                .move_guild(guild.clone(), message_cell.clone())
                .await
                .map_err(|error| anyhow::anyhow!("moving failed: {error:?}"))?;
            println!(
                "{} now lives on {} (epoch {})",
                placement.guild_id, placement.message_cell, placement.epoch
            );
        }
        Command::SetPlan { guild, plan } => {
            let placement = directory(&args)?
                .get_placement(guild.clone())
                .await
                .map_err(|error| anyhow::anyhow!("no such guild: {error:?}"))?;
            let (cluster, credentials) = operator(&args)?;
            let service = cluster
                .translation_cell
                .iter()
                .find(|cell| cell.name == placement.translation_cell)
                .with_context(|| {
                    format!("{} is not in the cluster file", placement.translation_cell)
                })?;
            let peer = Identity {
                role: Role::TranslationCell,
                name: service.name.clone(),
            };
            let client = credentials.http_client(&peer)?;
            let cell = RemoteTranslationCell::new(peer, service.url.clone(), client);
            let plan = match plan {
                PlanArg::Free => Plan::Free,
                PlanArg::Paid1 => Plan::Paid1,
                PlanArg::Paid2 => Plan::Paid2,
                PlanArg::Paid3 => Plan::Paid3,
            };
            let policy = cell
                .set_plan(guild.clone(), plan)
                .await
                .map_err(|error| anyhow::anyhow!("setting the plan failed: {error:?}"))?;
            println!(
                "{guild}: {:?}, languages {:?}, LLM languages {:?}",
                policy.plan, policy.languages, policy.llm_languages
            );
        }
        Command::Restore { backup_dir, data } => {
            if data.exists() && std::fs::read_dir(data)?.next().is_some() {
                bail!("{} is not empty; move it aside first", data.display());
            }
            RocksStore::restore(backup_dir, data)?;
            println!("restored {} from {}", data.display(), backup_dir.display());
        }
    }
    Ok(())
}
