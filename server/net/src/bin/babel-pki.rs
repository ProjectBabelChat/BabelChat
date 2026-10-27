//! Creates the cluster's certificates.
//!
//! ```sh
//! babel-pki cluster --cluster cluster.toml   # CA, token keys, one cert per service
//! babel-pki issue --dir pki --role admin --name ops
//! ```

use std::path::PathBuf;

use anyhow::anyhow;
use babel_common::{Identity, Role};
use babel_net::{config::Cluster, pki::Pki};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(about = "Certificates for a BabelChat cluster")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Creates the CA and the user token key pair, then a certificate for
    /// every service in the cluster file that has none yet.
    Cluster {
        #[arg(long, default_value = "cluster.toml")]
        cluster: PathBuf,
    },
    /// Creates the CA and the user token key pair, unless they exist.
    Init {
        #[arg(long, default_value = "pki")]
        dir: PathBuf,
    },
    /// Issues (or replaces) one certificate: a service, an operator
    /// (`admin`) or a platform using a Translation Cell (`tenant`).
    Issue {
        #[arg(long, default_value = "pki")]
        dir: PathBuf,
        #[arg(long)]
        role: String,
        #[arg(long)]
        name: String,
    },
}

fn main() -> anyhow::Result<()> {
    match Args::parse().command {
        Command::Cluster { cluster } => {
            let cluster = Cluster::load(&cluster)?;
            let pki = Pki::new(&cluster.pki);
            if pki.init()? {
                println!("created the CA and token keys in {}", pki.dir.display());
            }
            for (role, service) in cluster.services() {
                if cluster.cert_path(&service.name).exists() {
                    continue;
                }
                pki.issue(&Identity {
                    role,
                    name: service.name.clone(),
                })?;
                println!("issued {role}/{}", service.name);
            }
        }
        Command::Init { dir } => {
            if Pki::new(&dir).init()? {
                println!("created the CA and token keys in {}", dir.display());
            }
        }
        Command::Issue { dir, role, name } => {
            let role = Role::parse(&role).ok_or_else(|| {
                anyhow!(
                    "unknown role {role:?}; one of {}",
                    Role::ALL.map(Role::as_str).join(", ")
                )
            })?;
            Pki::new(&dir).issue(&Identity {
                role,
                name: name.clone(),
            })?;
            println!("issued {role}/{name}");
        }
    }
    Ok(())
}
