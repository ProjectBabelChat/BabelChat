# BabelChat on NixOS: one description of the whole cluster, shared by every
# machine; each machine runs the services placed on it.
#
#   services.babelchat = {
#     enable = true;
#     cluster = import ./cluster.nix;  # machines, and which service runs where
#     machine = "core";                # this machine's name in it
#   };
#
# From the description, every machine gets the same cluster file
# (/etc/babelchat/cluster.toml), a WireGuard interface to the others, a
# firewall that opens each service's port on WireGuard only, and a systemd
# unit per service, standby and local model placed on it. Machines with
# Edges also get Caddy, serving HTTPS to clients.
#
# Services listen on fixed ports (account 7101, directory 7102,
# message-cell-N 7110+N, translation-cell-N 7120+N, media-N 7130+N,
# engine-N 7140+N, edge-N 7000+N), so they can share a machine in any combination. Moving a
# service to another machine is changing its `machine`; one that keeps data
# needs its data moved too (docs/deployment.md).
#
# Every service and standby serves its metrics on its port plus 2000, and
# every machine runs node_exporter (9100), both on WireGuard only. With
# `monitoring`, one machine collects them all (Prometheus) and shows them
# (Grafana), on 127.0.0.1: reach them through SSH (`deploy.sh dashboard`).
#
# Secrets never enter the Nix store. They are put in `secretsDir` by the
# deploy tool (deploy/nixos/deploy.sh): `wireguard.key`, and in `pki/` the
# certificates of the services placed here. Services read theirs through
# systemd credentials.
{
  config,
  lib,
  pkgs,
  ...
}:

