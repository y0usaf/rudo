use crate::ui::CursorShape;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalCursor {
    pub row: usize,
    pub column: usize,
    pub shape: CursorShape,
    pub visible: bool,
    pub blinking: bool,
}

impl Default for TerminalCursor {
    fn default() -> Self {
        Self { row: 0, column: 0, shape: CursorShape::Block, visible: true, blinking: true }
    }
}
