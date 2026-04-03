use vte::{Params, Parser, Perform};

use crate::editor::CursorShape;
use crate::terminal::state::TerminalState;

pub struct TerminalParser {
    parser: Parser,
}

impl Default for TerminalParser {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalParser {
    pub fn new() -> Self {
        Self { parser: Parser::new() }
    }

    pub fn advance(&mut self, state: &mut TerminalState, bytes: &[u8]) {
        let mut performer = TerminalPerformer { state };
        self.parser.advance(&mut performer, bytes);
    }
}

struct TerminalPerformer<'a> {
    state: &'a mut TerminalState,
}

impl Perform for TerminalPerformer<'_> {
    fn print(&mut self, c: char) {
        self.state.print_char(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => self.state.linefeed(),
            b'\r' => self.state.carriage_return(),
            0x08 => self.state.backspace(),
            b'\t' => self.state.tab(),
            _ => {}
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}

    fn put(&mut self, _byte: u8) {}

    fn unhook(&mut self) {}

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if params.len() < 2 {
            return;
        }

        match params[0] {
            b"0" | b"2" => {
                if let Ok(title) = std::str::from_utf8(params[1]) {
                    self.state.set_title(title);
                }
            }
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        if ignore {
            return;
        }

        let flat_params = flatten_params(params);
        let private = intermediates.first() == Some(&b'?');
        let secondary = intermediates.first() == Some(&b'>');

        match action {
            'A' => self.state.move_cursor(-(param_or(&flat_params, 0, 1) as isize), 0),
            'B' => self.state.move_cursor(param_or(&flat_params, 0, 1) as isize, 0),
            'C' => self.state.move_cursor(0, param_or(&flat_params, 0, 1) as isize),
            'D' => self.state.move_cursor(0, -(param_or(&flat_params, 0, 1) as isize)),
            'E' => self.state.next_line(param_or(&flat_params, 0, 1) as usize),
            'F' => self.state.previous_line(param_or(&flat_params, 0, 1) as usize),
            'G' => self
                .state
                .set_cursor_column(param_or(&flat_params, 0, 1).saturating_sub(1) as usize),
            'H' | 'f' => {
                let row = param_or(&flat_params, 0, 1).saturating_sub(1) as usize;
                let col = param_or(&flat_params, 1, 1).saturating_sub(1) as usize;
                self.state.set_cursor_position(row, col);
            }
            'd' => {
                self.state.set_cursor_row(param_or(&flat_params, 0, 1).saturating_sub(1) as usize)
            }
            'J' => match param_or(&flat_params, 0, 0) {
                0 => self.state.clear_from_cursor(),
                1 => self.state.clear_to_cursor(),
                2 => self.state.clear_screen(),
                _ => {}
            },
            'K' => match param_or(&flat_params, 0, 0) {
                0 => self.state.clear_line_from_cursor(),
                1 => self.state.clear_line_to_cursor(),
                2 => self.state.clear_line(),
                _ => {}
            },
            '@' => self.state.insert_blank_chars(param_or(&flat_params, 0, 1) as usize),
            'P' => self.state.delete_chars(param_or(&flat_params, 0, 1) as usize),
            'X' => self.state.erase_chars(param_or(&flat_params, 0, 1) as usize),
            'L' => self.state.insert_lines(param_or(&flat_params, 0, 1) as usize),
            'M' => self.state.delete_lines(param_or(&flat_params, 0, 1) as usize),
            'S' => self.state.scroll_up_lines(param_or(&flat_params, 0, 1) as usize),
            'T' => self.state.scroll_down_lines(param_or(&flat_params, 0, 1) as usize),
            'm' => self.state.set_sgr(&flat_params),
            'r' => {
                let top = param_or(&flat_params, 0, 1).saturating_sub(1) as usize;
                let bottom = param_or(&flat_params, 1, self.state.rows() as i64) as usize;
                self.state.set_scroll_region(top, bottom);
            }
            'n' if !private => self.state.report_device_status(param_or(&flat_params, 0, 0)),
            'h' if private => {
                for mode in flat_params {
                    self.state.use_private_mode(mode, true);
                }
            }
            'l' if private => {
                for mode in flat_params {
                    self.state.use_private_mode(mode, false);
                }
            }
            'c' if secondary => self.state.report_secondary_device_attributes(),
            'c' => self.state.report_primary_device_attributes(),
            'q' => {
                if private {
                    match param_or(&flat_params, 0, 0) {
                        1 | 2 => self.state.set_cursor_shape(CursorShape::Block),
                        3 | 4 => self.state.set_cursor_shape(CursorShape::Horizontal),
                        5 | 6 => self.state.set_cursor_shape(CursorShape::Vertical),
                        _ => {}
                    }
                }
            }
            's' if !private && !secondary => self.state.save_cursor_position(),
            'u' if !private && !secondary => self.state.restore_cursor_position(),
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        match byte {
            b'7' => self.state.save_cursor_position(),
            b'8' => self.state.restore_cursor_position(),
            b'D' => self.state.linefeed(),
            b'E' => {
                self.state.linefeed();
                self.state.carriage_return();
            }
            b'M' => self.state.reverse_index(),
            _ => {}
        }
    }
}

fn flatten_params(params: &Params) -> Vec<i64> {
    let mut result = Vec::new();
    for param in params.iter() {
        for value in param {
            result.push(i64::from(*value));
        }
    }
    result
}

fn param_or(params: &[i64], index: usize, default: i64) -> i64 {
    params.get(index).copied().unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::TerminalParser;
    use crate::terminal::state::TerminalState;

    #[test]
    fn parses_plain_text_and_newlines() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(4, 2);

        parser.advance(&mut state, b"ab\r\ncd");

        assert_eq!(state.screen().row_text(0).trim(), "ab");
        assert_eq!(state.screen().row_text(1).trim(), "cd");
    }

    #[test]
    fn parses_sgr_and_title_sequences() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(8, 2);

        parser.advance(&mut state, b"\x1b]2;Termvide Demo\x07\x1b[31mR");

        assert_eq!(state.snapshot().title.as_deref(), Some("Termvide Demo"));
        assert!(state.pen().colors.foreground.is_some());
        assert_eq!(state.screen().get(0, 0).unwrap().text, "R");
    }

    #[test]
    fn parses_alternate_screen_toggle() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(8, 2);

        parser.advance(&mut state, b"\x1b[?1049h");
        assert!(state.snapshot().using_alternate_screen);

        parser.advance(&mut state, b"\x1b[?1049l");
        assert!(!state.snapshot().using_alternate_screen);
    }

    #[test]
    fn parses_common_cursor_positioning_commands() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(10, 5);

        parser.advance(&mut state, b"\x1b[3;4H\x1b[s\x1b[10G\x1b[2F\x1b[u");

        assert_eq!(state.cursor().row, 2);
        assert_eq!(state.cursor().column, 3);
    }

    #[test]
    fn parses_line_and_char_editing_commands() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(6, 3);

        parser.advance(&mut state, b"abcdef\r\x1b[2P");
        assert_eq!(state.screen().row_text(0), "cdef  ");

        parser.advance(&mut state, b"\r\x1b[2@XY");
        assert_eq!(state.screen().row_text(0), "XYcdef");

        parser.advance(&mut state, b"\r\x1b[3X");
        assert_eq!(state.screen().row_text(0), "   def");
    }

    #[test]
    fn parses_scroll_regions_and_region_scrolling() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(4, 4);

        parser.advance(&mut state, b"1111\r\n2222\r\n3333\r\n4444");
        parser.advance(&mut state, b"\x1b[2;4r\x1b[4;1H\n");

        assert_eq!(state.screen().row_text(0), "1111");
        assert_eq!(state.screen().row_text(1), "3333");
        assert_eq!(state.screen().row_text(2), "4444");
        assert_eq!(state.screen().row_text(3), "    ");
    }
}
