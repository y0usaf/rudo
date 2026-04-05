use vte::{Params, Parser, Perform};

use crate::terminal::{
    ClipboardSelection, Hyperlink,
    state::{CharsetSlot, DecCharset, TerminalState},
    theme::parse_color,
};
use crate::ui::CursorShape;

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
        let mut performer = TerminalPerformer { state, dcs: None };
        self.parser.advance(&mut performer, bytes);
    }
}

struct TerminalPerformer<'a> {
    state: &'a mut TerminalState,
    dcs: Option<PendingDcs>,
}

struct PendingDcs {
    params: Vec<i64>,
    intermediates: Vec<u8>,
    action: char,
    data: Vec<u8>,
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
            0x0e => self.state.shift_out(),
            0x0f => self.state.shift_in(),
            _ => {}
        }
    }

    fn hook(&mut self, params: &Params, intermediates: &[u8], ignore: bool, action: char) {
        if ignore {
            return;
        }

        self.dcs = Some(PendingDcs {
            params: params_iter(params).collect(),
            intermediates: intermediates.to_vec(),
            action,
            data: Vec::new(),
        });
    }

    fn put(&mut self, byte: u8) {
        if let Some(dcs) = &mut self.dcs {
            dcs.data.push(byte);
        }
    }

    fn unhook(&mut self) {
        let Some(dcs) = self.dcs.take() else {
            return;
        };

        if dcs.intermediates == b"=" && dcs.action == 's' {
            match dcs.params.first().copied() {
                Some(1) => self.state.set_synchronized_updates(true),
                Some(2) => self.state.set_synchronized_updates(false),
                _ => {}
            }
            return;
        }

        if dcs.intermediates == b"$" && dcs.action == 'q' {
            match dcs.data.as_slice() {
                b"m" => self.state.report_selection_or_setting(b"1$r", &sgr_report(self.state)),
                b" q" => {
                    let cursor_style = match self.state.cursor().shape {
                        CursorShape::Block => 2,
                        CursorShape::Horizontal => 4,
                        CursorShape::Vertical => 6,
                    };
                    self.state.report_selection_or_setting(b"1$r", &format!("{cursor_style} q"));
                }
                b"r" => {
                    let (top, bottom) = self.state.scroll_region();
                    self.state
                        .report_selection_or_setting(b"1$r", &format!("{};{}r", top + 1, bottom));
                }
                _ => self.state.report_selection_or_setting(b"0$r", ""),
            }
        }
    }

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
            b"8" => apply_hyperlink_osc(self.state, &params[1..]),
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
            b"52" => apply_clipboard_osc(self.state, &params[1..]),
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

        let param_values = params_iter(params).collect::<Vec<_>>();

        let private = intermediates.first() == Some(&b'?');
        let secondary = intermediates.first() == Some(&b'>');

        match action {
            'A' => self.state.move_cursor(-(param_nonzero_or(&param_values, 0, 1) as isize), 0),
            'B' => self.state.move_cursor(param_nonzero_or(&param_values, 0, 1) as isize, 0),
            'C' => self.state.move_cursor(0, param_nonzero_or(&param_values, 0, 1) as isize),
            'D' => self.state.move_cursor(0, -(param_nonzero_or(&param_values, 0, 1) as isize)),
            'E' => self.state.next_line(param_nonzero_or(&param_values, 0, 1) as usize),
            'F' => self.state.previous_line(param_nonzero_or(&param_values, 0, 1) as usize),
            'G' => {
                self.state
                    .set_cursor_column(param_nonzero_or(&param_values, 0, 1).saturating_sub(1) as usize)
            }
            'H' | 'f' => {
                let row = param_nonzero_or(&param_values, 0, 1).saturating_sub(1) as usize;
                let col = param_nonzero_or(&param_values, 1, 1).saturating_sub(1) as usize;
                self.state.set_cursor_position(row, col);
            }
            'd' => self
                .state
                .set_cursor_row(param_nonzero_or(&param_values, 0, 1).saturating_sub(1) as usize),
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
            '@' => self.state.insert_blank_chars(param_nonzero_or(&param_values, 0, 1) as usize),
            'P' => self.state.delete_chars(param_nonzero_or(&param_values, 0, 1) as usize),
            'X' => self.state.erase_chars(param_nonzero_or(&param_values, 0, 1) as usize),
            'L' => self.state.insert_lines(param_nonzero_or(&param_values, 0, 1) as usize),
            'M' => self.state.delete_lines(param_nonzero_or(&param_values, 0, 1) as usize),
            'S' => self.state.scroll_up_lines(param_nonzero_or(&param_values, 0, 1) as usize),
            'T' => self.state.scroll_down_lines(param_nonzero_or(&param_values, 0, 1) as usize),
            'm' => self.state.set_sgr_iter(params_iter(params)),
            'r' => {
                let top = param_nonzero_or(&param_values, 0, 1).saturating_sub(1) as usize;
                let bottom = match param_values.get(1).copied().unwrap_or(self.state.rows() as i64) {
                    0 => self.state.rows() as i64,
                    value => value,
                } as usize;
                self.state.set_scroll_region(top, bottom);
            }
            'n' if !private => self.state.report_device_status(first_param_or(params, 0)),
            'p' if intermediates == b"?$" => {
                for mode in params_iter(params) {
                    self.state.report_private_mode(mode);
                }
            }
            'p' if intermediates == b"!" => {
                // DECSTR – Soft Terminal Reset (CSI ! p)
                // neovim sends this during startup to normalise terminal state.
                self.state.soft_reset();
            }
            'u' if private => self.state.report_kitty_keyboard_flags(),
            'u' if secondary => self.state.push_kitty_keyboard_flags(first_param_or(params, 0)),
            'u' if intermediates.first() == Some(&b'<') => {
                self.state.pop_kitty_keyboard_flags(first_param_or(params, 1));
            }
            'u' if intermediates.first() == Some(&b'=') => {
                let mut iter = params_iter(params);
                let flags = iter.next().unwrap_or(0);
                let mode = iter.next().unwrap_or(1);
                self.state.set_kitty_keyboard_flags(flags, mode);
            }
            'h' if private => {
                for mode in &param_values {
                    self.state.use_private_mode(*mode, true);
                }
            }
            'l' if private => {
                for mode in &param_values {
                    self.state.use_private_mode(*mode, false);
                }
            }
            'h' => {
                for mode in &param_values {
                    if *mode == 4 {
                        self.state.set_insert_mode(true);
                    }
                }
            }
            'l' => {
                for mode in &param_values {
                    if *mode == 4 {
                        self.state.set_insert_mode(false);
                    }
                }
            }
            'c' if secondary => self.state.report_secondary_device_attributes(),
            'c' => self.state.report_primary_device_attributes(),
            'q' => {
                if private {
                    // DECSCUSR via private mode prefix (non-standard but tolerate it)
                    match first_param_or(params, 0) {
                        1 | 2 => self.state.set_cursor_shape(CursorShape::Block),
                        3 | 4 => self.state.set_cursor_shape(CursorShape::Horizontal),
                        5 | 6 => self.state.set_cursor_shape(CursorShape::Vertical),
                        _ => {}
                    }
                } else if intermediates == b" " {
                    // DECSCUSR – CSI Ps SP q (space as intermediate)
                    // This is the standard sequence; neovim, vim, and most TUI apps use it.
                    match first_param_or(params, 0) {
                        0 | 1 | 2 => self.state.set_cursor_shape(CursorShape::Block),
                        3 | 4 => self.state.set_cursor_shape(CursorShape::Horizontal),
                        5 | 6 => self.state.set_cursor_shape(CursorShape::Vertical),
                        _ => {}
                    }
                }
            }
            's' if !private && !secondary => {
                if self.state.left_right_margin_mode_enabled() && !param_values.is_empty() {
                    let left = param_nonzero_or(&param_values, 0, 1).saturating_sub(1) as usize;
                    let right = param_nonzero_or(&param_values, 1, self.state.cols() as i64) as usize;
                    self.state.set_left_right_margins(left, right);
                } else {
                    self.state.save_cursor_position();
                }
            }
            'u' if !private && !secondary => self.state.restore_cursor_position(),
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        match (intermediates, byte) {
            (b"(", b'0') => {
                self.state.designate_charset(CharsetSlot::G0, DecCharset::DecSpecialGraphics)
            }
            (b"(", b'B') => self.state.designate_charset(CharsetSlot::G0, DecCharset::Ascii),
            (b")", b'0') => {
                self.state.designate_charset(CharsetSlot::G1, DecCharset::DecSpecialGraphics)
            }
            (b")", b'B') => self.state.designate_charset(CharsetSlot::G1, DecCharset::Ascii),
            (_, b'7') => self.state.save_cursor_position(),
            (_, b'8') => self.state.restore_cursor_position(),
            (_, b'D') => self.state.linefeed(),
            (_, b'E') => {
                self.state.linefeed();
                self.state.carriage_return();
            }
            (_, b'M') => self.state.reverse_index(),
            (_, b'N') => {}
            (_, b'O') => {}
            (_, b'=') => self.state.set_application_keypad(true),
            (_, b'>') => self.state.set_application_keypad(false),
            _ => {}
        }
    }
}

