use std::{ops::Range, sync::Arc};

use skia_safe::Color4f;

pub type GridCell = (String, Option<Arc<Style>>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hyperlink {
    pub id: Option<String>,
    pub uri: String,
}

#[derive(new, Debug, Clone, PartialEq)]
pub struct Colors {
    pub foreground: Option<Color4f>,
    pub background: Option<Color4f>,
    pub special: Option<Color4f>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum UnderlineStyle {
    Underline,
    UnderDouble,
    UnderDash,
    UnderDot,
    UnderCurl,
}

#[derive(new, Debug, Clone, PartialEq)]
pub struct Style {
    pub colors: Colors,
    #[new(default)]
    pub reverse: bool,
    #[new(default)]
    pub italic: bool,
    #[new(default)]
    pub bold: bool,
    #[new(default)]
    pub strikethrough: bool,
    #[new(default)]
    pub blend: u8,
    #[new(default)]
    pub underline: Option<UnderlineStyle>,
}

impl Style {
    pub fn foreground(&self, default_colors: &Colors) -> Color4f {
        if self.reverse {
            self.colors.background.unwrap_or_else(|| default_colors.background.unwrap())
        } else {
            self.colors.foreground.unwrap_or_else(|| default_colors.foreground.unwrap())
        }
    }

    pub fn background(&self, default_colors: &Colors) -> Color4f {
        if self.reverse {
            self.colors.foreground.unwrap_or_else(|| default_colors.foreground.unwrap())
        } else {
            self.colors.background.unwrap_or_else(|| default_colors.background.unwrap())
        }
    }

    pub fn special(&self, default_colors: &Colors) -> Color4f {
        self.colors.special.unwrap_or_else(|| self.foreground(default_colors))
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum CursorShape {
    Block,
    Horizontal,
    Vertical,
}


#[derive(Clone, Debug, PartialEq)]
pub struct Cursor {
    pub grid_position: (u64, u64),
    pub parent_window_id: u64,
    pub shape: CursorShape,
    pub cell_percentage: Option<f32>,
    pub blinkwait: Option<u64>,
    pub blinkon: Option<u64>,
    pub blinkoff: Option<u64>,
    pub style: Option<Arc<Style>>,
    pub enabled: bool,
    pub double_width: bool,
    pub grid_cell: GridCell,
}

impl Default for Cursor {
    fn default() -> Self {
        Self::new()
    }
}

impl Cursor {
    pub fn new() -> Cursor {
        Cursor {
            grid_position: (0, 0),
            parent_window_id: 0,
            shape: CursorShape::Block,
            style: None,
            cell_percentage: None,
            blinkwait: None,
            blinkon: None,
            blinkoff: None,
            enabled: true,
            double_width: false,
            grid_cell: (" ".to_string(), None),
        }
    }

    fn default_cell_colors(&self, default_colors: &Colors) -> (Color4f, Color4f) {
        let foreground = default_colors
            .foreground
            .expect("cursor rendering requires a default foreground color");
        let background = default_colors
            .background
            .expect("cursor rendering requires a default background color");

        (foreground, background)
    }

    fn cell_colors(&self, default_colors: &Colors) -> (Color4f, Color4f) {
        self.grid_cell
            .1
            .as_ref()
            .map(|style| (style.foreground(default_colors), style.background(default_colors)))
            .unwrap_or_else(|| self.default_cell_colors(default_colors))
    }

    fn default_cursor_colors(&self, default_colors: &Colors) -> (Color4f, Color4f) {
        self.reverse_colors(self.default_cell_colors(default_colors))
    }

    fn reverse_colors(&self, (foreground, background): (Color4f, Color4f)) -> (Color4f, Color4f) {
        (background, foreground)
    }

    fn style_colors(
        &self,
        style: &Style,
        fallback_colors: (Color4f, Color4f),
        apply_reverse: bool,
    ) -> (Color4f, Color4f) {
        let (fallback_foreground, fallback_background) = fallback_colors;
        let colors = (
            style.colors.foreground.unwrap_or(fallback_foreground),
            style.colors.background.unwrap_or(fallback_background),
        );

        match (apply_reverse, style.reverse) {
            (true, true) => self.reverse_colors(colors),
            _ => colors,
        }
    }

    fn resolve_colors(
        &self,
        default_colors: &Colors,
        cell_color_fallback: bool,
    ) -> (Color4f, Color4f) {
        let default_cursor_colors = self.default_cursor_colors(default_colors);
        let cell_colors = self.cell_colors(default_colors);
        let style_fallback_colors = match cell_color_fallback {
            true => cell_colors,
            false => default_cursor_colors,
        };

        self.style
            .as_deref()
            .map(|style| self.style_colors(style, style_fallback_colors, cell_color_fallback))
            .unwrap_or_else(|| match cell_color_fallback {
                true => self.reverse_colors(cell_colors),
                false => default_cursor_colors,
            })
    }

    pub fn foreground(&self, default_colors: &Colors, cell_color_fallback: bool) -> Color4f {
        self.resolve_colors(default_colors, cell_color_fallback).0
    }

    pub fn background(&self, default_colors: &Colors, cell_color_fallback: bool) -> Color4f {
        self.resolve_colors(default_colors, cell_color_fallback).1
    }

    pub fn alpha(&self) -> u8 {
        self.style
            .as_ref()
            .map(|s| (255_f32 * ((100 - s.blend) as f32 / 100.0_f32)) as u8)
            .unwrap_or(255)
    }

}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WindowType {
    Editor,
    Message { scrolled: bool },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    fragments: Vec<LineFragmentData>,
    cells: Option<Vec<String>>,
    hyperlinks: Option<Vec<Option<Arc<Hyperlink>>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LineFragmentData {
    text_range: Range<u32>,
    style: Option<Arc<Style>>,
    cells: Range<u32>,
    words: Vec<WordData>,
}

impl LineFragmentData {
    pub fn new(
        text_range: Range<u32>,
        cells: Range<u32>,
        style: Option<Arc<Style>>,
        words: Vec<WordData>,
    ) -> Self {
        Self { text_range, style, cells, words }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct WordData {
    pub text_offset: u32,
    pub cell: u32,
    pub cluster_sizes: Vec<u8>,
}

pub struct LineFragment<'a> {
    pub style: &'a Option<Arc<Style>>,
    pub text: &'a str,
    pub cells: &'a Range<u32>,
    data: &'a LineFragmentData,
}

pub struct Word<'a> {
    pub text: &'a str,
    pub cell: u32,
    cluster_sizes: &'a [u8],
}

impl Line {
    pub fn new(
        text: String,
        fragments: Vec<LineFragmentData>,
        cells: Option<Vec<String>>,
        hyperlinks: Option<Vec<Option<Arc<Hyperlink>>>>,
    ) -> Self {
        Self { text, fragments, cells, hyperlinks }
    }

    pub fn fragments(&self) -> impl Iterator<Item = LineFragment<'_>> {
        self.fragments.iter().map(|fragment| {
            let range = fragment.text_range.start as usize..fragment.text_range.end as usize;
            LineFragment {
                style: &fragment.style,
                text: &self.text[range],
                cells: &fragment.cells,
                data: fragment,
            }
        })
    }

    pub fn cells(&self) -> Option<&[String]> {
        self.cells.as_deref()
    }

    pub fn hyperlink_at_cell(&self, cell: usize) -> Option<&Arc<Hyperlink>> {
        self.hyperlinks.as_deref().and_then(|links| links.get(cell)).and_then(|link| link.as_ref())
    }
}

impl LineFragment<'_> {
    pub fn words(&self) -> impl Iterator<Item = Word<'_>> {
        self.data.words.iter().map(|word| {
            let size: usize = word.cluster_sizes.iter().map(|v| *v as usize).sum();
            let cluster_sizes = &word.cluster_sizes;
            let start = word.text_offset as usize;
            let end = start + size;
            let text = &self.text[start..end];
            Word { text, cell: word.cell, cluster_sizes }
        })
    }
}

impl<'a> Word<'a> {
    pub fn new(text: &'a str, cluster_sizes: &'a [u8]) -> Self {
        Self { text, cell: 0, cluster_sizes }
    }

    pub fn grapheme_clusters(&self) -> impl Iterator<Item = (usize, &'a str)> + Clone {
        self.cluster_sizes.iter().enumerate().filter(|(_, size)| **size > 0).scan(
            0,
            |current_pos, (cell_nr, size)| {
                let start = *current_pos;
                *current_pos += *size as u32;
                Some((cell_nr, &self.text[start as usize..*current_pos as usize]))
            },
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SortOrder {
    pub z_index: u64,
    pub composition_order: u64,
}

impl Ord for SortOrder {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let a = (self.z_index, self.composition_order as i64);
        let b = (other.z_index, other.composition_order as i64);
        a.cmp(&b)
    }
}

impl PartialOrd for SortOrder {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnchorInfo {
    pub anchor_grid_id: u64,
    pub anchor_type: WindowAnchor,
    pub anchor_left: f64,
    pub anchor_top: f64,
    pub sort_order: SortOrder,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub enum WindowAnchor {
    NorthWest,
    NorthEast,
    SouthWest,
    SouthEast,
    Absolute,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub enum EditorMode {
    Normal,
    Insert,
    Visual,
    Replace,
    CmdLine,
    Unknown(String),
}


