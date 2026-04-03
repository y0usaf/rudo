use std::{collections::VecDeque, sync::Arc};

use skia_safe::Color4f;
use unicode_width::UnicodeWidthChar;

use crate::editor::{Colors, CursorShape, Style, UnderlineStyle};
use crate::terminal::{
    cell::{CellWidth, TerminalCell},
    cursor::TerminalCursor,
    input::{TerminalInputSettings, TerminalMouseMode},
    screen::TerminalScreen,
};

const DEFAULT_SCROLLBACK_LIMIT: usize = 10_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalDamage {
    None,
    Full,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalSnapshot {
    pub cols: usize,
    pub rows: usize,
    pub cursor: TerminalCursor,
    pub title: Option<String>,
    pub using_alternate_screen: bool,
    pub scrollback_len: usize,
    pub input: TerminalInputSettings,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalPen {
    pub colors: Colors,
    pub reverse: bool,
    pub italic: bool,
    pub bold: bool,
    pub strikethrough: bool,
    pub underline: Option<UnderlineStyle>,
}

impl Default for TerminalPen {
    fn default() -> Self {
        Self {
            colors: Colors { foreground: None, background: None, special: None },
            reverse: false,
            italic: false,
            bold: false,
            strikethrough: false,
            underline: None,
        }
    }
}

impl TerminalPen {
    pub fn to_style(&self) -> Option<Arc<Style>> {
        if self == &Self::default() {
            None
        } else {
            Some(Arc::new(Style {
                colors: self.colors.clone(),
                reverse: self.reverse,
                italic: self.italic,
                bold: self.bold,
                strikethrough: self.strikethrough,
                blend: 0,
                underline: self.underline,
            }))
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

pub struct TerminalState {
    primary_screen: TerminalScreen,
    alternate_screen: TerminalScreen,
    cursor: TerminalCursor,
    saved_cursor: Option<TerminalCursor>,
    saved_cursor_position: Option<TerminalCursor>,
    pen: TerminalPen,
    title: Option<String>,
    using_alternate_screen: bool,
    scrollback: VecDeque<Vec<TerminalCell>>,
    scrollback_limit: usize,
    damage: TerminalDamage,
    input: TerminalInputSettings,
    pending_responses: Vec<Vec<u8>>,
    scroll_region_top: usize,
    scroll_region_bottom: usize,
}

impl TerminalState {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self {
            primary_screen: TerminalScreen::new(cols, rows),
            alternate_screen: TerminalScreen::new(cols, rows),
            cursor: TerminalCursor::default(),
            saved_cursor: None,
            saved_cursor_position: None,
            pen: TerminalPen::default(),
            title: None,
            using_alternate_screen: false,
            scrollback: VecDeque::with_capacity(DEFAULT_SCROLLBACK_LIMIT),
            scrollback_limit: DEFAULT_SCROLLBACK_LIMIT,
            damage: TerminalDamage::Full,
            input: TerminalInputSettings::default(),
            pending_responses: Vec::new(),
            scroll_region_top: 0,
            scroll_region_bottom: rows.max(1),
        }
    }

    pub fn cols(&self) -> usize {
        self.screen().cols()
    }

    pub fn rows(&self) -> usize {
        self.screen().rows()
    }

    pub fn screen(&self) -> &TerminalScreen {
        if self.using_alternate_screen { &self.alternate_screen } else { &self.primary_screen }
    }

    pub fn screen_mut(&mut self) -> &mut TerminalScreen {
        if self.using_alternate_screen {
            &mut self.alternate_screen
        } else {
            &mut self.primary_screen
        }
    }

    pub fn cursor(&self) -> &TerminalCursor {
        &self.cursor
    }

    pub fn pen(&self) -> &TerminalPen {
        &self.pen
    }

    pub fn snapshot(&self) -> TerminalSnapshot {
        TerminalSnapshot {
            cols: self.cols(),
            rows: self.rows(),
            cursor: self.cursor.clone(),
            title: self.title.clone(),
            using_alternate_screen: self.using_alternate_screen,
            scrollback_len: self.scrollback.len(),
            input: self.input,
        }
    }

    pub fn input_settings(&self) -> TerminalInputSettings {
        self.input
    }

    pub fn take_pending_responses(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.pending_responses)
    }

    pub fn take_damage(&mut self) -> TerminalDamage {
        std::mem::replace(&mut self.damage, TerminalDamage::None)
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.primary_screen.resize(cols, rows);
        self.alternate_screen.resize(cols, rows);
        self.cursor.column = self.cursor.column.min(cols.saturating_sub(1));
        self.cursor.row = self.cursor.row.min(rows.saturating_sub(1));
        self.scroll_region_top = 0;
        self.scroll_region_bottom = rows.max(1);
        self.damage = TerminalDamage::Full;
    }

    pub fn print_char(&mut self, ch: char) {
        let width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width == 0 {
            self.apply_combining_mark(ch);
            return;
        }

        self.wrap_if_needed(width);

        let style = self.pen.to_style();
        let col = self.cursor.column;
        let row = self.cursor.row;
        let cell_width = if width > 1 { CellWidth::Double } else { CellWidth::Single };
        self.screen_mut().set(
            col,
            row,
            TerminalCell::occupied(ch.to_string(), style.clone(), cell_width),
        );
        if width > 1 && col + 1 < self.cols() {
            self.screen_mut().set(col + 1, row, TerminalCell::continuation(style));
        }

        self.cursor.column = (self.cursor.column + width).min(self.cols().saturating_sub(1));
        self.damage = TerminalDamage::Full;
    }

    pub fn carriage_return(&mut self) {
        self.cursor.column = 0;
    }

    pub fn backspace(&mut self) {
        self.cursor.column = self.cursor.column.saturating_sub(1);
    }

    pub fn save_cursor_position(&mut self) {
        self.saved_cursor_position = Some(self.cursor.clone());
    }

    pub fn restore_cursor_position(&mut self) {
        if let Some(saved) = self.saved_cursor_position.clone() {
            self.cursor = saved;
            self.damage = TerminalDamage::Full;
        }
    }

    pub fn next_line(&mut self, count: usize) {
        let count = count.max(1);
        self.cursor.row = (self.cursor.row + count).min(self.rows().saturating_sub(1));
        self.cursor.column = 0;
        self.damage = TerminalDamage::Full;
    }

    pub fn previous_line(&mut self, count: usize) {
        let count = count.max(1);
        self.cursor.row = self.cursor.row.saturating_sub(count);
        self.cursor.column = 0;
        self.damage = TerminalDamage::Full;
    }

    pub fn linefeed(&mut self) {
        let scroll_region_top = self.scroll_region_top;
        let scroll_region_bottom = self.scroll_region_bottom;
        let bottom_margin = scroll_region_bottom.saturating_sub(1);
        if self.cursor.row == bottom_margin {
            let removed =
                self.screen_mut().scroll_up_in_region(scroll_region_top, scroll_region_bottom, 1);
            if !self.using_alternate_screen
                && self.scroll_region_top == 0
                && self.scroll_region_bottom == self.rows()
            {
                for row in removed {
                    self.scrollback.push_back(row);
                }
                while self.scrollback.len() > self.scrollback_limit {
                    self.scrollback.pop_front();
                }
            }
        } else if self.cursor.row + 1 >= self.rows() {
            let removed = self.screen_mut().scroll_up(1);
            if !self.using_alternate_screen {
                for row in removed {
                    self.scrollback.push_back(row);
                }
                while self.scrollback.len() > self.scrollback_limit {
                    self.scrollback.pop_front();
                }
            }
        } else {
            self.cursor.row += 1;
        }
        self.damage = TerminalDamage::Full;
    }

    pub fn reverse_index(&mut self) {
        let scroll_region_top = self.scroll_region_top;
        let scroll_region_bottom = self.scroll_region_bottom;
        if self.cursor.row == scroll_region_top {
            self.screen_mut().scroll_down_in_region(scroll_region_top, scroll_region_bottom, 1);
        } else {
            self.cursor.row = self.cursor.row.saturating_sub(1);
        }
        self.damage = TerminalDamage::Full;
    }

    pub fn tab(&mut self) {
        let next_tab_stop = ((self.cursor.column / 8) + 1) * 8;
        self.cursor.column = next_tab_stop.min(self.cols().saturating_sub(1));
    }

    pub fn set_cursor_position(&mut self, row: usize, col: usize) {
        self.cursor.row = row.min(self.rows().saturating_sub(1));
        self.cursor.column = col.min(self.cols().saturating_sub(1));
        self.damage = TerminalDamage::Full;
    }

    pub fn set_cursor_column(&mut self, col: usize) {
        self.cursor.column = col.min(self.cols().saturating_sub(1));
        self.damage = TerminalDamage::Full;
    }

    pub fn set_cursor_row(&mut self, row: usize) {
        self.cursor.row = row.min(self.rows().saturating_sub(1));
        self.damage = TerminalDamage::Full;
    }

    pub fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        let rows = self.rows();
        if top < bottom && bottom <= rows {
            self.scroll_region_top = top;
            self.scroll_region_bottom = bottom;
        } else {
            self.scroll_region_top = 0;
            self.scroll_region_bottom = rows;
        }
        self.set_cursor_position(0, 0);
    }

    pub fn move_cursor(&mut self, rows: isize, cols: isize) {
        let new_row =
            self.cursor.row.saturating_add_signed(rows).min(self.rows().saturating_sub(1));
        let new_col =
            self.cursor.column.saturating_add_signed(cols).min(self.cols().saturating_sub(1));
        self.cursor.row = new_row;
        self.cursor.column = new_col;
        self.damage = TerminalDamage::Full;
    }

    pub fn clear_screen(&mut self) {
        self.screen_mut().clear();
        self.damage = TerminalDamage::Full;
    }

    pub fn clear_from_cursor(&mut self) {
        let row = self.cursor.row;
        let col = self.cursor.column;
        self.screen_mut().clear_from_cursor(col, row);
        self.damage = TerminalDamage::Full;
    }

    pub fn clear_to_cursor(&mut self) {
        let row = self.cursor.row;
        let col = self.cursor.column;
        self.screen_mut().clear_to_cursor(col, row);
        self.damage = TerminalDamage::Full;
    }

    pub fn clear_line(&mut self) {
        let row = self.cursor.row;
        self.screen_mut().clear_line(row);
        self.damage = TerminalDamage::Full;
    }

    pub fn clear_line_from_cursor(&mut self) {
        let row = self.cursor.row;
        let col = self.cursor.column;
        self.screen_mut().clear_line_from(row, col);
        self.damage = TerminalDamage::Full;
    }

    pub fn clear_line_to_cursor(&mut self) {
        let row = self.cursor.row;
        let col = self.cursor.column;
        self.screen_mut().clear_line_to(row, col);
        self.damage = TerminalDamage::Full;
    }

    pub fn erase_chars(&mut self, count: usize) {
        let row = self.cursor.row;
        let col = self.cursor.column;
        self.screen_mut().erase_chars(row, col, count.max(1));
        self.damage = TerminalDamage::Full;
    }

    pub fn delete_chars(&mut self, count: usize) {
        let row = self.cursor.row;
        let col = self.cursor.column;
        self.screen_mut().delete_chars(row, col, count.max(1));
        self.damage = TerminalDamage::Full;
    }

    pub fn insert_blank_chars(&mut self, count: usize) {
        let row = self.cursor.row;
        let col = self.cursor.column;
        self.screen_mut().insert_blank_chars(row, col, count.max(1));
        self.damage = TerminalDamage::Full;
    }

    pub fn insert_lines(&mut self, count: usize) {
        let row = self.cursor.row;
        let scroll_region_bottom = self.scroll_region_bottom;
        if row >= self.scroll_region_top && row < scroll_region_bottom {
            self.screen_mut().insert_lines_in_region(row, scroll_region_bottom, count.max(1));
        } else {
            self.screen_mut().insert_lines(row, count.max(1));
        }
        self.damage = TerminalDamage::Full;
    }

    pub fn delete_lines(&mut self, count: usize) {
        let row = self.cursor.row;
        let scroll_region_bottom = self.scroll_region_bottom;
        if row >= self.scroll_region_top && row < scroll_region_bottom {
            self.screen_mut().delete_lines_in_region(row, scroll_region_bottom, count.max(1));
        } else {
            self.screen_mut().delete_lines(row, count.max(1));
        }
        self.damage = TerminalDamage::Full;
    }

    pub fn scroll_up_lines(&mut self, count: usize) {
        let scroll_region_top = self.scroll_region_top;
        let scroll_region_bottom = self.scroll_region_bottom;
        let removed = self.screen_mut().scroll_up_in_region(
            scroll_region_top,
            scroll_region_bottom,
            count.max(1),
        );
        if !self.using_alternate_screen
            && self.scroll_region_top == 0
            && self.scroll_region_bottom == self.rows()
        {
            for row in removed {
                self.scrollback.push_back(row);
            }
            while self.scrollback.len() > self.scrollback_limit {
                self.scrollback.pop_front();
            }
        }
        self.damage = TerminalDamage::Full;
    }

    pub fn scroll_down_lines(&mut self, count: usize) {
        let scroll_region_top = self.scroll_region_top;
        let scroll_region_bottom = self.scroll_region_bottom;
        self.screen_mut().scroll_down_in_region(
            scroll_region_top,
            scroll_region_bottom,
            count.max(1),
        );
        self.damage = TerminalDamage::Full;
    }

    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = Some(title.into());
        self.damage = TerminalDamage::Full;
    }

    pub fn set_cursor_visible(&mut self, visible: bool) {
        self.cursor.visible = visible;
        self.damage = TerminalDamage::Full;
    }

    pub fn set_cursor_shape(&mut self, shape: CursorShape) {
        self.cursor.shape = shape;
        self.damage = TerminalDamage::Full;
    }

    pub fn enter_alternate_screen(&mut self) {
        if self.using_alternate_screen {
            return;
        }

        self.saved_cursor = Some(self.cursor.clone());
        self.using_alternate_screen = true;
        self.alternate_screen.clear();
        self.cursor = TerminalCursor::default();
        self.damage = TerminalDamage::Full;
    }

    pub fn exit_alternate_screen(&mut self) {
        if !self.using_alternate_screen {
            return;
        }

        self.using_alternate_screen = false;
        if let Some(saved) = self.saved_cursor.take() {
            self.cursor = saved;
        }
        self.damage = TerminalDamage::Full;
    }

    pub fn set_sgr(&mut self, params: &[i64]) {
        if params.is_empty() {
            self.pen.reset();
            return;
        }

        let mut index = 0;
        while index < params.len() {
            match params[index] {
                0 => self.pen.reset(),
                1 => self.pen.bold = true,
                3 => self.pen.italic = true,
                4 => self.pen.underline = Some(UnderlineStyle::Underline),
                7 => self.pen.reverse = true,
                9 => self.pen.strikethrough = true,
                22 => self.pen.bold = false,
                23 => self.pen.italic = false,
                24 => self.pen.underline = None,
                27 => self.pen.reverse = false,
                29 => self.pen.strikethrough = false,
                30..=37 => {
                    self.pen.colors.foreground = Some(ansi_color((params[index] - 30) as u8, false))
                }
                39 => self.pen.colors.foreground = None,
                40..=47 => {
                    self.pen.colors.background = Some(ansi_color((params[index] - 40) as u8, false))
                }
                49 => self.pen.colors.background = None,
                90..=97 => {
                    self.pen.colors.foreground = Some(ansi_color((params[index] - 90) as u8, true))
                }
                100..=107 => {
                    self.pen.colors.background = Some(ansi_color((params[index] - 100) as u8, true))
                }
                38 | 48 => {
                    if let Some((color, consumed)) = parse_extended_color(&params[index + 1..]) {
                        if params[index] == 38 {
                            self.pen.colors.foreground = Some(color);
                        } else {
                            self.pen.colors.background = Some(color);
                        }
                        index += consumed;
                    }
                }
                _ => {}
            }

            index += 1;
        }
    }

    pub fn report_device_status(&mut self, status: i64) {
        match status {
            5 => self.pending_responses.push(b"\x1b[0n".to_vec()),
            6 => self.pending_responses.push(
                format!("\x1b[{};{}R", self.cursor.row + 1, self.cursor.column + 1).into_bytes(),
            ),
            _ => {}
        }
    }

    pub fn report_primary_device_attributes(&mut self) {
        self.pending_responses.push(b"\x1b[?62;c".to_vec());
    }

    pub fn report_secondary_device_attributes(&mut self) {
        self.pending_responses.push(b"\x1b[>1;10;0c".to_vec());
    }

    pub fn use_private_mode(&mut self, mode: i64, enabled: bool) {
        match mode {
            25 => self.set_cursor_visible(enabled),
            1000 => {
                if enabled {
                    self.input.mouse_mode = TerminalMouseMode::Click;
                } else if self.input.mouse_mode == TerminalMouseMode::Click {
                    self.input.mouse_mode = TerminalMouseMode::Disabled;
                }
            }
            1002 => {
                if enabled {
                    self.input.mouse_mode = TerminalMouseMode::Drag;
                } else if self.input.mouse_mode == TerminalMouseMode::Drag {
                    self.input.mouse_mode = TerminalMouseMode::Disabled;
                }
            }
            1003 => {
                if enabled {
                    self.input.mouse_mode = TerminalMouseMode::Motion;
                } else if self.input.mouse_mode == TerminalMouseMode::Motion {
                    self.input.mouse_mode = TerminalMouseMode::Disabled;
                }
            }
            1004 => self.input.focus_reporting = enabled,
            1006 => self.input.sgr_mouse = enabled,
            2004 => self.input.bracketed_paste = enabled,
            1049 => {
                if enabled {
                    self.enter_alternate_screen();
                } else {
                    self.exit_alternate_screen();
                }
            }
            _ => {}
        }
    }

    fn wrap_if_needed(&mut self, width: usize) {
        if self.cursor.column + width > self.cols() {
            self.carriage_return();
            self.linefeed();
        }
    }

    fn apply_combining_mark(&mut self, ch: char) {
        let target_col = self.cursor.column.saturating_sub(1);
        let row = self.cursor.row;
        if let Some(cell) = self.screen_mut().get_mut(target_col, row) {
            cell.text.push(ch);
        }
        self.damage = TerminalDamage::Full;
    }
}

fn parse_extended_color(params: &[i64]) -> Option<(Color4f, usize)> {
    match params {
        [5, index, ..] => Some((palette_color(*index as u8), 2)),
        [2, r, g, b, ..] => Some((rgb(*r as u8, *g as u8, *b as u8), 4)),
        _ => None,
    }
}

fn ansi_color(index: u8, bright: bool) -> Color4f {
    let palette = if bright {
        [
            rgb(128, 128, 128),
            rgb(255, 85, 85),
            rgb(80, 250, 123),
            rgb(241, 250, 140),
            rgb(189, 147, 249),
            rgb(255, 121, 198),
            rgb(139, 233, 253),
            rgb(255, 255, 255),
        ]
    } else {
        [
            rgb(0, 0, 0),
            rgb(205, 49, 49),
            rgb(13, 188, 121),
            rgb(229, 229, 16),
            rgb(36, 114, 200),
            rgb(188, 63, 188),
            rgb(17, 168, 205),
            rgb(229, 229, 229),
        ]
    };

    palette[index as usize % palette.len()]
}

fn palette_color(index: u8) -> Color4f {
    match index {
        0..=7 => ansi_color(index, false),
        8..=15 => ansi_color(index - 8, true),
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

fn rgb(r: u8, g: u8, b: u8) -> Color4f {
    Color4f::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::TerminalState;

    #[test]
    fn linefeed_pushes_primary_screen_into_scrollback() {
        let mut state = TerminalState::new(4, 2);
        state.print_char('a');
        state.carriage_return();
        state.linefeed();
        state.print_char('b');
        state.carriage_return();
        state.linefeed();

        let snapshot = state.snapshot();

        assert_eq!(snapshot.scrollback_len, 1);
        assert_eq!(state.screen().row_text(0).trim(), "b");
    }

    #[test]
    fn alternate_screen_does_not_consume_primary_scrollback() {
        let mut state = TerminalState::new(3, 2);
        state.enter_alternate_screen();
        state.print_char('x');
        state.linefeed();
        state.linefeed();

        let snapshot = state.snapshot();

        assert!(snapshot.using_alternate_screen);
        assert_eq!(snapshot.scrollback_len, 0);
    }

    #[test]
    fn sgr_rgb_sets_foreground() {
        let mut state = TerminalState::new(2, 2);
        state.set_sgr(&[38, 2, 10, 20, 30]);

        let color = state.pen().colors.foreground.unwrap();
        assert!(color.r > 0.03 && color.r < 0.05);
        assert!(color.g > 0.07 && color.g < 0.09);
        assert!(color.b > 0.11 && color.b < 0.13);
    }
}
