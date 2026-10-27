//! The egress proxy: the cluster's only way out to the internet.
//!
//! ```sh
//! babel-egress --listen 10.8.0.9:3128 --allow-host api.example.com
//! babel-engine ... --egress-proxy http://10.8.0.9:3128
//! ```
//!
//! Listen on the internal network only. See `babel_net::egress`.

use babel_net::egress::{EgressConfig, run};
use clap::Parser;

#[derive(Parser)]
#[command(about = "BabelChat egress proxy: CONNECT tunnels to public addresses only")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:3128")]
    listen: String,
    /// Ports that may be reached (repeatable).
    #[arg(long = "port", default_values_t = [443])]
    ports: Vec<u16>,
    /// Hosts that may be reached (repeatable; `*.example.com` for
    /// subdomains). Without any, every public host may be.
    #[arg(long = "allow-host")]
    allow_hosts: Vec<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    babel_net::init_tracing();
    let args = Args::parse();
    let listener = tokio::net::TcpListener::bind(&args.listen).await?;
    tracing::info!(listen = %args.listen, ports = ?args.ports, hosts = ?args.allow_hosts, "egress proxy");
    run(
        listener,
        EgressConfig {
            ports: args.ports,
            allow_hosts: args.allow_hosts,
            allow_private: false,
        },
    )
    .await;
    Ok(())
}
