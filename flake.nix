{
  description = "rudo-c - C11 Wayland terminal emulator";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          lib = pkgs.lib;
          runtimeLibs = with pkgs; [ wayland libxkbcommon fontconfig freetype ];
          runtimeBins = with pkgs; [ wl-clipboard ];
        in rec {
          default = rudo;
          rudo = pkgs.stdenv.mkDerivation {
            pname = "rudo-c";
            version = "0.1.0";
            src = lib.cleanSourceWith {
              src = ./.;
              filter = path: type:
                let
                  base = builtins.baseNameOf path;
                in
                  !(lib.elem base [ ".git" "build" "build-full" "build-rel" "build-asan" "build-bench" ])
                  && !(lib.hasSuffix ".o" base)
                  && base != "core"
                  && base != "core.0";
            };

            nativeBuildInputs = with pkgs; [ meson ninja pkg-config wayland-scanner makeWrapper ];
            buildInputs = runtimeLibs;

            configurePhase = ''
              runHook preConfigure
              meson setup build --buildtype=release --prefix=$out
              runHook postConfigure
            '';

            buildPhase = ''
              runHook preBuild
              meson compile -C build
              runHook postBuild
            '';

            installPhase = ''
              runHook preInstall
              meson install -C build --destdir "$TMPDIR/dest"
              mkdir -p $out
              cp -a "$TMPDIR/dest/$out"/. "$out/"
              chmod +x $out/bin/rudo
              wrapProgram $out/bin/rudo \
                --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs} \
                --prefix PATH : ${lib.makeBinPath runtimeBins}
              runHook postInstall
            '';
          };
        });

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.rudo}/bin/rudo";
        };
        rudo = {
          type = "app";
          program = "${self.packages.${system}.rudo}/bin/rudo";
        };
      });

      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
          libs = with pkgs; [ wayland libxkbcommon fontconfig freetype ];
        in {
          default = pkgs.mkShell {
            packages = with pkgs; [
              meson ninja pkg-config gcc wayland-scanner
              wayland wayland-protocols libxkbcommon fontconfig freetype
            ];
            shellHook = ''
              export PATH="${pkgs.lib.makeBinPath [ pkgs.meson pkgs.ninja pkgs.pkg-config pkgs.gcc pkgs.wayland-scanner pkgs.wayland pkgs.wayland-protocols pkgs.libxkbcommon pkgs.fontconfig pkgs.freetype ]}:$PATH"
              export PKG_CONFIG_PATH="${pkgs.lib.makeSearchPathOutput "dev" "lib/pkgconfig" libs}:${pkgs.lib.makeSearchPath "share/pkgconfig" [ pkgs.wayland-protocols ]}:$PKG_CONFIG_PATH"
              echo "rudo-c dev shell"
            '';
          };
        });
    };
}