fn sgr_report(state: &TerminalState) -> String {
    let pen = state.pen();
    let mut params = Vec::new();

    if pen.bold() {
        params.push(1);
    }
    if pen.italic() {
        params.push(3);
    }
    if pen.underline().is_some() {
        params.push(4);
    }
    if pen.reverse() {
        params.push(7);
    }
    if pen.strikethrough() {
        params.push(9);
    }

    push_color_report(&mut params, 30, 40, pen.colors().foreground.as_ref());
    push_color_report(&mut params, 40, 50, pen.colors().background.as_ref());

    if params.is_empty() {
        "m".to_string()
    } else {
        format!("m{}", params.iter().map(i64::to_string).collect::<Vec<_>>().join(";"))
    }
}

fn push_color_report(
    params: &mut Vec<i64>,
    base: i64,
    default_reset: i64,
    color: Option<&crate::terminal::style::TerminalColor>,
) {
    use crate::terminal::style::TerminalColor;

    match color {
        None => params.push(default_reset - 1),
        Some(TerminalColor::Palette(index @ 0..=7)) => params.push(base + i64::from(*index)),
        Some(TerminalColor::Palette(index @ 8..=15)) => {
            params.push(base + 60 + i64::from(*index - 8))
        }
        Some(TerminalColor::Palette(index)) => params.extend([base + 8, 5, i64::from(*index)]),
        Some(TerminalColor::Rgb(color)) => params.extend([
            base + 8,
            2,
            (color.r * 255.0).round() as i64,
            (color.g * 255.0).round() as i64,
            (color.b * 255.0).round() as i64,
        ]),
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

fn apply_clipboard_osc(state: &mut TerminalState, params: &[&[u8]]) {
    let Some(selection_param) = params.first() else {
        return;
    };
    let Some(data_param) = params.get(1) else {
        return;
    };

    let Ok(selection_spec) = std::str::from_utf8(selection_param) else {
        return;
    };
    let data_spec = parse_param(data_param).unwrap_or_default();

    if data_spec == "?" {
        let selection = parse_clipboard_selections(selection_spec)
            .into_iter()
            .next()
            .unwrap_or(ClipboardSelection::Clipboard);
        state.queue_clipboard_query(selection);
        return;
    }

    let Some(decoded) = decode_base64(data_spec) else {
        return;
    };
    let Ok(content) = String::from_utf8(decoded) else {
        return;
    };

    for selection in parse_clipboard_selections(selection_spec) {
        state.queue_clipboard_set(selection, content.clone());
    }
}

fn parse_clipboard_selections(spec: &str) -> Vec<ClipboardSelection> {
    let mut selections = Vec::new();
    if spec.is_empty() {
        selections.push(ClipboardSelection::Clipboard);
        return selections;
    }

    for ch in spec.chars() {
        let selection = match ch {
            'c' => ClipboardSelection::Clipboard,
            'p' => ClipboardSelection::Primary,
            'q' => ClipboardSelection::Secondary,
            's' => ClipboardSelection::Select,
            '0' => ClipboardSelection::Cut0,
            '1' => ClipboardSelection::Cut1,
            '2' => ClipboardSelection::Cut2,
            '3' => ClipboardSelection::Cut3,
            '4' => ClipboardSelection::Cut4,
            '5' => ClipboardSelection::Cut5,
            '6' => ClipboardSelection::Cut6,
            '7' => ClipboardSelection::Cut7,
            _ => continue,
        };
        if !selections.contains(&selection) {
            selections.push(selection);
        }
    }

    if selections.is_empty() {
        selections.push(ClipboardSelection::Clipboard);
    }
    selections
}

fn apply_hyperlink_osc(state: &mut TerminalState, params: &[&[u8]]) {
    let metadata = params.first().copied().unwrap_or_default();
    let uri = params.get(1).copied().unwrap_or_default();

    if uri.is_empty() {
        state.set_current_hyperlink(None);
        return;
    }

    let Ok(uri) = std::str::from_utf8(uri) else {
        return;
    };

    let id = std::str::from_utf8(metadata).ok().and_then(parse_hyperlink_id);
    state.set_current_hyperlink(Some(Hyperlink { id, uri: uri.to_string() }));
}

fn parse_hyperlink_id(metadata: &str) -> Option<String> {
    metadata.split(':').find_map(|part| part.strip_prefix("id=").map(ToOwned::to_owned))
}

fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0u8;

    for byte in input.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b'\r' | b'\n' | b'\t' | b' ' => continue,
            _ => return None,
        } as u32;

        buffer = (buffer << 6) | value;
        bits += 6;
        while bits >= 8 {
            bits -= 8;
            output.push(((buffer >> bits) & 0xff) as u8);
        }
    }

    Some(output)
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

