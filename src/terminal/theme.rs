use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use skia_safe::Color4f;

use crate::editor::Colors;

const WALLUST_THEME_PATH: &str = ".cache/wallust/termvide-theme.toml";
const TERMVIDE_THEME_PATH: &str = ".config/termvide/theme.toml";
const NEOVIDE_THEME_PATH: &str = ".config/neovide/termvide-theme.toml";
const TERMVIDE_THEME_ENV: &str = "TERMVIDE_THEME";

#[derive(Clone, Debug)]
pub struct TerminalTheme {
    pub background: Color4f,
    pub foreground: Color4f,
    pub cursor: Color4f,
    palette: [Color4f; 256],
}

#[derive(Debug, Deserialize)]
struct TerminalThemeFile {
    background: String,
    foreground: String,
    cursor: Option<String>,
    color0: String,
    color1: String,
    color2: String,
    color3: String,
    color4: String,
    color5: String,
    color6: String,
    color7: String,
    color8: String,
    color9: String,
    color10: String,
    color11: String,
    color12: String,
    color13: String,
    color14: String,
    color15: String,
}

impl Default for TerminalTheme {
    fn default() -> Self {
        let mut palette = default_palette();
        palette[0] = rgb(0, 0, 0);
        palette[1] = rgb(205, 49, 49);
        palette[2] = rgb(13, 188, 121);
        palette[3] = rgb(229, 229, 16);
        palette[4] = rgb(36, 114, 200);
        palette[5] = rgb(188, 63, 188);
        palette[6] = rgb(17, 168, 205);
        palette[7] = rgb(229, 229, 229);
        palette[8] = rgb(128, 128, 128);
        palette[9] = rgb(255, 85, 85);
        palette[10] = rgb(80, 250, 123);
        palette[11] = rgb(241, 250, 140);
        palette[12] = rgb(189, 147, 249);
        palette[13] = rgb(255, 121, 198);
        palette[14] = rgb(139, 233, 253);
        palette[15] = rgb(255, 255, 255);

        Self {
            background: rgb(13, 13, 13),
            foreground: rgb(230, 230, 230),
            cursor: rgb(230, 230, 230),
            palette,
        }
    }
}

impl TerminalTheme {
    pub fn load() -> Self {
        match Self::theme_search_paths().into_iter().find(|path| path.exists()) {
            Some(path) => match Self::load_from_path(&path) {
                Ok(theme) => theme,
                Err(error) => {
                    log::warn!("Failed to load terminal theme from {}: {error}", path.display());
                    Self::default()
                }
            },
            None => Self::default(),
        }
    }

    pub fn load_from_path(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path)
            .map_err(|error| format!("could not read theme file: {error}"))?;
        let file: TerminalThemeFile = toml::from_str(&text)
            .map_err(|error| format!("could not parse theme file: {error}"))?;

        let mut theme = Self::default();
        theme.background = parse_color(&file.background)?;
        theme.foreground = parse_color(&file.foreground)?;
        theme.cursor = parse_color(file.cursor.as_deref().unwrap_or(&file.foreground))?;
        theme.palette[0] = parse_color(&file.color0)?;
        theme.palette[1] = parse_color(&file.color1)?;
        theme.palette[2] = parse_color(&file.color2)?;
        theme.palette[3] = parse_color(&file.color3)?;
        theme.palette[4] = parse_color(&file.color4)?;
        theme.palette[5] = parse_color(&file.color5)?;
        theme.palette[6] = parse_color(&file.color6)?;
        theme.palette[7] = parse_color(&file.color7)?;
        theme.palette[8] = parse_color(&file.color8)?;
        theme.palette[9] = parse_color(&file.color9)?;
        theme.palette[10] = parse_color(&file.color10)?;
        theme.palette[11] = parse_color(&file.color11)?;
        theme.palette[12] = parse_color(&file.color12)?;
        theme.palette[13] = parse_color(&file.color13)?;
        theme.palette[14] = parse_color(&file.color14)?;
        theme.palette[15] = parse_color(&file.color15)?;
        Ok(theme)
    }

    pub fn default_colors(&self) -> Colors {
        Colors {
            foreground: Some(self.foreground),
            background: Some(self.background),
            special: Some(self.foreground),
        }
    }

    pub fn ansi_color(&self, index: u8, bright: bool) -> Color4f {
        let offset = if bright { 8 } else { 0 };
        self.palette[(offset + index as usize) % 16]
    }

    pub fn palette_color(&self, index: u8) -> Color4f {
        self.palette[index as usize]
    }

    pub fn set_palette_color(&mut self, index: u8, color: Color4f) {
        self.palette[index as usize] = color;
    }

    pub fn set_foreground(&mut self, color: Color4f) {
        self.foreground = color;
    }

    pub fn set_background(&mut self, color: Color4f) {
        self.background = color;
    }

    pub fn set_cursor(&mut self, color: Color4f) {
        self.cursor = color;
    }

    fn theme_search_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();

        if let Ok(path) = env::var(TERMVIDE_THEME_ENV) {
            let trimmed = path.trim();
            if !trimmed.is_empty() {
                paths.push(PathBuf::from(trimmed));
            }
        }

        if let Some(home) = dirs::home_dir() {
            paths.push(home.join(WALLUST_THEME_PATH));
            paths.push(home.join(TERMVIDE_THEME_PATH));
            paths.push(home.join(NEOVIDE_THEME_PATH));
        }

        paths
    }
}

