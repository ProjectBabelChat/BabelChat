//! Creates the cluster's certificates.
//!
//! ```sh
//! babel-pki cluster --cluster cluster.toml   # CA, token keys, one cert per service
//! babel-pki issue --dir pki --role admin --name ops
//! babel-pki status --dir pki
//! babel-pki renew --dir pki                  # those expiring within 90 days
//! ```
//!
//! Replacing the CA or the token key takes three steps, each deployed
//! before the next (see `babel_net::pki`): `new-ca`, `renew`,
//! `drop-old-cas`; `new-token-key`, `use-new-token-key`, and a week later
//! `drop-old-token-keys`.

use std::path::PathBuf;

use anyhow::anyhow;
use babel_common::{Identity, Role};
use babel_net::{config::Cluster, pki::Pki};
use clap::{Parser, Subcommand};
use time::Duration;

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
    /// Lists the CAs trusted and the certificates here: who for, whether
    /// the current CA signed them, and when they expire.
    Status {
        #[arg(long, default_value = "pki")]
        dir: PathBuf,
    },
    /// Issues again the certificates that expire soon, or that an old CA
    /// signed.
    Renew {
        #[arg(long, default_value = "pki")]
        dir: PathBuf,
        /// Those expiring within this many days.
        #[arg(long, default_value_t = 90)]
        within_days: i64,
    },
    /// Makes a new CA, which signs from now on; the old one stays trusted
    /// until `drop-old-cas`.
    NewCa {
        #[arg(long, default_value = "pki")]
        dir: PathBuf,
    },
    /// Trusts only the current CA, once every certificate is from it.
    DropOldCas {
        #[arg(long, default_value = "pki")]
        dir: PathBuf,
    },
    /// Makes a new token key (`token.next.key`); Edges given the new
    /// `token.pub` accept tokens of both.
    NewTokenKey {
        #[arg(long, default_value = "pki")]
        dir: PathBuf,
    },
    /// Makes the new token key the one tokens are signed with.
    UseNewTokenKey {
        #[arg(long, default_value = "pki")]
        dir: PathBuf,
    },
    /// Accepts only tokens of the current key. Tokens of the old one stop
    /// working: wait until they expired (a week) unless it leaked.
    DropOldTokenKeys {
        #[arg(long, default_value = "pki")]
        dir: PathBuf,
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
        Command::Status { dir } => {
            let now = time::OffsetDateTime::now_utc();
            for status in Pki::new(&dir).status()? {
                let what = match &status.identity {
                    Some(identity) => identity.to_string(),
                    None => "certificate authority".into(),
                };
                println!(
                    "{:<20} {:<36} {:<12} expires {} ({} days)",
                    status.name,
                    what,
                    if status.current {
                        "current CA"
                    } else {
                        "old CA"
                    },
                    status.not_after.date(),
                    (status.not_after - now).whole_days()
                );
            }
        }
        Command::Renew { dir, within_days } => {
            for identity in Pki::new(&dir).renew(Duration::days(within_days))? {
                println!("renewed {identity}");
            }
        }
        Command::NewCa { dir } => {
            Pki::new(&dir).new_ca()?;
            println!("made a new CA; deploy, then `renew`, deploy, `drop-old-cas`, deploy");
        }
        Command::DropOldCas { dir } => {
            if Pki::new(&dir).drop_old_cas()? {
                println!("trusting only the current CA");
            }
        }
        Command::NewTokenKey { dir } => {
            Pki::new(&dir).new_token_key()?;
            println!("made a new token key; deploy, then `use-new-token-key`, deploy");
        }
        Command::UseNewTokenKey { dir } => {
            Pki::new(&dir).use_new_token_key()?;
            println!("tokens are signed with the new key once deployed");
        }
        Command::DropOldTokenKeys { dir } => {
            if Pki::new(&dir).drop_old_token_keys()? {
                println!("accepting only tokens of the current key");
            }
        }
    }
    Ok(())
}
