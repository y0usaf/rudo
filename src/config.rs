//! TOML-based configuration system for rudo.
//! Loads from ~/.config/rudo/config.toml, with legacy SwiftTerm fallback.

use crate::defaults::{
    APP_NAME, CONFIG_FILE_NAME, DEFAULT_ANSI_HEX, DEFAULT_BACKGROUND_HEX, DEFAULT_BOLD_IS_BRIGHT,
    DEFAULT_COLORTERM, DEFAULT_CURSOR_ANIMATION_LENGTH_SECS, DEFAULT_CURSOR_BLINK_ENABLED,
    DEFAULT_CURSOR_BLINK_INTERVAL_SECS, DEFAULT_CURSOR_HEX,
    DEFAULT_CURSOR_SHORT_ANIMATION_LENGTH_SECS, DEFAULT_CURSOR_STYLE, DEFAULT_CURSOR_TRAIL_SIZE,
    DEFAULT_FONT_FAMILY, DEFAULT_FONT_SIZE, DEFAULT_FONT_SIZE_ADJUSTMENT, DEFAULT_FOREGROUND_HEX,
    DEFAULT_SCROLLBACK_LINES, DEFAULT_SELECTION_HEX, DEFAULT_SHELL_FALLBACK, DEFAULT_TERM,
    DEFAULT_TERMINAL_COLS, DEFAULT_TERMINAL_ROWS, DEFAULT_WINDOW_ALPHA_MODE,
    DEFAULT_WINDOW_INITIAL_HEIGHT, DEFAULT_WINDOW_INITIAL_WIDTH, DEFAULT_WINDOW_OPACITY,
    DEFAULT_WINDOW_PADDING_PX, LEGACY_CONFIG_DIR_NAME,
};
use crate::toml_parser::TomlTable;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub font: FontConfig,
    pub colors: ColorConfig,
    pub cursor: CursorConfig,
    pub window: WindowConfig,
    pub terminal: TerminalConfig,
    pub scrollback: ScrollbackConfig,
}

#[derive(Debug, Clone)]
pub struct FontConfig {
    pub family: String,
    pub size: f32,
    pub size_adjustment: f32,
    pub bold_is_bright: bool,
}

#[derive(Debug, Clone)]
pub struct ColorConfig {
    pub foreground: String,
    pub background: String,
    pub cursor: String,
    pub selection: String,
    // ANSI colors 0-7
    pub black: String,
    pub red: String,
    pub green: String,
    pub yellow: String,
    pub blue: String,
    pub magenta: String,
    pub cyan: String,
    pub white: String,
    // ANSI bright colors 8-15
    pub bright_black: String,
    pub bright_red: String,
    pub bright_green: String,
    pub bright_yellow: String,
    pub bright_blue: String,
    pub bright_magenta: String,
    pub bright_cyan: String,
    pub bright_white: String,
}

#[derive(Debug, Clone)]
pub struct CursorConfig {
    pub style: String,
    pub animation_length: f32,
    pub short_animation_length: f32,
    pub trail_size: f32,
    pub blink: bool,
    pub blink_interval: f32,
}

#[derive(Debug, Clone)]
pub struct WindowConfig {
    pub opacity: f32,
    pub alpha_mode: String,
    pub padding: u32,
    pub title: String,
    pub app_id: String,
    pub initial_width: u32,
    pub initial_height: u32,
}

#[derive(Debug, Clone)]
pub struct TerminalConfig {
    pub cols: usize,
    pub rows: usize,
    pub term: String,
    pub colorterm: String,
    pub shell_fallback: String,
}

#[derive(Debug, Clone)]
pub struct ScrollbackConfig {
    pub lines: usize,
}

// --- Default implementations ---

impl Default for Config {
    fn default() -> Self {
        Self {
            font: FontConfig::default(),
            colors: ColorConfig::default(),
            cursor: CursorConfig::default(),
            window: WindowConfig::default(),
            terminal: TerminalConfig::default(),
            scrollback: ScrollbackConfig::default(),
        }
    }
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: DEFAULT_FONT_FAMILY.to_string(),
            size: DEFAULT_FONT_SIZE,
            size_adjustment: DEFAULT_FONT_SIZE_ADJUSTMENT,
            bold_is_bright: DEFAULT_BOLD_IS_BRIGHT,
        }
    }
}

