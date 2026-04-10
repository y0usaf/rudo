{
  description = "rudo - a Wayland-native terminal emulator with animated cursor rendering";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    crane.url = "github:ipetkov/crane";
  };

  outputs = {
    self,
    nixpkgs,
    crane,
  }: let
    systems = ["x86_64-linux" "aarch64-linux"];
    forAllSystems = nixpkgs.lib.genAttrs systems;
    pkgsFor = forAllSystems (system: import nixpkgs {inherit system;});
    runtimeLibsFor = system:
      with pkgsFor.${system}; [
        freetype
        libGL
        libxkbcommon
        wayland
      ];
    runtimePackagesFor = system:
      with pkgsFor.${system}; [
        fontconfig
        dejavu_fonts
        liberation_ttf
      ];
    mkApp = program: {
      type = "app";
      inherit program;
    };
    nixosModule = import ./nix/modules/nixos.nix {inherit self;};
    homeManagerModule = import ./nix/modules/home-manager.nix {inherit self;};
  in {
    packages = forAllSystems (system: let
      pkgs = pkgsFor.${system};
      lib = pkgs.lib;
      craneLib = crane.mkLib pkgs;
      runtimeLibs = runtimeLibsFor system;
      runtimePackages = runtimePackagesFor system;

      useMold = pkgs.stdenv.isLinux;

      commonArgs = {
        src = let
          xmlFilter = path: _type: builtins.match ".*\\.xml$" path != null;
          srcFilter = path: type:
            (xmlFilter path type) || (craneLib.filterCargoSources path type);
        in lib.cleanSourceWith {
          src = ./.;
          filter = srcFilter;
        };
        strictDeps = true;

        nativeBuildInputs = with pkgs;
          [
            pkg-config
            makeWrapper
          ]
          ++ lib.optionals useMold [clang mold];

        buildInputs = runtimeLibs ++ runtimePackages;

        LD_LIBRARY_PATH = lib.makeLibraryPath runtimeLibs;
      }
      // lib.optionalAttrs useMold {
        CARGO_BUILD_RUSTFLAGS = "-C linker=clang -C link-arg=-fuse-ld=${pkgs.mold}/bin/mold";
      };

      cargoArtifacts = craneLib.buildDepsOnly commonArgs;

      rudo = craneLib.buildPackage (commonArgs // {
        inherit cargoArtifacts;
        # `cargo test` passes locally, but currently exits non-zero when invoked
        # from the Nix builder even though the produced test binary succeeds when
        # run directly in the kept build directory. Disable package-time checks so
        # `nix run`/`nix build` remain usable until the Nix test harness issue is
        # resolved.
        doCheck = false;

        postFixup = ''
          wrapProgram $out/bin/rudo \
            --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs} \
            --prefix PATH : ${lib.makeBinPath runtimePackages} \
            --prefix XDG_DATA_DIRS : ${lib.concatStringsSep ":" (map (pkg: "${pkg}/share") runtimePackages)}
        '';
      });
    in rec {
      default = rudo;
      inherit rudo;
    });

    apps = forAllSystems (system: let
      program = "${self.packages.${system}.rudo}/bin/rudo";
      app = mkApp program;
    in {
      default = app;
      rudo = app;
    });

    nixosModules = {
      default = nixosModule;
      rudo = nixosModule;
    };

    homeManagerModules = {
      default = homeManagerModule;
      rudo = homeManagerModule;
    };

    devShells = forAllSystems (system: let
      pkgs = pkgsFor.${system};
      lib = pkgs.lib;
      runtimeLibs = runtimeLibsFor system;
      runtimePackages = runtimePackagesFor system;

      useMold = pkgs.stdenv.isLinux;
    in {
      default = pkgs.mkShell ({
          packages = with pkgs;
            [cargo rustc rustfmt clippy pkg-config]
            ++ lib.optionals useMold [clang mold]
            ++ runtimeLibs
            ++ runtimePackages;

          shellHook = ''
            export LD_LIBRARY_PATH="${lib.makeLibraryPath runtimeLibs}:''${LD_LIBRARY_PATH:-}"
            export PATH="${lib.makeBinPath runtimePackages}:''${PATH:-}"
            export XDG_DATA_DIRS="${lib.concatStringsSep ":" (map (pkg: "${pkg}/share") runtimePackages)}:''${XDG_DATA_DIRS:-}"
            echo "rudo dev shell"
          '';
        }
        // lib.optionalAttrs useMold {
          CARGO_BUILD_RUSTFLAGS = "-C linker=clang -C link-arg=-fuse-ld=${pkgs.mold}/bin/mold";
        });
    });
  };
}
