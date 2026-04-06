//! TOML-based configuration system for rudo.
//! Loads from ~/.config/rudo/config.toml, with legacy SwiftTerm fallback.

use crate::toml_parser::TomlTable;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub font: FontConfig,
    pub colors: ColorConfig,
    pub cursor: CursorConfig,
    pub window: WindowConfig,
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
    pub trail_size: f32,
    pub blink: bool,
}

#[derive(Debug, Clone)]
pub struct WindowConfig {
    pub opacity: f32,
    pub padding: u32,
    pub title: String,
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
            scrollback: ScrollbackConfig::default(),
        }
    }
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: "monospace".to_string(),
            size: 14.0,
            size_adjustment: 0.5,
            bold_is_bright: false,
        }
    }
}

impl Default for ColorConfig {
    fn default() -> Self {
        Self {
            foreground: "#d4d4d4".to_string(),
            background: "#1e1e1e".to_string(),
            cursor: "#ffffff".to_string(),
            selection: "#264f78".to_string(),
            // Normal ANSI colors
            black: "#000000".to_string(),
            red: "#cc0000".to_string(),
            green: "#00cc00".to_string(),
            yellow: "#cccc00".to_string(),
            blue: "#0000cc".to_string(),
            magenta: "#cc00cc".to_string(),
            cyan: "#00cccc".to_string(),
            white: "#cccccc".to_string(),
            // Bright ANSI colors
            bright_black: "#555555".to_string(),
            bright_red: "#ff5555".to_string(),
            bright_green: "#55ff55".to_string(),
            bright_yellow: "#ffff55".to_string(),
            bright_blue: "#5555ff".to_string(),
            bright_magenta: "#ff55ff".to_string(),
            bright_cyan: "#55ffff".to_string(),
            bright_white: "#ffffff".to_string(),
        }
    }
}

impl Default for CursorConfig {
    fn default() -> Self {
        Self {
            style: "block".to_string(),
            animation_length: 0.150,
            trail_size: 1.0,
            blink: false,
        }
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            padding: 2,
            title: "rudo".to_string(),
        }
    }
}

impl Default for ScrollbackConfig {
    fn default() -> Self {
        Self { lines: 10000 }
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
                trail_size: t
                    .get_f32("cursor", "trail_size")
                    .unwrap_or(def.cursor.trail_size),
                blink: t.get_bool("cursor", "blink").unwrap_or(def.cursor.blink),
            },
            window: WindowConfig {
                opacity: t.get_f32("window", "opacity").unwrap_or(def.window.opacity),
                padding: t
                    .get_usize("window", "padding")
                    .unwrap_or(def.window.padding as usize) as u32,
                title: str_field!("window", "title", &def.window.title),
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
        config_dir().map(|dir| dir.join("rudo").join("config.toml"))
    }

    fn config_paths() -> Vec<PathBuf> {
        config_dir()
            .map(|dir| {
                vec![
                    dir.join("rudo").join("config.toml"),
                    dir.join("swiftterm").join("config.toml"),
                ]
            })
            .unwrap_or_default()
    }
}

/// XDG config directory: $XDG_CONFIG_HOME or $HOME/.config
fn config_dir() -> Option<PathBuf> {
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
pub fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
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
        assert_eq!(config.font.family, "monospace");
        assert_eq!(config.font.size, 14.0);
        assert_eq!(config.font.size_adjustment, 0.5);
        assert!(!config.font.bold_is_bright);
        assert_eq!(config.colors.foreground, "#d4d4d4");
        assert_eq!(config.colors.background, "#1e1e1e");
        assert_eq!(config.cursor.style, "block");
        assert_eq!(config.cursor.animation_length, 0.150);
        assert_eq!(config.window.opacity, 1.0);
        assert_eq!(config.window.title, "rudo");
        assert_eq!(config.scrollback.lines, 10000);
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
        assert_eq!(config.font.family, "monospace"); // default preserved
        assert_eq!(config.colors.foreground, "#e0e0e0");
        assert_eq!(config.colors.background, "#1e1e1e"); // default preserved
    }

    #[test]
    fn test_default_ansi_colors() {
        let colors = ColorConfig::default();
        assert_eq!(colors.black, "#000000");
        assert_eq!(colors.red, "#cc0000");
        assert_eq!(colors.green, "#00cc00");
        assert_eq!(colors.yellow, "#cccc00");
        assert_eq!(colors.blue, "#0000cc");
        assert_eq!(colors.magenta, "#cc00cc");
        assert_eq!(colors.cyan, "#00cccc");
        assert_eq!(colors.white, "#cccccc");
        assert_eq!(colors.bright_black, "#555555");
        assert_eq!(colors.bright_red, "#ff5555");
        assert_eq!(colors.bright_green, "#55ff55");
        assert_eq!(colors.bright_yellow, "#ffff55");
        assert_eq!(colors.bright_blue, "#5555ff");
        assert_eq!(colors.bright_magenta, "#ff55ff");
        assert_eq!(colors.bright_cyan, "#55ffff");
        assert_eq!(colors.bright_white, "#ffffff");
    }
}
