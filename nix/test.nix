# The NixOS module on two virtual machines: services finding each other
# over WireGuard and mutual TLS, ports closed outside WireGuard, sign-ups by
# invitation, a standby following its primary, signing other devices out.
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
        chmod -R u+w,go-rwx /var/lib/babelchat-secrets
      '';
      environment.systemPackages = [
        pkgs.curl
        pkgs.jq
      ];
      virtualisation.memorySize = 1536;
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

      with subtest("signing the other devices out revokes their tokens"):
          other = api("auth/login", {"name": "alice", "password": "correct horse"})["jwt"]
          fresh = api("me/sign_out_others", {}, other)["jwt"]
          edge.wait_until_succeeds(
              f"curl -s -o /dev/null -w '%{{http_code}}' -H 'authorization: Bearer {token}' http://127.0.0.1:7000/me/get_settings | grep -q 401"
          )
          edge.succeed(
              f"curl -sf -H 'authorization: Bearer {fresh}' http://127.0.0.1:7000/me/get_settings"
          )
    '';
}
