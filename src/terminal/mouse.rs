use crate::input::{Modifiers, MouseButton};

const MOUSE_BUTTON_LEFT: u8 = 0;
const MOUSE_BUTTON_MIDDLE: u8 = 1;
const MOUSE_BUTTON_RIGHT: u8 = 2;
const MOUSE_RELEASE_CODE: u8 = 3;
const MOUSE_MOD_SHIFT: u8 = 4;
const MOUSE_MOD_ALT: u8 = 8;
const MOUSE_MOD_CTRL: u8 = 16;
const MOUSE_COORD_OFFSET: u8 = 32;
const MOUSE_MOTION_FLAG: u8 = 32;
const MOUSE_MOVE_CODE: u8 = 35;
const MOUSE_SCROLL_UP_CODE: u8 = 64;
const MOUSE_SCROLL_DOWN_CODE: u8 = 65;

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
        MouseButton::Left => Some(MOUSE_BUTTON_LEFT),
        MouseButton::Middle => Some(MOUSE_BUTTON_MIDDLE),
        MouseButton::Right => Some(MOUSE_BUTTON_RIGHT),
        MouseButton::Other(_) => None,
    }
}

pub fn modifier_bits(modifiers: Modifiers) -> u8 {
    let mut bits = 0u8;
    if modifiers.shift_key() {
        bits |= MOUSE_MOD_SHIFT;
    }
    if modifiers.alt_key() {
        bits |= MOUSE_MOD_ALT;
    }
    if modifiers.control_key() {
        bits |= MOUSE_MOD_CTRL;
    }
    bits
}

fn push_decimal(mut value: u32, output: &mut Vec<u8>) {
    let mut digits = [0u8; 10];
    let mut len = 0usize;

    loop {
        digits[len] = b'0' + (value % 10) as u8;
        len += 1;
        value /= 10;
        if value == 0 {
            break;
        }
    }

    output.reserve(len);
    for digit in digits[..len].iter().rev() {
        output.push(*digit);
    }
}

fn encode_sgr(code: u8, col: u16, row: u16, press: bool) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(24);
    bytes.extend_from_slice(b"\x1b[<");
    push_decimal(code as u32, &mut bytes);
    bytes.push(b';');
    push_decimal(u32::from(col) + 1, &mut bytes);
    bytes.push(b';');
    push_decimal(u32::from(row) + 1, &mut bytes);
    bytes.push(if press { b'M' } else { b'm' });
    bytes
}

fn encode_normal(code: u8, col: u16, row: u16) -> Option<Vec<u8>> {
    let code = code.checked_add(MOUSE_COORD_OFFSET)?;
    let col = u8::try_from(col)
        .ok()?
        .checked_add(MOUSE_COORD_OFFSET + 1)?;
    let row = u8::try_from(row)
        .ok()?
        .checked_add(MOUSE_COORD_OFFSET + 1)?;

    Some([b'\x1b', b'[', b'M', code, col, row].to_vec())
}

fn encode_mouse_event(
    state: &MouseState,
    code: u8,
    col: u16,
    row: u16,
    press: bool,
) -> Option<Vec<u8>> {
    if state.sgr {
        Some(encode_sgr(code, col, row, press))
    } else {
        encode_normal(code, col, row)
    }
}

pub fn encode_mouse_press(
    state: &MouseState,
    button_code: u8,
    modifiers_bits: u8,
    col: u16,
    row: u16,
) -> Option<Vec<u8>> {
    if state.mode == MouseMode::None {
        return None;
    }

    encode_mouse_event(state, button_code | modifiers_bits, col, row, true)
}

pub fn encode_mouse_release(
    state: &MouseState,
    button_code: u8,
    modifiers_bits: u8,
    col: u16,
    row: u16,
) -> Option<Vec<u8>> {
    if state.mode == MouseMode::None {
        return None;
    }

    if state.sgr {
        encode_mouse_event(state, button_code | modifiers_bits, col, row, false)
    } else {
        encode_mouse_event(state, MOUSE_RELEASE_CODE | modifiers_bits, col, row, false)
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
        _ => return None,
    }

    encode_mouse_event(
        state,
        button_code | MOUSE_MOTION_FLAG | modifiers_bits,
        col,
        row,
        true,
    )
}

pub fn encode_mouse_move(
    state: &MouseState,
    modifiers_bits: u8,
    col: u16,
    row: u16,
) -> Option<Vec<u8>> {
    if state.mode != MouseMode::Motion {
        return None;
    }

    encode_mouse_event(state, MOUSE_MOVE_CODE | modifiers_bits, col, row, true)
}

pub fn encode_mouse_scroll(
    state: &MouseState,
    modifiers_bits: u8,
    up: bool,
    col: u16,
    row: u16,
) -> Option<Vec<u8>> {
    if state.mode == MouseMode::None {
        return None;
    }

    let base = if up {
        MOUSE_SCROLL_UP_CODE
    } else {
        MOUSE_SCROLL_DOWN_CODE
    };
    encode_mouse_event(state, base | modifiers_bits, col, row, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_state() -> MouseState {
        MouseState {
            mode: MouseMode::Click,
            sgr: false,
        }
    }

    #[test]
    fn legacy_release_preserves_modifier_bits() {
        let encoded =
            encode_mouse_release(&legacy_state(), MOUSE_BUTTON_LEFT, MOUSE_MOD_SHIFT, 0, 0)
                .expect("legacy release should encode");

        assert_eq!(encoded, vec![b'\x1b', b'[', b'M', 39, 33, 33]);
    }

    #[test]
    fn sgr_press_uses_expected_encoding() {
        let state = MouseState {
            mode: MouseMode::Click,
            sgr: true,
        };

        let encoded = encode_mouse_press(&state, MOUSE_BUTTON_LEFT, MOUSE_MOD_ALT, 4, 6)
            .expect("sgr press should encode");

        assert_eq!(encoded, b"\x1b[<8;5;7M".to_vec());
    }

    #[test]
    fn sgr_release_uses_lowercase_suffix() {
        let state = MouseState {
            mode: MouseMode::Click,
            sgr: true,
        };

        let encoded = encode_mouse_release(&state, MOUSE_BUTTON_LEFT, MOUSE_MOD_CTRL, 12, 34)
            .expect("sgr release should encode");

        assert_eq!(encoded, b"\x1b[<16;13;35m".to_vec());
    }

    #[test]
    fn legacy_press_rejects_overflowing_column() {
        assert_eq!(
            encode_mouse_press(&legacy_state(), MOUSE_BUTTON_LEFT, 0, 223, 0),
            None
        );
    }

    #[test]
    fn legacy_press_rejects_overflowing_row() {
        assert_eq!(
            encode_mouse_press(&legacy_state(), MOUSE_BUTTON_LEFT, 0, 0, 223),
            None
        );
    }
}
