use winit::event::MouseButton;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TerminalMouseMode {
    #[default]
    Disabled,
    Click,
    Drag,
    Motion,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalInputSettings {
    pub mouse_mode: TerminalMouseMode,
    pub sgr_mouse: bool,
    pub bracketed_paste: bool,
    pub focus_reporting: bool,
}

pub fn encode_bracketed_paste(text: &str, enabled: bool) -> Vec<u8> {
    if enabled {
        let mut output = b"\x1b[200~".to_vec();
        output.extend_from_slice(text.as_bytes());
        output.extend_from_slice(b"\x1b[201~");
        output
    } else {
        text.as_bytes().to_vec()
    }
}

pub fn encode_focus_report(focused: bool) -> &'static [u8] {
    if focused { b"\x1b[I" } else { b"\x1b[O" }
}

pub fn encode_mouse_press(
    settings: TerminalInputSettings,
    button: MouseButton,
    col: u32,
    row: u32,
) -> Option<Vec<u8>> {
    if settings.mouse_mode == TerminalMouseMode::Disabled {
        return None;
    }
    Some(encode_mouse_event(settings, mouse_button_code(button)?, col, row, true))
}

pub fn encode_mouse_release(
    settings: TerminalInputSettings,
    button: MouseButton,
    col: u32,
    row: u32,
) -> Option<Vec<u8>> {
    if settings.mouse_mode == TerminalMouseMode::Disabled {
        return None;
    }
    Some(encode_mouse_event(settings, mouse_button_code(button)?, col, row, false))
}

pub fn encode_mouse_drag(
    settings: TerminalInputSettings,
    button: MouseButton,
    col: u32,
    row: u32,
) -> Option<Vec<u8>> {
    if !matches!(settings.mouse_mode, TerminalMouseMode::Drag | TerminalMouseMode::Motion) {
        return None;
    }
    Some(encode_mouse_event(settings, mouse_button_code(button)? + 32, col, row, true))
}

pub fn encode_mouse_move(settings: TerminalInputSettings, col: u32, row: u32) -> Option<Vec<u8>> {
    (settings.mouse_mode == TerminalMouseMode::Motion)
        .then(|| encode_mouse_event(settings, 35, col, row, true))
}

pub fn encode_mouse_scroll(
    settings: TerminalInputSettings,
    up: bool,
    col: u32,
    row: u32,
) -> Option<Vec<u8>> {
    (settings.mouse_mode != TerminalMouseMode::Disabled)
        .then(|| encode_mouse_event(settings, if up { 64 } else { 65 }, col, row, true))
}

fn mouse_button_code(button: MouseButton) -> Option<u8> {
    match button {
        MouseButton::Left => Some(0),
        MouseButton::Middle => Some(1),
        MouseButton::Right => Some(2),
        _ => None,
    }
}

fn encode_mouse_event(
    settings: TerminalInputSettings,
    code: u8,
    col: u32,
    row: u32,
    press: bool,
) -> Vec<u8> {
    let x = col.saturating_add(1);
    let y = row.saturating_add(1);
    if settings.sgr_mouse {
        let suffix = if press { 'M' } else { 'm' };
        format!("\x1b[<{code};{x};{y}{suffix}").into_bytes()
    } else {
        vec![
            0x1b,
            b'[',
            b'M',
            code + 32,
            (x as u8).saturating_add(32),
            (y as u8).saturating_add(32),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::{
        TerminalInputSettings, TerminalMouseMode, encode_bracketed_paste, encode_focus_report,
        encode_mouse_press,
    };
    use winit::event::MouseButton;

    #[test]
    fn plain_paste_omits_markers_when_disabled() {
        assert_eq!(encode_bracketed_paste("hello", false), b"hello");
    }

    #[test]
    fn bracketed_paste_wraps_payload() {
        assert_eq!(encode_bracketed_paste("hi", true), b"\x1b[200~hi\x1b[201~");
    }

    #[test]
    fn focus_report_encodes_xterm_sequences() {
        assert_eq!(encode_focus_report(true), b"\x1b[I");
        assert_eq!(encode_focus_report(false), b"\x1b[O");
    }

    #[test]
    fn sgr_mouse_press_encodes_expected_sequence() {
        let settings = TerminalInputSettings {
            mouse_mode: TerminalMouseMode::Click,
            sgr_mouse: true,
            bracketed_paste: false,
            focus_reporting: false,
        };
        assert_eq!(encode_mouse_press(settings, MouseButton::Left, 4, 2).unwrap(), b"\x1b[<0;5;3M");
    }
}
