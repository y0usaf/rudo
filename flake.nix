{
  description = "Termvide development shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        runtimeLibs = with pkgs; [
          fontconfig
          freetype
          libGL
          libxkbcommon
          wayland
          libx11
          libxcursor
          libxi
          libxrandr
          libxext
          libxinerama
          libxcb
        ];
        nativeTools = with pkgs; [
          pkg-config
          clang
          cmake
          ninja
          python3
          makeWrapper
        ];
        termvide = pkgs.writeShellApplication {
          name = "termvide";
          runtimeInputs = with pkgs; [
            cargo
            rustc
            gcc
          ] ++ nativeTools ++ runtimeLibs;
          text = ''
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath runtimeLibs}:''${LD_LIBRARY_PATH:-}"
            export LIBRARY_PATH="${pkgs.lib.makeLibraryPath runtimeLibs}:''${LIBRARY_PATH:-}"
            export CARGO_TARGET_DIR="''${XDG_CACHE_HOME:-$HOME/.cache}/termvide/target"
            mkdir -p "$CARGO_TARGET_DIR"
            exec cargo run --manifest-path ${./.}/Cargo.toml -- "$@"
          '';
        };
      in {
        packages.default = termvide;
        apps.default = flake-utils.lib.mkApp {
          drv = termvide;
          name = "termvide";
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            cargo
            rustc
            rustfmt
            clippy
          ] ++ nativeTools ++ runtimeLibs;

          shellHook = ''
            export LD_LIBRARY_PATH="${pkgs.lib.makeLibraryPath runtimeLibs}:$LD_LIBRARY_PATH"
            echo "Entered Termvide dev shell"
            echo "Run: cargo run"
          '';
        };
      });
}
