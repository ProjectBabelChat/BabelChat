{
  description = "BabelChat: chat without language barriers";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # Disk layouts for installing servers (deploy/nixos).
    disko = {
      url = "github:nix-community/disko";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      disko,
    }:
    let
      lib = nixpkgs.lib;
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = lib.genAttrs systems;
      pkgsFor = system: import nixpkgs { inherit system; };
    in
    {
      # The server binaries.
      packages = forAllSystems (
        system:
        let
          server = (pkgsFor system).callPackage ./nix/package.nix { };
        in
        {
          inherit server;
          default = server;
        }
      );

      # services.babelchat: the services a machine runs, from a description
      # of the whole cluster (nix/module.nix).
      nixosModules.default =
        { pkgs, ... }:
        {
          imports = [ ./nix/module.nix ];
          services.babelchat.package = lib.mkDefault self.packages.${pkgs.stdenv.hostPlatform.system}.server;
        };

      # Two machines in virtual machines: WireGuard, mutual TLS, sign-ups by
      # invitation, a standby (`nix build .#checks.x86_64-linux.cluster`).
      checks.x86_64-linux.cluster = import ./nix/test.nix {
        pkgs = pkgsFor "x86_64-linux";
        package = self.packages.x86_64-linux.server;
      };

      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          # Loaded with dlopen at runtime by gpui's Linux backend.
          runtimeLibs = with pkgs; [
            wayland
            libxkbcommon
            vulkan-loader
            libglvnd
            fontconfig
            freetype
            libxcb
            libx11
            libxcursor
            libxi
            libxrandr
          ];
        in
        {
          default = pkgs.mkShell {
            nativeBuildInputs = with pkgs; [
              rustc
              cargo
              rustfmt
              clippy
              rust-analyzer
              pkg-config
              cmake
              python3
              # Debugging helpers for the gallery (screenshots, synthetic input).
              grim
              wtype
            ];
            buildInputs =
              runtimeLibs
              ++ (with pkgs; [
                openssl
                zstd
              ]);
            # rocksdb generates its bindings with bindgen.
            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
            LD_LIBRARY_PATH = lib.makeLibraryPath runtimeLibs;
          };
        }
      );
    };
}
