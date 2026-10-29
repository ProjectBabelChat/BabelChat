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
        ┌────────────┼───────────┬───────┴──────┬─────────────────┬──────────┐
        ▼            ▼           ▼              ▼                 ▼          ▼
     account     directory   message-cell-N  translation-cell-N  engine-N   media-N
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
| `babel-media` | `babel-media` | Edges, Message Cells, operators | yes (uploaded files) |
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

[[media]]                         # optional: without one, uploads are refused
name = "media-0"
url = "https://10.8.0.7:7130"

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
| `--pki` (`BABEL_PKI`) | all, `babel-admin` | the cluster file's `pki` |
| `--metrics` (`BABEL_METRICS`): an address | all, standbys | no metrics served |
| `--signups` (`BABEL_SIGNUPS`): `open`, `invite`, `closed` | the Account service | `open` |
| `--data` | storing services | `data/<name>` |
| `--backup-dir`, `--backup-every-mins` (`BABEL_BACKUP_EVERY_MINS`), `--backup-keep` | storing services | no backups; 15; 96 |
| `--plan`, `--daily-budget`, `--mock-latency-ms` | Translation Cells | `free`; none; 300 |
| `--writes-per-second`, `--write-burst` | Edges | 5; 20 |
| `--models` (`BABEL_MODELS`), or `--backend`, `--base-url`, `--model`, `--style` for one model; `--egress-proxy` | engines | one mock model |

Every service logs to standard error, as text, or as one JSON object per
line with `BABEL_LOG_FORMAT=json` (`RUST_LOG` filters as usual).

## On NixOS machines

`deploy/nixos` deploys a cluster to NixOS machines from one description,
`deploy/<name>/cluster.nix`: the machines, and which service runs on which.
`deploy/vultr/cluster.nix` is a production-like example on nine virtual
machines; any provider of KVM machines with one disk works the same.

```nix
cluster = {
  machines.core = { address = "203.0.113.2"; wireguardAddress = "10.8.0.2"; };
  # ...
  account = { machine = "core"; standby = "spare"; signups = "invite"; };
  messageCells.message-cell-0 = { machine = "cell-a"; standby = "cell-b"; };
  edges.edge-0.machine = "edge-a";   # https://edge-0.<address>.sslip.io
  # ...
};
```

From it, the NixOS module (`nix/module.nix`, also the flake's
`nixosModules.default`) gives every machine the same cluster file, a
WireGuard interface to the others, a firewall that opens each service's
port on WireGuard only, and a hardened systemd unit (`babelchat-<name>`)
per service, standby (`babelchat-standby-<name>`) and local model placed on
it; machines with Edges get Caddy, with a Let's Encrypt certificate for
each Edge's host name (`sslip.io` names need no DNS). Services listen on
fixed ports (account 7101, directory 7102, message-cell-N 7110+N,
translation-cell-N 7120+N, media-N 7130+N, engine-N 7140+N, edge-N 7000+N), so any of them
can share a machine: putting several on one is naming the same `machine`.

```sh
deploy/nixos/deploy.sh init                # WireGuard keys, CA, certificates
deploy/nixos/deploy.sh install core        # Debian -> NixOS (erases the disk)
deploy/nixos/deploy.sh switch              # build here once, copy to all at once, activate one by one
deploy/nixos/deploy.sh switch --via cell-b # the same, sending once (compressed) to cell-b, which copies on
deploy/nixos/deploy.sh admin health        # babel-admin on the operator's machine
deploy/nixos/deploy.sh admin signup-code --uses 5
deploy/nixos/deploy.sh ssh core journalctl -u babelchat-account -f
deploy/nixos/deploy.sh dashboard           # Grafana at http://localhost:3000, through SSH
deploy/nixos/deploy.sh failover cell-a       # standbys take over its services (below)
deploy/nixos/deploy.sh restore message-cell-0  # back to its newest backup (below)
deploy/nixos/deploy.sh certificates          # who for, which CA, until when
deploy/nixos/deploy.sh renew                 # those expiring within 90 days, handed out
deploy/nixos/deploy.sh rotate ca             # a new CA, in three steps (below)
deploy/nixos/deploy.sh rotate token-key      # a new token key; `--finish` a week later
```

