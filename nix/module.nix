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
# message-cell-N 7110+N, translation-cell-N 7120+N, engine-N 7140+N,
# edge-N 7000+N), so they can share a machine in any combination. Moving a
# service to another machine is changing its `machine`; one that keeps data
# needs its data moved too (docs/deployment.md).
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
      description = "Back the store up every 15 minutes (on the same disk; the standby is the copy elsewhere).";
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
  common = name: [
    "--cluster"
    "/etc/babelchat/cluster.toml"
    "--name"
    name
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
    common service.name
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
      else if service.kind == "directory" || service.kind == "message-cell" then
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
          ++ common service.name
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

  others = lib.filterAttrs (name: _: name != cfg.machine) cluster.machines;

  # Every port of the cluster with what uses it, to refuse two on one
  # machine.
  portsHere =
    map (service: service.port) here
    ++ lib.optional egressHere cluster.egress.port
    ++ map (engine: engine.llama.port) llamasHere;
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

    systemd.tmpfiles.rules = [ "d ${cfg.secretsDir} 0700 root root -" ];

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
        map (service: service.port) here ++ lib.optional egressHere cluster.egress.port;
    };

    systemd.services = lib.listToAttrs (
      map serviceUnit here
      ++ map standbyUnit standbysHere
      ++ map llamaUnit llamasHere
      ++ lib.optional egressHere egressUnit
    );

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
