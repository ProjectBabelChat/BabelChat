//! The egress proxy: the only way out to the internet.
//!
//! Services that call the internet (an engine calling a hosted model, later
//! link previews and webhooks) send those requests through this proxy
//! instead of connecting directly. That way:
//!
//! - no request carries a cell's own address to the outside world;
//! - a URL that someone else chose (a webhook, a link) cannot reach inside
//!   the cluster: the proxy refuses private, loopback, link-local and other
//!   internal addresses (SSRF), and connects to the very address it checked,
//!   so a name that resolves differently the second time (DNS rebinding)
//!   gains nothing.
//!
//! It speaks HTTP `CONNECT` (what clients use for `https://` URLs) and
//! nothing else, so it never sees the traffic it tunnels. It listens on the
//! internal network only.

use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const MAX_REQUEST: usize = 8 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct EgressConfig {
    /// Ports that may be reached; 443 unless told otherwise.
    pub ports: Vec<u16>,
    /// Host names that may be reached (`api.example.com`, or
    /// `*.example.com` for its subdomains). Empty: any public host.
    pub allow_hosts: Vec<String>,
    /// For tests only: let private addresses through.
    pub allow_private: bool,
}

impl Default for EgressConfig {
    fn default() -> Self {
        Self {
            ports: vec![443],
            allow_hosts: Vec::new(),
            allow_private: false,
        }
    }
}

impl EgressConfig {
    fn host_allowed(&self, host: &str) -> bool {
        let host = host.to_ascii_lowercase();
        self.allow_hosts.is_empty()
            || self.allow_hosts.iter().any(|allowed| {
                let allowed = allowed.to_ascii_lowercase();
                match allowed.strip_prefix("*.") {
                    Some(domain) => host.ends_with(&format!(".{domain}")),
                    None => host == allowed,
                }
            })
    }
}

/// Whether `address` is somewhere on the public internet. Everything else
/// (private networks, loopback, link-local, carrier-grade NAT, multicast,
/// documentation and reserved ranges) may be inside the cluster or near it.
pub fn is_public(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => is_public_v4(v4),
            None => is_public_v6(v6),
        },
    }
}

fn is_public_v4(address: Ipv4Addr) -> bool {
    let [a, b, c, _] = address.octets();
    !(address.is_private()
        || address.is_loopback()
        || address.is_link_local()
        || address.is_broadcast()
        || address.is_documentation()
        || address.is_unspecified()
        || address.is_multicast()
        || a == 0
        // Carrier-grade NAT, also used by overlay networks.
        || (a == 100 && (64..128).contains(&b))
        // IETF protocol assignments.
        || (a == 192 && b == 0 && c == 0)
        // Benchmarking.
        || (a == 198 && (18..20).contains(&b))
        // Reserved.
        || a >= 240)
}

fn is_public_v6(address: Ipv6Addr) -> bool {
    let first = address.segments()[0];
    !(address.is_loopback()
        || address.is_unspecified()
        || address.is_multicast()
        // Unique local (fc00::/7) and link-local (fe80::/10).
        || (first & 0xfe00) == 0xfc00
        || (first & 0xffc0) == 0xfe80
        // Documentation (2001:db8::/32).
        || (first == 0x2001 && address.segments()[1] == 0x0db8)
        // NAT64 (64:ff9b::/96) could lead back to private IPv4 addresses.
        || (first == 0x0064 && address.segments()[1] == 0xff9b))
}

/// Serves the proxy on `listener` until the process ends.
pub async fn run(listener: TcpListener, config: EgressConfig) {
    let config = Arc::new(config);
    loop {
        let (client, address) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                tracing::warn!(%error, "accepting a connection failed");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let config = config.clone();
        tokio::spawn(async move {
            if let Err(error) = handle(client, &config).await {
                tracing::debug!(%error, %address, "egress connection ended");
            }
        });
    }
}

async fn reply(client: &mut TcpStream, status: &str) -> std::io::Result<()> {
    client
        .write_all(format!("HTTP/1.1 {status}\r\ncontent-length: 0\r\n\r\n").as_bytes())
        .await
}

/// Reads the request head, up to the blank line.
async fn read_head(client: &mut TcpStream) -> std::io::Result<Option<String>> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() >= MAX_REQUEST || client.read(&mut byte).await? == 0 {
            return Ok(None);
        }
        head.push(byte[0]);
    }
    Ok(String::from_utf8(head).ok())
}

