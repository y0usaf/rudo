//! Terminal color theme/palette system.
//!
//! Holds foreground, background, cursor, and selection colors plus the full
//! 256-color palette. The first 16 entries (ANSI colors) come from the theme;
//! indices 16-231 are the 6×6×6 color cube and 232-255 are a grayscale ramp.
//!
//! Themes can be loaded from:
//! - Color strings passed via `ThemeColorStrings`
//! - A standalone theme TOML file (wallust / termvide compatible)

use std::path::PathBuf;

use super::cell::PackedColor;
use crate::defaults::{
    APP_NAME, DEFAULT_ANSI_HEX, DEFAULT_BACKGROUND_HEX, DEFAULT_CURSOR_HEX, DEFAULT_FOREGROUND_HEX,
    DEFAULT_SELECTION_HEX, THEME_ENV_VAR, THEME_FILE_NAME,
};
use crate::info_log;
use crate::toml_parser::TomlTable;
use crate::warn_log;

const COLOR_CUBE_START: usize = 16;
const COLOR_CUBE_SIDE: usize = 6;
const COLOR_CUBE_LAYER: usize = COLOR_CUBE_SIDE * COLOR_CUBE_SIDE;
const COLOR_CUBE_BASE: u8 = 55;
const COLOR_CUBE_STEP: u8 = 40;
const GRAYSCALE_RAMP_START: usize = 232;
const GRAYSCALE_RAMP_LEN: u8 = 24;
const GRAYSCALE_BASE: u8 = 8;
const GRAYSCALE_STEP: u8 = 10;

/// Color strings for constructing a theme from configuration.
///
/// Each field is a hex color in `"#rrggbb"` or `"rrggbb"` format.
pub struct ThemeColorStrings<'a> {
    pub foreground: &'a str,
    pub background: &'a str,
    pub cursor: &'a str,
    pub selection: &'a str,
    /// ANSI colors 0-15 (normal 0-7, then bright 8-15).
    pub ansi: [&'a str; 16],
}

/// Parse a hex color string in "#rrggbb" or "rrggbb" format into (r, g, b).
/// Returns `None` if the string is not a valid hex color.
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

struct ThemeFile {
    background: Option<String>,
    foreground: Option<String>,
    cursor: Option<String>,
    selection: Option<String>,
    colors: [Option<String>; 16],
}

impl ThemeFile {
    /// Parse a theme TOML string. All keys live in root section.
    fn parse(input: &str) -> Option<Self> {
        let table = TomlTable::parse(input).ok()?;

        let mut colors: [Option<String>; 16] = Default::default();
        for i in 0..16 {
            let key = format!("color{}", i);
            colors[i] = table.get_str_flat(&key).map(ToString::to_string);
        }

        Some(Self {
            background: table.get_str_flat("background").map(ToString::to_string),
            foreground: table.get_str_flat("foreground").map(ToString::to_string),
            cursor: table.get_str_flat("cursor").map(ToString::to_string),
            selection: table.get_str_flat("selection").map(ToString::to_string),
            colors,
        })
    }

    fn ansi_color(&self, index: u8) -> &str {
        self.colors[index as usize]
            .as_deref()
            .unwrap_or(DEFAULT_ANSI_HEX[index as usize])
    }
}

fn hex_to_packed(hex: &str) -> PackedColor {
    parse_hex_color(hex)
        .map(|(r, g, b)| PackedColor::new(r, g, b))
        .unwrap_or(PackedColor(0))
}

#[inline]
fn cube_component(v: u8) -> u8 {
    if v == 0 {
        0
    } else {
        COLOR_CUBE_BASE + COLOR_CUBE_STEP * v
    }
}

