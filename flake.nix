{
  description = "BabelChat: chat without language barriers";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    { self, nixpkgs }:
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
            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
            LD_LIBRARY_PATH = lib.makeLibraryPath runtimeLibs;
          };
        }
      );
    };
}