impl Default for ColorConfig {
    fn default() -> Self {
        Self {
            foreground: DEFAULT_FOREGROUND_HEX.to_string(),
            background: DEFAULT_BACKGROUND_HEX.to_string(),
            cursor: DEFAULT_CURSOR_HEX.to_string(),
            selection: DEFAULT_SELECTION_HEX.to_string(),
            black: DEFAULT_ANSI_HEX[0].to_string(),
            red: DEFAULT_ANSI_HEX[1].to_string(),
            green: DEFAULT_ANSI_HEX[2].to_string(),
            yellow: DEFAULT_ANSI_HEX[3].to_string(),
            blue: DEFAULT_ANSI_HEX[4].to_string(),
            magenta: DEFAULT_ANSI_HEX[5].to_string(),
            cyan: DEFAULT_ANSI_HEX[6].to_string(),
            white: DEFAULT_ANSI_HEX[7].to_string(),
            bright_black: DEFAULT_ANSI_HEX[8].to_string(),
            bright_red: DEFAULT_ANSI_HEX[9].to_string(),
            bright_green: DEFAULT_ANSI_HEX[10].to_string(),
            bright_yellow: DEFAULT_ANSI_HEX[11].to_string(),
            bright_blue: DEFAULT_ANSI_HEX[12].to_string(),
            bright_magenta: DEFAULT_ANSI_HEX[13].to_string(),
            bright_cyan: DEFAULT_ANSI_HEX[14].to_string(),
            bright_white: DEFAULT_ANSI_HEX[15].to_string(),
        }
    }
}

impl Default for CursorConfig {
    fn default() -> Self {
        Self {
            style: DEFAULT_CURSOR_STYLE.to_string(),
            animation_length: DEFAULT_CURSOR_ANIMATION_LENGTH_SECS,
            short_animation_length: DEFAULT_CURSOR_SHORT_ANIMATION_LENGTH_SECS,
            trail_size: DEFAULT_CURSOR_TRAIL_SIZE,
            blink: DEFAULT_CURSOR_BLINK_ENABLED,
            blink_interval: DEFAULT_CURSOR_BLINK_INTERVAL_SECS,
        }
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            opacity: DEFAULT_WINDOW_OPACITY,
            alpha_mode: DEFAULT_WINDOW_ALPHA_MODE.to_string(),
            padding: DEFAULT_WINDOW_PADDING_PX,
            title: APP_NAME.to_string(),
            app_id: APP_NAME.to_string(),
            initial_width: DEFAULT_WINDOW_INITIAL_WIDTH,
            initial_height: DEFAULT_WINDOW_INITIAL_HEIGHT,
        }
    }
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            cols: DEFAULT_TERMINAL_COLS,
            rows: DEFAULT_TERMINAL_ROWS,
            term: DEFAULT_TERM.to_string(),
            colorterm: DEFAULT_COLORTERM.to_string(),
            shell_fallback: DEFAULT_SHELL_FALLBACK.to_string(),
        }
    }
}

impl Default for ScrollbackConfig {
    fn default() -> Self {
        Self {
            lines: DEFAULT_SCROLLBACK_LINES,
        }
    }
}

// --- Config loading ---

impl Config {
    /// Try to load configuration from ~/.config/rudo/config.toml.
    /// Falls back to the legacy SwiftTerm config path, then defaults.
    pub fn load() -> Self {
        let Some(primary_path) = Self::primary_config_path() else {
            eprintln!("[INFO] No config directory found, using defaults");
            return Config::default();
        };

        let path = Self::config_paths()
            .into_iter()
            .find(|candidate| candidate.exists())
            .unwrap_or_else(|| primary_path.clone());

        if !path.exists() {
            eprintln!(
                "[INFO] Config file not found at {}, using defaults",
                primary_path.display()
            );
            return Config::default();
        }

        if path != primary_path {
            eprintln!(
                "[INFO] Loaded legacy config from {}, consider moving it to {}",
                path.display(),
                primary_path.display()
            );
        }

        match std::fs::read_to_string(&path) {
            Ok(contents) => match TomlTable::parse(&contents) {
                Ok(table) => {
                    eprintln!("[INFO] Loaded config from {}", path.display());
                    Config::from_toml(&table)
                }
                Err(e) => {
                    eprintln!(
                        "[WARN] Failed to parse config at {}: {}, using defaults",
                        path.display(),
                        e
                    );
                    Config::default()
                }
            },
            Err(e) => {
                eprintln!(
                    "[WARN] Failed to read config at {}: {}, using defaults",
                    path.display(),
                    e
                );
                Config::default()
            }
        }
    }

