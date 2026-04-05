# Termvide

A lightweight GPU-accelerated terminal emulator.

Termvide started as a fork of [Neovide](https://github.com/neovide/neovide), reusing its
GPU rendering stack (Skia + OpenGL/Metal/D3D) while replacing the Neovim bridge with a
native PTY and VT parser.

## Building

```bash
cargo build --release
```

## Usage

```bash
./target/release/termvide
```

## Status

Early development. The goal is a fast, minimal terminal emulator with GPU-accelerated rendering.

## License

Licensed under [MIT](./LICENSE).
