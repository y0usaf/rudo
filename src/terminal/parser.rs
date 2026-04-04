use vte::{Params, Parser, Perform};

use crate::editor::CursorShape;
use crate::terminal::{state::TerminalState, theme::parse_color};

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
        let Some(command) = params.first() else {
            return;
        };

        match *command {
            b"0" | b"2" => {
                if let Some(title) = params.get(1).and_then(|value| std::str::from_utf8(value).ok())
                {
                    self.state.set_title(title);
                }
            }
            b"4" => apply_palette_osc(self.state, &params[1..]),
            b"10" => apply_dynamic_color_osc(
                self.state,
                "10",
                &params[1..],
                DynamicColorKind::Foreground,
            ),
            b"11" => apply_dynamic_color_osc(
                self.state,
                "11",
                &params[1..],
                DynamicColorKind::Background,
            ),
            b"12" => {
                apply_dynamic_color_osc(self.state, "12", &params[1..], DynamicColorKind::Cursor)
            }
            b"104" => reset_palette_osc(self.state, &params[1..]),
            b"110" => self.state.reset_default_foreground(),
            b"111" => self.state.reset_default_background(),
            b"112" => self.state.reset_cursor_color(),
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        if ignore {
            return;
        }

        let private = intermediates.first() == Some(&b'?');
        let secondary = intermediates.first() == Some(&b'>');

        match action {
            'A' => self.state.move_cursor(-(first_param_or(params, 1) as isize), 0),
            'B' => self.state.move_cursor(first_param_or(params, 1) as isize, 0),
            'C' => self.state.move_cursor(0, first_param_or(params, 1) as isize),
            'D' => self.state.move_cursor(0, -(first_param_or(params, 1) as isize)),
            'E' => self.state.next_line(first_param_or(params, 1) as usize),
            'F' => self.state.previous_line(first_param_or(params, 1) as usize),
            'G' => {
                self.state.set_cursor_column(first_param_or(params, 1).saturating_sub(1) as usize)
            }
            'H' | 'f' => {
                let mut iter = params_iter(params);
                let row = iter.next().unwrap_or(1).saturating_sub(1) as usize;
                let col = iter.next().unwrap_or(1).saturating_sub(1) as usize;
                self.state.set_cursor_position(row, col);
            }
            'd' => self.state.set_cursor_row(first_param_or(params, 1).saturating_sub(1) as usize),
            'J' => match first_param_or(params, 0) {
                0 => self.state.clear_from_cursor(),
                1 => self.state.clear_to_cursor(),
                2 => self.state.clear_screen(),
                _ => {}
            },
            'K' => match first_param_or(params, 0) {
                0 => self.state.clear_line_from_cursor(),
                1 => self.state.clear_line_to_cursor(),
                2 => self.state.clear_line(),
                _ => {}
            },
            '@' => self.state.insert_blank_chars(first_param_or(params, 1) as usize),
            'P' => self.state.delete_chars(first_param_or(params, 1) as usize),
            'X' => self.state.erase_chars(first_param_or(params, 1) as usize),
            'L' => self.state.insert_lines(first_param_or(params, 1) as usize),
            'M' => self.state.delete_lines(first_param_or(params, 1) as usize),
            'S' => self.state.scroll_up_lines(first_param_or(params, 1) as usize),
            'T' => self.state.scroll_down_lines(first_param_or(params, 1) as usize),
            'm' => self.state.set_sgr_iter(params_iter(params)),
            'r' => {
                let mut iter = params_iter(params);
                let top = iter.next().unwrap_or(1).saturating_sub(1) as usize;
                let bottom = iter.next().unwrap_or(self.state.rows() as i64) as usize;
                self.state.set_scroll_region(top, bottom);
            }
            'n' if !private => self.state.report_device_status(first_param_or(params, 0)),
            'h' if private => {
                for mode in params_iter(params) {
                    self.state.use_private_mode(mode, true);
                }
            }
            'l' if private => {
                for mode in params_iter(params) {
                    self.state.use_private_mode(mode, false);
                }
            }
            'c' if secondary => self.state.report_secondary_device_attributes(),
            'c' => self.state.report_primary_device_attributes(),
            'q' => {
                if private {
                    match first_param_or(params, 0) {
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

#[derive(Clone, Copy)]
enum DynamicColorKind {
    Foreground,
    Background,
    Cursor,
}

fn apply_palette_osc(state: &mut TerminalState, params: &[&[u8]]) {
    let mut pairs = params.chunks_exact(2);
    for pair in &mut pairs {
        let Some(index) = parse_palette_index(pair[0]) else {
            continue;
        };
        let Some(value) = parse_param(pair[1]) else {
            continue;
        };

        if value == "?" {
            state.queue_osc_palette_response(index, state.theme().palette_color(index));
        } else if let Ok(color) = parse_color(value) {
            state.set_palette_color(index, color);
        }
    }
}

fn reset_palette_osc(state: &mut TerminalState, params: &[&[u8]]) {
    if params.is_empty() {
        state.reset_palette();
        return;
    }

    for param in params {
        if let Some(index) = parse_palette_index(param) {
            state.reset_palette_color(index);
        }
    }
}

fn apply_dynamic_color_osc(
    state: &mut TerminalState,
    code: &str,
    params: &[&[u8]],
    kind: DynamicColorKind,
) {
    let Some(value) = params.first().and_then(|value| parse_param(value)) else {
        return;
    };

    if value == "?" {
        let color = match kind {
            DynamicColorKind::Foreground => state.theme().foreground,
            DynamicColorKind::Background => state.theme().background,
            DynamicColorKind::Cursor => state.theme().cursor,
        };
        state.queue_osc_color_response(code, color);
        return;
    }

    let Ok(color) = parse_color(value) else {
        return;
    };

    match kind {
        DynamicColorKind::Foreground => state.set_default_foreground(color),
        DynamicColorKind::Background => state.set_default_background(color),
        DynamicColorKind::Cursor => state.set_cursor_color(color),
    }
}

fn parse_palette_index(value: &[u8]) -> Option<u8> {
    parse_param(value)?.parse().ok()
}

fn parse_param(value: &[u8]) -> Option<&str> {
    std::str::from_utf8(value).ok().map(str::trim)
}

fn params_iter(params: &Params) -> impl Iterator<Item = i64> + '_ {
    params.iter().flat_map(|param| param.iter().map(|value| i64::from(*value)))
}

fn first_param_or(params: &Params, default: i64) -> i64 {
    params_iter(params).next().unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::TerminalParser;
    use crate::{
        render::bridge::TerminalRenderBridge,
        renderer::{DrawCommand, WindowDrawCommand},
        terminal::state::TerminalState,
    };

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

    #[test]
    fn osc_palette_updates_existing_indexed_cells() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(4, 1);
        let bridge = TerminalRenderBridge::default();

        parser.advance(&mut state, b"\x1b[31mR");
        parser.advance(&mut state, b"\x1b]4;1;#00ff00\x07");

        let commands = bridge.full_draw_commands(&state);
        let mut found_green = false;
        for command in commands {
            if let DrawCommand::Window {
                command: WindowDrawCommand::DrawLine { line, .. }, ..
            } = command
            {
                for fragment in line.fragments() {
                    if fragment.text == "R" {
                        let color = fragment
                            .style
                            .as_ref()
                            .and_then(|style| style.colors.foreground)
                            .expect("foreground color");
                        assert!(color.g > 0.9 && color.r < 0.1 && color.b < 0.1);
                        found_green = true;
                    }
                }
            }
        }

        assert!(found_green);
    }

    #[test]
    fn osc_color_query_generates_response() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(4, 1);

        parser.advance(&mut state, b"\x1b]10;?\x07\x1b]4;1;?\x07");
        let responses = state.take_pending_responses();

        assert_eq!(responses.len(), 2);
        assert!(String::from_utf8_lossy(&responses[0]).contains("]10;rgb:"));
        assert!(String::from_utf8_lossy(&responses[1]).contains("]4;1;rgb:"));
    }
}