fn build_palette(ansi: [PackedColor; 16]) -> [PackedColor; 256] {
    let mut palette = [PackedColor(0); 256];
    palette[..16].copy_from_slice(&ansi);

    for r in 0u8..COLOR_CUBE_SIDE as u8 {
        for g in 0u8..COLOR_CUBE_SIDE as u8 {
            for b in 0u8..COLOR_CUBE_SIDE as u8 {
                let idx = COLOR_CUBE_START
                    + COLOR_CUBE_LAYER * r as usize
                    + COLOR_CUBE_SIDE * g as usize
                    + b as usize;
                palette[idx] =
                    PackedColor::new(cube_component(r), cube_component(g), cube_component(b));
            }
        }
    }

    for i in 0u8..GRAYSCALE_RAMP_LEN {
        let v = GRAYSCALE_BASE + GRAYSCALE_STEP * i;
        palette[GRAYSCALE_RAMP_START + i as usize] = PackedColor::new(v, v, v);
    }

    palette
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub foreground: PackedColor,
    pub background: PackedColor,
    pub cursor: PackedColor,
    pub selection: PackedColor,
    palette: [PackedColor; 256],
}

#[allow(dead_code)]
impl Theme {
    pub fn from_color_strings(colors: &ThemeColorStrings) -> Self {
        let mut ansi = [PackedColor(0); 16];
        for (i, hex) in colors.ansi.iter().enumerate() {
            ansi[i] = hex_to_packed(hex);
        }

        Self {
            foreground: hex_to_packed(colors.foreground),
            background: hex_to_packed(colors.background),
            cursor: hex_to_packed(colors.cursor),
            selection: hex_to_packed(colors.selection),
            palette: build_palette(ansi),
        }
    }

    pub fn default() -> Self {
        Self::from_color_strings(&ThemeColorStrings {
            foreground: DEFAULT_FOREGROUND_HEX,
            background: DEFAULT_BACKGROUND_HEX,
            cursor: DEFAULT_CURSOR_HEX,
            selection: DEFAULT_SELECTION_HEX,
            ansi: DEFAULT_ANSI_HEX,
        })
    }

    pub fn load_theme_file() -> Option<Self> {
        for path in Self::theme_search_paths() {
            if !path.exists() {
                continue;
            }

            match std::fs::read_to_string(&path) {
                Ok(contents) => match ThemeFile::parse(&contents) {
                    Some(tf) => {
                        info_log!("Loaded theme from {}", path.display());
                        return Some(Self::from_theme_file(&tf));
                    }
                    None => {
                        warn_log!("Failed to parse theme file {}", path.display());
                    }
                },
                Err(e) => {
                    warn_log!("Failed to read theme file {}: {}", path.display(), e);
                }
            }
        }

        None
    }

    #[inline]
    pub fn palette(&self, index: u8) -> PackedColor {
        self.palette[index as usize]
    }

    #[inline]
    pub fn set_palette(&mut self, index: u8, color: PackedColor) {
        self.palette[index as usize] = color;
    }

    #[inline]
    pub fn fg_rgba(&self) -> [f32; 4] {
        self.foreground.to_rgba_f32()
    }

    #[inline]
    pub fn bg_rgba(&self) -> [f32; 4] {
        self.background.to_rgba_f32()
    }

    #[inline]
    pub fn cursor_rgba(&self) -> [f32; 4] {
        self.cursor.to_rgba_f32()
    }

    #[inline]
    pub fn selection_rgba(&self) -> [f32; 4] {
        self.selection.to_rgba_f32()
    }

    fn theme_search_paths() -> Vec<PathBuf> {
        let mut paths = Vec::with_capacity(2);

        // Env var override — lets external tools (wallust, pywal, etc.)
        // point at an arbitrary theme file without touching the config dir.
        if let Ok(env_path) = std::env::var(THEME_ENV_VAR) {
            paths.push(PathBuf::from(env_path));
        }

        // Standard XDG config location.
        if let Some(dir) = crate::config::config_dir() {
            paths.push(dir.join(APP_NAME).join(THEME_FILE_NAME));
        }

        paths
    }