    fn from_toml(t: &TomlTable) -> Self {
        let def = Config::default();

        /// Helper macro to extract a string config value with fallback to default
        macro_rules! str_field {
            ($section:expr, $key:expr, $default:expr) => {
                t.get_str($section, $key).unwrap_or($default).to_string()
            };
        }

        Config {
            font: FontConfig {
                family: str_field!("font", "family", &def.font.family),
                size: t.get_f32("font", "size").unwrap_or(def.font.size),
                size_adjustment: t
                    .get_f32("font", "size_adjustment")
                    .unwrap_or(def.font.size_adjustment),
                bold_is_bright: t
                    .get_bool("font", "bold_is_bright")
                    .unwrap_or(def.font.bold_is_bright),
            },
            colors: ColorConfig {
                foreground: str_field!("colors", "foreground", &def.colors.foreground),
                background: str_field!("colors", "background", &def.colors.background),
                cursor: str_field!("colors", "cursor", &def.colors.cursor),
                selection: str_field!("colors", "selection", &def.colors.selection),
                black: str_field!("colors", "black", &def.colors.black),
                red: str_field!("colors", "red", &def.colors.red),
                green: str_field!("colors", "green", &def.colors.green),
                yellow: str_field!("colors", "yellow", &def.colors.yellow),
                blue: str_field!("colors", "blue", &def.colors.blue),
                magenta: str_field!("colors", "magenta", &def.colors.magenta),
                cyan: str_field!("colors", "cyan", &def.colors.cyan),
                white: str_field!("colors", "white", &def.colors.white),
                bright_black: str_field!("colors", "bright_black", &def.colors.bright_black),
                bright_red: str_field!("colors", "bright_red", &def.colors.bright_red),
                bright_green: str_field!("colors", "bright_green", &def.colors.bright_green),
                bright_yellow: str_field!("colors", "bright_yellow", &def.colors.bright_yellow),
                bright_blue: str_field!("colors", "bright_blue", &def.colors.bright_blue),
                bright_magenta: str_field!("colors", "bright_magenta", &def.colors.bright_magenta),
                bright_cyan: str_field!("colors", "bright_cyan", &def.colors.bright_cyan),
                bright_white: str_field!("colors", "bright_white", &def.colors.bright_white),
            },
            cursor: CursorConfig {
                style: str_field!("cursor", "style", &def.cursor.style),
                animation_length: t
                    .get_f32("cursor", "animation_length")
                    .unwrap_or(def.cursor.animation_length),
                short_animation_length: t
                    .get_f32("cursor", "short_animation_length")
                    .unwrap_or(def.cursor.short_animation_length),
                trail_size: t
                    .get_f32("cursor", "trail_size")
                    .unwrap_or(def.cursor.trail_size),
                blink: t.get_bool("cursor", "blink").unwrap_or(def.cursor.blink),
                blink_interval: t
                    .get_f32("cursor", "blink_interval")
                    .unwrap_or(def.cursor.blink_interval),
            },
            window: WindowConfig {
                opacity: t.get_f32("window", "opacity").unwrap_or(def.window.opacity),
                alpha_mode: str_field!("window", "alpha_mode", &def.window.alpha_mode),
                padding: t
                    .get_usize("window", "padding")
                    .unwrap_or(def.window.padding as usize) as u32,
                title: str_field!("window", "title", &def.window.title),
                app_id: str_field!("window", "app_id", &def.window.app_id),
                initial_width: t
                    .get_usize("window", "initial_width")
                    .unwrap_or(def.window.initial_width as usize)
                    as u32,
                initial_height: t
                    .get_usize("window", "initial_height")
                    .unwrap_or(def.window.initial_height as usize)
                    as u32,
            },
            terminal: TerminalConfig {
                cols: t.get_usize("terminal", "cols").unwrap_or(def.terminal.cols),
                rows: t.get_usize("terminal", "rows").unwrap_or(def.terminal.rows),
                term: str_field!("terminal", "term", &def.terminal.term),
                colorterm: str_field!("terminal", "colorterm", &def.terminal.colorterm),
                shell_fallback: str_field!(
                    "terminal",
                    "shell_fallback",
                    &def.terminal.shell_fallback
                ),
            },
            scrollback: ScrollbackConfig {
                lines: t
                    .get_usize("scrollback", "lines")
                    .unwrap_or(def.scrollback.lines),
            },
        }
    }

