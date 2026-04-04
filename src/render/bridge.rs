use std::sync::Arc;

use crate::{
    bridge::EditorMode,
    editor::{Cursor, Style, WindowType, line_from_cells},
    renderer::{DrawCommand, WindowDrawCommand},
    terminal::{
        cell::CellWidth,
        state::{TerminalDamage, TerminalState},
        theme::TerminalTheme,
    },
};

const PRIMARY_GRID_ID: u64 = 1;

pub struct TerminalRenderBridge {
    grid_id: u64,
}

impl Default for TerminalRenderBridge {
    fn default() -> Self {
        Self::new(PRIMARY_GRID_ID)
    }
}

impl TerminalRenderBridge {
    pub fn new(grid_id: u64) -> Self {
        Self { grid_id }
    }

    pub fn with_theme(_theme: TerminalTheme) -> Self {
        Self::new(PRIMARY_GRID_ID)
    }

    pub fn with_theme_for_grid(grid_id: u64, _theme: TerminalTheme) -> Self {
        Self::new(grid_id)
    }

    pub fn with_default_style(grid_id: u64, _default_style: Style) -> Self {
        Self::new(grid_id)
    }

    pub fn full_draw_commands(&self, state: &TerminalState) -> Vec<DrawCommand> {
        let screen = state.screen();
        let mut commands = Vec::with_capacity(screen.rows() + 6);

        commands.push(DrawCommand::DefaultStyleChanged(Style::new(default_colors(state.theme()))));
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
        self.push_row_draw_commands(&mut commands, state, (0..screen.rows()).rev());
        commands.push(DrawCommand::UpdateCursor(self.cursor_from_state(state)));
        commands
            .push(DrawCommand::Window { grid_id: self.grid_id, command: WindowDrawCommand::Show });
        commands.push(DrawCommand::UIReady);
        commands
    }

    pub fn draw_commands(&self, state: &TerminalState, damage: TerminalDamage) -> Vec<DrawCommand> {
        match damage {
            TerminalDamage::Full => self.full_draw_commands(state),
            TerminalDamage::None => vec![DrawCommand::UpdateCursor(self.cursor_from_state(state))],
            TerminalDamage::Rows(rows) => {
                let mut commands = Vec::with_capacity(rows.len() + 1);
                self.push_row_draw_commands(&mut commands, state, rows.into_iter().rev());
                commands.push(DrawCommand::UpdateCursor(self.cursor_from_state(state)));
                commands
            }
        }
    }

    fn push_row_draw_commands<I>(
        &self,
        commands: &mut Vec<DrawCommand>,
        state: &TerminalState,
        rows: I,
    ) where
        I: IntoIterator<Item = usize>,
    {
        let screen = state.screen();
        for row in rows {
            commands.push(DrawCommand::Window {
                grid_id: self.grid_id,
                command: WindowDrawCommand::DrawLine {
                    row,
                    line: line_for_row(screen, row, state.theme()),
                },
            });
        }
    }

    fn cursor_from_state(&self, state: &TerminalState) -> Cursor {
        let screen = state.screen();
        let terminal_cursor = state.cursor();
        let (text, style, double_width) =
            cursor_cell(screen, terminal_cursor.column, terminal_cursor.row, state.theme());

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

fn line_for_row(
    screen: &crate::terminal::screen::TerminalScreen,
    row: usize,
    theme: &TerminalTheme,
) -> crate::editor::Line {
    let cells = screen.row(row).expect("row draw requested for valid terminal row");

    let row_cells = cells
        .iter()
        .map(|cell| {
            let style = cell.style.as_ref().map(|style| style.resolve(theme));
            (cell.text.clone(), style)
        })
        .collect::<Vec<_>>();
    line_from_cells(&row_cells)
}

fn cursor_cell(
    screen: &crate::terminal::screen::TerminalScreen,
    col: usize,
    row: usize,
    theme: &TerminalTheme,
) -> (String, Option<Arc<Style>>, bool) {
    let Some(cell) = screen.get(col, row) else {
        return (" ".to_string(), None, false);
    };

    match cell.width {
        CellWidth::Continuation if col > 0 => {
            let left = screen.get(col - 1, row).unwrap_or(cell);
            (left.text.clone(), left.style.as_ref().map(|style| style.resolve(theme)), true)
        }
        CellWidth::Double => {
            (cell.text.clone(), cell.style.as_ref().map(|style| style.resolve(theme)), true)
        }
        _ => (cell.text.clone(), cell.style.as_ref().map(|style| style.resolve(theme)), false),
    }
}

fn default_colors(theme: &TerminalTheme) -> crate::editor::Colors {
    theme.default_colors()
}

#[cfg(test)]
mod tests {
    use super::TerminalRenderBridge;
    use crate::{
        renderer::{DrawCommand, WindowDrawCommand},
        terminal::{
            parser::TerminalParser,
            state::{TerminalDamage, TerminalState},
        },
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

    #[test]
    fn row_damage_only_redraws_changed_rows() {
        let bridge = TerminalRenderBridge::default();
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(4, 2);
        parser.advance(&mut state, b"ab\r\ncd");

        let commands = bridge.draw_commands(&state, TerminalDamage::Rows(vec![1]));

        assert_eq!(
            commands
                .iter()
                .filter(|command| matches!(
                    command,
                    DrawCommand::Window { command: WindowDrawCommand::DrawLine { .. }, .. }
                ))
                .count(),
            1
        );
        assert!(commands.iter().any(|command| matches!(command, DrawCommand::UpdateCursor(_))));
    }
}