`BABEL_DEPLOYMENT` picks another deployment directory. Everything is built
on the machine running `deploy.sh` (machines download the rest from the
NixOS cache); machines need nothing but SSH as root. Secrets stay out of
the Nix store: `init` makes them in `deploy/<name>/secrets/` (ignored by
git; back it up, it holds the CA's key), and `install` and `switch` copy to
each machine only what its services need, into `/var/lib/babelchat-secrets`,
where systemd hands each service its own certificate. A change of
certificates restarts the machine's services (of its WireGuard key,
WireGuard). `renew` and `rotate` change the certificates in `secrets/` and
switch every machine, one after another, after each step (see
Certificates); run again after an interruption, they go on.
On the eight-machine example, `rotate ca` took 5 minutes and `rotate
token-key` 3, every call through both Edges answering meanwhile (tokens
from before included), and certificates of the old CA refused after.

`switch` sends what changed to every machine from here, so a new server
binary (160 MB) crosses this machine's link once per machine. Over a slow
or metered link, `--via <machine>` sends it once, compressed by SSH (about
60 MB), to that machine, which copies to the others over the provider's
network. It logs in to them with this machine's SSH agent, forwarded (no
key is left on it), checking their host keys against `secrets/known_hosts`.
The relay keeps every machine's system, so the first `--via` through a
machine copies more than the change. A copy stopped midway can leave a
machine's receiving `nix-store --serve` holding a path's lock, and the
next copy then waits: `deploy.sh ssh <machine> pkill -f 'nix-store --[s]erve'`.

Data lives in `/var/lib/babelchat/<name>` (a standby keeps its copy at the
same path on its machine) and backups in `/var/lib/babelchat-backups/<name>`
(every 15 minutes, the last 24 hours), copied to `backupCopies.machine`
when set (below).
A change of the cluster file restarts every service that reads it.

### Failing over

```sh
deploy/nixos/deploy.sh failover message-cell-0             # its standby takes over
deploy/nixos/deploy.sh failover cell-a                     # every primary on cell-a
deploy/nixos/deploy.sh failover cell-a --primary-lost      # cell-a is gone
```

`failover` moves services that keep data (or every one a machine is the
primary of) to their standbys' machines:

1. it builds every machine for the new placement first, while nothing has
   changed;
2. it stops the old primary; with `--primary-lost` (the machine does not
   answer, or should not be trusted), it cuts that machine off instead:
   every other machine drops it from WireGuard at once, and for good once
   switched, so that if it still runs, or comes back with its old system,
   calls to it fail rather than reaching a second primary;
3. the standby stops copying, the service starts on its data on that
   machine, and every other machine switches at once, restarting its
   services with the new cluster file;
4. the old primary's data is put aside (`/var/lib/babelchat-retired`) and
   its machine becomes the new standby, copying from a checkpoint; a lost
   one leaves the service without a standby.

The move is recorded in `deploy/<name>/placement.json`, which overrides
`cluster.nix` for the services and machines it names: commit it with the
rest, and fold it into `cluster.nix` when convenient (then delete it).
`deploy.sh install` on a cut-off machine erases it and lets it back in
(after `switch`); give services a standby on it again in `placement.json`
or `cluster.nix`.

Writes the old primary acknowledged but its standby had not copied yet are
lost: those of its last moment. On the eight-machine example, services
were unavailable for 6 to 10 seconds, and no acknowledged message was
lost, planned or with a primary declared lost while it still ran.

### Backups on another machine, and restoring

```sh
deploy/nixos/deploy.sh restore message-cell-0              # from its machine's backups
deploy/nixos/deploy.sh restore message-cell-0 --from-copy  # from backupCopies.machine
```

