#[macro_use]
pub(crate) mod contracts;
pub(crate) mod cli;
pub(crate) mod config;
pub(crate) mod core_app;
pub(crate) mod cursor;
pub(crate) mod defaults;
pub(crate) mod dlopen;
pub(crate) mod font;
pub(crate) mod fontconfig_ffi;
pub(crate) mod freetype_ffi;
pub(crate) mod input;
pub(crate) mod keybindings;
pub(crate) mod logging;
pub(crate) mod platform;
pub(crate) mod protocols;
pub(crate) mod pty;
pub(crate) mod renderer_font;
pub(crate) mod software_renderer;
pub(crate) mod terminal;
pub(crate) mod toml_parser;
pub(crate) mod xkb_ffi;


/// Library entry point — parses CLI args, installs signal handlers, and runs
/// the Wayland event loop. This is the only public API surface.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = cli::CliArgs::parse();
    info_log!("{} starting (Wayland)...", defaults::APP_NAME);
    pty::install_sigchld_reaper();
    platform::wayland::run(cli)
}
