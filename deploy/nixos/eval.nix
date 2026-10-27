# What deploy.sh builds for a deployment directory (deploy/<name>, holding
# cluster.nix and the secrets made by `deploy.sh init`).
#
# Evaluated from the working tree rather than as a flake, so that changes
# not committed yet and the secrets directory (ignored by git, and holding
# the WireGuard public keys) are seen. Nixpkgs and disko are the versions
# flake.lock pins.
{
  deployment,
  system ? "x86_64-linux",
}:

let
  lock = builtins.fromJSON (builtins.readFile ../../flake.lock);
  input = name: builtins.fetchTree lock.nodes.${name}.locked;
  nixpkgs = input "nixpkgs";
  disko = input "disko";
  pkgs = import nixpkgs { inherit system; };
  inherit (pkgs) lib;

  package = pkgs.callPackage ../../nix/package.nix { };
  description = import (/. + deployment + "/cluster.nix");
  secrets = /. + deployment + "/secrets";

  publicKey =
    name:
    let
      file = secrets + "/wireguard/${name}.pub";
    in
    if builtins.pathExists file then lib.removeSuffix "\n" (builtins.readFile file) else "";
  cluster = description.cluster // {
    machines = lib.mapAttrs (
      name: machine: machine // { wireguardKey = publicKey name; }
    ) description.cluster.machines;
  };

  nixos =
    name:
    import (nixpkgs + "/nixos/lib/eval-config.nix") {
      inherit system;
      modules = [
        ../../nix/module.nix
        (disko + "/module.nix")
        ./machine.nix
        {
          services.babelchat = {
            enable = true;
            inherit package cluster;
            machine = name;
          };
          babelchat.deploy.sshKeys = description.sshKeys;
        }
        (description.extraConfig or { })
      ];
    };
  machines = lib.mapAttrs (name: _: nixos name) cluster.machines;
  any = lib.head (lib.attrValues machines);
in
{
  inherit package;
  operator = cluster.operator.machine;

  # One line per machine for deploy.sh: name, public address, then the
  # files of secrets/ it needs.
  inventory = lib.concatMapStrings (
    name:
    let
      babelchat = machines.${name}.config.services.babelchat;
    in
    "${name} ${cluster.machines.${name}.address} ${lib.concatStringsSep " " babelchat.secretFiles}\n"
  ) (lib.attrNames machines);

  # The cluster file, for making the certificates.
  clusterFile = any.config.environment.etc."babelchat/cluster.toml".source;

  toplevel = lib.mapAttrs (_: machine: machine.config.system.build.toplevel) machines;
  disko = lib.mapAttrs (_: machine: machine.config.system.build.diskoScript) machines;
}
