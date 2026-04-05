#![allow(dead_code)]

pub mod cell;
pub mod cursor;
pub mod input;
pub mod parser;
pub mod runtime;
pub mod screen;
pub mod session;
pub mod state;
pub mod style;
pub mod theme;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipboardSelection {
    Clipboard,
    Primary,
    Secondary,
    Select,
    Cut0,
    Cut1,
    Cut2,
    Cut3,
    Cut4,
    Cut5,
    Cut6,
    Cut7,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardRequestKind {
    Set(String),
    Query,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipboardRequest {
    pub selection: ClipboardSelection,
    pub kind: ClipboardRequestKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hyperlink {
    pub id: Option<String>,
    pub uri: String,
}
