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

  # What `deploy.sh failover` changed, over cluster.nix: where services
  # that keep data run now ({ "<service>": { machine, standby } }), and the
  # machines lost on the way (cut off from the others).
  placementFile = /. + deployment + "/placement.json";
  placement = {
    services = { };
    cutOff = [ ];
  }
  // lib.optionalAttrs (builtins.pathExists placementFile) (
    builtins.fromJSON (builtins.readFile placementFile)
  );
  place = name: service: service // (placement.services.${name} or { });

  cluster = description.cluster // {
    machines = lib.mapAttrs (
      name: machine:
      machine
      // {
        wireguardKey = publicKey name;
        cutOff = lib.elem name placement.cutOff;
      }
    ) description.cluster.machines;
    account = place "account" description.cluster.account;
    directory = place "directory" description.cluster.directory;
    messageCells = lib.mapAttrs place description.cluster.messageCells;
    translationCells = lib.mapAttrs place description.cluster.translationCells;
  };

  # Every service that keeps data, with its machine and standby.
  stores = [
    (cluster.account // { name = "account"; })
    (cluster.directory // { name = "directory"; })
  ]
  ++ lib.mapAttrsToList (name: service: service // { inherit name; }) (
    cluster.messageCells // cluster.translationCells
  );

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
  monitoring =
    if cluster ? monitoring && cluster.monitoring != null then cluster.monitoring.machine else "";
  backupCopies =
    if cluster ? backupCopies && cluster.backupCopies != null then cluster.backupCopies.machine else "";

  # One line per machine for deploy.sh: name, public address, then the
  # files of secrets/ it needs. Machines cut off are left out.
  inventory = lib.concatMapStrings (
    name:
    let
      babelchat = machines.${name}.config.services.babelchat;
    in
    "${name} ${cluster.machines.${name}.address} ${lib.concatStringsSep " " babelchat.secretFiles}\n"
  ) (lib.attrNames (lib.filterAttrs (_: machine: !machine.cutOff) cluster.machines));

  # One line per service that keeps data: name, machine, standby (or -).
  stores = lib.concatMapStrings (
    service:
    "${service.name} ${service.machine} ${
      if service.standby or null == null then "-" else service.standby
    }\n"
  ) stores;

  # Every machine's public address, cut off or not.
  addresses = lib.concatStrings (
    lib.mapAttrsToList (name: machine: "${name} ${machine.address}\n") cluster.machines
  );

  # The cluster file, for making the certificates.
  clusterFile = any.config.environment.etc."babelchat/cluster.toml".source;

  toplevel = lib.mapAttrs (_: machine: machine.config.system.build.toplevel) machines;
  disko = lib.mapAttrs (_: machine: machine.config.system.build.diskoScript) machines;
}