A standby covers a lost machine. Backups cover what a standby copies too
(a mistake, a bug writing nonsense) and losing a machine with its standby.
With `backupCopies.machine` set, that machine fetches every store's backups
every 15 minutes over WireGuard (from rsync's daemon on each store's
machine: read-only, for that machine alone) into
`/var/lib/babelchat-backup-copies/<service>/<machine>`: one copy per
machine the service ran on, so a failover leaves the copy of the old
machine's backups alone (remove old ones by hand).
`babel_backup_copy_last_success_timestamp_seconds` and the
`BackupCopyStale` alert tell when copying stops. Pick a machine holding no
store or standby; better still, one with another provider or in another
region.

`restore` stops the service's standby, puts the service's data aside
(`/var/lib/babelchat-retired`), restores the newest backup into its place
(the service's machine's own, or with `--from-copy` the newest copy,
carried over through the machine running `deploy.sh`), starts the service,
and has the standby start over from a checkpoint. Writes since that backup
are lost, up to 15 minutes of them, and other stores are not taken back
with it: a guild created since may be listed by the Directory, for
instance. To bring back a service whose machine and standby are both lost,
reinstall the machine (or place the service on another one), `switch`
(the service starts empty), then `restore <service> --from-copy`.
On the eight-machine example, restoring a Message Cell from the copy (a
small one, 1 MB of backups) took 5 seconds, the messages up to the backup
back and its standby following again at once.

Moving a service that keeps data anywhere else is changing its `machine`
with its data copied over first (a standby, or a restored backup).

`nix build .#checks.x86_64-linux.cluster` runs the module on two virtual
machines: WireGuard, mutual TLS, closed ports, sign-ups by invitation, a
standby, backups copied and restored, revoked tokens, metrics collected and
shown.

## Certificates

```sh
babel-pki cluster --cluster cluster.toml           # CA, token keys, one cert per service
babel-pki issue --dir pki --role admin --name ops  # an operator
babel-pki issue --dir pki --role tenant --name acme  # a platform using a Translation Cell
babel-pki status --dir pki                         # who for, which CA, until when
babel-pki renew --dir pki                          # again, those expiring within 90 days
```

| File | Goes to |
|---|---|
| `ca.crt` | every machine |
| `ca.key` | nowhere: keep it offline |
| `<name>.crt`, `<name>.key` | the machine running `<name>` (and its standby) |
| `token.pub` | Edges |
| `token.key` | the Account service only |
| `token.next.key` | nowhere yet (while the token key is replaced) |

A service's certificate names it (`babel://message-cell/message-cell-0`).
Callers check that name, not the host name: certificates need no addresses,
and a service can move to another machine (or its standby take over)
without new certificates. User tokens are signed with Ed25519 by the Account
service; Edges only hold the public key, so a compromised Edge cannot mint
tokens.

Certificates last two years, the CA ten; every service reports when its
own stops being accepted (`babel_certificate_expiry_timestamp_seconds`),
and on NixOS an alert fires 30 days before. Services read their
certificates when they start: renew, hand out the new files, restart.

The CA and the token key are replaced without stopping anything, in steps,
each handed out everywhere before the next, so that nothing stops trusting
what another still uses:

| | CA | Token key |
|---|---|---|
| 1 | `new-ca`: `ca.crt` holds the new CA, which signs from now on, then the old one; both trusted | `new-token-key`: `token.next.key`, and its public key added to `token.pub`; Edges accept tokens of either |
| 2 | `renew`: certificates the old CA signed issued again | `use-new-token-key`: the Account service signs with it |
| 3 | `drop-old-cas`: only the new CA trusted | `drop-old-token-keys`, once tokens of the old key expired (a week) |

There is no revocation of one certificate: for a leaked key, replace the
CA (certificates of the old one are refused once step 3 is out), or the
token key, finishing at once (everyone signs in again).

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
stopped. Its standbys start over from an empty data directory: going on,
they would take the restored store's writes for the ones they hold.

On NixOS, `deploy.sh restore` does all of it, and another machine can
keep copies of every store's backups (see "Backups on another machine,
and restoring").

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

