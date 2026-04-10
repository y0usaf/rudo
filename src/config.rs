//! TOML-based configuration system for rudo.
//! Loads from ~/.config/rudo/config.toml, with legacy config-path fallback.

use crate::contracts::CheckInvariant;
use crate::defaults::{
    APP_NAME, CONFIG_FILE_NAME, DEFAULT_ANSI_HEX, DEFAULT_BACKGROUND_HEX, DEFAULT_BOLD_IS_BRIGHT,
    DEFAULT_COLORTERM, DEFAULT_CURSOR_ANIMATION_LENGTH_SECS, DEFAULT_CURSOR_BLINK_ENABLED,
    DEFAULT_CURSOR_BLINK_INTERVAL_SECS, DEFAULT_CURSOR_HEX,
    DEFAULT_CURSOR_SHORT_ANIMATION_LENGTH_SECS, DEFAULT_CURSOR_STYLE, DEFAULT_CURSOR_TRAIL_SIZE,
    DEFAULT_FONT_FAMILY, DEFAULT_FONT_SIZE, DEFAULT_FONT_SIZE_ADJUSTMENT, DEFAULT_FOREGROUND_HEX,
    DEFAULT_SCROLLBACK_LINES, DEFAULT_SELECTION_HEX, DEFAULT_SHELL_FALLBACK, DEFAULT_TERM,
    DEFAULT_TERMINAL_COLS, DEFAULT_TERMINAL_ROWS, DEFAULT_WINDOW_INITIAL_HEIGHT,
    DEFAULT_WINDOW_INITIAL_WIDTH, DEFAULT_WINDOW_PADDING_PX, LEGACY_CONFIG_DIR_NAME,
};
use crate::info_log;
use crate::keybindings::{parse_binding_list, KeybindingsConfig};
use crate::toml_parser::TomlTable;
use crate::warn_log;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Config {
    pub font: FontConfig,
    pub colors: ColorConfig,
    pub cursor: CursorConfig,
    pub window: WindowConfig,
    pub terminal: TerminalConfig,
    pub scrollback: ScrollbackConfig,
    pub keybindings: KeybindingsConfig,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FontConfig {
    pub family: String,
    pub size: f32,
    pub size_adjustment: f32,
    pub bold_is_bright: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq)]
