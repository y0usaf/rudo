use std::sync::Arc;

use compact_str::CompactString;

use crate::terminal::{Hyperlink, style::TerminalStyle};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellWidth {
    Zero,
    Single,
    Double,
    Continuation,
}

const BLANK_SPACE: CompactString = CompactString::const_new(" ");
const EMPTY: CompactString = CompactString::const_new("");

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalCell {
    text: CompactString,
    pub style: Option<Arc<TerminalStyle>>,
    pub hyperlink: Option<Arc<Hyperlink>>,
    pub width: CellWidth,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self::blank(None, None)
    }
}

impl TerminalCell {
    pub fn blank(style: Option<Arc<TerminalStyle>>, hyperlink: Option<Arc<Hyperlink>>) -> Self {
        Self { text: BLANK_SPACE, style, hyperlink, width: CellWidth::Single }
    }

    pub fn blank_plain(style: Option<Arc<TerminalStyle>>) -> Self {
        Self::blank(style, None)
    }

    pub fn continuation(
        style: Option<Arc<TerminalStyle>>,
        hyperlink: Option<Arc<Hyperlink>>,
    ) -> Self {
        Self { text: EMPTY, style, hyperlink, width: CellWidth::Continuation }
    }

    pub fn zero_width(
        text: impl Into<String>,
        style: Option<Arc<TerminalStyle>>,
        hyperlink: Option<Arc<Hyperlink>>,
    ) -> Self {
        Self { text: CompactString::from(text.into()), style, hyperlink, width: CellWidth::Zero }
    }

    pub fn occupied(
        text: impl Into<String>,
        style: Option<Arc<TerminalStyle>>,
        width: CellWidth,
    ) -> Self {
        Self { text: CompactString::from(text.into()), style, hyperlink: None, width }
    }

    pub fn occupied_hyperlink(
        text: impl Into<String>,
        style: Option<Arc<TerminalStyle>>,
        hyperlink: Option<Arc<Hyperlink>>,
        width: CellWidth,
    ) -> Self {
        Self { text: CompactString::from(text.into()), style, hyperlink, width }
    }

    /// Create a cell from a single character without an intermediate `String` allocation.
    pub fn from_char(
        ch: char,
        style: Option<Arc<TerminalStyle>>,
        hyperlink: Option<Arc<Hyperlink>>,
        width: CellWidth,
    ) -> Self {
        let mut text = CompactString::with_capacity(ch.len_utf8());
        text.push(ch);
        Self { text, style, hyperlink, width }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn text_mut(&mut self) -> &mut CompactString {
        &mut self.text
    }

    pub fn into_text(self) -> String {
        self.text.into_string()
    }

    pub fn is_blank(&self) -> bool {
        matches!(self.width, CellWidth::Single)
            && self.text == " "
            && self.style.is_none()
            && self.hyperlink.is_none()
    }
}
