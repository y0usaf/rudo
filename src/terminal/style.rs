use std::sync::Arc;

use skia_safe::Color4f;

use crate::{
    editor::{Colors, Style, UnderlineStyle},
    terminal::theme::TerminalTheme,
};

#[derive(Clone, Debug, PartialEq)]
pub enum TerminalColor {
    Palette(u8),
    Rgb(Color4f),
}

impl TerminalColor {
    pub fn resolve(&self, theme: &TerminalTheme) -> Color4f {
        match self {
            Self::Palette(index) => theme.palette_color(*index),
            Self::Rgb(color) => *color,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TerminalColors {
    pub foreground: Option<TerminalColor>,
    pub background: Option<TerminalColor>,
    pub special: Option<TerminalColor>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TerminalStyle {
    pub colors: TerminalColors,
    pub reverse: bool,
    pub italic: bool,
    pub bold: bool,
    pub strikethrough: bool,
    pub underline: Option<UnderlineStyle>,
}

impl TerminalStyle {
    pub fn resolve(&self, theme: &TerminalTheme) -> Arc<Style> {
        Arc::new(Style {
            colors: Colors {
                foreground: self.colors.foreground.as_ref().map(|color| color.resolve(theme)),
                background: self.colors.background.as_ref().map(|color| color.resolve(theme)),
                special: self.colors.special.as_ref().map(|color| color.resolve(theme)),
            },
            reverse: self.reverse,
            italic: self.italic,
            bold: self.bold,
            strikethrough: self.strikethrough,
            blend: 0,
            underline: self.underline,
        })
    }

    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}