pub struct CursorConfig {
    pub style: String,
    pub animation_length: f32,
    pub short_animation_length: f32,
    pub trail_size: f32,
    pub blink: bool,
    pub blink_interval: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowConfig {
    pub padding: u32,
    pub title: String,
    pub app_id: String,
    pub initial_width: u32,
    pub initial_height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalConfig {
    pub cols: usize,
    pub rows: usize,
    pub term: String,
    pub colorterm: String,
    pub shell_fallback: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrollbackConfig {
    pub lines: usize,
}

// --- Default implementations ---

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
    /// Falls back to the legacy config path, then defaults.
    pub fn load() -> Self {
        let Some((primary_path, legacy_path)) = Self::config_paths() else {
            info_log!("No config directory found, using defaults");
            return Config::default();
        };

        let path = if primary_path.exists() {
            primary_path.clone()
        } else if legacy_path.exists() {
            legacy_path
        } else {
            info_log!(
                "Config file not found at {}, using defaults",
                primary_path.display()
            );
            return Config::default();
        };

        if path != primary_path {
            info_log!(
                "Loaded legacy config from {}, consider moving it to {}",
                path.display(),
                primary_path.display()
            );
        }

        match std::fs::read_to_string(&path) {
            Ok(contents) => match TomlTable::parse(&contents) {
                Ok(table) => {
                    info_log!("Loaded config from {}", path.display());
                    Config::from_toml(&table)
                }
                Err(e) => {
                    warn_log!(
                        "Failed to parse config at {}: {}, using defaults",
                        path.display(),
                        e
                    );
                    Config::default()
                }
            },
            Err(e) => {
                warn_log!(
                    "Failed to read config at {}: {}, using defaults",
                    path.display(),
                    e
                );
                Config::default()
            }
        }
    }

    fn from_toml(t: &TomlTable) -> Self {
        let def = Config::default();

        fn keybinding_field(
            t: &TomlTable,
            key: &str,
            default: &[crate::keybindings::KeyBinding],
        ) -> Vec<crate::keybindings::KeyBinding> {
            if let Some(false) = t.get_bool("keybindings", key) {
                return Vec::new();
            }

            let Some(spec) = t.get_str("keybindings", key) else {
                return default.to_vec();
            };

            match parse_binding_list(spec) {
                Ok(bindings) => bindings,
                Err(err) => {
                    warn_log!(
                        "Invalid keybinding for [keybindings].{}: {}, using defaults",
                        key,
                        err
                    );
                    default.to_vec()
                }
            }
        }

        fn string_field(t: &TomlTable, section: &str, key: &str, default: &str) -> String {
            t.get_str(section, key).unwrap_or(default).to_string()
        }

        fn u32_field(t: &TomlTable, section: &str, key: &str, default: u32) -> u32 {
            t.get_usize(section, key)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(default)
        }

        let mut config = Config {
            font: FontConfig {
                family: string_field(t, "font", "family", &def.font.family),
                size: t.get_f32("font", "size").unwrap_or(def.font.size),
                size_adjustment: t
                    .get_f32("font", "size_adjustment")
                    .unwrap_or(def.font.size_adjustment),
                bold_is_bright: t
                    .get_bool("font", "bold_is_bright")
                    .unwrap_or(def.font.bold_is_bright),
            },
            colors: ColorConfig {
                foreground: string_field(t, "colors", "foreground", &def.colors.foreground),
                background: string_field(t, "colors", "background", &def.colors.background),
                cursor: string_field(t, "colors", "cursor", &def.colors.cursor),
                selection: string_field(t, "colors", "selection", &def.colors.selection),
                black: string_field(t, "colors", "black", &def.colors.black),
                red: string_field(t, "colors", "red", &def.colors.red),
                green: string_field(t, "colors", "green", &def.colors.green),
                yellow: string_field(t, "colors", "yellow", &def.colors.yellow),
                blue: string_field(t, "colors", "blue", &def.colors.blue),
                magenta: string_field(t, "colors", "magenta", &def.colors.magenta),
                cyan: string_field(t, "colors", "cyan", &def.colors.cyan),
                white: string_field(t, "colors", "white", &def.colors.white),
                bright_black: string_field(t, "colors", "bright_black", &def.colors.bright_black),
                bright_red: string_field(t, "colors", "bright_red", &def.colors.bright_red),
                bright_green: string_field(t, "colors", "bright_green", &def.colors.bright_green),
                bright_yellow: string_field(
                    t,
                    "colors",
                    "bright_yellow",
                    &def.colors.bright_yellow,
                ),
                bright_blue: string_field(t, "colors", "bright_blue", &def.colors.bright_blue),
                bright_magenta: string_field(
                    t,
                    "colors",
                    "bright_magenta",
                    &def.colors.bright_magenta,
                ),
                bright_cyan: string_field(t, "colors", "bright_cyan", &def.colors.bright_cyan),
                bright_white: string_field(t, "colors", "bright_white", &def.colors.bright_white),
            },
            cursor: CursorConfig {
                style: string_field(t, "cursor", "style", &def.cursor.style),
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
                padding: u32_field(t, "window", "padding", def.window.padding),
                title: string_field(t, "window", "title", &def.window.title),
                app_id: string_field(t, "window", "app_id", &def.window.app_id),
                initial_width: u32_field(t, "window", "initial_width", def.window.initial_width),
                initial_height: u32_field(t, "window", "initial_height", def.window.initial_height),
            },
            terminal: TerminalConfig {
                cols: t.get_usize("terminal", "cols").unwrap_or(def.terminal.cols),
                rows: t.get_usize("terminal", "rows").unwrap_or(def.terminal.rows),
                term: string_field(t, "terminal", "term", &def.terminal.term),
                colorterm: string_field(t, "terminal", "colorterm", &def.terminal.colorterm),
                shell_fallback: string_field(
                    t,
                    "terminal",
                    "shell_fallback",
                    &def.terminal.shell_fallback,
                ),
            },
            scrollback: ScrollbackConfig {
                lines: t
                    .get_usize("scrollback", "lines")
                    .unwrap_or(def.scrollback.lines),
            },
            keybindings: KeybindingsConfig {
                copy: keybinding_field(t, "copy", &def.keybindings.copy),
                paste: keybinding_field(t, "paste", &def.keybindings.paste),
                zoom_in: keybinding_field(t, "zoom_in", &def.keybindings.zoom_in),
                zoom_out: keybinding_field(t, "zoom_out", &def.keybindings.zoom_out),
                zoom_reset: keybinding_field(t, "zoom_reset", &def.keybindings.zoom_reset),
            },
        };
        config.normalize();
        config
    }

    fn sanitize_f32(value: f32, minimum: f32, default: f32) -> f32 {
        if value.is_finite() {
            value.max(minimum)
        } else {
            default
        }
    }

    fn normalize(&mut self) {
        self.font.size = Self::sanitize_f32(self.font.size, 1.0, DEFAULT_FONT_SIZE);
        self.font.size_adjustment =
            Self::sanitize_f32(self.font.size_adjustment, 0.1, DEFAULT_FONT_SIZE_ADJUSTMENT);
        self.cursor.animation_length = Self::sanitize_f32(
            self.cursor.animation_length,
            0.0,
            DEFAULT_CURSOR_ANIMATION_LENGTH_SECS,
        );
        self.cursor.short_animation_length = Self::sanitize_f32(
            self.cursor.short_animation_length,
            0.0,
            DEFAULT_CURSOR_SHORT_ANIMATION_LENGTH_SECS,
        );
        self.cursor.trail_size =
            Self::sanitize_f32(self.cursor.trail_size, 0.0, DEFAULT_CURSOR_TRAIL_SIZE);
        self.cursor.blink_interval = Self::sanitize_f32(
            self.cursor.blink_interval,
            f32::EPSILON,
            DEFAULT_CURSOR_BLINK_INTERVAL_SECS,
        );
        self.window.initial_width = self.window.initial_width.max(1);
        self.window.initial_height = self.window.initial_height.max(1);
        self.terminal.cols = self.terminal.cols.max(2);
        self.terminal.rows = self.terminal.rows.max(2);

        ensures!(self.font.size >= 1.0);
        ensures!(self.terminal.cols >= 2);
        ensures!(self.terminal.rows >= 2);
        ensures!(self.window.initial_width >= 1);
        ensures!(self.window.initial_height >= 1);
        ensures!(self.cursor.blink_interval > 0.0);
        debug_check_invariant!(self);
    }

}

impl CheckInvariant for Config {
    fn check_invariant(&self) {
        invariant!(self.font.size >= 1.0, "font.size must be >= 1.0");
        invariant!(self.terminal.cols >= 2, "terminal.cols must be >= 2");
        invariant!(self.terminal.rows >= 2, "terminal.rows must be >= 2");
        invariant!(self.window.initial_width >= 1, "window.initial_width must be >= 1");
        invariant!(self.window.initial_height >= 1, "window.initial_height must be >= 1");
        invariant!(self.cursor.blink_interval > 0.0, "cursor.blink_interval must be > 0.0");
    }
}

impl Config {
    /// Returns the primary and legacy config paths, or None if the config
    /// directory cannot be determined.
    fn config_paths() -> Option<(PathBuf, PathBuf)> {
        let dir = config_dir()?;
        Some((
            dir.join(APP_NAME).join(CONFIG_FILE_NAME),
            dir.join(LEGACY_CONFIG_DIR_NAME).join(CONFIG_FILE_NAME),
        ))
    }
}

/// XDG config directory: $XDG_CONFIG_HOME or $HOME/.config
pub fn config_dir() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return Some(xdg);
    }

    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".config"))
        .filter(|p| p.is_absolute())
}