Failing over, by hand (on NixOS, `deploy.sh failover` does it: see
[Failing over](#failing-over)):

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

## Accounts

The Account service takes new accounts from anyone, from people with a
sign-up code, or from nobody (`--signups`). Operators make codes:

```sh
babel-admin --cluster cluster.toml --name ops signup-code --uses 5 --hours 48
```

Users change their password and sign their other devices out from the
apps. Both revoke the user's other tokens: the Account service records the
revocation, Edges follow the stream of revocations (as they follow
placements) and refuse those tokens from then on, and end their sessions.

## Health

Every internal service answers `GET /health` to any certificate of the
cluster, and Edges answer it in plain HTTP (for load balancers).
`babel-admin health` asks every service of the cluster file and fails if
one does not answer.
`babel-admin media-usage` says how many files each Media service holds and
their size.

## Metrics

With `--metrics <address>`, a service or standby serves its numbers at
`GET /metrics` on that address, in Prometheus' text format. It is plain
HTTP: give it an address on the private network only (the NixOS module
uses the service's port plus 2000, open on WireGuard only). The numbers are
counts and timings by route, model or outcome; no user, guild or message
appears in them. Prometheus adds `service`, `kind`, `role` and `machine` to
what it collects (on NixOS), so services do not use those labels.

| Metric | From | What |
|---|---|---|
| `babel_requests_total`, `babel_request_duration_seconds` (`route`, `status`) | every service | calls of its API; long polls take as long as they wait |
| `babel_engine_translation_seconds`, `babel_engine_summary_seconds` (`model`, `outcome`) | engines | time a model took: `ok`, `unavailable`, `rate_limited`, `invalid_output`… |
| `babel_engine_tokens_total` (`model`, `type`) | engines | `input`, `cache_read`, `cache_write`, `output` |
| `babel_translation_messages_total` (`status`) | Translation Cells | messages `queued` for a model, `over_budget`, or with `no_targets` |
| `babel_translation_job_seconds` (`outcome`), `babel_translation_jobs` | Translation Cells | from queueing to the answer, retries included; jobs waiting or translating |
| `babel_edge_sessions`, `babel_edge_websockets` | Edges | sessions; open sockets |
| `babel_edge_rate_limited_total`, `babel_edge_sessions_dropped_total` | Edges | writes refused; sessions dropped for falling behind |
| `babel_outbox_delivered_total`, `babel_outbox_retries_total` | Message Cells | guild events forwarded; retries mean a Translation Cell or the Directory is not taking them |
| `babel_standby_behind`, `babel_standby_last_contact_timestamp_seconds` | standbys | writes behind the primary; when it last answered |
| `babel_backup_last_success_timestamp_seconds`, `babel_backup_last_duration_seconds`, `babel_backup_failures_total` | services with `--backup-dir` | |

On NixOS, `monitoring.machine` in the cluster description runs Prometheus
there, collecting every 15 s from every service and standby and from
`node_exporter` on every machine (CPU, memory, disks, systemd units), kept
30 days (`monitoring.retention`). Grafana shows it on a provisioned
dashboard (`nix/dashboard.nix`). Both listen on 127.0.0.1 only:

```sh
deploy/nixos/deploy.sh dashboard   # http://localhost:3000 (Grafana), :9090 (Prometheus)
```

Alert rules (`nix/module.nix`) watch for targets down, failed units,
standbys out of touch or far behind, stale or failing backups or copies,
a stuck outbox, server errors, failing or slow translations, full disks
and low memory. Firing alerts show on the dashboard and in Prometheus; nothing
sends them anywhere yet.

## Logs and retention

Guild logs, space logs and the Directory's membership stream keep an hour:
long enough for any follower that is only briefly away. An Edge that falls
further behind drops the sessions concerned, and their clients reload.

## Not yet

- Moving a guild's Translation Cell space.
- Automatic failover (noticing a lost primary and failing over without an
  operator); a replicated Directory.
- Revoking one certificate (replacing the CA revokes them all).
- Sending alerts (to e-mail, chat or a phone).
- PostgreSQL for the (closed) Account service.
- TLS for clients served by Edges themselves (Caddy does it in front).
- Sign-up control in `babel-server` (it takes everyone).
