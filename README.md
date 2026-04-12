# rudo-c

C11 port of `rudo` in `/home/y0usaf/Dev/rudo-c`.

## Build

### Nix

```sh
nix develop 'path:/home/y0usaf/Dev/rudo-c' -c bash -c '
  export PATH=$(echo /nix/store/*wayland-scanner*-bin/bin):$PATH
  cd /home/y0usaf/Dev/rudo-c
  meson setup build-full --buildtype=release
  meson compile -C build-full
'
```

### Plain Meson

Requires: `meson`, `ninja`, `pkg-config`, `wayland-client`, `xkbcommon`, `wayland-scanner`, `fontconfig`, `freetype`.

```sh
meson setup build-full --buildtype=release
meson compile -C build-full
```

If Wayland dev deps are missing, the project still builds the core static library only.

## Layout

- `include/rudo/`: public headers
- `src/`: C11 implementation
- `protocols/`: Wayland protocol XML + fallback shim headers
- `flake.nix`: dev shell

## Current state

- core library builds without Wayland dev deps
- full Wayland executable builds in Nix dev shell
- smoke test: `./build-full/rudo -e true` exits successfully
- performance still needs work vs `foot`/Rust build
