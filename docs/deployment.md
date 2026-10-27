# Deployment

How to run BabelChat the way it is meant to run: every service in its own
process, on as many machines as needed, talking over a private network with
mutual TLS. (`babel-server` still runs everything in one process, for
development and small self-hosting.)

## The shape

```
                    internet
   clients ── HTTPS ──▶ load balancer / reverse proxy (TLS)
                              │  sticky per client
                    ┌─────────┴─────────┐
                    ▼                   ▼
                 edge-0              edge-1            public: HTTP behind the proxy
═════════════════════│═══════════════════│══════════ WireGuard + mutual TLS
        ┌────────────┼───────────┬───────┴──────┬─────────────────┐
        ▼            ▼           ▼              ▼                 ▼
     account     directory   message-cell-N  translation-cell-N  engine-N
                                                   │
                                                   └─▶ egress proxy ─▶ internet
                                                       (hosted models only)
```

| Binary | Crate | Serves | Keeps data |
|---|---|---|---|
| `babel-edge` | `babel-edge` | the public API (clients) | no |
| `babel-account` | `babel-account` | Edges | yes |
| `babel-directory` | `babel-directory` | Edges, Message Cells, operators | yes |
| `babel-message-cell` | `babel-message-cell` | Edges, the Directory | yes |
| `babel-translation-cell` | `babel-translation-cell` | Edges, Message Cells, tenants | yes |
| `babel-engine` | `babel-engine` | Translation Cells | no |
| `babel-egress` | `babel-net` | services calling the internet | no |
| `babel-standby` | `babel-net` | — (copies a service's store) | yes |
| `babel-pki`, `babel-admin` | `babel-net` | operators' tools | — |

Who may call what is in one place, `server/net/src/policies.rs`; every
service refuses everyone else, by certificate.

## Trying it on one machine

```sh
nix develop -c deploy/local/run.sh --plan paid3
```

`deploy/local/run.sh` makes the certificates on first run, then starts
nine services from `deploy/local/cluster.toml`, each in its own process
over mutual TLS: the Account service, the Directory, two Message Cells, two
Translation Cells, an engine (the mock) and two Edges, plus the egress
proxy. Data and backups go to `deploy/local/data`, logs to
`deploy/local/logs`; Ctrl-C stops everything.

Clients connect to an Edge, `http://127.0.0.1:7000` or
`http://127.0.0.1:7001`; people on different Edges chat with each other as
on one. See [development](development.md#a-cluster-on-one-machine) for two
desktop apps on two Edges.

## The cluster file

Every process reads the same file and finds itself in it by `--name`.

```toml
pki = "pki"                       # certificates, relative to this file

[account]
name = "account"
url = "https://10.8.0.2:7101"     # where the others reach it

[directory]
name = "directory"
url = "https://10.8.0.2:7102"

[[message_cell]]
name = "message-cell-0"
url = "https://10.8.0.3:7110"

[[translation_cell]]
name = "translation-cell-0"
url = "https://10.8.0.4:7120"

[[engine]]                        # optional: without one, Translation Cells use a mock;
name = "engine-0"                 # with several, guilds choose from all their models
url = "https://10.8.0.5:7140"

[[edge]]
name = "edge-0"
url = "http://10.8.0.6:7000"      # behind the load balancer
listen = "0.0.0.0:7000"
```

`listen` defaults to the host and port of `url`. Names are lowercase
letters, digits and dashes; a cell's name is what the Directory stores in
placements, so do not rename a cell that holds guilds.

```sh
babel-message-cell --cluster cluster.toml --name message-cell-0 \
  --data /var/lib/babel/message-cell-0 --backup-dir /mnt/backups/message-cell-0
```

| Option | Services | Default |
|---|---|---|
| `--cluster` (`BABEL_CLUSTER`) | all | `cluster.toml` |
| `--name` (`BABEL_NAME`) | all | — |
| `--data` | storing services | `data/<name>` |
| `--backup-dir`, `--backup-every-mins`, `--backup-keep` | storing services | no backups; 15; 96 |
| `--plan`, `--daily-budget`, `--mock-latency-ms` | Translation Cells | `free`; none; 300 |
| `--writes-per-second`, `--write-burst` | Edges | 5; 20 |
| `--models` (`BABEL_MODELS`), or `--backend`, `--base-url`, `--model`, `--style` for one model; `--egress-proxy` | engines | one mock model |

## Certificates

```sh
babel-pki cluster --cluster cluster.toml           # CA, token keys, one cert per service
babel-pki issue --dir pki --role admin --name ops  # an operator
babel-pki issue --dir pki --role tenant --name acme  # a platform using a Translation Cell
```

| File | Goes to |
|---|---|
| `ca.crt` | every machine |
| `ca.key` | nowhere: keep it offline |
| `<name>.crt`, `<name>.key` | the machine running `<name>` (and its standby) |
| `token.pub` | Edges |
| `token.key` | the Account service only |

A service's certificate names it (`babel://message-cell/message-cell-0`).
Callers check that name, not the host name: certificates need no addresses,
and a service can move to another machine (or its standby take over)
without new certificates. User tokens are signed with Ed25519 by the Account
service; Edges only hold the public key, so a compromised Edge cannot mint
tokens. Certificates last two years; there is no revocation yet, so a lost
key means a new CA.

## The private network

Only Edges are reachable from the internet, and only through the load
balancer. Everything else listens on WireGuard addresses:

```ini
# /etc/wireguard/babel.conf on the Message Cell machine
[Interface]
Address = 10.8.0.3/24
PrivateKey = ...
ListenPort = 51820

[Peer]   # one per machine of the cluster
PublicKey = ...
AllowedIPs = 10.8.0.2/32
Endpoint = 198.51.100.2:51820
```

WireGuard keeps strangers off the network; mutual TLS keeps each service to
the calls it is allowed (an Edge cannot read another service's store, a
tenant cannot reach a Message Cell). Firewall every port except WireGuard's
and, on Edges, the public one.

## Edges and load balancers

Edges hold sessions in memory. A client's gateway session (`connect`, then
the WebSocket or long polls) must stay on the Edge that created it, so the
load balancer keeps each client on one Edge (source-IP or cookie affinity),
or each Edge gets its own host name. Everything else works across Edges:

- guild events reach every Edge that has readers;
- read markers and notification settings travel in the guild's log, so a
  user's other devices follow on whichever Edge they are;
- joining or creating a guild reaches the user's sessions on other Edges
  through the Directory's membership stream.

Put TLS for clients in front of the Edges (a load balancer, Caddy, nginx);
Edges speak plain HTTP and WebSocket.

## Outbound traffic

Nothing in the cluster connects to the internet directly. An engine for a
hosted model sends its requests through `babel-egress`:

```sh
babel-egress --listen 10.8.0.9:3128 --allow-host api.example.com
babel-engine --cluster cluster.toml --name engine-0 --backend openai \
  --base-url https://api.example.com/v1 --egress-proxy http://10.8.0.9:3128
```

It only makes `CONNECT` tunnels (HTTPS), to port 443 unless told otherwise,
and refuses private, loopback, link-local and other internal addresses,
checking the address it actually connects to. A local model server on the
private network is reached directly, without the proxy.

## Backups

Each storing service backs its store up with `--backup-dir` (RocksDB's
backup engine; unchanged files are shared, so frequent backups are cheap).
Point it at another disk, a network volume or an object storage mount.

```sh
babel-admin restore --backup-dir /mnt/backups/message-cell-0 --data /var/lib/babel/message-cell-0
```

restores the newest backup into an empty data directory, with the service
stopped.

## Standbys

A standby keeps a copy of a service's store on another machine, a moment
behind:

```sh
# on the standby machine, with message-cell-0's own certificate
babel-standby --cluster cluster.toml --name message-cell-0 --data /var/lib/babel/message-cell-0
```

It starts from a checkpoint of the primary, then replays the primary's
writes (RocksDB's write-ahead log) as they happen. It works the same for
every storing service: Message and Translation Cells, the Directory, the
Account service. The primary keeps six hours of log for standbys that fell
behind; one further behind stops and asks to start over from an empty
directory.

Failing over, by hand:

1. Make sure the primary is down for good (power it off): two primaries
   would diverge.
2. Stop `babel-standby`.
3. Start the service on the standby's data directory.
4. Point the service's address at the standby machine: move the WireGuard
   address, or change `url` in the cluster file and restart the services
   that call it. Callers check the certificate, not the host, so nothing
   else changes.

Writes the primary made in its last moment may be lost.

## Models and plans

Each engine serves the models of its models file
([Translation Engine](architecture/translation-engine.md#models)): local ones on
its machine or the private network, hosted ones through the egress proxy.
Translation Cells use every `[[engine]]` of the cluster file; owners of paid
guilds choose among all their models, and engines serving the same model
share it by guild.

```sh
babel-engine --cluster cluster.toml --name engine-0 --models /etc/babel/models.toml \
  --egress-proxy http://10.8.0.9:3128
```

API keys stay in the engine's environment (`api_key_env`). Plans are set
per guild by operators until billing does it:

```sh
babel-admin --cluster cluster.toml --name ops set-plan <guild id> paid1
```

`--plan` on Translation Cells is only the plan new guilds start with.

## Moving guilds

```sh
babel-admin --cluster cluster.toml --name ops placements
babel-admin --cluster cluster.toml --name ops move-guild <guild id> message-cell-1
```

The guild's writes wait while it is copied (a moment for a small guild);
reads and live events go on. Edges follow it to its new cell by themselves.
Moving spreads load (new guilds go to the least loaded cell, but guilds
grow), empties a cell before it is retired, and blurs which guilds share a
cell. See [Message Cell › Moving a guild](architecture/message-cell.md#moving-a-guild).

## Logs and retention

Guild logs, space logs and the Directory's membership stream keep an hour:
long enough for any follower that is only briefly away. An Edge that falls
further behind drops the sessions concerned, and their clients reload.

## Not yet

- Moving a guild's Translation Cell space.
- Automatic failover, fencing of a returning primary, replicated Directory.
- Certificate rotation and revocation.
- PostgreSQL for the (closed) Account service.
- TLS for clients served by Edges themselves.
