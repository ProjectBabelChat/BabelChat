# The NixOS module on two virtual machines: services finding each other
# over WireGuard and mutual TLS, ports closed outside WireGuard, sign-ups by
# invitation, a standby following its primary, backups copied to another
# machine, signing other devices out, Prometheus collecting every service
# and Grafana showing them.
#
#   nix build .#checks.x86_64-linux.cluster
{ pkgs, package }:

let
  inherit (pkgs) lib;
  keys = import (pkgs.path + "/nixos/tests/wireguard/snakeoil-keys.nix");

  # Certificates for the services below (made at build time: test keys).
  pki = pkgs.runCommand "babelchat-test-pki" { } ''
    mkdir -p $out && cd $out
    cat > cluster.toml <<EOF
    pki = "pki"
    [account]
    name = "account"
    url = "https://x:1"
    [directory]
    name = "directory"
    url = "https://x:1"
    [[message_cell]]
    name = "message-cell-0"
    url = "https://x:1"
    [[translation_cell]]
    name = "translation-cell-0"
    url = "https://x:1"
    [[edge]]
    name = "edge-0"
    url = "http://x:1"
    EOF
    ${package}/bin/babel-pki cluster --cluster cluster.toml
    ${package}/bin/babel-pki issue --dir pki --role admin --name ops
  '';

  cluster =
    nodes:
    let
      address = node: nodes.${node}.networking.primaryIPAddress;
    in
    {
      machines = {
        core = {
          address = address "core";
          wireguardAddress = "10.8.0.2";
          wireguardKey = keys.peer0.publicKey;
        };
        edge = {
          address = address "edge";
          wireguardAddress = "10.8.0.11";
          wireguardKey = lib.removeSuffix "\n" keys.peer1.publicKey;
        };
      };
      account = {
        machine = "core";
        signups = "invite";
      };
      directory.machine = "core";
      messageCells.message-cell-0 = {
        machine = "core";
        standby = "edge";
      };
      translationCells.translation-cell-0 = {
        machine = "core";
        plan = "paid3";
      };
      edges.edge-0 = {
        machine = "edge";
        https = false;
      };
      operator.machine = "core";
      monitoring.machine = "core";
      backupCopies.machine = "edge";
    };

  node =
    name: privateKey:
    { nodes, ... }:
    {
      imports = [ ./module.nix ];
      services.babelchat = {
        enable = true;
        inherit package;
        cluster = cluster nodes;
        machine = name;
      };
      # What deploy.sh would put there.
      system.activationScripts.babelchatSecrets = ''
        mkdir -p /var/lib/babelchat-secrets/pki
        cp ${pki}/pki/* /var/lib/babelchat-secrets/pki/
        echo ${privateKey} > /var/lib/babelchat-secrets/wireguard.key
        echo test-only > /var/lib/babelchat-secrets/grafana.key
        chmod -R u+w,go-rwx /var/lib/babelchat-secrets
      '';
      environment.systemPackages = [
        pkgs.curl
        pkgs.jq
      ];
      virtualisation.memorySize = if name == "core" then 2048 else 1536;
      # A backup a minute after starting, not 15.
      systemd.services = lib.optionalAttrs (name == "core") {
        babelchat-message-cell-0.environment.BABEL_BACKUP_EVERY_MINS = "1";
      };
    };
in
pkgs.testers.runNixOSTest {
  name = "babelchat-cluster";
  nodes = {
    core = node "core" keys.peer0.privateKey;
    edge = node "edge" keys.peer1.privateKey;
  };

  testScript =
    { nodes, ... }:
    ''
      import json

      start_all()
      for unit in ["account", "directory", "message-cell-0", "translation-cell-0"]:
          core.wait_for_unit(f"babelchat-{unit}.service")
      edge.wait_for_unit("babelchat-edge-0.service")
      edge.wait_for_unit("babelchat-standby-message-cell-0.service")

      with subtest("every service answers over WireGuard and mutual TLS"):
          core.wait_until_succeeds("babel-admin health", timeout=60)

      with subtest("internal ports are closed outside WireGuard"):
          edge.fail("curl -sk --max-time 3 https://${nodes.core.networking.primaryIPAddress}:7101/health")

      def api(path, body, token=None):
          auth = f"-H 'authorization: Bearer {token}'" if token else ""
          out = edge.succeed(
              f"curl -s -X POST http://127.0.0.1:7000/{path} -H 'content-type: application/json' {auth} -d '{json.dumps(body)}'"
          )
          return json.loads(out) if out else None

      with subtest("sign-ups need a code from an operator"):
          alice = {"name": "alice", "password": "correct horse", "locale": "ja-JP"}
          assert api("auth/register", alice) == "invalid_signup_code"
          code = core.succeed("babel-admin signup-code --uses 2").split()[0]
          token = api("auth/register", {**alice, "signup_code": code})["jwt"]

      with subtest("guilds live on the Message Cell, copied by its standby"):
          guild = api("guilds/create_guild", {"name": "Lobby"}, token)
          channel = api(
              "guilds/create_channel",
              {"guild_id": guild["guild"]["id"], "kind": "text", "name": "general"},
              token,
          )["id"]
          sent = api(
              "messages/send_message",
              {"guild_id": guild["guild"]["id"], "channel_id": channel, "text": "こんにちは"},
              token,
          )
          assert sent["message"]["text"] == "こんにちは", sent
          edge.wait_until_succeeds(
              "journalctl -u babelchat-standby-message-cell-0 | grep -q 'following the primary'"
          )

      with subtest("backups are copied to another machine, and restore"):
          core.wait_until_succeeds("test -f /var/lib/private/babelchat-backups/message-cell-0/meta/1", timeout=180)
          # For the copies machine only, on WireGuard only.
          core.fail("${pkgs.rsync}/bin/rsync --contimeout=5 rsync://10.8.0.2/babelchat-backups/message-cell-0/")
          edge.fail("${pkgs.rsync}/bin/rsync --contimeout=5 rsync://${nodes.core.networking.primaryIPAddress}/babelchat-backups/message-cell-0/")
          edge.succeed("systemctl start babelchat-backup-copy.service")
          edge.succeed("test -f /var/lib/babelchat-backup-copies/message-cell-0/core/meta/1")
          edge.succeed(
              "${package}/bin/babel-admin restore"
              " --backup-dir /var/lib/babelchat-backup-copies/message-cell-0/core --data /tmp/restored"
          )
          edge.succeed("test -f /tmp/restored/CURRENT")

      with subtest("signing the other devices out revokes their tokens"):
          other = api("auth/login", {"name": "alice", "password": "correct horse"})["jwt"]
          fresh = api("me/sign_out_others", {}, other)["jwt"]
          edge.wait_until_succeeds(
              f"curl -s -o /dev/null -w '%{{http_code}}' -H 'authorization: Bearer {token}' http://127.0.0.1:7000/me/get_settings | grep -q 401"
          )
          edge.succeed(
              f"curl -sf -H 'authorization: Bearer {fresh}' http://127.0.0.1:7000/me/get_settings"
          )

      with subtest("Prometheus collects every service, standby and machine"):
          core.wait_for_unit("prometheus.service")
          edge.succeed("curl -sf http://10.8.0.11:9110/metrics | grep -q babel_standby_behind")
          edge.fail("curl -s --max-time 3 http://${nodes.core.networking.primaryIPAddress}:9101/metrics")
          # 5 services, 1 standby, 2 machines.
          core.wait_until_succeeds(
              "curl -sfG http://127.0.0.1:9090/api/v1/query --data-urlencode 'query=count(up == 1)'"
              " | jq -e '.data.result[0].value[1] == \"8\"'",
              timeout=120,
          )
          core.wait_until_succeeds(
              "curl -sfG http://127.0.0.1:9090/api/v1/query"
              " --data-urlencode 'query=babel_requests_total{service=\"edge-0\", route=\"/messages/send_message\"}'"
              " | jq -e '.data.result | length > 0'",
              timeout=60,
          )
          core.wait_until_succeeds(
              "curl -sfG http://127.0.0.1:9090/api/v1/query"
              " --data-urlencode 'query=babel_backup_copy_last_success_timestamp_seconds{service=\"message-cell-0\"}'"
              " | jq -e '.data.result | length > 0'",
              timeout=60,
          )
          # Certificates of two years, the CA's of ten.
          core.succeed(
              "curl -sfG http://127.0.0.1:9090/api/v1/query"
              " --data-urlencode 'query=min(babel_certificate_expiry_timestamp_seconds) - time() > 700 * 86400'"
              " | jq -e '.data.result | length == 1'"
          )
          core.wait_until_succeeds(
              "curl -sfG http://127.0.0.1:9090/api/v1/query"
              " --data-urlencode 'query=babel_translation_messages_total'"
              " | jq -e '.data.result | length > 0'",
              timeout=60,
          )

      with subtest("Grafana shows the dashboard"):
          core.wait_for_unit("grafana.service")
          # The first start sets its database up: slow on a small machine.
          core.wait_for_open_port(3000, timeout=600)
          core.wait_until_succeeds(
              "curl -sf http://127.0.0.1:3000/api/dashboards/uid/babelchat | jq -e '.dashboard.title == \"BabelChat\"'",
              timeout=60,
          )
          # Through its Prometheus data source, as the panels ask.
          body = json.dumps({
              "from": "now-5m",
              "to": "now",
              "queries": [{"refId": "A", "datasource": {"uid": "prometheus"}, "expr": "up", "instant": True}],
          })
          out = core.succeed(
              f"curl -s -X POST http://127.0.0.1:3000/api/ds/query -H 'content-type: application/json' -d '{body}'"
          )
          frames = json.loads(out).get("results", {}).get("A", {}).get("frames", [])
          assert len(frames) == 8, out
    '';
}