let
  inherit (lib)
    mkOption
    mkEnableOption
    mkIf
    types
    ;
  cfg = config.services.babelchat;
  cluster = cfg.cluster;
  toml = pkgs.formats.toml { };

  # The number at the end of a name: message-cell-3 -> 3.
  index =
    name:
    let
      found = builtins.match ".*[^0-9]([0-9]+)" name;
    in
    if found == null then
      throw "babelchat: ${name} needs a number at the end of its name, or a port"
    else
      lib.toInt (builtins.head found);

  placed = {
    machine = mkOption {
      type = types.str;
      description = "The machine running it.";
    };
  };
  port =
    base:
    mkOption {
      type = types.nullOr types.port;
      default = null;
      description = "Its port; by default ${toString base} plus the number its name ends with.";
    };
  stores = {
    standby = mkOption {
      type = types.nullOr types.str;
      default = null;
      description = "A machine keeping a copy of its store, a moment behind.";
    };
    backups = mkOption {
      type = types.bool;
      default = true;
      description = "Back the store up every 15 minutes, on its machine (and to `backupCopies`).";
    };
  };

  machineType = types.submodule {
    options = {
      address = mkOption {
        type = types.str;
        description = "Public address: WireGuard's endpoint, and where Edges are reached.";
      };
      wireguardAddress = mkOption {
        type = types.str;
        example = "10.8.0.2";
        description = "Address on the cluster's WireGuard network.";
      };
      wireguardKey = mkOption {
        type = types.str;
        description = "WireGuard public key (the deploy tool fills it in).";
      };
      cutOff = mkOption {
        type = types.bool;
        default = false;
        description = "Lost: no other machine talks to it over WireGuard, so if it comes back with an old system it cannot act as the primary of a service that moved (`deploy.sh failover --primary-lost`). Reinstall it before using it again.";
      };
    };
  };

  clusterType = types.submodule {
    options = {
      machines = mkOption {
        type = types.attrsOf machineType;
        description = "Every machine of the cluster, by name.";
      };
      wireguard = {
        interface = mkOption {
          type = types.str;
          default = "babel";
        };
        port = mkOption {
          type = types.port;
          default = 51820;
        };
        prefixLength = mkOption {
          type = types.int;
          default = 24;
        };
      };
      account =
        placed
        // stores
        // {
          signups = mkOption {
            type = types.enum [
              "open"
              "invite"
              "closed"
            ];
            default = "invite";
            description = "Who may create an account: anyone, people with a code from `babel-admin signup-code`, or nobody.";
          };
        };
      directory = placed // stores;
      messageCells = mkOption {
        type = types.attrsOf (types.submodule { options = placed // stores // { port = port 7110; }; });
      };
      translationCells = mkOption {
        type = types.attrsOf (
          types.submodule {
            options =
              placed
              // stores
              // {
                port = port 7120;
                plan = mkOption {
                  type = types.enum [
                    "free"
                    "paid1"
                    "paid2"
                    "paid3"
                  ];
                  default = "free";
                  description = "The plan new spaces start with (`babel-admin set-plan` changes one).";
                };
                dailyBudget = mkOption {
                  type = types.nullOr types.int;
                  default = null;
                };
              };
          }
        );
      };
      media = mkOption {
        default = { };
        description = "Media services: the files people upload. Without one, uploads are refused.";
        type = types.attrsOf (types.submodule { options = placed // stores // { port = port 7130; }; });
      };
      engines = mkOption {
        default = { };
        description = "Without engines, Translation Cells translate with a mock.";
        type = types.attrsOf (
          types.submodule {
            options = placed // {
              port = port 7140;
              models = mkOption {
                type = types.listOf toml.type;
                description = "The models file's `[[model]]` entries (server/engine/src/config.rs); the first is the default.";
              };
              environmentFile = mkOption {
                type = types.bool;
                default = false;
                description = "Read API keys from `<secretsDir>/<engine>.env` (put there by the deploy tool).";
              };
              llama = mkOption {
                default = null;
                description = "A local model, served by llama.cpp on this machine (CPU).";
                type = types.nullOr (
                  types.submodule {
                    options = {
                      url = mkOption { type = types.str; };
                      sha256 = mkOption { type = types.str; };
                      port = mkOption {
                        type = types.port;
                        default = 8001;
                        description = "On 127.0.0.1; models reach it at http://127.0.0.1:<port>/v1.";
                      };
                      contextSize = mkOption {
                        type = types.int;
                        default = 8192;
                      };
                      threads = mkOption {
                        type = types.nullOr types.int;
                        default = null;
                      };
                    };
                  }
                );
              };
            };
          }
        );
      };
      edges = mkOption {
        type = types.attrsOf (
          types.submodule {
            options = placed // {
              port = port 7000;
              host = mkOption {
                type = types.nullOr types.str;
                default = null;
                description = "The host name clients use; by default `<edge>.<address with dashes>.sslip.io`, which needs no DNS.";
              };
              https = mkOption {
                type = types.bool;
                default = true;
                description = "Serve clients through Caddy with a Let's Encrypt certificate; otherwise only on WireGuard, in plain HTTP.";
              };
              writesPerSecond = mkOption {
                type = types.number;
                default = 5;
              };
              writeBurst = mkOption {
                type = types.number;
                default = 20;
              };
            };
          }
        );
      };
      egress = mkOption {
        default = null;
        description = "The proxy engines reach hosted models through.";
        type = types.nullOr (
          types.submodule {
            options = placed // {
              port = mkOption {
                type = types.port;
                default = 3128;
              };
              allowHosts = mkOption {
                type = types.listOf types.str;
                default = [ ];
                description = "Hosts it may reach; without any, every public host.";
              };
            };
          }
        );
      };
      operator = {
        name = mkOption {
          type = types.str;
          default = "ops";
          description = "The operator's certificate (role admin).";
        };
        machine = mkOption {
          type = types.str;
          description = "The machine `babel-admin` runs on, holding the operator's certificate.";
        };
      };
      monitoring = mkOption {
        default = null;
        description = "Prometheus and Grafana on one machine, collecting the metrics of every service and machine.";
        type = types.nullOr (
          types.submodule {
            options = placed // {
              retention = mkOption {
                type = types.str;
                default = "30d";
                description = "How long Prometheus keeps what it collected.";
              };
            };
          }
        );
      };
      backupCopies = mkOption {
        default = null;
        description = "A machine keeping a copy of every store's backups, fetched every 15 minutes over WireGuard: what is left when a store's machine and its standby are both lost. Best on a machine holding neither.";
        type = types.nullOr (types.submodule { options = placed; });
      };
      acmeEmail = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Told to Let's Encrypt for the Edges' certificates.";
      };
    };
  };

  # ---- Services and where they are ---------------------------------------

  wgAddress = machine: cluster.machines.${machine}.wireguardAddress;
  portOf =
    base: name: service:
    if service.port != null then service.port else base + index name;

  # name -> { kind, name, port, url, ... } for every service of the cluster.
  services = {
    account = cluster.account // {
      kind = "account";
      name = "account";
      port = 7101;
      stores = true;
    };
    directory = cluster.directory // {
      kind = "directory";
      name = "directory";
      port = 7102;
      stores = true;
    };
  }
  // lib.mapAttrs (
    name: cell:
    cell
    // {
      kind = "message-cell";
      inherit name;
      port = portOf 7110 name cell;
      stores = true;
    }
  ) cluster.messageCells
  // lib.mapAttrs (
    name: cell:
    cell
    // {
      kind = "translation-cell";
      inherit name;
      port = portOf 7120 name cell;
      stores = true;
    }
  ) cluster.translationCells
  // lib.mapAttrs (
    name: media:
    media
    // {
      kind = "media";
      inherit name;
      port = portOf 7130 name media;
      stores = true;
    }
  ) cluster.media
  // lib.mapAttrs (
    name: engine:
    engine
    // {
      kind = "engine";
      inherit name;
      port = portOf 7140 name engine;
      stores = false;
      standby = null;
    }
  ) cluster.engines
  // lib.mapAttrs (
    name: edge:
    edge
    // {
      kind = "edge";
      inherit name;
      port = portOf 7000 name edge;
      stores = false;
      standby = null;
    }
  ) cluster.edges;

  url =
    service:
    "${
      if service.kind == "edge" then "http" else "https"
    }://${wgAddress service.machine}:${toString service.port}";

  ofKind = kind: lib.filter (service: service.kind == kind) (lib.attrValues services);
  entry = service: {
    inherit (service) name;
    url = url service;
    # Listen on every interface; the firewall opens the port on WireGuard
    # only, and services need not wait for its address.
    listen = "0.0.0.0:${toString service.port}";
  };

  clusterFile = toml.generate "cluster.toml" (
    {
      pki = "${cfg.secretsDir}/pki";
      account = entry services.account;
      directory = entry services.directory;
      message_cell = map entry (ofKind "message-cell");
      translation_cell = map entry (ofKind "translation-cell");
      edge = map entry (ofKind "edge");
    }
    // lib.optionalAttrs (cluster.engines != { }) { engine = map entry (ofKind "engine"); }
    // lib.optionalAttrs (cluster.media != { }) { media = map entry (ofKind "media"); }
  );

  here = lib.filter (service: service.machine == cfg.machine) (lib.attrValues services);
  standbysHere = lib.filter (service: service.stores && service.standby == cfg.machine) (
    lib.attrValues services
  );
  egressHere = cluster.egress != null && cluster.egress.machine == cfg.machine;
  edgesHere = lib.filter (service: service.kind == "edge") here;
  llamasHere = lib.filter (service: service.kind == "engine" && service.llama != null) here;

  edgeHost =
    edge:
    if edge.host != null then
      edge.host
    else
      "${edge.name}.${
        lib.replaceStrings [ "." ] [ "-" ] cluster.machines.${edge.machine}.address
      }.sslip.io";

  pkiDir = "${cfg.secretsDir}/pki";
  credentials =
    name: extra:
    map (file: "${file}:${pkiDir}/${file}") (
      [
        "ca.crt"
        "${name}.crt"
        "${name}.key"
      ]
      ++ extra
    );

  hardening = {
    DynamicUser = true;
    NoNewPrivileges = true;
    ProtectSystem = "strict";
    ProtectHome = true;
    PrivateTmp = true;
    PrivateDevices = true;
    ProtectKernelTunables = true;
    ProtectKernelModules = true;
    ProtectControlGroups = true;
    ProtectClock = true;
    ProtectHostname = true;
    RestrictNamespaces = true;
    RestrictRealtime = true;
    RestrictSUIDSGID = true;
    LockPersonality = true;
    CapabilityBoundingSet = "";
    RestrictAddressFamilies = [
      "AF_INET"
      "AF_INET6"
      "AF_UNIX"
    ];
    SystemCallArchitectures = "native";
    SystemCallFilter = [ "@system-service" ];
    UMask = "0077";
    Restart = "always";
    RestartSec = 2;
  };

  environment = {
    BABEL_LOG_FORMAT = cfg.logFormat;
    RUST_LOG = cfg.logLevel;
  };

  binary = kind: "${cfg.package}/bin/babel-${kind}";
  metricsPort = service: service.port + 2000;
  # A standby serves its metrics on the same port as its primary, on
  # another machine.
  common = service: [
    "--cluster"
    "/etc/babelchat/cluster.toml"
    "--name"
    service.name
    "--metrics"
    "0.0.0.0:${toString (metricsPort service)}"
  ];
  storeArgs =
    service:
    [
      "--data"
      "/var/lib/babelchat/${service.name}"
    ]
    ++ lib.optionals service.backups [
      "--backup-dir"
      "/var/lib/babelchat-backups/${service.name}"
    ];

  argsOf =
    service:
    common service
    ++ [
      "--pki"
      "%d"
    ]
    ++ (
      if service.kind == "account" then
        storeArgs service
        ++ [
          "--signups"
          service.signups
        ]
      else if lib.elem service.kind [
        "directory"
        "message-cell"
        "media"
      ] then
        storeArgs service
      else if service.kind == "translation-cell" then
        storeArgs service
        ++ [
          "--plan"
          service.plan
        ]
        ++ lib.optionals (service.dailyBudget != null) [
          "--daily-budget"
          (toString service.dailyBudget)
        ]
      else if service.kind == "engine" then
        [
          "--models"
          (toml.generate "${service.name}-models.toml" { model = service.models; })
        ]
        ++ lib.optionals (cluster.egress != null) [
          "--egress-proxy"
          "http://${wgAddress cluster.egress.machine}:${toString cluster.egress.port}"
        ]
      else
        [
          "--writes-per-second"
          (toString service.writesPerSecond)
          "--write-burst"
          (toString service.writeBurst)
        ]
    );

  extraCredentials =
    service:
    if service.kind == "account" then
      [ "token.key" ]
    else if service.kind == "edge" then
      [ "token.pub" ]
    else
      [ ];

  serviceUnit = service: {
    name = "babelchat-${service.name}";
    value = {
      description = "BabelChat ${service.kind} ${service.name}";
      # Services read the cluster file when they start.
      restartTriggers = [ clusterFile ];
      wantedBy = [ "multi-user.target" ];
      wants = [ "network-online.target" ];
      after = [
        "network-online.target"
        "wireguard-${cluster.wireguard.interface}.service"
      ]
      ++ lib.optionals (service.kind == "engine" && service.llama != null) [
        "babelchat-llama-${service.name}.service"
      ];
      inherit environment;
      serviceConfig =
        hardening
        // {
          ExecStart = lib.escapeShellArgs ([ (binary service.kind) ] ++ argsOf service);
          LoadCredential = credentials service.name (extraCredentials service);
        }
        // lib.optionalAttrs service.stores {
          StateDirectory = [
            "babelchat/${service.name}"
          ]
          ++ lib.optional service.backups "babelchat-backups/${service.name}";
        }
        // lib.optionalAttrs (service.kind == "engine" && service.environmentFile) {
          EnvironmentFile = "${cfg.secretsDir}/${service.name}.env";
        };
    };
  };

  standbyUnit = service: {
    name = "babelchat-standby-${service.name}";
    value = {
      description = "BabelChat standby of ${service.name}";
      restartTriggers = [ clusterFile ];
      wantedBy = [ "multi-user.target" ];
      wants = [ "network-online.target" ];
      after = [
        "network-online.target"
        "wireguard-${cluster.wireguard.interface}.service"
      ];
      inherit environment;
      serviceConfig = hardening // {
        # The same directory the service would use here: failing over is
        # stopping this and placing the service on this machine.
        ExecStart = lib.escapeShellArgs (
          [ (binary "standby") ]
          ++ common service
          ++ [
            "--pki"
            "%d"
            "--data"
            "/var/lib/babelchat/${service.name}"
          ]
        );
        StateDirectory = [ "babelchat/${service.name}" ];
        LoadCredential = credentials service.name [ ];
      };
    };
  };

  llamaUnit =
    engine:
    let
      llama = engine.llama;
      file = "/var/lib/babelchat-llama/${engine.name}.gguf";
      fetch = pkgs.writeShellScript "babelchat-fetch-${engine.name}" ''
        set -eu
        if [ -f ${file} ] && echo "${llama.sha256}  ${file}" | ${pkgs.coreutils}/bin/sha256sum -c --status; then
          exit 0
        fi
        ${pkgs.curl}/bin/curl -L --fail --retry 3 -o ${file}.part ${lib.escapeShellArg llama.url}
        echo "${llama.sha256}  ${file}.part" | ${pkgs.coreutils}/bin/sha256sum -c
        mv ${file}.part ${file}
      '';
    in
    {
      name = "babelchat-llama-${engine.name}";
      value = {
        description = "llama.cpp serving ${engine.name}'s local model";
        wantedBy = [ "multi-user.target" ];
        wants = [ "network-online.target" ];
        after = [ "network-online.target" ];
        serviceConfig = hardening // {
          # The first start downloads the model.
          ExecStartPre = fetch;
          TimeoutStartSec = "30min";
          ExecStart = lib.escapeShellArgs (
            [
              "${pkgs.llama-cpp}/bin/llama-server"
              "-m"
              file
              "--host"
              "127.0.0.1"
              "--port"
              (toString llama.port)
              "-c"
              (toString llama.contextSize)
              # One slot: requests queue, and consecutive ones of a
              # conversation reuse the prompt they share from the cache.
              "-np"
              "1"
              "--jinja"
            ]
            ++ lib.optionals (llama.threads != null) [
              "-t"
              (toString llama.threads)
            ]
          );
          StateDirectory = [ "babelchat-llama" ];
          # llama.cpp maps the model; let it allocate executable memory if
          # a backend wants to.
          MemoryDenyWriteExecute = false;
        };
      };
    };

  egressUnit = {
    name = "babelchat-egress";
    value = {
      description = "BabelChat egress proxy";
      wantedBy = [ "multi-user.target" ];
      after = [ "wireguard-${cluster.wireguard.interface}.service" ];
      inherit environment;
      serviceConfig = hardening // {
        ExecStart = lib.escapeShellArgs (
          [
            (binary "egress")
            "--listen"
            "0.0.0.0:${toString cluster.egress.port}"
          ]
          ++ lib.concatMap (host: [
            "--allow-host"
            host
          ]) cluster.egress.allowHosts
        );
      };
    };
  };

  others = lib.filterAttrs (name: machine: name != cfg.machine && !machine.cutOff) cluster.machines;

  # ---- Copies of backups -------------------------------------------------

  # Machines with backups lend them, read-only, to the backupCopies machine
  # (rsync's daemon, on WireGuard only). It fetches each service's from the
  # service's machine into <service>/<machine>: after a failover, the copy
  # of the old machine's backups stays as it was.
  backedUp = lib.filter (service: service.stores && service.backups) (lib.attrValues services);
  lendsBackups =
    cluster.backupCopies != null && lib.any (service: service.machine == cfg.machine) backedUp;
  copiesHere = cluster.backupCopies != null && cluster.backupCopies.machine == cfg.machine;
  copiesDir = "/var/lib/babelchat-backup-copies";
  # Read by node_exporter (its textfile collector).
  textfileDir = "/var/lib/babelchat-metrics";

  copyScript = pkgs.writeShellScript "babelchat-backup-copy" ''
    set -u
    copy() {
      local name=$1 machine=$2 address=$3 status=0
      mkdir -p ${copiesDir}/$name/$machine
      # Changed files are put in place together at the end: a copy cut
      # short holds no backup without its files.
      ${pkgs.rsync}/bin/rsync -rt --delete --delay-updates --contimeout=10 --timeout=60 \
        rsync://$address/babelchat-backups/$name/ ${copiesDir}/$name/$machine/ || status=$?
      # 24: files vanished on the way (an old backup purged meanwhile).
      if [ $status -ne 0 ] && [ $status -ne 24 ]; then
        echo "copying the backups of $name from $machine failed ($status)" >&2
        return
      fi
      echo "babel_backup_copy_last_success_timestamp_seconds{service=\"$name\"} $(date +%s)" \
        >${textfileDir}/$name.prom.new
      mv ${textfileDir}/$name.prom.new ${textfileDir}/$name.prom
    }
    ${lib.concatMapStrings (
      service: "copy ${service.name} ${service.machine} ${wgAddress service.machine}\n"
    ) backedUp}
    # Services no longer in the cluster.
    keep=" ${lib.concatMapStringsSep " " (service: service.name) backedUp} "
    for file in ${textfileDir}/*.prom; do
      [ -e "$file" ] || continue
      case $keep in
        *" $(basename "$file" .prom) "*) ;;
        *) rm -f "$file" ;;
      esac
    done
  '';

  # ---- Monitoring --------------------------------------------------------

  nodeExporterPort = 9100;
  monitoringHere = cluster.monitoring != null && cluster.monitoring.machine == cfg.machine;

  # What Prometheus collects: every service and standby, every machine.
  target = machine: port: labels: {
    targets = [ "${wgAddress machine}:${toString port}" ];
    labels = labels // {
      inherit machine;
    };
  };
  serviceTargets =
    map (
      service:
      target service.machine (metricsPort service) {
        service = service.name;
        inherit (service) kind;
        role = "primary";
      }
    ) (lib.attrValues services)
    ++ map (
      service:
      target service.standby (metricsPort service) {
        service = service.name;
        inherit (service) kind;
        role = "standby";
      }
    ) (lib.filter (service: service.stores && service.standby != null) (lib.attrValues services));
  machineTargets = lib.mapAttrsToList (name: _: target name nodeExporterPort { }) (
    lib.filterAttrs (_: machine: !machine.cutOff) cluster.machines
  );

  # Shown in Grafana ("Firing alerts") and Prometheus; nothing sends them
  # anywhere yet.
  alert = name: expr: for: summary: {
    alert = name;
    inherit expr for;
    annotations = { inherit summary; };
  };
  alertRules = {
    groups = [
      {
        name = "babelchat";
        rules = [
          (alert "TargetDown" "up == 0" "2m"
            "{{ $labels.service }}{{ $labels.job }} on {{ $labels.machine }} does not answer"
          )
          (alert "UnitFailed"
            ''node_systemd_unit_state{state="failed", name=~"(babelchat|caddy|wireguard|prometheus|grafana).*"} == 1''
            "1m"
            "{{ $labels.name }} failed on {{ $labels.machine }}"
          )
          (alert "StandbyOutOfTouch" "time() - babel_standby_last_contact_timestamp_seconds > 120" "2m"
            "The standby of {{ $labels.service }} on {{ $labels.machine }} has not heard from its primary"
          )
          (alert "StandbyBehind" "babel_standby_behind > 10000" "5m"
            "The standby of {{ $labels.service }} is {{ $value }} writes behind"
          )
          (alert "BackupStale" "time() - babel_backup_last_success_timestamp_seconds > 3600" "5m"
            "{{ $labels.service }} has not backed up for an hour"
          )
          (alert "BackupFailing" "increase(babel_backup_failures_total[1h]) > 0" "0m"
            "Backing {{ $labels.service }} up failed"
          )
          (alert "BackupCopyStale" "time() - babel_backup_copy_last_success_timestamp_seconds > 3600" "5m"
            "The backups of {{ $labels.service }} have not been copied for an hour"
          )
          (alert "CertificateExpiring" "babel_certificate_expiry_timestamp_seconds - time() < 30 * 86400" "1h"
            "The certificate of {{ $labels.service }} expires within 30 days: deploy.sh renew"
          )
          (alert "OutboxStuck" "rate(babel_outbox_retries_total[5m]) > 0.5" "10m"
            "{{ $labels.service }} cannot deliver guild events"
          )
          (alert "ServerErrors" ''sum by (service) (rate(babel_requests_total{status=~"5.."}[5m])) > 0.1''
            "10m"
            "{{ $labels.service }} answers with server errors"
          )
          (alert "TranslationsFailing"
            ''sum(rate(babel_translation_job_seconds_count{outcome!="ok"}[10m])) / sum(rate(babel_translation_job_seconds_count[10m])) > 0.1''
            "10m"
            "More than one translation in ten fails"
          )
          (alert "TranslationsSlow"
            "histogram_quantile(0.95, sum by (le) (rate(babel_translation_job_seconds_bucket[10m]))) > 30"
            "10m"
            "Translations take more than 30 s (p95)"
          )
          (alert "DiskFilling"
            ''node_filesystem_avail_bytes{mountpoint="/"} / node_filesystem_size_bytes{mountpoint="/"} < 0.15''
            "10m"
            "{{ $labels.machine }}'s disk is more than 85% full"
          )
          (alert "MemoryLow" "node_memory_MemAvailable_bytes / node_memory_MemTotal_bytes < 0.1" "10m"
            "{{ $labels.machine }} has less than 10% of its memory free"
          )
        ];
      }
    ];
  };

  dashboards = pkgs.writeTextDir "babelchat.json" (builtins.toJSON (import ./dashboard.nix));

  # Every port of the cluster with what uses it, to refuse two on one
  # machine.
  portsHere =
    map (service: service.port) here
    ++ map metricsPort (here ++ standbysHere)
    ++ [ nodeExporterPort ]
    ++ lib.optional egressHere cluster.egress.port
    ++ map (engine: engine.llama.port) llamasHere
    ++ lib.optionals monitoringHere [
      9090
      3000
    ];
in
{
  options.services.babelchat = {
    enable = mkEnableOption "the BabelChat services placed on this machine";
    package = mkOption {
      type = types.package;
      description = "The BabelChat server binaries.";
    };
    cluster = mkOption {
      type = clusterType;
      description = "The whole cluster: machines, and which service runs on which.";
    };
    machine = mkOption {
      type = types.str;
      description = "This machine's name in `cluster.machines`.";
    };
    secretsDir = mkOption {
      type = types.str;
      default = "/var/lib/babelchat-secrets";
      description = "Where the deploy tool puts this machine's keys and certificates.";
    };
    logFormat = mkOption {
      type = types.enum [
        "json"
        "text"
      ];
      default = "json";
      description = "`json`: one object per line in the journal, for log collectors.";
    };
    logLevel = mkOption {
      type = types.str;
      default = "info";
      description = "RUST_LOG of every service.";
    };
    secretFiles = mkOption {
      type = types.listOf types.str;
      readOnly = true;
      description = "The files this machine needs in `secretsDir`, for the deploy tool.";
      default = lib.unique (
        [
          "wireguard.key"
          "pki/ca.crt"
        ]
        ++ lib.concatMap (
          service:
          map (file: "pki/${file}") (
            [
              "${service.name}.crt"
              "${service.name}.key"
            ]
            ++ extraCredentials service
          )
          ++ lib.optional (service.kind == "engine" && service.environmentFile) "${service.name}.env"
        ) here
        ++ lib.concatMap (service: [
          "pki/${service.name}.crt"
          "pki/${service.name}.key"
        ]) standbysHere
        ++ lib.optionals (cluster.operator.machine == cfg.machine) [
          "pki/${cluster.operator.name}.crt"
          "pki/${cluster.operator.name}.key"
        ]
        ++ lib.optional monitoringHere "grafana.key"
      );
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = cluster.machines ? ${cfg.machine};
        message = "babelchat: ${cfg.machine} is not in cluster.machines";
      }
      {
        assertion = lib.all (service: cluster.machines ? ${service.machine}) (lib.attrValues services);
        message = "babelchat: a service is placed on a machine cluster.machines does not list";
      }
      {
        assertion = lib.all (service: service.standby != service.machine) (lib.attrValues services);
        message = "babelchat: a standby on its primary's machine copies nothing worth having";
      }
      {
        assertion = lib.length portsHere == lib.length (lib.unique portsHere);
        message = "babelchat: two services of ${cfg.machine} share a port";
      }
    ];

    environment.etc."babelchat/cluster.toml".source = clusterFile;

    environment.systemPackages = lib.optional (cluster.operator.machine == cfg.machine) (
      pkgs.writeShellScriptBin "babel-admin" ''
        exec ${binary "admin"} --cluster /etc/babelchat/cluster.toml \
          --pki ${pkiDir} --name ${cluster.operator.name} "$@"
      ''
    );

    systemd.tmpfiles.rules = [
      "d ${cfg.secretsDir} 0700 root root -"
    ]
    ++ lib.optionals copiesHere [
      "d ${copiesDir} 0700 root root -"
      "d ${textfileDir} 0755 root root -"
    ];

    networking.wireguard.interfaces.${cluster.wireguard.interface} = {
      ips = [ "${wgAddress cfg.machine}/${toString cluster.wireguard.prefixLength}" ];
      listenPort = cluster.wireguard.port;
      privateKeyFile = "${cfg.secretsDir}/wireguard.key";
      peers = lib.mapAttrsToList (_: machine: {
        publicKey = machine.wireguardKey;
        allowedIPs = [ "${machine.wireguardAddress}/32" ];
        endpoint = "${machine.address}:${toString cluster.wireguard.port}";
        persistentKeepalive = 25;
      }) others;
    };

    networking.firewall = {
      allowedUDPPorts = [ cluster.wireguard.port ];
      allowedTCPPorts = lib.optionals (lib.any (edge: edge.https) edgesHere) [
        80
        443
      ];
      interfaces.${cluster.wireguard.interface}.allowedTCPPorts =
        map (service: service.port) here
        ++ map metricsPort (here ++ standbysHere)
        ++ [ nodeExporterPort ]
        ++ lib.optional egressHere cluster.egress.port
        ++ lib.optional lendsBackups 873;
    };

    systemd.services =
      lib.listToAttrs (
        map serviceUnit here
        ++ map standbyUnit standbysHere
        ++ map llamaUnit llamasHere
        ++ lib.optional egressHere egressUnit
      )
      // lib.optionalAttrs monitoringHere {
        grafana.serviceConfig.LoadCredential = [ "secret_key:${cfg.secretsDir}/grafana.key" ];
      }
      // lib.optionalAttrs copiesHere {
        babelchat-backup-copy = {
          description = "Copy every store's backups here";
          after = [ "wireguard-${cluster.wireguard.interface}.service" ];
          serviceConfig = {
            Type = "oneshot";
            ExecStart = copyScript;
            ProtectSystem = "strict";
            ReadWritePaths = [
              copiesDir
              textfileDir
            ];
            ProtectHome = true;
            PrivateTmp = true;
            PrivateDevices = true;
            NoNewPrivileges = true;
          };
        };
      };

    systemd.timers.babelchat-backup-copy = mkIf copiesHere {
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnBootSec = "5min";
        OnUnitActiveSec = "15min";
      };
    };

    services.rsyncd = mkIf lendsBackups {
      enable = true;
      settings = {
        # As root, to read every service's own; for the backupCopies
        # machine only, on WireGuard only (the firewall).
        globalSection = {
          uid = "root";
          gid = "root";
          "use chroot" = true;
          "hosts allow" = wgAddress cluster.backupCopies.machine;
          "hosts deny" = "*";
          "reverse lookup" = false;
        };
        sections.babelchat-backups = {
          path = "/var/lib/private/babelchat-backups";
          "read only" = true;
          list = false;
        };
      };
    };

    services.prometheus = lib.mkMerge [
      {
        # On WireGuard only (the firewall), for the monitoring machine.
        exporters.node = {
          enable = true;
          listenAddress = "0.0.0.0";
          port = nodeExporterPort;
          enabledCollectors = [ "systemd" ];
          extraFlags = lib.optional copiesHere "--collector.textfile.directory=${textfileDir}";
        };
      }
      (mkIf monitoringHere {
        enable = true;
        listenAddress = "127.0.0.1";
        port = 9090;
        retentionTime = cluster.monitoring.retention;
        globalConfig.scrape_interval = "15s";
        scrapeConfigs = [
          {
            job_name = "babelchat";
            static_configs = serviceTargets;
          }
          {
            job_name = "node";
            static_configs = machineTargets;
          }
        ];
        rules = [ (builtins.toJSON alertRules) ];
      })
    ];

    services.grafana = mkIf monitoringHere {
      enable = true;
      # Grafana fetches its Prometheus data source on its first start unless
      # it is given one: take it from nixpkgs (no downloads, below).
      declarativePlugins = [ pkgs.grafanaPlugins.prometheus ];
      settings = {
        server = {
          http_addr = "127.0.0.1";
          http_port = 3000;
        };
        # Only reachable through SSH to this machine: no accounts. Viewers may
        # still try queries (Explore).
        "auth.anonymous" = {
          enabled = true;
          org_role = "Viewer";
        };
        users.viewers_can_edit = true;
        auth.disable_login_form = true;
        security.secret_key = "$__file{/run/credentials/grafana.service/secret_key}";
        analytics = {
          reporting_enabled = false;
          check_for_updates = false;
          check_for_plugin_updates = false;
        };
        news.news_feed_enabled = false;
        # Everything it needs comes with it: no downloads from grafana.com.
        plugins = {
          preinstall_disabled = true;
          public_key_retrieval_disabled = true;
        };
      };
      provision = {
        enable = true;
        datasources.settings.datasources = [
          {
            name = "Prometheus";
            type = "prometheus";
            uid = "prometheus";
            url = "http://127.0.0.1:9090";
            isDefault = true;
          }
        ];
        dashboards.settings.providers = [
          {
            name = "BabelChat";
            options.path = dashboards;
          }
        ];
      };
    };

    services.caddy = mkIf (lib.any (edge: edge.https) edgesHere) {
      enable = true;
      email = cluster.acmeEmail;
      virtualHosts = lib.listToAttrs (
        map (edge: {
          name = edgeHost edge;
          value.extraConfig = ''
            reverse_proxy 127.0.0.1:${toString edge.port}
          '';
        }) (lib.filter (edge: edge.https) edgesHere)
      );
    };
  };
}
