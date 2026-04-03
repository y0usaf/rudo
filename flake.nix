{
  description = "Termvide development shell and package";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
        lib = pkgs.lib;

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

        packageNativeBuildInputs = [
          pkgs.makeWrapper
          pkgs.pkg-config
          pkgs.python3
          pkgs.rustPlatform.bindgenHook
          pkgs.removeReferencesTo
        ];

        skiaSourceDir =
          let
            repo = pkgs.fetchFromGitHub {
              owner = "rust-skia";
              repo = "skia";
              tag = "m145-0.92.0";
              hash = "sha256-9N780AwheKBJRcZC4l/uWFNq+oOyoNp4M6dJAVVAFeo=";
            };
            externals = pkgs.linkFarm "skia-externals"
              (lib.mapAttrsToList (name: value: {
                inherit name;
                path = pkgs.fetchgit value;
              }) (builtins.fromJSON (builtins.readFile ./skia-externals.json)));
          in
          pkgs.runCommand "termvide-skia-source" { } ''
            cp -R ${repo} $out
            chmod -R +w $out
            ln -s ${externals} $out/third_party/externals
          '';

        termvide = pkgs.rustPlatform.buildRustPackage.override {
          stdenv = pkgs.clangStdenv;
        } {
          pname = "termvide";
          version = "0.16.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          env = {
            SKIA_SOURCE_DIR = skiaSourceDir;
            SKIA_GN_COMMAND = "${pkgs.gn}/bin/gn";
            SKIA_NINJA_COMMAND = "${pkgs.ninja}/bin/ninja";
          };

          nativeBuildInputs = packageNativeBuildInputs;

          buildInputs = [ pkgs.SDL2 pkgs.fontconfig ] ++ runtimeLibs;

          doCheck = false;

          postPatch = ''
            for path in $(find . /build/cargo-vendor-dir -path '*/skia-bindings-0.93.1/src/bindings.cpp' 2>/dev/null); do
              substituteInPlace "$path" \
                --replace '#include "include/effects/SkGradient.h"' ""
            done
          '';

          postFixup = ''
            remove-references-to -t "$SKIA_SOURCE_DIR" $out/bin/termvide || true
            wrapProgram $out/bin/termvide \
              --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs}
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
            export LD_LIBRARY_PATH="${lib.makeLibraryPath runtimeLibs}:''${LD_LIBRARY_PATH:-}"
            export LIBRARY_PATH="${lib.makeLibraryPath runtimeLibs}:''${LIBRARY_PATH:-}"
            echo "Entered Termvide dev shell"
            echo "Run: cargo run"
          '';
        };
      });
}
