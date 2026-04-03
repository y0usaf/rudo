use std::sync::Arc;

use crate::editor::Style;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellWidth {
    Zero,
    Single,
    Double,
    Continuation,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalCell {
    pub text: String,
    pub style: Option<Arc<Style>>,
    pub width: CellWidth,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self::blank(None)
    }
}

impl TerminalCell {
    pub fn blank(style: Option<Arc<Style>>) -> Self {
        Self { text: " ".to_string(), style, width: CellWidth::Single }
    }

    pub fn continuation(style: Option<Arc<Style>>) -> Self {
        Self { text: String::new(), style, width: CellWidth::Continuation }
    }

    pub fn zero_width(text: impl Into<String>, style: Option<Arc<Style>>) -> Self {
        Self { text: text.into(), style, width: CellWidth::Zero }
    }

    pub fn occupied(text: impl Into<String>, style: Option<Arc<Style>>, width: CellWidth) -> Self {
        Self { text: text.into(), style, width }
    }

    pub fn is_blank(&self) -> bool {
        matches!(self.width, CellWidth::Single) && self.text == " " && self.style.is_none()
    }
}
