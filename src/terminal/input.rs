use winit::{event::MouseButton, keyboard::ModifiersState};

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
    pub application_cursor: bool,
    pub application_keypad: bool,
    pub kitty_keyboard_flags: KittyKeyboardFlags,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KittyKeyboardFlags(u8);

impl KittyKeyboardFlags {
    pub const DISAMBIGUATE_ESCAPE_CODES: u8 = 1;
    pub const SUPPORTED_MASK: u8 = Self::DISAMBIGUATE_ESCAPE_CODES;

    pub const fn new(bits: u8) -> Self {
        Self(bits & Self::SUPPORTED_MASK)
    }

    pub const fn bits(self) -> u8 {
        self.0
    }
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
    modifiers: ModifiersState,
    col: u32,
    row: u32,
) -> Option<Vec<u8>> {
    if settings.mouse_mode == TerminalMouseMode::Disabled {
        return None;
    }
    Some(encode_mouse_event(
        settings,
        mouse_button_code(button)? | mouse_modifier_bits(modifiers),
        col,
        row,
        true,
    ))
}

pub fn encode_mouse_release(
    settings: TerminalInputSettings,
    button: MouseButton,
    modifiers: ModifiersState,
    col: u32,
    row: u32,
) -> Option<Vec<u8>> {
    if settings.mouse_mode == TerminalMouseMode::Disabled {
        return None;
    }
    Some(encode_mouse_event(
        settings,
        mouse_button_code(button)? | mouse_modifier_bits(modifiers),
        col,
        row,
        false,
    ))
}

pub fn encode_mouse_drag(
    settings: TerminalInputSettings,
    button: MouseButton,
    modifiers: ModifiersState,
    col: u32,
    row: u32,
) -> Option<Vec<u8>> {
    if !matches!(settings.mouse_mode, TerminalMouseMode::Drag | TerminalMouseMode::Motion) {
        return None;
    }
    Some(encode_mouse_event(
        settings,
        mouse_button_code(button)? | 32 | mouse_modifier_bits(modifiers),
        col,
        row,
        true,
    ))
}

pub fn encode_mouse_move(
    settings: TerminalInputSettings,
    modifiers: ModifiersState,
    col: u32,
    row: u32,
) -> Option<Vec<u8>> {
    (settings.mouse_mode == TerminalMouseMode::Motion)
        .then(|| encode_mouse_event(settings, 35 | mouse_modifier_bits(modifiers), col, row, true))
}

pub fn encode_mouse_scroll(
    settings: TerminalInputSettings,
    modifiers: ModifiersState,
    up: bool,
    col: u32,
    row: u32,
) -> Option<Vec<u8>> {
    (settings.mouse_mode != TerminalMouseMode::Disabled).then(|| {
        encode_mouse_event(
            settings,
            (if up { 64 } else { 65 }) | mouse_modifier_bits(modifiers),
            col,
            row,
            true,
        )
    })
}

fn mouse_button_code(button: MouseButton) -> Option<u8> {
    match button {
        MouseButton::Left => Some(0),
        MouseButton::Middle => Some(1),
        MouseButton::Right => Some(2),
        _ => None,
    }
}

fn mouse_modifier_bits(modifiers: ModifiersState) -> u8 {
    let mut bits = 0;
    if modifiers.shift_key() {
        bits |= 4;
    }
    if modifiers.alt_key() {
        bits |= 8;
    }
    if modifiers.control_key() {
        bits |= 16;
    }
    bits
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
        KittyKeyboardFlags, TerminalInputSettings, TerminalMouseMode, encode_bracketed_paste,
        encode_focus_report, encode_mouse_drag, encode_mouse_move, encode_mouse_press,
        encode_mouse_release, encode_mouse_scroll,
    };
    use winit::{event::MouseButton, keyboard::ModifiersState};

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
            application_cursor: false,
            application_keypad: false,
            kitty_keyboard_flags: KittyKeyboardFlags::default(),
        };
        assert_eq!(
            encode_mouse_press(settings, MouseButton::Left, ModifiersState::empty(), 4, 2).unwrap(),
            b"\x1b[<0;5;3M"
        );
    }

    #[test]
    fn plain_mouse_encoding_is_unchanged_without_modifiers() {
        let settings = TerminalInputSettings {
            mouse_mode: TerminalMouseMode::Click,
            sgr_mouse: false,
            ..TerminalInputSettings::default()
        };
        assert_eq!(
            encode_mouse_press(settings, MouseButton::Left, ModifiersState::empty(), 4, 2).unwrap(),
            vec![0x1b, b'[', b'M', 32, 37, 35]
        );
    }

    #[test]
    fn sgr_mouse_press_and_release_include_modifier_bits() {
        let settings = TerminalInputSettings {
            mouse_mode: TerminalMouseMode::Click,
            sgr_mouse: true,
            ..TerminalInputSettings::default()
        };
        assert_eq!(
            encode_mouse_press(
                settings,
                MouseButton::Left,
                ModifiersState::SHIFT | ModifiersState::ALT | ModifiersState::CONTROL,
                4,
                2,
            )
            .unwrap(),
            b"\x1b[<28;5;3M"
        );
        assert_eq!(
            encode_mouse_release(settings, MouseButton::Right, ModifiersState::ALT, 1, 1).unwrap(),
            b"\x1b[<10;2;2m"
        );
    }

    #[test]
    fn drag_move_and_scroll_include_modifier_bits() {
        let settings = TerminalInputSettings {
            mouse_mode: TerminalMouseMode::Motion,
            sgr_mouse: true,
            ..TerminalInputSettings::default()
        };
        assert_eq!(
            encode_mouse_drag(settings, MouseButton::Middle, ModifiersState::CONTROL, 0, 0).unwrap(),
            b"\x1b[<49;1;1M"
        );
        assert_eq!(
            encode_mouse_move(settings, ModifiersState::SHIFT, 2, 3).unwrap(),
            b"\x1b[<39;3;4M"
        );
        assert_eq!(
            encode_mouse_scroll(settings, ModifiersState::ALT, true, 5, 6).unwrap(),
            b"\x1b[<72;6;7M"
        );
    }

    #[test]
    fn kitty_keyboard_flags_mask_to_supported_bits() {
        let settings = TerminalInputSettings {
            kitty_keyboard_flags: KittyKeyboardFlags::new(0xff),
            ..TerminalInputSettings::default()
        };

        assert_eq!(settings.kitty_keyboard_flags.bits(), KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES);
    }
}
