use std::sync::Arc;

use skia_safe::Color4f;

use crate::{
    bridge::EditorMode,
    editor::{Colors, Cursor, Style, WindowType, line_from_cells},
    renderer::{DrawCommand, WindowDrawCommand},
    terminal::{cell::CellWidth, state::TerminalState},
};

const PRIMARY_GRID_ID: u64 = 1;

pub struct TerminalRenderBridge {
    grid_id: u64,
    default_style: Style,
}

impl Default for TerminalRenderBridge {
    fn default() -> Self {
        Self::new(PRIMARY_GRID_ID)
    }
}

impl TerminalRenderBridge {
    pub fn new(grid_id: u64) -> Self {
        Self { grid_id, default_style: Style::new(default_colors()) }
    }

    pub fn with_default_style(grid_id: u64, default_style: Style) -> Self {
        Self { grid_id, default_style }
    }

    pub fn full_draw_commands(&self, state: &TerminalState) -> Vec<DrawCommand> {
        let screen = state.screen();
        let mut commands = Vec::with_capacity(screen.rows() + 6);

        commands.push(DrawCommand::DefaultStyleChanged(self.default_style.clone()));
        commands.push(DrawCommand::ModeChanged(EditorMode::Unknown("terminal".to_string())));
        commands.push(DrawCommand::Window {
            grid_id: self.grid_id,
            command: WindowDrawCommand::Position {
                grid_position: (0.0, 0.0),
                grid_size: (screen.cols() as u64, screen.rows() as u64),
                anchor_info: None,
                window_type: WindowType::Editor,
            },
        });
        commands
            .push(DrawCommand::Window { grid_id: self.grid_id, command: WindowDrawCommand::Clear });

        for row in (0..screen.rows()).rev() {
            let row_cells = (0..screen.cols())
                .map(|col| {
                    let cell = screen.get(col, row).cloned().unwrap_or_default();
                    (cell.text, cell.style)
                })
                .collect::<Vec<_>>();
            let line = line_from_cells(&row_cells);
            commands.push(DrawCommand::Window {
                grid_id: self.grid_id,
                command: WindowDrawCommand::DrawLine { row, line },
            });
        }

        commands.push(DrawCommand::UpdateCursor(self.cursor_from_state(state)));
        commands
            .push(DrawCommand::Window { grid_id: self.grid_id, command: WindowDrawCommand::Show });
        commands.push(DrawCommand::UIReady);
        commands
    }

    fn cursor_from_state(&self, state: &TerminalState) -> Cursor {
        let screen = state.screen();
        let terminal_cursor = state.cursor();
        let (text, style, double_width) =
            cursor_cell(screen, terminal_cursor.column, terminal_cursor.row);

        Cursor {
            grid_position: (terminal_cursor.column as u64, terminal_cursor.row as u64),
            parent_window_id: self.grid_id,
            shape: terminal_cursor.shape.clone(),
            cell_percentage: None,
            blinkwait: None,
            blinkon: None,
            blinkoff: None,
            style: None,
            enabled: terminal_cursor.visible,
            double_width,
            grid_cell: (text, style),
        }
    }
}

fn cursor_cell(
    screen: &crate::terminal::screen::TerminalScreen,
    col: usize,
    row: usize,
) -> (String, Option<Arc<Style>>, bool) {
    let Some(cell) = screen.get(col, row).cloned() else {
        return (" ".to_string(), None, false);
    };

    match cell.width {
        CellWidth::Continuation if col > 0 => {
            let left = screen.get(col - 1, row).cloned().unwrap_or(cell);
            (left.text, left.style, true)
        }
        CellWidth::Double => (cell.text, cell.style, true),
        _ => (cell.text, cell.style, false),
    }
}

fn default_colors() -> Colors {
    Colors {
        foreground: Some(Color4f::new(0.9, 0.9, 0.9, 1.0)),
        background: Some(Color4f::new(0.05, 0.05, 0.05, 1.0)),
        special: Some(Color4f::new(0.9, 0.9, 0.9, 1.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::TerminalRenderBridge;
    use crate::{
        renderer::{DrawCommand, WindowDrawCommand},
        terminal::{parser::TerminalParser, state::TerminalState},
    };

    #[test]
    fn full_draw_commands_emit_window_lines_and_cursor() {
        let bridge = TerminalRenderBridge::default();
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(4, 2);
        parser.advance(&mut state, b"ab\r\ncd");

        let commands = bridge.full_draw_commands(&state);

        assert!(matches!(commands[0], DrawCommand::DefaultStyleChanged(_)));
        assert!(commands.iter().any(|command| matches!(
            command,
            DrawCommand::Window {
                command: WindowDrawCommand::Position { grid_size: (4, 2), .. },
                ..
            }
        )));
        assert!(commands.iter().any(|command| matches!(command, DrawCommand::Window { command: WindowDrawCommand::DrawLine { row: 0, line }, .. } if line.text == "ab")));
        assert!(commands.iter().any(|command| matches!(command, DrawCommand::Window { command: WindowDrawCommand::DrawLine { row: 1, line }, .. } if line.text == "cd")));
        assert!(commands.iter().any(|command| matches!(command, DrawCommand::UpdateCursor(cursor) if cursor.grid_position == (2, 1))));
        assert!(matches!(commands.last(), Some(DrawCommand::UIReady)));
    }

    #[test]
    fn cursor_uses_leading_cell_for_wide_characters() {
        let bridge = TerminalRenderBridge::default();
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(4, 1);
        parser.advance(&mut state, "好".as_bytes());
        state.set_cursor_position(0, 1);

        let commands = bridge.full_draw_commands(&state);

        assert!(commands.iter().any(|command| matches!(command, DrawCommand::UpdateCursor(cursor) if cursor.double_width && cursor.grid_cell.0 == "好")));
    }
}