pub fn parse_color(value: &str) -> Result<Color4f, String> {
    let value = value.trim();
    if let Some(value) = value.strip_prefix("rgb:") {
        return parse_x11_rgb(value);
    }

    let color = csscolorparser::parse(value)
        .map_err(|error| format!("invalid color {value:?}: {error}"))?
        .to_rgba8();
    Ok(rgb(color[0], color[1], color[2]))
}

fn default_palette() -> [Color4f; 256] {
    std::array::from_fn(|index| palette_entry(index as u8))
}

fn palette_entry(index: u8) -> Color4f {
    match index {
        0..=15 => rgb(0, 0, 0),
        16..=231 => {
            let index = index - 16;
            let r = index / 36;
            let g = (index % 36) / 6;
            let b = index % 6;
            let component = |value: u8| if value == 0 { 0 } else { value * 40 + 55 };
            rgb(component(r), component(g), component(b))
        }
        232..=255 => {
            let gray = 8 + (index - 232) * 10;
            rgb(gray, gray, gray)
        }
    }
}

fn parse_x11_rgb(value: &str) -> Result<Color4f, String> {
    let parts = value.split('/').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err(format!("invalid rgb color {value:?}"));
    }

    Ok(rgb(
        parse_x11_component(parts[0])?,
        parse_x11_component(parts[1])?,
        parse_x11_component(parts[2])?,
    ))
}

fn parse_x11_component(value: &str) -> Result<u8, String> {
    if value.is_empty() || value.len() > 4 {
        return Err(format!("invalid rgb component {value:?}"));
    }

    let parsed = u16::from_str_radix(value, 16)
        .map_err(|error| format!("invalid rgb component: {error}"))?;
    let max = ((1u32 << (value.len() * 4)) - 1).max(1);
    Ok(((parsed as u32 * 255 + max / 2) / max) as u8)
}

fn rgb(r: u8, g: u8, b: u8) -> Color4f {
    Color4f::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
}

pub fn to_osc_rgb_spec(color: Color4f) -> String {
    let r = (color.r.clamp(0.0, 1.0) * 255.0).round() as u8;
    let g = (color.g.clamp(0.0, 1.0) * 255.0).round() as u8;
    let b = (color.b.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("rgb:{:02x}{:02x}/{:02x}{:02x}/{:02x}{:02x}", r, r, g, g, b, b)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{TerminalTheme, parse_color, rgb};

    fn temp_theme_path() -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        path.push(format!("termvide-theme-{unique}.toml"));
        path
    }

    #[test]
    fn loads_theme_file() {
        let path = temp_theme_path();
        fs::write(
            &path,
            r##"
background = "#101112"
foreground = "#f0f1f2"
cursor = "#abcdef"
color0 = "#000000"
color1 = "#111111"
color2 = "#222222"
color3 = "#333333"
color4 = "#444444"
color5 = "#555555"
color6 = "#666666"
color7 = "#777777"
color8 = "#888888"
color9 = "#999999"
color10 = "#aaaaaa"
color11 = "#bbbbbb"
color12 = "#cccccc"
color13 = "#dddddd"
color14 = "#eeeeee"
color15 = "#ffffff"
"##,
        )
        .unwrap();

        let theme = TerminalTheme::load_from_path(&path).unwrap();
        assert_eq!(theme.background, rgb(0x10, 0x11, 0x12));
        assert_eq!(theme.foreground, rgb(0xf0, 0xf1, 0xf2));
        assert_eq!(theme.cursor, rgb(0xab, 0xcd, 0xef));
        assert_eq!(theme.ansi_color(0, false), rgb(0x00, 0x00, 0x00));
        assert_eq!(theme.ansi_color(7, true), rgb(0xff, 0xff, 0xff));

        let _ = fs::remove_file(path);
    }

    #[test]
    fn falls_back_to_foreground_for_cursor() {
        let path = temp_theme_path();
        fs::write(
            &path,
            r##"
background = "#101112"
foreground = "#f0f1f2"
color0 = "#000000"
color1 = "#111111"
color2 = "#222222"
color3 = "#333333"
color4 = "#444444"
color5 = "#555555"
color6 = "#666666"
color7 = "#777777"
color8 = "#888888"
color9 = "#999999"
color10 = "#aaaaaa"
color11 = "#bbbbbb"
color12 = "#cccccc"
color13 = "#dddddd"
color14 = "#eeeeee"
color15 = "#ffffff"
"##,
        )
        .unwrap();

        let theme = TerminalTheme::load_from_path(&path).unwrap();
        assert_eq!(theme.cursor, theme.foreground);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn parses_x11_rgb_colors() {
        assert_eq!(parse_color("rgb:ff/00/80").unwrap(), rgb(255, 0, 128));
        assert_eq!(parse_color("rgb:f/8/0").unwrap(), rgb(255, 136, 0));
    }
}