async fn handle(mut client: TcpStream, config: &EgressConfig) -> std::io::Result<()> {
    let Ok(Ok(Some(head))) = tokio::time::timeout(REQUEST_TIMEOUT, read_head(&mut client)).await
    else {
        return reply(&mut client, "400 Bad Request").await;
    };
    let mut words = head.lines().next().unwrap_or_default().split_whitespace();
    let (Some("CONNECT"), Some(target)) = (words.next(), words.next()) else {
        // Only tunnels: plain HTTP would show the proxy everything.
        return reply(&mut client, "405 Method Not Allowed").await;
    };
    let Some((host, port)) = target
        .rsplit_once(':')
        .and_then(|(host, port)| Some((host.trim_matches(['[', ']']), port.parse::<u16>().ok()?)))
    else {
        return reply(&mut client, "400 Bad Request").await;
    };

    if !config.ports.contains(&port) || !config.host_allowed(host) {
        tracing::warn!(%host, port, "refused: not allowed");
        return reply(&mut client, "403 Forbidden").await;
    }
    let addresses = match tokio::net::lookup_host((host, port)).await {
        Ok(addresses) => addresses.collect::<Vec<SocketAddr>>(),
        Err(_) => return reply(&mut client, "502 Bad Gateway").await,
    };
    // Every address must be public: a name that also points inside is
    // refused outright.
    if addresses.is_empty()
        || (!config.allow_private && !addresses.iter().all(|address| is_public(address.ip())))
    {
        tracing::warn!(%host, ?addresses, "refused: internal address");
        return reply(&mut client, "403 Forbidden").await;
    }

    let mut upstream = None;
    for address in &addresses {
        if let Ok(Ok(stream)) =
            tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(address)).await
        {
            upstream = Some(stream);
            break;
        }
    }
    let Some(mut upstream) = upstream else {
        return reply(&mut client, "502 Bad Gateway").await;
    };
    tracing::info!(%host, port, "tunnel");
    client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;
    tokio::io::copy_bidirectional(&mut client, &mut upstream).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_addresses_are_public() {
        for private in [
            "10.0.0.1",
            "172.16.5.4",
            "192.168.1.1",
            "127.0.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "224.0.0.1",
            "255.255.255.255",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:10.0.0.1",
            "64:ff9b::a00:1",
        ] {
            assert!(!is_public(private.parse().unwrap()), "{private}");
        }
        for public in ["93.184.216.34", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(is_public(public.parse().unwrap()), "{public}");
        }
    }

    #[test]
    fn host_lists_match_names_and_subdomains() {
        let config = EgressConfig {
            allow_hosts: vec!["api.example.com".into(), "*.models.example".into()],
            ..EgressConfig::default()
        };
        assert!(config.host_allowed("API.example.com"));
        assert!(config.host_allowed("eu.models.example"));
        assert!(!config.host_allowed("models.example"));
        assert!(!config.host_allowed("evil-example.com"));
        assert!(EgressConfig::default().host_allowed("anything.example"));
    }

    async fn connect(proxy: SocketAddr, target: &str) -> (String, TcpStream) {
        let mut stream = TcpStream::connect(proxy).await.unwrap();
        stream
            .write_all(format!("CONNECT {target} HTTP/1.1\r\nhost: {target}\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        while !line.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).await.unwrap();
            line.push(byte[0]);
        }
        (String::from_utf8(line).unwrap(), stream)
    }

    #[tokio::test]
    async fn tunnels_out_but_never_in() {
        // Something listening inside.
        let inside = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let inside_address = inside.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = inside.accept().await {
                stream.write_all(b"secret").await.unwrap();
            }
        });
        let start = |config: EgressConfig| async move {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            tokio::spawn(run(listener, config));
            address
        };
        let target = format!("127.0.0.1:{}", inside_address.port());
        let any_port = |allow_private| EgressConfig {
            ports: vec![inside_address.port()],
            allow_private,
            ..EgressConfig::default()
        };

        let proxy = start(any_port(false)).await;
        let (answer, _) = connect(proxy, &target).await;
        assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
        let (answer, _) = connect(proxy, &format!("localhost:{}", inside_address.port())).await;
        assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");

        // Only allowed ports.
        let strict = start(EgressConfig::default()).await;
        let (answer, _) = connect(strict, &target).await;
        assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");

        // The tunnel itself works (private addresses allowed, for the test).
        let open = start(any_port(true)).await;
        let (answer, mut stream) = connect(open, &target).await;
        assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
        let mut secret = [0u8; 6];
        stream.read_exact(&mut secret).await.unwrap();
        assert_eq!(&secret, b"secret");

        // No plain HTTP.
        let mut stream = TcpStream::connect(open).await.unwrap();
        stream
            .write_all(b"GET http://example.com/ HTTP/1.1\r\n\r\n")
            .await
            .unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).await.unwrap();
        assert!(answer.starts_with("HTTP/1.1 405"), "{answer}");
    }
}
