# The BabelChat server binaries: every service, the standby, the egress
# proxy and the operators' tools (babel-pki, babel-admin).
{
  lib,
  rustPlatform,
  pkg-config,
  openssl,
  llvmPackages,
}:

let
  root = ../.;
  # The whole workspace (cargo reads every member's manifest), without
  # build output, local data and models.
  src = lib.fileset.toSource {
    inherit root;
    fileset = lib.fileset.unions [
      (root + "/Cargo.toml")
      (root + "/Cargo.lock")
      (root + "/rust-toolchain.toml")
      (root + "/crates")
      (root + "/server")
      (root + "/client")
      (root + "/gpui-reloaded")
      (root + "/gpui-md3e")
    ];
  };
  packages = [
    "babel-account"
    "babel-directory"
    "babel-message-cell"
    "babel-translation-cell"
    "babel-engine"
    "babel-edge"
    "babel-net"
  ];
in
rustPlatform.buildRustPackage {
  pname = "babelchat-server";
  version = "0.1.0";
  inherit src;

  cargoLock = {
    lockFile = ../Cargo.lock;
    # Git dependencies of the desktop app's UI crates; cargo needs them
    # vendored even though no server binary uses them.
    allowBuiltinFetchGit = true;
  };

  cargoBuildFlags = lib.concatMap (package: [
    "-p"
    package
  ]) packages;
  # The tests start servers and use the network.
  doCheck = false;
  # Smaller to build and to copy to servers; panics still say where.
  CARGO_PROFILE_RELEASE_DEBUG = "0";

  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ openssl ];
  # rocksdb generates its bindings with bindgen.
  LIBCLANG_PATH = "${llvmPackages.libclang.lib}/lib";

  meta = {
    description = "BabelChat servers: chat without language barriers";
    license = lib.licenses.asl20;
    platforms = lib.platforms.linux;
    mainProgram = "babel-admin";
  };
}