fn param_nonzero_or(params: &[i64], index: usize, default: i64) -> i64 {
    match params.get(index).copied().unwrap_or(default) {
        0 => default,
        value => value,
    }
}

#[cfg(test)]
mod tests {
    use super::TerminalParser;
    use crate::{
        render::bridge::TerminalRenderBridge,
        renderer::{DrawCommand, WindowDrawCommand},
        terminal::{ClipboardRequestKind, ClipboardSelection, state::TerminalState},
        ui::CursorShape,
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
        assert!(state.pen().colors().foreground.is_some());
        assert_eq!(state.screen().get(0, 0).unwrap().text(), "R");
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
    fn parses_application_cursor_mode() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(8, 2);

        parser.advance(&mut state, b"\x1b[?1h");
        assert!(state.input_settings().application_cursor);

        parser.advance(&mut state, b"\x1b[?1l");
        assert!(!state.input_settings().application_cursor);
    }

    #[test]
    fn parses_cursor_save_restore_private_mode() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(8, 4);
        state.set_cursor_position(2, 3);

        parser.advance(&mut state, b"\x1b[?1048h");
        state.set_cursor_position(0, 0);
        parser.advance(&mut state, b"\x1b[?1048l");

        assert_eq!(state.cursor().row, 2);
        assert_eq!(state.cursor().column, 3);
    }

    #[test]
    fn parses_origin_mode() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(8, 6);

        parser.advance(&mut state, b"\x1b[2;5r\x1b[?6h\x1b[1;1H");

        assert_eq!(state.cursor().row, 1);
        assert_eq!(state.cursor().column, 0);
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
    fn csi_zero_defaults_to_home_instead_of_bottom_row() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(10, 5);
        state.set_cursor_position(4, 9);

        parser.advance(&mut state, b"\x1b[0;0H");
        assert_eq!(state.cursor().row, 0);
        assert_eq!(state.cursor().column, 0);

        state.set_cursor_position(4, 9);
        parser.advance(&mut state, b"\x1b[H");
        assert_eq!(state.cursor().row, 0);
        assert_eq!(state.cursor().column, 0);
    }

    #[test]
    fn parses_standard_decscusr_sequence_with_space_intermediate() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(8, 2);

        parser.advance(&mut state, b"\x1b[6 q");
        assert_eq!(state.cursor().shape, CursorShape::Vertical);

        parser.advance(&mut state, b"\x1b[2 q");
        assert_eq!(state.cursor().shape, CursorShape::Block);
    }

    #[test]
    fn declrm_slrm_sequence_is_not_misparsed_as_save_cursor() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(10, 5);

        state.set_cursor_position(2, 3);
        parser.advance(&mut state, b"\x1b[?69h\x1b[1;4s");

        state.set_cursor_position(4, 5);
        parser.advance(&mut state, b"\x1b[u");

        assert_eq!(state.cursor().row, 4);
        assert_eq!(state.cursor().column, 5);
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

    #[test]
    fn parses_dec_special_graphics_and_shift_states() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(4, 1);

        parser.advance(&mut state, b"\x1b(0qx\x1b(B");
        assert_eq!(state.screen().row_text(0)[..2].to_string(), "─│");

        let mut state = TerminalState::new(4, 1);
        parser.advance(&mut state, b"\x1b)0\x0eqx\x0f");
        assert_eq!(state.screen().row_text(0)[..2].to_string(), "─│");
    }

    #[test]
    fn parses_application_keypad_mode_and_decrqm() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(4, 1);

        parser.advance(&mut state, b"\x1b=\x1b[?66$p\x1b>");
        let responses = state.take_pending_responses();

        assert!(state.input_settings().application_keypad == false);
        assert_eq!(responses[0], b"\x1b[?66;1$y");
    }

    #[test]
    fn parses_private_mode_queries_and_kitty_keyboard_query() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(4, 1);

        parser.advance(
            &mut state,
            b"\x1b[?2026h\x1b[?2026$p\x1b[?2027$p\x1b[?2031$p\x1b[?2048$p\x1b[?u",
        );
        let responses = state.take_pending_responses();

        assert_eq!(responses[0], b"\x1b[?2026;1$y");
        assert_eq!(responses[1], b"\x1b[?2027;0$y");
        assert_eq!(responses[2], b"\x1b[?2031;0$y");
        assert_eq!(responses[3], b"\x1b[?2048;0$y");
        assert_eq!(responses[4], b"\x1b[?0u");
    }

    #[test]
    fn parses_kitty_keyboard_push_set_pop_and_query() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(4, 1);

        parser.advance(&mut state, b"\x1b[>1u\x1b[?u\x1b[=1;3u\x1b[?u\x1b[<1u\x1b[?u");
        let responses = state.take_pending_responses();

        assert_eq!(responses[0], b"\x1b[?1u");
        assert_eq!(responses[1], b"\x1b[?0u");
        assert_eq!(responses[2], b"\x1b[?0u");
    }

    #[test]
    fn parses_kitty_keyboard_masking_and_default_modes() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(4, 1);

        parser.advance(&mut state, b"\x1b[=255;2u\x1b[?u\x1b[>0u\x1b[<u\x1b[?u");
        let responses = state.take_pending_responses();

        assert_eq!(responses[0], b"\x1b[?1u");
        assert_eq!(responses[1], b"\x1b[?1u");
    }

    #[test]
    fn parses_decrqss_sgr_query() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(4, 1);

        parser.advance(&mut state, b"\x1b[1;31m\x1bP$qm\x1b\\");
        let responses = state.take_pending_responses();

        assert_eq!(responses[0], b"\x1bP1$rm1;31\x1b\\");
    }

    #[test]
    fn parses_decrqss_decstbm_query() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(4, 5);

        parser.advance(&mut state, b"\x1b[2;4r\x1bP$qr\x1b\\");
        let responses = state.take_pending_responses();

        assert_eq!(responses[0], b"\x1bP1$r2;4r\x1b\\");
    }

    #[test]
    fn parses_sync_updates_private_mode_and_dcs_compat() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(4, 1);

        parser.advance(&mut state, b"\x1b[?2026h");
        assert!(state.synchronized_updates_active());
        parser.advance(&mut state, b"\x1bP=2s\x1b\\");
        assert!(!state.synchronized_updates_active());
        parser.advance(&mut state, b"\x1bP=1s\x1b\\");
        assert!(state.synchronized_updates_active());
        parser.advance(&mut state, b"\x1b[?2026l");
        assert!(!state.synchronized_updates_active());
    }

    #[test]
    fn parses_osc_52_clipboard_set() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(4, 1);

        parser.advance(&mut state, b"\x1b]52;c;aGVsbG8=\x07");
        let requests = state.take_pending_clipboard_requests();

        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].selection, ClipboardSelection::Clipboard);
        assert_eq!(requests[0].kind, ClipboardRequestKind::Set("hello".into()));
    }

    #[test]
    fn parses_osc_52_clipboard_query() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(4, 1);

        parser.advance(&mut state, b"\x1b]52;c;?\x07");
        let requests = state.take_pending_clipboard_requests();

        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].selection, ClipboardSelection::Clipboard);
        assert_eq!(requests[0].kind, ClipboardRequestKind::Query);
    }

    #[test]
    fn parses_osc_8_hyperlinks_into_cells() {
        let mut parser = TerminalParser::new();
        let mut state = TerminalState::new(8, 1);

        parser.advance(&mut state, b"\x1b]8;id=link1;https://example.com\x07hi\x1b]8;;\x07!");

        let first = state.screen().get(0, 0).unwrap();
        let second = state.screen().get(1, 0).unwrap();
        let third = state.screen().get(2, 0).unwrap();
        assert_eq!(first.hyperlink.as_ref().unwrap().uri, "https://example.com");
        assert_eq!(first.hyperlink.as_ref().unwrap().id.as_deref(), Some("link1"));
        assert!(second.hyperlink.is_some());
        assert!(third.hyperlink.is_none());
    }
}