    fn from_theme_file(tf: &ThemeFile) -> Self {
        let mut ansi = [PackedColor(0); 16];
        for i in 0u8..16 {
            ansi[i as usize] = hex_to_packed(tf.ansi_color(i));
        }

        Self {
            foreground: hex_to_packed(tf.foreground.as_deref().unwrap_or(DEFAULT_FOREGROUND_HEX)),
            background: hex_to_packed(tf.background.as_deref().unwrap_or(DEFAULT_BACKGROUND_HEX)),
            cursor: hex_to_packed(tf.cursor.as_deref().unwrap_or(DEFAULT_CURSOR_HEX)),
            selection: hex_to_packed(tf.selection.as_deref().unwrap_or(DEFAULT_SELECTION_HEX)),
            palette: build_palette(ansi),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_theme_has_256_palette() {
        let theme = Theme::default();
        for i in 0u8..=255 {
            let _ = theme.palette(i);
        }
    }

    #[test]
    fn default_fg_bg_cursor() {
        let theme = Theme::default();
        assert_eq!(theme.foreground, PackedColor::new(0xd4, 0xd4, 0xd4));
        assert_eq!(theme.background, PackedColor::new(0x1e, 0x1e, 0x1e));
        assert_eq!(theme.cursor, PackedColor::new(0xff, 0xff, 0xff));
        assert_eq!(theme.selection, PackedColor::new(0x26, 0x4f, 0x78));
    }

    #[test]
    fn ansi_colors_in_palette() {
        let theme = Theme::default();
        assert_eq!(theme.palette(0), PackedColor::new(0, 0, 0));
        assert_eq!(theme.palette(1), PackedColor::new(0xcc, 0, 0));
        assert_eq!(theme.palette(7), PackedColor::new(0xcc, 0xcc, 0xcc));
        assert_eq!(theme.palette(8), PackedColor::new(0x55, 0x55, 0x55));
        assert_eq!(theme.palette(15), PackedColor::new(0xff, 0xff, 0xff));
    }

    #[test]
    fn color_cube_index_16() {
        assert_eq!(Theme::default().palette(16), PackedColor::new(0, 0, 0));
    }
    #[test]
    fn color_cube_index_17() {
        assert_eq!(Theme::default().palette(17), PackedColor::new(0, 0, 95));
    }
    #[test]
    fn color_cube_index_21() {
        assert_eq!(Theme::default().palette(21), PackedColor::new(0, 0, 255));
    }
    #[test]
    fn color_cube_index_196() {
        assert_eq!(Theme::default().palette(196), PackedColor::new(255, 0, 0));
    }
    #[test]
    fn color_cube_index_231() {
        assert_eq!(
            Theme::default().palette(231),
            PackedColor::new(255, 255, 255)
        );
    }
    #[test]
    fn color_cube_index_82() {
        assert_eq!(Theme::default().palette(82), PackedColor::new(95, 255, 0));
    }
    #[test]
    fn color_cube_mid() {
        assert_eq!(
            Theme::default().palette(145),
            PackedColor::new(175, 175, 175)
        );
    }
    #[test]
    fn grayscale_ramp_start() {
        assert_eq!(Theme::default().palette(232), PackedColor::new(8, 8, 8));
    }
    #[test]
    fn grayscale_ramp_end() {
        assert_eq!(
            Theme::default().palette(255),
            PackedColor::new(238, 238, 238)
        );
    }
    #[test]
    fn grayscale_ramp_mid() {
        assert_eq!(
            Theme::default().palette(244),
            PackedColor::new(128, 128, 128)
        );
    }

    #[test]
    fn from_color_strings_uses_colors() {
        let mut ansi = DEFAULT_ANSI_HEX;
        ansi[1] = "#ff0000"; // override red

        let colors = ThemeColorStrings {
            foreground: "#aabbcc",
            background: "#112233",
            cursor: DEFAULT_CURSOR_HEX,
            selection: DEFAULT_SELECTION_HEX,
            ansi,
        };

        let theme = Theme::from_color_strings(&colors);
        assert_eq!(theme.foreground, PackedColor::new(0xaa, 0xbb, 0xcc));
        assert_eq!(theme.background, PackedColor::new(0x11, 0x22, 0x33));
        assert_eq!(theme.palette(1), PackedColor::new(0xff, 0x00, 0x00));
    }

    #[test]
    fn set_palette_works() {
        let mut theme = Theme::default();
        let new_color = PackedColor::new(0x12, 0x34, 0x56);
        theme.set_palette(42, new_color);
        assert_eq!(theme.palette(42), new_color);
    }

    #[test]
    fn fg_rgba_correctness() {
        let rgba = Theme::default().fg_rgba();
        let expected = 212.0 / 255.0;
        assert!((rgba[0] - expected).abs() < 1e-4);
        assert!((rgba[1] - expected).abs() < 1e-4);
        assert!((rgba[2] - expected).abs() < 1e-4);
        assert!((rgba[3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn bg_rgba_correctness() {
        let rgba = Theme::default().bg_rgba();
        let expected = 30.0 / 255.0;
        assert!((rgba[0] - expected).abs() < 1e-4);
        assert!((rgba[1] - expected).abs() < 1e-4);
        assert!((rgba[2] - expected).abs() < 1e-4);
        assert!((rgba[3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cursor_rgba_correctness() {
        let rgba = Theme::default().cursor_rgba();
        assert!((rgba[0] - 1.0).abs() < 1e-6);
        assert!((rgba[1] - 1.0).abs() < 1e-6);
        assert!((rgba[2] - 1.0).abs() < 1e-6);
        assert!((rgba[3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn selection_rgba_correctness() {
        let rgba = Theme::default().selection_rgba();
        assert!((rgba[0] - 38.0 / 255.0).abs() < 1e-4);
        assert!((rgba[1] - 79.0 / 255.0).abs() < 1e-4);
        assert!((rgba[2] - 120.0 / 255.0).abs() < 1e-4);
        assert!((rgba[3] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn hex_to_packed_invalid_fallback() {
        assert_eq!(hex_to_packed("not_a_color"), PackedColor(0));
        assert_eq!(hex_to_packed(""), PackedColor(0));
    }

    #[test]
    fn cube_component_values() {
        assert_eq!(cube_component(0), 0);
        assert_eq!(cube_component(1), 95);
        assert_eq!(cube_component(2), 135);
        assert_eq!(cube_component(3), 175);
        assert_eq!(cube_component(4), 215);
        assert_eq!(cube_component(5), 255);
    }

    #[test]
    fn theme_file_parsing() {
        let toml_str = r##"
background = "#282828"
foreground = "#ebdbb2"
cursor = "#fabd2f"
color0 = "#282828"
color1 = "#cc241d"
color2 = "#98971a"
color3 = "#d79921"
color4 = "#458588"
color5 = "#b16286"
color6 = "#689d6a"
color7 = "#a89984"
color8 = "#928374"
color9 = "#fb4934"
color10 = "#b8bb26"
color11 = "#fabd2f"
color12 = "#83a598"
color13 = "#d3869b"
color14 = "#8ec07c"
color15 = "#ebdbb2"
"##;
        let tf = ThemeFile::parse(toml_str).unwrap();
        let theme = Theme::from_theme_file(&tf);

        assert_eq!(theme.background, PackedColor::new(0x28, 0x28, 0x28));
        assert_eq!(theme.foreground, PackedColor::new(0xeb, 0xdb, 0xb2));
        assert_eq!(theme.cursor, PackedColor::new(0xfa, 0xbd, 0x2f));
        assert_eq!(theme.palette(0), PackedColor::new(0x28, 0x28, 0x28));
        assert_eq!(theme.palette(1), PackedColor::new(0xcc, 0x24, 0x1d));
        assert_eq!(theme.palette(15), PackedColor::new(0xeb, 0xdb, 0xb2));
        assert_eq!(theme.palette(196), PackedColor::new(255, 0, 0));
        assert_eq!(theme.palette(232), PackedColor::new(8, 8, 8));
    }

    #[test]
    fn theme_file_partial_defaults() {
        let toml_str = r##"
foreground = "#ffffff"
color0 = "#111111"
"##;
        let tf = ThemeFile::parse(toml_str).unwrap();
        let theme = Theme::from_theme_file(&tf);

        assert_eq!(theme.foreground, PackedColor::new(0xff, 0xff, 0xff));
        assert_eq!(theme.background, PackedColor::new(0x1e, 0x1e, 0x1e));
        assert_eq!(theme.palette(0), PackedColor::new(0x11, 0x11, 0x11));
        assert_eq!(theme.palette(1), PackedColor::new(0xcc, 0x00, 0x00));
    }

    #[test]
    fn load_theme_file_returns_none_when_no_files() {
        let _ = Theme::load_theme_file();
    }
}
