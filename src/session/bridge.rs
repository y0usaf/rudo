use std::{collections::HashMap, sync::Arc};

use crate::{
    renderer::{DrawCommand, WindowDrawCommand},
    terminal::{
        Hyperlink as TerminalHyperlink,
        cell::CellWidth,
        screen::TerminalScreen,
        state::{TerminalDamage, TerminalState},
        style::TerminalStyle,
        theme::TerminalTheme,
    },
    ui::EditorMode,
    ui::{Cursor, Hyperlink, Line, LineFragmentData, Style, WindowType, WordData},
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
        let mut style_cache = StyleCache::new();

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
        self.push_row_draw_commands(
            &mut commands,
            state,
            (0..screen.rows()).rev(),
            &mut style_cache,
        );
        commands.push(DrawCommand::UpdateCursor(self.cursor_from_state(state, &mut style_cache)));
        commands
            .push(DrawCommand::Window { grid_id: self.grid_id, command: WindowDrawCommand::Show });
        commands.push(DrawCommand::UIReady);
        commands
    }

    pub fn draw_commands(&self, state: &TerminalState, damage: TerminalDamage) -> Vec<DrawCommand> {
        match damage {
            TerminalDamage::Full => self.full_draw_commands(state),
            TerminalDamage::None => {
                let mut sc = StyleCache::new();
                vec![DrawCommand::UpdateCursor(self.cursor_from_state(state, &mut sc))]
            }
            TerminalDamage::Rows(rows) => {
                let dirty: Vec<usize> = rows.iter().collect();
                let mut commands = Vec::with_capacity(dirty.len() + 1);
                let mut sc = StyleCache::new();
                self.push_row_draw_commands(&mut commands, state, dirty.into_iter().rev(), &mut sc);
                commands.push(DrawCommand::UpdateCursor(self.cursor_from_state(state, &mut sc)));
                commands
            }
        }
    }

    fn push_row_draw_commands<I>(
        &self,
        commands: &mut Vec<DrawCommand>,
        state: &TerminalState,
        rows: I,
        style_cache: &mut StyleCache,
    ) where
        I: IntoIterator<Item = usize>,
    {
        let screen = state.screen();
        let theme = state.theme();
        for row in rows {
            commands.push(DrawCommand::Window {
                grid_id: self.grid_id,
                command: WindowDrawCommand::DrawLine {
                    row,
                    line: line_for_row(screen, row, theme, style_cache),
                },
            });
        }
    }

    fn cursor_from_state(&self, state: &TerminalState, style_cache: &mut StyleCache) -> Cursor {
        let screen = state.screen();
        let terminal_cursor = state.cursor();
        let theme = state.theme();
        let (text, style, double_width) =
            cursor_cell(screen, terminal_cursor.column, terminal_cursor.row, theme, style_cache);

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

struct StyleCache {
    map: HashMap<*const TerminalStyle, Arc<Style>>,
}

impl StyleCache {
    fn new() -> Self {
        Self { map: HashMap::with_capacity(8) }
    }

    fn resolve(&mut self, style: &Arc<TerminalStyle>, theme: &TerminalTheme) -> Arc<Style> {
        let ptr = Arc::as_ptr(style);
        self.map.entry(ptr).or_insert_with(|| style.resolve(theme)).clone()
    }

    fn resolve_opt(
        &mut self,
        style: Option<&Arc<TerminalStyle>>,
        theme: &TerminalTheme,
    ) -> Option<Arc<Style>> {
        style.map(|s| self.resolve(s, theme))
    }
}

struct HyperlinkCache {
    map: HashMap<*const TerminalHyperlink, Arc<Hyperlink>>,
}

impl HyperlinkCache {
    fn new() -> Self {
        Self { map: HashMap::with_capacity(4) }
    }

    fn resolve_opt(&mut self, hyperlink: Option<&Arc<TerminalHyperlink>>) -> Option<Arc<Hyperlink>> {
        hyperlink.map(|link| {
            let ptr = Arc::as_ptr(link);
            self.map
                .entry(ptr)
                .or_insert_with(|| Arc::new(Hyperlink { id: link.id.clone(), uri: link.uri.clone() }))
                .clone()
        })
    }
}

fn line_for_row(
    screen: &TerminalScreen,
    row: usize,
    theme: &TerminalTheme,
    style_cache: &mut StyleCache,
) -> Line {
    let cells = screen.row(row).expect("row draw requested for valid terminal row");
    let width = cells.len();

    let mut text = String::with_capacity(width);
    let mut fragments = Vec::with_capacity(width.min(8));
    let mut cell_strings: Vec<String> = Vec::with_capacity(width);
    let mut hyperlinks: Vec<Option<Arc<Hyperlink>>> = Vec::with_capacity(width);
    let mut hyperlink_cache = HyperlinkCache::new();
    let mut start = 0;

    while start < width {
        let resolved_style = style_cache.resolve_opt(cells[start].style.as_ref(), theme);
        let text_start = text.len() as u32;
        let frag_start = start as u32;
        let mut words = Vec::with_capacity((width - start).min(8));
        let mut current_word = WordData::default();
        let mut consumed = 0u32;
        let mut last_box_char: Option<&str> = None;

        for cell in cells.iter().skip(start) {
            let cell_style = style_cache.resolve_opt(cell.style.as_ref(), theme);
            if cell_style != resolved_style {
                break;
            }

            let cluster = cell.text();

            if crate::renderer::box_drawing::is_box_char(cluster) {
                if text_start == text.len() as u32 && consumed == 0 {
                    last_box_char = Some(cluster);
                }
                if (text.len() as u32 > text_start && last_box_char.is_none())
                    || last_box_char != Some(cluster)
                {
                    break;
                }
            } else if last_box_char.is_some() {
                break;
            }

            consumed += 1;
            let cluster = if cluster.len() > 255 { " " } else { cluster };

            if cluster.is_empty() {
                if !current_word.cluster_sizes.is_empty() {
                    current_word.cluster_sizes.push(0);
                }
                cell_strings.push(String::new());
                hyperlinks.push(hyperlink_cache.resolve_opt(cell.hyperlink.as_ref()));
                continue;
            }

            let is_ws = cluster.chars().next().is_some_and(|c| c.is_whitespace());
            if is_ws {
                if !current_word.cluster_sizes.is_empty() {
                    words.push(current_word);
                    current_word = WordData::default();
                }
            } else if current_word.cluster_sizes.is_empty() {
                current_word.cell = consumed - 1;
                current_word.cluster_sizes.push(cluster.len() as u8);
                current_word.text_offset = text.len() as u32 - text_start;
            } else {
                current_word.cluster_sizes.push(cluster.len() as u8);
            }

            text.push_str(cluster);
            cell_strings.push(cluster.to_string());
            hyperlinks.push(hyperlink_cache.resolve_opt(cell.hyperlink.as_ref()));
        }

        if !current_word.cluster_sizes.is_empty() {
            words.push(current_word);
        }

        fragments.push(LineFragmentData::new(
            text_start..text.len() as u32,
            frag_start..frag_start + consumed,
            resolved_style,
            words,
        ));
        start += consumed as usize;
    }

    Line::new(text, fragments, Some(cell_strings), Some(hyperlinks))
}

fn cursor_cell(
    screen: &TerminalScreen,
    col: usize,
    row: usize,
    theme: &TerminalTheme,
    style_cache: &mut StyleCache,
) -> (String, Option<Arc<Style>>, bool) {
    let Some(cell) = screen.get(col, row) else {
        return (" ".to_string(), None, false);
    };

    match cell.width {
        CellWidth::Continuation if col > 0 => {
            let left = screen.get(col - 1, row).unwrap_or(cell);
            let style = style_cache.resolve_opt(left.style.as_ref(), theme);
            (left.text().to_string(), style, true)
        }
        CellWidth::Double => {
            let style = style_cache.resolve_opt(cell.style.as_ref(), theme);
            (cell.text().to_string(), style, true)
        }
        _ => {
            let style = style_cache.resolve_opt(cell.style.as_ref(), theme);
            (cell.text().to_string(), style, false)
        }
    }
}

fn default_colors(theme: &TerminalTheme) -> crate::ui::Colors {
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
        assert!(commands.iter().any(|command| matches!(command, DrawCommand::Window { command: WindowDrawCommand::DrawLine { row: 0, line }, .. } if line.text == "ab  ")));
        assert!(commands.iter().any(|command| matches!(command, DrawCommand::Window { command: WindowDrawCommand::DrawLine { row: 1, line }, .. } if line.text == "cd  ")));
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

        let mut dirty = crate::terminal::state::DirtyRows::new();
        dirty.set(1);
        let commands = bridge.draw_commands(&state, TerminalDamage::Rows(dirty));

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

    #[test]
    fn line_carries_hyperlink_metadata() {
        let bridge = TerminalRenderBridge::default();
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(6, 1);
        parser.advance(&mut state, b"\x1b]8;id=link1;https://example.com\x07hi\x1b]8;;\x07");

        let commands = bridge.full_draw_commands(&state);
        let line = commands
            .iter()
            .find_map(|command| match command {
                DrawCommand::Window {
                    command: WindowDrawCommand::DrawLine { row: 0, line },
                    ..
                } => Some(line),
                _ => None,
            })
            .unwrap();

        assert_eq!(line.hyperlink_at_cell(0).unwrap().uri, "https://example.com");
        assert_eq!(line.hyperlink_at_cell(1).unwrap().id.as_deref(), Some("link1"));
        assert!(line.hyperlink_at_cell(2).is_none());
    }
}