/// Parse a hex color string in "#rrggbb" or "rrggbb" format into (r, g, b).
/// Returns `None` if the string is not a valid hex color.
#[cfg(test)]
fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.strip_prefix('#').unwrap_or(hex);

    if hex.len() != 6 {
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
    use crate::input::{Key, KeyEvent, Modifiers};
    use crate::keybindings::LocalAction;

    fn mods(ctrl: bool, shift: bool, alt: bool) -> Modifiers {
        Modifiers { ctrl, shift, alt }
    }

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
        let toml_str = r#"
[cursor]
short_animation_length = 0.05
blink_interval = 0.7

[window]
initial_width = 1024
initial_height = 768

[terminal]
cols = 132
rows = 43
term = "foot"
colorterm = "24bit"
shell_fallback = "/usr/bin/zsh"
"#;
        let table = TomlTable::parse(toml_str).unwrap();
        let config = Config::from_toml(&table);
        assert_eq!(config.cursor.short_animation_length, 0.05);
        assert_eq!(config.cursor.blink_interval, 0.7);
        assert_eq!(config.window.initial_width, 1024);
        assert_eq!(config.window.initial_height, 768);
        assert_eq!(config.terminal.cols, 132);
        assert_eq!(config.terminal.rows, 43);
        assert_eq!(config.terminal.term, "foot");
        assert_eq!(config.terminal.colorterm, "24bit");
        assert_eq!(config.terminal.shell_fallback, "/usr/bin/zsh");
    }

    #[test]
    fn test_negative_integer_config_values_fall_back_to_defaults() {
        let toml_str = r"
[window]
padding = -1
initial_width = -1024
initial_height = -768

[terminal]
cols = -132
rows = -43

[scrollback]
lines = -10000
";
        let table = TomlTable::parse(toml_str).unwrap();
        let config = Config::from_toml(&table);
        let defaults = Config::default();

        assert_eq!(config.window.padding, defaults.window.padding);
        assert_eq!(config.window.initial_width, defaults.window.initial_width);
        assert_eq!(config.window.initial_height, defaults.window.initial_height);
        assert_eq!(config.terminal.cols, defaults.terminal.cols);
        assert_eq!(config.terminal.rows, defaults.terminal.rows);
        assert_eq!(config.scrollback.lines, defaults.scrollback.lines);
    }

    #[test]
    fn test_semantically_invalid_numeric_values_are_normalized() {
        let toml_str = r"
[font]
size = 0.0
size_adjustment = 0.0

[cursor]
animation_length = -1.0
short_animation_length = -2.0
trail_size = -3.0
blink_interval = 0.0

[window]
initial_width = 0
initial_height = 0

[terminal]
cols = 1
rows = 1

[scrollback]
lines = 0
";
        let table = TomlTable::parse(toml_str).unwrap();
        let config = Config::from_toml(&table);

        assert_eq!(config.font.size, 1.0);
        assert_eq!(config.font.size_adjustment, 0.1);
        assert_eq!(config.cursor.animation_length, 0.0);
        assert_eq!(config.cursor.short_animation_length, 0.0);
        assert_eq!(config.cursor.trail_size, 0.0);
        assert!(config.cursor.blink_interval > 0.0);
        assert_eq!(config.window.initial_width, 1);
        assert_eq!(config.window.initial_height, 1);
        assert_eq!(config.terminal.cols, 2);
        assert_eq!(config.terminal.rows, 2);
        assert_eq!(config.scrollback.lines, 0);
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

    #[test]
    fn test_custom_keybindings_parse() {
        let toml_str = r#"
[keybindings]
copy = "alt+c"
paste = "alt+v"
zoom_in = "alt+equal, alt+plus"
zoom_out = "alt+minus"
zoom_reset = "alt+0"
"#;
        let table = TomlTable::parse(toml_str).unwrap();
        let config = Config::from_toml(&table);

        assert!(config.keybindings.matches(
            LocalAction::Copy,
            &KeyEvent {
                pressed: true,
                key: Key::Text("c".to_string()),
            },
            mods(false, false, true)
        ));
        assert!(config.keybindings.matches(
            LocalAction::ZoomIn,
            &KeyEvent {
                pressed: true,
                key: Key::Text("+".to_string()),
            },
            mods(false, true, true)
        ));
        assert!(config.keybindings.matches(
            LocalAction::ZoomOut,
            &KeyEvent {
                pressed: true,
                key: Key::Text("-".to_string()),
            },
            mods(false, false, true)
        ));
    }

    #[test]
    fn test_keybindings_can_be_disabled() {
        let toml_str = r"
[keybindings]
paste = false
";
        let table = TomlTable::parse(toml_str).unwrap();
        let config = Config::from_toml(&table);

        assert!(!config.keybindings.matches(
            LocalAction::Paste,
            &KeyEvent {
                pressed: true,
                key: Key::Text("v".to_string()),
            },
            mods(true, true, false)
        ));
    }

    #[test]
    fn test_config_dir_prefers_xdg_config_home() {
        let _lock = env_test_lock();
        let _xdg_guard = EnvGuard::set("XDG_CONFIG_HOME", Some("/tmp/rudo-xdg"));
        let _home_guard = EnvGuard::set("HOME", Some("/tmp/rudo-home"));

        assert_eq!(config_dir(), Some(PathBuf::from("/tmp/rudo-xdg")));
    }

    #[test]
    fn test_config_dir_falls_back_to_home_config() {
        let _lock = env_test_lock();
        let _xdg_guard = EnvGuard::set("XDG_CONFIG_HOME", None);
        let _home_guard = EnvGuard::set("HOME", Some("/tmp/rudo-home"));

        assert_eq!(config_dir(), Some(PathBuf::from("/tmp/rudo-home/.config")));
    }

    #[test]
    fn test_config_dir_returns_none_without_home_or_xdg() {
        let _lock = env_test_lock();
        let _xdg_guard = EnvGuard::set("XDG_CONFIG_HOME", None);
        let _home_guard = EnvGuard::set("HOME", None);

        assert_eq!(config_dir(), None);
    }

    fn env_test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap()
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let previous = std::env::var_os(key);
            match value {
                Some(value) => unsafe { std::env::set_var(key, value) },
                None => unsafe { std::env::remove_var(key) },
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => unsafe { std::env::set_var(self.key, value) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }
}