    /// Returns the path to the config file, or None if the config directory
    /// cannot be determined.
    fn primary_config_path() -> Option<PathBuf> {
        config_dir().map(|dir| dir.join(APP_NAME).join(CONFIG_FILE_NAME))
    }

    fn config_paths() -> Vec<PathBuf> {
        config_dir()
            .map(|dir| {
                vec![
                    dir.join(APP_NAME).join(CONFIG_FILE_NAME),
                    dir.join(LEGACY_CONFIG_DIR_NAME).join(CONFIG_FILE_NAME),
                ]
            })
            .unwrap_or_default()
    }
}

/// XDG config directory: $XDG_CONFIG_HOME or $HOME/.config
pub fn config_dir() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg));
        }
    }
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".config"))
}

/// Parse a hex color string in "#rrggbb" or "rrggbb" format into (r, g, b).
/// Returns `None` if the string is not a valid hex color.
#[cfg(test)]
fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.strip_prefix('#').unwrap_or(hex);

    if hex.len() != 6 {
        return None;
    }

    // Validate all characters are hex digits
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }

    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;

    Some((r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_color_with_hash() {
        assert_eq!(parse_hex_color("#ff0000"), Some((255, 0, 0)));
        assert_eq!(parse_hex_color("#00ff00"), Some((0, 255, 0)));
        assert_eq!(parse_hex_color("#0000ff"), Some((0, 0, 255)));
        assert_eq!(parse_hex_color("#d4d4d4"), Some((212, 212, 212)));
        assert_eq!(parse_hex_color("#1e1e1e"), Some((30, 30, 30)));
    }

    #[test]
    fn test_parse_hex_color_without_hash() {
        assert_eq!(parse_hex_color("ff0000"), Some((255, 0, 0)));
        assert_eq!(parse_hex_color("ffffff"), Some((255, 255, 255)));
        assert_eq!(parse_hex_color("000000"), Some((0, 0, 0)));
    }

    #[test]
    fn test_parse_hex_color_invalid() {
        assert_eq!(parse_hex_color(""), None);
        assert_eq!(parse_hex_color("#"), None);
        assert_eq!(parse_hex_color("#fff"), None);
        assert_eq!(parse_hex_color("#gggggg"), None);
        assert_eq!(parse_hex_color("not_a_color"), None);
        assert_eq!(parse_hex_color("#1234567"), None);
    }

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.font.family, DEFAULT_FONT_FAMILY);
        assert_eq!(config.font.size, DEFAULT_FONT_SIZE);
        assert_eq!(config.font.size_adjustment, DEFAULT_FONT_SIZE_ADJUSTMENT);
        assert!(!config.font.bold_is_bright);
        assert_eq!(config.colors.foreground, DEFAULT_FOREGROUND_HEX);
        assert_eq!(config.colors.background, DEFAULT_BACKGROUND_HEX);
        assert_eq!(config.cursor.style, DEFAULT_CURSOR_STYLE);
        assert_eq!(
            config.cursor.animation_length,
            DEFAULT_CURSOR_ANIMATION_LENGTH_SECS
        );
        assert_eq!(
            config.cursor.short_animation_length,
            DEFAULT_CURSOR_SHORT_ANIMATION_LENGTH_SECS
        );
        assert_eq!(
            config.cursor.blink_interval,
            DEFAULT_CURSOR_BLINK_INTERVAL_SECS
        );
        assert_eq!(config.window.opacity, DEFAULT_WINDOW_OPACITY);
        assert_eq!(config.window.alpha_mode, DEFAULT_WINDOW_ALPHA_MODE);
        assert_eq!(config.window.title, APP_NAME);
        assert_eq!(config.window.initial_width, DEFAULT_WINDOW_INITIAL_WIDTH);
        assert_eq!(config.window.initial_height, DEFAULT_WINDOW_INITIAL_HEIGHT);
        assert_eq!(config.terminal.cols, DEFAULT_TERMINAL_COLS);
        assert_eq!(config.terminal.rows, DEFAULT_TERMINAL_ROWS);
        assert_eq!(config.terminal.term, DEFAULT_TERM);
        assert_eq!(config.scrollback.lines, DEFAULT_SCROLLBACK_LINES);
    }

    #[test]
    fn test_partial_toml_parse() {
        let toml_str = r##"
[font]
size = 16.0

[colors]
foreground = "#e0e0e0"
"##;
        let table = TomlTable::parse(toml_str).unwrap();
        let config = Config::from_toml(&table);
        assert_eq!(config.font.size, 16.0);
        assert_eq!(config.font.family, DEFAULT_FONT_FAMILY); // default preserved
        assert_eq!(config.colors.foreground, "#e0e0e0");
        assert_eq!(config.colors.background, DEFAULT_BACKGROUND_HEX); // default preserved
    }

    #[test]
    fn test_extended_toml_parse() {
        let toml_str = r##"
[cursor]
short_animation_length = 0.05
blink_interval = 0.7

[window]
alpha_mode = "all"
initial_width = 1024
initial_height = 768

[terminal]
cols = 132
rows = 43
term = "foot"
colorterm = "24bit"
shell_fallback = "/usr/bin/zsh"
"##;
        let table = TomlTable::parse(toml_str).unwrap();
        let config = Config::from_toml(&table);
        assert_eq!(config.cursor.short_animation_length, 0.05);
        assert_eq!(config.cursor.blink_interval, 0.7);
        assert_eq!(config.window.alpha_mode, "all");
        assert_eq!(config.window.initial_width, 1024);
        assert_eq!(config.window.initial_height, 768);
        assert_eq!(config.terminal.cols, 132);
        assert_eq!(config.terminal.rows, 43);
        assert_eq!(config.terminal.term, "foot");
        assert_eq!(config.terminal.colorterm, "24bit");
        assert_eq!(config.terminal.shell_fallback, "/usr/bin/zsh");
    }

    #[test]
    fn test_default_ansi_colors() {
        let colors = ColorConfig::default();
        assert_eq!(colors.black, DEFAULT_ANSI_HEX[0]);
        assert_eq!(colors.red, DEFAULT_ANSI_HEX[1]);
        assert_eq!(colors.green, DEFAULT_ANSI_HEX[2]);
        assert_eq!(colors.yellow, DEFAULT_ANSI_HEX[3]);
        assert_eq!(colors.blue, DEFAULT_ANSI_HEX[4]);
        assert_eq!(colors.magenta, DEFAULT_ANSI_HEX[5]);
        assert_eq!(colors.cyan, DEFAULT_ANSI_HEX[6]);
        assert_eq!(colors.white, DEFAULT_ANSI_HEX[7]);
        assert_eq!(colors.bright_black, DEFAULT_ANSI_HEX[8]);
        assert_eq!(colors.bright_red, DEFAULT_ANSI_HEX[9]);
        assert_eq!(colors.bright_green, DEFAULT_ANSI_HEX[10]);
        assert_eq!(colors.bright_yellow, DEFAULT_ANSI_HEX[11]);
        assert_eq!(colors.bright_blue, DEFAULT_ANSI_HEX[12]);
        assert_eq!(colors.bright_magenta, DEFAULT_ANSI_HEX[13]);
        assert_eq!(colors.bright_cyan, DEFAULT_ANSI_HEX[14]);
        assert_eq!(colors.bright_white, DEFAULT_ANSI_HEX[15]);
    }
}
