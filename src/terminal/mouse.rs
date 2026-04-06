use crate::input::{Modifiers, MouseButton};

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum MouseMode {
    #[default]
    None,
    Click,
    Drag,
    Motion,
}

pub struct MouseState {
    pub mode: MouseMode,
    pub sgr: bool,
}

impl MouseState {
    pub fn new() -> Self {
        Self {
            mode: MouseMode::None,
            sgr: false,
        }
    }

    pub fn is_active(&self) -> bool {
        self.mode != MouseMode::None
    }
}

pub fn mouse_button_code(button: MouseButton) -> Option<u8> {
    match button {
        MouseButton::Left => Some(0),
        MouseButton::Middle => Some(1),
        MouseButton::Right => Some(2),
        MouseButton::Other(_) => Option::None,
    }
}

pub fn modifier_bits(modifiers: Modifiers) -> u8 {
    let mut bits = 0u8;
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

fn encode_sgr(code: u8, col: u16, row: u16, press: bool) -> Vec<u8> {
    let suffix = if press { 'M' } else { 'm' };
    format!("\x1b[<{};{};{}{}", code, col + 1, row + 1, suffix).into_bytes()
}

fn encode_normal(code: u8, col: u16, row: u16) -> Vec<u8> {
    vec![
        b'\x1b',
        b'[',
        b'M',
        code.wrapping_add(32),
        (col + 1 + 32) as u8,
        (row + 1 + 32) as u8,
    ]
}

pub fn encode_mouse_press(
    state: &MouseState,
    button_code: u8,
    modifiers_bits: u8,
    col: u16,
    row: u16,
) -> Option<Vec<u8>> {
    if state.mode == MouseMode::None {
        return Option::None;
    }
    let code = button_code | modifiers_bits;
    if state.sgr {
        Some(encode_sgr(code, col, row, true))
    } else {
        Some(encode_normal(code, col, row))
    }
}

pub fn encode_mouse_release(
    state: &MouseState,
    button_code: u8,
    modifiers_bits: u8,
    col: u16,
    row: u16,
) -> Option<Vec<u8>> {
    if state.mode == MouseMode::None {
        return Option::None;
    }
    if state.sgr {
        let code = button_code | modifiers_bits;
        Some(encode_sgr(code, col, row, false))
    } else {
        Some(encode_normal(3, col, row))
    }
}

pub fn encode_mouse_drag(
    state: &MouseState,
    button_code: u8,
    modifiers_bits: u8,
    col: u16,
    row: u16,
) -> Option<Vec<u8>> {
    match state.mode {
        MouseMode::Drag | MouseMode::Motion => {}
        _ => return Option::None,
    }
    let code = button_code | 32 | modifiers_bits;
    if state.sgr {
        Some(encode_sgr(code, col, row, true))
    } else {
        Some(encode_normal(code, col, row))
    }
}

pub fn encode_mouse_move(
    state: &MouseState,
    modifiers_bits: u8,
    col: u16,
    row: u16,
) -> Option<Vec<u8>> {
    if state.mode != MouseMode::Motion {
        return Option::None;
    }
    let code = 35 | modifiers_bits;
    if state.sgr {
        Some(encode_sgr(code, col, row, true))
    } else {
        Some(encode_normal(code, col, row))
    }
}

pub fn encode_mouse_scroll(
    state: &MouseState,
    modifiers_bits: u8,
    up: bool,
    col: u16,
    row: u16,
) -> Option<Vec<u8>> {
    if state.mode == MouseMode::None {
        return Option::None;
    }
    let base = if up { 64 } else { 65 };
    let code = base | modifiers_bits;
    if state.sgr {
        Some(encode_sgr(code, col, row, true))
    } else {
        Some(encode_normal(code, col, row))
    }
}
