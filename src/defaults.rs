//! Shared application constants and default values.
#![allow(dead_code)]

pub const APP_NAME: &str = env!("CARGO_PKG_NAME");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const CONFIG_FILE_NAME: &str = "config.toml";
pub const THEME_FILE_NAME: &str = "theme.toml";
pub const LEGACY_CONFIG_DIR_NAME: &str = "swiftterm";
pub const THEME_ENV_VAR: &str = "RUDO_THEME";

pub const DEFAULT_FONT_FAMILY: &str = "monospace";
pub const DEFAULT_FONT_SIZE: f32 = 14.0;
pub const DEFAULT_FONT_SIZE_ADJUSTMENT: f32 = 0.5;
pub const DEFAULT_BOLD_IS_BRIGHT: bool = false;

pub const DEFAULT_FOREGROUND_HEX: &str = "#d4d4d4";
pub const DEFAULT_BACKGROUND_HEX: &str = "#1e1e1e";
pub const DEFAULT_CURSOR_HEX: &str = "#ffffff";
pub const DEFAULT_SELECTION_HEX: &str = "#264f78";
pub const DEFAULT_FOREGROUND_RGB: u32 = 0xd4d4d4;
pub const DEFAULT_BACKGROUND_RGB: u32 = 0x1e1e1e;

pub const DEFAULT_ANSI_HEX: [&str; 16] = [
    "#000000", "#cc0000", "#00cc00", "#cccc00", "#0000cc", "#cc00cc", "#00cccc", "#cccccc",
    "#555555", "#ff5555", "#55ff55", "#ffff55", "#5555ff", "#ff55ff", "#55ffff", "#ffffff",
];

pub const DEFAULT_CURSOR_STYLE: &str = "block";
pub const DEFAULT_CURSOR_ANIMATION_LENGTH_SECS: f32 = 0.150;
pub const DEFAULT_CURSOR_SHORT_ANIMATION_LENGTH_SECS: f32 = 0.04;
pub const DEFAULT_CURSOR_TRAIL_SIZE: f32 = 1.0;
pub const DEFAULT_CURSOR_BLINK_ENABLED: bool = false;
pub const DEFAULT_CURSOR_BLINK_INTERVAL_SECS: f32 = 0.6;

pub const DEFAULT_WINDOW_OPACITY: f32 = 1.0;
pub const DEFAULT_WINDOW_ALPHA_MODE: &str = "default";
pub const DEFAULT_WINDOW_PADDING_PX: u32 = 2;
pub const DEFAULT_WINDOW_INITIAL_WIDTH: u32 = 800;
pub const DEFAULT_WINDOW_INITIAL_HEIGHT: u32 = 600;

pub const DEFAULT_TERMINAL_COLS: usize = 80;
pub const DEFAULT_TERMINAL_ROWS: usize = 24;
pub const DEFAULT_TERM: &str = "xterm-256color";
pub const DEFAULT_COLORTERM: &str = "truecolor";
pub const DEFAULT_SHELL_FALLBACK: &str = "/bin/sh";

pub const DEFAULT_SCROLLBACK_LINES: usize = 10_000;

pub const DEFAULT_CLIPBOARD_COPY_COMMAND: &str = "wl-copy";
pub const DEFAULT_CLIPBOARD_PASTE_COMMAND: &str = "wl-paste";
