mod config;
mod core_app;
mod cursor;
mod font;
mod freetype_ffi;
mod input;
mod platform;
mod pty;
mod renderer_font;
mod software_renderer;
mod terminal;
mod toml_parser;
mod xkb_ffi;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("[INFO] rudo starting (Wayland)...");
    pty::install_sigchld_reaper();
    platform::wayland::run()
}
