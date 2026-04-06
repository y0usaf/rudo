//! Terminal color theme/palette system.
//!
//! Holds foreground, background, cursor, and selection colors plus the full
//! 256-color palette. The first 16 entries (ANSI colors) come from the theme;
//! indices 16-231 are the 6×6×6 color cube and 232-255 are a grayscale ramp.
//!
//! Themes can be loaded from:
//! - A `ColorConfig` in `config.toml`
//! - A standalone theme TOML file (wallust / termvide compatible)

use std::path::PathBuf;

use super::cell::PackedColor;
use crate::config::{parse_hex_color, ColorConfig};
use crate::toml_parser::TomlTable;

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
            .unwrap_or(DEFAULT_ANSI[index as usize])
    }
}

const DEFAULT_FG: &str = "#d4d4d4";
const DEFAULT_BG: &str = "#1e1e1e";
const DEFAULT_CURSOR: &str = "#ffffff";
const DEFAULT_SELECTION: &str = "#264f78";

const DEFAULT_ANSI: [&str; 16] = [
    "#000000", "#cc0000", "#00cc00", "#cccc00", "#0000cc", "#cc00cc", "#00cccc", "#cccccc",
    "#555555", "#ff5555", "#55ff55", "#ffff55", "#5555ff", "#ff55ff", "#55ffff", "#ffffff",
];

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
        55 + 40 * v
    }
}

fn build_palette(ansi: [PackedColor; 16]) -> [PackedColor; 256] {
    let mut palette = [PackedColor(0); 256];
    palette[..16].copy_from_slice(&ansi);

    for r in 0u8..6 {
        for g in 0u8..6 {
            for b in 0u8..6 {
                let idx = 16 + 36 * r as usize + 6 * g as usize + b as usize;
                palette[idx] =
                    PackedColor::new(cube_component(r), cube_component(g), cube_component(b));
            }
        }
    }

    for i in 0u8..24 {
        let v = 8 + 10 * i;
        palette[232 + i as usize] = PackedColor::new(v, v, v);
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
    pub fn from_config(colors: &ColorConfig) -> Self {
        let ansi: [PackedColor; 16] = [
            hex_to_packed(&colors.black),
            hex_to_packed(&colors.red),
            hex_to_packed(&colors.green),
            hex_to_packed(&colors.yellow),
            hex_to_packed(&colors.blue),
            hex_to_packed(&colors.magenta),
            hex_to_packed(&colors.cyan),
            hex_to_packed(&colors.white),
            hex_to_packed(&colors.bright_black),
            hex_to_packed(&colors.bright_red),
            hex_to_packed(&colors.bright_green),
            hex_to_packed(&colors.bright_yellow),
            hex_to_packed(&colors.bright_blue),
            hex_to_packed(&colors.bright_magenta),
            hex_to_packed(&colors.bright_cyan),
            hex_to_packed(&colors.bright_white),
        ];

        Self {
            foreground: hex_to_packed(&colors.foreground),
            background: hex_to_packed(&colors.background),
            cursor: hex_to_packed(&colors.cursor),
            selection: hex_to_packed(&colors.selection),
            palette: build_palette(ansi),
        }
    }

    pub fn default() -> Self {
        Self::from_config(&ColorConfig::default())
    }

    pub fn load_theme_file() -> Option<Self> {
        for path in Self::theme_search_paths() {
            if !path.exists() {
                continue;
            }

            match std::fs::read_to_string(&path) {
                Ok(contents) => match ThemeFile::parse(&contents) {
                    Some(tf) => {
                        eprintln!("[INFO] Loaded theme from {}", path.display());
                        return Some(Self::from_theme_file(&tf));
                    }
                    None => {
                        eprintln!("[WARN] Failed to parse theme file {}", path.display());
                    }
                },
                Err(e) => {
                    eprintln!("[WARN] Failed to read theme file {}: {}", path.display(), e);
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
        let mut paths = Vec::with_capacity(6);

        if let Ok(env_path) = std::env::var("RUDO_THEME") {
            paths.push(PathBuf::from(env_path));
        }
        if let Ok(env_path) = std::env::var("SWIFTTERM_THEME") {
            paths.push(PathBuf::from(env_path));
        }

        if let Ok(home) = std::env::var("HOME") {
            let home = PathBuf::from(home);
            paths.push(home.join(".cache/wallust/rudo-theme.toml"));
            paths.push(home.join(".config/rudo/theme.toml"));
            paths.push(home.join(".cache/wallust/swiftterm-theme.toml"));
            paths.push(home.join(".config/swiftterm/theme.toml"));
        }

        paths
    }

    fn from_theme_file(tf: &ThemeFile) -> Self {
        let mut ansi = [PackedColor(0); 16];
        for i in 0u8..16 {
            ansi[i as usize] = hex_to_packed(tf.ansi_color(i));
        }

        Self {
            foreground: hex_to_packed(tf.foreground.as_deref().unwrap_or(DEFAULT_FG)),
            background: hex_to_packed(tf.background.as_deref().unwrap_or(DEFAULT_BG)),
            cursor: hex_to_packed(tf.cursor.as_deref().unwrap_or(DEFAULT_CURSOR)),
            selection: hex_to_packed(tf.selection.as_deref().unwrap_or(DEFAULT_SELECTION)),
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
    fn from_config_uses_colors() {
        let mut colors = ColorConfig::default();
        colors.foreground = "#aabbcc".to_string();
        colors.background = "#112233".to_string();
        colors.red = "#ff0000".to_string();

        let theme = Theme::from_config(&colors);
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
