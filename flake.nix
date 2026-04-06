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
    pkgsFor = forAllSystems (system: import nixpkgs { inherit system; });
    runtimeLibsFor = system:
      with pkgsFor.${system}; [
        freetype
        libGL
        libxkbcommon
        wayland
      ];
  in {
    packages = forAllSystems (system: let
      pkgs = pkgsFor.${system};
      lib = pkgs.lib;
      craneLib = crane.mkLib pkgs;
      runtimeLibs = runtimeLibsFor system;

      useMold = pkgs.stdenv.isLinux;

      commonArgs = {
        src = craneLib.cleanCargoSource ./.;
        strictDeps = true;

        nativeBuildInputs = with pkgs;
          [
            pkg-config
            makeWrapper
          ]
          ++ lib.optionals useMold [ clang mold ];

        buildInputs = runtimeLibs;

        LD_LIBRARY_PATH = lib.makeLibraryPath runtimeLibs;
      } // lib.optionalAttrs useMold {
        CARGO_BUILD_RUSTFLAGS = "-C linker=clang -C link-arg=-fuse-ld=${pkgs.mold}/bin/mold";
      };

      cargoArtifacts = craneLib.buildDepsOnly commonArgs;

      rudo = craneLib.buildPackage (commonArgs // {
        inherit cargoArtifacts;
        doCheck = true;

        postFixup = ''
          wrapProgram $out/bin/rudo \
            --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs}
        '';
      });
    in {
      default = rudo;
      rudo = rudo;
    });

    apps = forAllSystems (system: {
      default = {
        type = "app";
        program = "${self.packages.${system}.default}/bin/rudo";
      };
      rudo = {
        type = "app";
        program = "${self.packages.${system}.rudo}/bin/rudo";
      };
    });

    nixosModules = {
      default = import ./nix/modules/nixos.nix { inherit self; };
      rudo = import ./nix/modules/nixos.nix { inherit self; };
    };

    homeManagerModules = {
      default = import ./nix/modules/home-manager.nix { inherit self; };
      rudo = import ./nix/modules/home-manager.nix { inherit self; };
    };

    devShells = forAllSystems (system: let
      pkgs = pkgsFor.${system};
      lib = pkgs.lib;
      runtimeLibs = runtimeLibsFor system;

      useMold = pkgs.stdenv.isLinux;
    in {
      default = pkgs.mkShell ({
          packages = with pkgs;
            [ cargo rustc rustfmt clippy pkg-config ]
            ++ lib.optionals useMold [ clang mold ]
            ++ runtimeLibs;

          shellHook = ''
            export LD_LIBRARY_PATH="${lib.makeLibraryPath runtimeLibs}:''${LD_LIBRARY_PATH:-}"
            echo "rudo dev shell"
          '';
        }
        // lib.optionalAttrs useMold {
          CARGO_BUILD_RUSTFLAGS = "-C linker=clang -C link-arg=-fuse-ld=${pkgs.mold}/bin/mold";
        });
    });
  };
}
