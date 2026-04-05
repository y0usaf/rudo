use std::sync::Arc;

use crate::{
    settings::Settings,
    terminal::input::{KittyKeyboardFlags, TerminalInputSettings},
};

use winit::{
    event::{ElementState, Ime, KeyEvent, Modifiers, WindowEvent},
    keyboard::{Key, KeyCode, KeyLocation, NamedKey, PhysicalKey},
};
#[cfg(target_os = "macos")]
use {
    crate::{window::WindowSettings, window::settings::OptionAsMeta},
    winit::keyboard::ModifiersKeyState,
};

pub struct KeyboardManager {
    modifiers: Modifiers,
    ime_preedit: (String, Option<(usize, usize)>),
    meta_is_pressed: bool, // see note on 'meta' below
    #[allow(dead_code)]
    settings: Arc<Settings>,
}

impl KeyboardManager {
    pub fn new(settings: Arc<Settings>) -> Self {
        KeyboardManager {
            modifiers: Modifiers::default(),
            ime_preedit: ("".to_string(), None),
            meta_is_pressed: false,
            settings,
        }
    }

    pub fn current_modifiers(&self) -> Modifiers {
        self.modifiers
    }

    pub fn handle_terminal_event(
        &mut self,
        event: &WindowEvent,
        input: TerminalInputSettings,
    ) -> Option<Vec<u8>> {
        match event {
            WindowEvent::KeyboardInput { event: key_event, is_synthetic: false, .. }
                if self.ime_preedit.0.is_empty() =>
            {
                if key_event.state == ElementState::Pressed {
                    self.encode_terminal_key_event(key_event, input)
                } else {
                    None
                }
            }
            WindowEvent::Ime(Ime::Commit(text)) => Some(text.as_bytes().to_vec()),
            WindowEvent::Ime(Ime::Preedit(text, cursor_offset)) => {
                self.ime_preedit = (text.to_string(), *cursor_offset);
                None
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.update_modifiers(*modifiers);
                None
            }
            _ => None,
        }
    }

    fn update_modifiers(&mut self, modifiers: Modifiers) {
        log::trace!("{:?}", modifiers);
        self.modifiers = modifiers;

        #[cfg(target_os = "macos")]
        {
            let ws = self.settings.get::<WindowSettings>();
            self.meta_is_pressed = match ws.input_macos_option_key_is_meta {
                OptionAsMeta::Both => self.modifiers.state().alt_key(),
                OptionAsMeta::OnlyLeft => self.modifiers.lalt_state() == ModifiersKeyState::Pressed,
                OptionAsMeta::OnlyRight => {
                    self.modifiers.ralt_state() == ModifiersKeyState::Pressed
                }
                OptionAsMeta::None => false,
            };
        }

        #[cfg(not(target_os = "macos"))]
        {
            self.meta_is_pressed = self.modifiers.state().alt_key();
        }
    }

    fn encode_terminal_key_event(
        &self,
        key_event: &KeyEvent,
        input: TerminalInputSettings,
    ) -> Option<Vec<u8>> {
        let state = self.modifiers.state();
        let special = encode_special_terminal_key(key_event, input, state);
        let special_encoded = special.is_some();
        let kitty_encoded = encode_kitty_printable_or_ctrl(key_event, state, input);
        let kitty_used = kitty_encoded.is_some();
        let mut bytes = if let Some(special) = special {
            special
        } else if let Some(kitty) = kitty_encoded {
            kitty
        } else if let Some(text) = key_event
            .text
            .as_ref()
            .or(match &key_event.logical_key {
                Key::Character(text) => Some(text),
                _ => None,
            })
            .filter(|_| !state.super_key())
        {
            if state.control_key() {
                encode_control_text(text.as_str())?
            } else {
                text.as_bytes().to_vec()
            }
        } else {
            return None;
        };

        if self.meta_is_pressed && !special_encoded && !kitty_used {
            let mut prefixed = vec![0x1b];
            prefixed.append(&mut bytes);
            bytes = prefixed;
        }

        Some(bytes)
    }
}

fn encode_control_text(text: &str) -> Option<Vec<u8>> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }

    let byte = match ch {
        '@' | '2' | ' ' => 0x00,
        'a'..='z' => (ch as u8) - b'a' + 1,
        'A'..='Z' => (ch as u8) - b'A' + 1,
        '[' => 0x1b,
        '\\' => 0x1c,
        ']' => 0x1d,
        '^' => 0x1e,
        '_' => 0x1f,
        '?' => 0x7f,
        _ => return None,
    };

    Some(vec![byte])
}

fn kitty_keyboard_disambiguation_enabled(input: TerminalInputSettings) -> bool {
    input.kitty_keyboard_flags.bits() & KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES != 0
}

fn kitty_modifier_param(state: winit::keyboard::ModifiersState) -> Option<u8> {
    let mut param = 1;
    if state.shift_key() {
        param += 1;
    }
    if state.alt_key() {
        param += 2;
    }
    if state.control_key() {
        param += 4;
    }
    if state.super_key() {
        param += 8;
    }
    (param > 1).then_some(param)
}

fn encode_kitty_csi_u(codepoint: u32, modifier_param: Option<u8>) -> Vec<u8> {
    if let Some(param) = modifier_param {
        format!("\x1b[{codepoint};{param}u").into_bytes()
    } else {
        format!("\x1b[{codepoint}u").into_bytes()
    }
}

fn kitty_base_character(key_event: &KeyEvent) -> Option<char> {
    let text = match &key_event.logical_key {
        Key::Character(text) => text,
        _ => return None,
    };

    let mut chars = text.chars();
    let ch = chars.next()?;
    (chars.next().is_none()).then_some(ch)
}

fn encode_kitty_printable_or_ctrl(
    key_event: &KeyEvent,
    state: winit::keyboard::ModifiersState,
    input: TerminalInputSettings,
) -> Option<Vec<u8>> {
    if !kitty_keyboard_disambiguation_enabled(input) || state.super_key() {
        return None;
    }

    let modifier_param = kitty_modifier_param(state);

    if state.control_key() {
        let base = kitty_base_character(key_event)?;
        return match base {
            'a'..='z' | 'A'..='Z' | '@' | '[' | '\\' | ']' | '^' | '_' | '?' => {
                Some(encode_kitty_csi_u(base as u32, modifier_param))
            }
            _ => None,
        };
    }

    if !state.shift_key() && !state.alt_key() {
        return None;
    }

    let text = key_event
        .text
        .as_ref()
        .or(match &key_event.logical_key {
            Key::Character(text) => Some(text),
            _ => None,
        })?;

    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() || ch.is_control() {
        return None;
    }

    Some(encode_kitty_csi_u(ch as u32, modifier_param))
}

fn xterm_modifier_param(state: winit::keyboard::ModifiersState) -> Option<u8> {
    let mut param = 1;
    if state.shift_key() {
        param += 1;
    }
    if state.alt_key() {
        param += 2;
    }
    if state.control_key() {
        param += 4;
    }
    (param > 1).then_some(param)
}

fn encode_xterm_csi_modifier(final_char: char, modifier_param: u8) -> Vec<u8> {
    format!("\x1b[1;{modifier_param}{final_char}").into_bytes()
}

fn encode_xterm_ss3(final_char: char) -> Vec<u8> {
    format!("\x1bO{final_char}").into_bytes()
}

fn encode_xterm_tilde_modifier(number: u8, modifier_param: u8) -> Vec<u8> {
    format!("\x1b[{number};{modifier_param}~").into_bytes()
}

fn encode_xterm_function_modifier(final_char: char, modifier_param: u8) -> Vec<u8> {
    format!("\x1b[1;{modifier_param}{final_char}").into_bytes()
}

fn application_keypad_symbol(code: KeyCode) -> Option<char> {
    match code {
        KeyCode::NumpadEnter => Some('M'),
        KeyCode::NumpadDivide => Some('o'),
        KeyCode::NumpadStar => Some('j'),
        KeyCode::NumpadSubtract => Some('m'),
        KeyCode::NumpadAdd => Some('k'),
        KeyCode::NumpadComma => Some('l'),
        KeyCode::NumpadDecimal => Some('n'),
        KeyCode::Numpad0 => Some('p'),
        KeyCode::Numpad1 => Some('q'),
        KeyCode::Numpad2 => Some('r'),
        KeyCode::Numpad3 => Some('s'),
        KeyCode::Numpad4 => Some('t'),
        KeyCode::Numpad5 => Some('u'),
        KeyCode::Numpad6 => Some('v'),
        KeyCode::Numpad7 => Some('w'),
        KeyCode::Numpad8 => Some('x'),
        KeyCode::Numpad9 => Some('y'),
        _ => None,
    }
}

fn encode_application_keypad_symbol(symbol: char, modifier_param: Option<u8>) -> Vec<u8> {
    if let Some(param) = modifier_param {
        format!("\x1bO{param}{symbol}").into_bytes()
    } else {
        format!("\x1bO{symbol}").into_bytes()
    }
}

fn encode_application_keypad_sequence(
    key_event: &KeyEvent,
    modifier_param: Option<u8>,
) -> Option<Vec<u8>> {
    let PhysicalKey::Code(code) = key_event.physical_key else {
        return None;
    };

    application_keypad_symbol(code)
        .map(|symbol| encode_application_keypad_symbol(symbol, modifier_param))
}

fn encode_cursor_key(
    final_char: char,
    application_cursor: bool,
    modifier_param: Option<u8>,
) -> Vec<u8> {
    if let Some(param) = modifier_param {
        encode_xterm_csi_modifier(final_char, param)
    } else if application_cursor {
        encode_xterm_ss3(final_char)
    } else {
        format!("\x1b[{final_char}").into_bytes()
    }
}

fn encode_home_key(application_cursor: bool, modifier_param: Option<u8>) -> Vec<u8> {
    if let Some(param) = modifier_param {
        encode_xterm_csi_modifier('H', param)
    } else if application_cursor {
        encode_xterm_ss3('H')
    } else {
        b"\x1b[H".to_vec()
    }
}

fn encode_end_key(application_cursor: bool, modifier_param: Option<u8>) -> Vec<u8> {
    if let Some(param) = modifier_param {
        encode_xterm_csi_modifier('F', param)
    } else if application_cursor {
        encode_xterm_ss3('F')
    } else {
        b"\x1b[F".to_vec()
    }
}

fn encode_special_terminal_key(
    key_event: &KeyEvent,
    input: TerminalInputSettings,
    state: winit::keyboard::ModifiersState,
) -> Option<Vec<u8>> {
    if key_event.location == KeyLocation::Numpad {
        let modifier_param = xterm_modifier_param(state);

        if input.application_keypad {
            if let Some(sequence) = encode_application_keypad_sequence(key_event, modifier_param) {
                return Some(sequence);
            }
        }

        if let Some(text) = key_event.text.as_ref() {
            return Some(text.as_bytes().to_vec());
        }

        let Key::Named(key) = &key_event.logical_key else {
            return None;
        };

        return match key {
            NamedKey::Enter => Some(b"\r".to_vec()),
            NamedKey::Tab if state.shift_key() => Some(b"\x1b[Z".to_vec()),
            NamedKey::Tab => Some(b"\t".to_vec()),
            NamedKey::ArrowUp => {
                Some(encode_cursor_key('A', input.application_cursor, modifier_param))
            }
            NamedKey::ArrowDown => {
                Some(encode_cursor_key('B', input.application_cursor, modifier_param))
            }
            NamedKey::ArrowRight => {
                Some(encode_cursor_key('C', input.application_cursor, modifier_param))
            }
            NamedKey::ArrowLeft => {
                Some(encode_cursor_key('D', input.application_cursor, modifier_param))
            }
            NamedKey::Home => Some(encode_home_key(input.application_cursor, modifier_param)),
            NamedKey::End => Some(encode_end_key(input.application_cursor, modifier_param)),
            NamedKey::Insert => Some(if let Some(param) = modifier_param {
                encode_xterm_tilde_modifier(2, param)
            } else {
                b"\x1b[2~".to_vec()
            }),
            NamedKey::Delete => Some(if let Some(param) = modifier_param {
                encode_xterm_tilde_modifier(3, param)
            } else {
                b"\x1b[3~".to_vec()
            }),
            NamedKey::PageUp => Some(if let Some(param) = modifier_param {
                encode_xterm_tilde_modifier(5, param)
            } else {
                b"\x1b[5~".to_vec()
            }),
            NamedKey::PageDown => Some(if let Some(param) = modifier_param {
                encode_xterm_tilde_modifier(6, param)
            } else {
                b"\x1b[6~".to_vec()
            }),
            _ => None,
        };
    }

    let Key::Named(key) = &key_event.logical_key else {
        return None;
    };

    let modifier_param = xterm_modifier_param(state);

    match key {
        NamedKey::ArrowUp => Some(encode_cursor_key('A', input.application_cursor, modifier_param)),
        NamedKey::ArrowDown => {
            Some(encode_cursor_key('B', input.application_cursor, modifier_param))
        }
        NamedKey::ArrowRight => {
            Some(encode_cursor_key('C', input.application_cursor, modifier_param))
        }
        NamedKey::ArrowLeft => {
            Some(encode_cursor_key('D', input.application_cursor, modifier_param))
        }
        NamedKey::Home => Some(encode_home_key(input.application_cursor, modifier_param)),
        NamedKey::End => Some(encode_end_key(input.application_cursor, modifier_param)),
        NamedKey::Insert => Some(if let Some(param) = modifier_param {
            encode_xterm_tilde_modifier(2, param)
        } else {
            b"\x1b[2~".to_vec()
        }),
        NamedKey::Delete => Some(if let Some(param) = modifier_param {
            encode_xterm_tilde_modifier(3, param)
        } else {
            b"\x1b[3~".to_vec()
        }),
        NamedKey::PageUp => Some(if let Some(param) = modifier_param {
            encode_xterm_tilde_modifier(5, param)
        } else {
            b"\x1b[5~".to_vec()
        }),
        NamedKey::PageDown => Some(if let Some(param) = modifier_param {
            encode_xterm_tilde_modifier(6, param)
        } else {
            b"\x1b[6~".to_vec()
        }),
        NamedKey::Enter => Some(b"\r".to_vec()),
        NamedKey::Tab if state.shift_key() => Some(b"\x1b[Z".to_vec()),
        NamedKey::Tab => Some(b"\t".to_vec()),
        NamedKey::Escape => Some(b"\x1b".to_vec()),
        NamedKey::Backspace => Some(b"\x7f".to_vec()),
        NamedKey::F1 => Some(if let Some(param) = modifier_param {
            encode_xterm_function_modifier('P', param)
        } else {
            b"\x1bOP".to_vec()
        }),
        NamedKey::F2 => Some(if let Some(param) = modifier_param {
            encode_xterm_function_modifier('Q', param)
        } else {
            b"\x1bOQ".to_vec()
        }),
        NamedKey::F3 => Some(if let Some(param) = modifier_param {
            encode_xterm_function_modifier('R', param)
        } else {
            b"\x1bOR".to_vec()
        }),
        NamedKey::F4 => Some(if let Some(param) = modifier_param {
            encode_xterm_function_modifier('S', param)
        } else {
            b"\x1bOS".to_vec()
        }),
        NamedKey::F5 => Some(if let Some(param) = modifier_param {
            encode_xterm_tilde_modifier(15, param)
        } else {
            b"\x1b[15~".to_vec()
        }),
        NamedKey::F6 => Some(if let Some(param) = modifier_param {
            encode_xterm_tilde_modifier(17, param)
        } else {
            b"\x1b[17~".to_vec()
        }),
        NamedKey::F7 => Some(if let Some(param) = modifier_param {
            encode_xterm_tilde_modifier(18, param)
        } else {
            b"\x1b[18~".to_vec()
        }),
        NamedKey::F8 => Some(if let Some(param) = modifier_param {
            encode_xterm_tilde_modifier(19, param)
        } else {
            b"\x1b[19~".to_vec()
        }),
        NamedKey::F9 => Some(if let Some(param) = modifier_param {
            encode_xterm_tilde_modifier(20, param)
        } else {
            b"\x1b[20~".to_vec()
        }),
        NamedKey::F10 => Some(if let Some(param) = modifier_param {
            encode_xterm_tilde_modifier(21, param)
        } else {
            b"\x1b[21~".to_vec()
        }),
        NamedKey::F11 => Some(if let Some(param) = modifier_param {
            encode_xterm_tilde_modifier(23, param)
        } else {
            b"\x1b[23~".to_vec()
        }),
        NamedKey::F12 => Some(if let Some(param) = modifier_param {
            encode_xterm_tilde_modifier(24, param)
        } else {
            b"\x1b[24~".to_vec()
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        application_keypad_symbol, encode_application_keypad_symbol, encode_control_text,
        encode_cursor_key, encode_end_key, encode_home_key, encode_kitty_csi_u,
        encode_xterm_csi_modifier, encode_xterm_function_modifier,
        encode_xterm_ss3, encode_xterm_tilde_modifier, kitty_modifier_param,
        xterm_modifier_param,
    };
    use crate::terminal::input::{KittyKeyboardFlags, TerminalInputSettings};
    use winit::keyboard::{KeyCode, ModifiersState};

    fn input_with_kitty() -> TerminalInputSettings {
        TerminalInputSettings {
            kitty_keyboard_flags: KittyKeyboardFlags::new(
                KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES,
            ),
            ..TerminalInputSettings::default()
        }
    }

    #[test]
    fn xterm_modifier_values_match_common_encoding() {
        assert_eq!(xterm_modifier_param(ModifiersState::empty()), None);
        assert_eq!(xterm_modifier_param(ModifiersState::SHIFT), Some(2));
        assert_eq!(xterm_modifier_param(ModifiersState::ALT), Some(3));
        assert_eq!(xterm_modifier_param(ModifiersState::CONTROL), Some(5));
        assert_eq!(xterm_modifier_param(ModifiersState::SHIFT | ModifiersState::ALT), Some(4));
        assert_eq!(xterm_modifier_param(ModifiersState::SHIFT | ModifiersState::CONTROL), Some(6));
        assert_eq!(
            xterm_modifier_param(
                ModifiersState::SHIFT | ModifiersState::ALT | ModifiersState::CONTROL
            ),
            Some(8)
        );
    }

    #[test]
    fn csi_modifier_sequences_match_xterm_conventions() {
        assert_eq!(encode_xterm_csi_modifier('A', 2), b"\x1b[1;2A");
        assert_eq!(encode_xterm_csi_modifier('H', 5), b"\x1b[1;5H");
        assert_eq!(encode_xterm_csi_modifier('B', 3), b"\x1b[1;3B");
    }

    #[test]
    fn tilde_modifier_sequences_match_xterm_conventions() {
        assert_eq!(encode_xterm_tilde_modifier(3, 2), b"\x1b[3;2~");
        assert_eq!(encode_xterm_tilde_modifier(6, 7), b"\x1b[6;7~");
        assert_eq!(encode_xterm_tilde_modifier(24, 8), b"\x1b[24;8~");
    }

    #[test]
    fn function_modifier_sequences_match_xterm_conventions() {
        assert_eq!(encode_xterm_function_modifier('P', 2), b"\x1b[1;2P");
        assert_eq!(encode_xterm_function_modifier('S', 5), b"\x1b[1;5S");
    }

    #[test]
    fn ss3_sequences_match_xterm_conventions() {
        assert_eq!(encode_xterm_ss3('A'), b"\x1bOA");
        assert_eq!(encode_xterm_ss3('H'), b"\x1bOH");
        assert_eq!(encode_xterm_ss3('P'), b"\x1bOP");
    }

    #[test]
    fn home_and_end_follow_application_cursor_mode() {
        assert_eq!(encode_home_key(false, None), b"\x1b[H");
        assert_eq!(encode_end_key(false, None), b"\x1b[F");
        assert_eq!(encode_home_key(true, None), b"\x1bOH");
        assert_eq!(encode_end_key(true, None), b"\x1bOF");
        assert_eq!(encode_home_key(true, Some(5)), b"\x1b[1;5H");
        assert_eq!(encode_end_key(true, Some(3)), b"\x1b[1;3F");
    }

    #[test]
    fn cursor_keys_use_ss3_only_without_modifiers() {
        assert_eq!(encode_cursor_key('A', false, None), b"\x1b[A");
        assert_eq!(encode_cursor_key('A', true, None), b"\x1bOA");
        assert_eq!(encode_cursor_key('D', true, Some(2)), b"\x1b[1;2D");
    }

    #[test]
    fn application_keypad_encodes_common_operator_and_digit_keys() {
        assert_eq!(application_keypad_symbol(KeyCode::NumpadEnter), Some('M'));
        assert_eq!(application_keypad_symbol(KeyCode::NumpadDecimal), Some('n'));
        assert_eq!(application_keypad_symbol(KeyCode::Numpad8), Some('x'));
        assert_eq!(application_keypad_symbol(KeyCode::Numpad3), Some('s'));

        assert_eq!(encode_application_keypad_symbol('M', None), b"\x1bOM");
        assert_eq!(encode_application_keypad_symbol('n', None), b"\x1bOn");
        assert_eq!(encode_application_keypad_symbol('x', Some(3)), b"\x1bO3x");
        assert_eq!(encode_application_keypad_symbol('s', None), b"\x1bOs");
    }

    #[test]
    fn kitty_modifier_values_include_super() {
        assert_eq!(kitty_modifier_param(ModifiersState::empty()), None);
        assert_eq!(kitty_modifier_param(ModifiersState::SHIFT), Some(2));
        assert_eq!(kitty_modifier_param(ModifiersState::ALT), Some(3));
        assert_eq!(kitty_modifier_param(ModifiersState::CONTROL), Some(5));
        assert_eq!(kitty_modifier_param(ModifiersState::SUPER), Some(9));
        assert_eq!(kitty_modifier_param(ModifiersState::ALT | ModifiersState::CONTROL), Some(7));
    }

    #[test]
    fn kitty_csi_u_sequences_encode_codepoint_and_modifiers() {
        assert_eq!(encode_kitty_csi_u('a' as u32, None), b"\x1b[97u");
        assert_eq!(encode_kitty_csi_u('A' as u32, Some(6)), b"\x1b[65;6u");
        assert_eq!(encode_kitty_csi_u('?' as u32, Some(5)), b"\x1b[63;5u");
    }

    #[test]
    fn kitty_mode_formats_modified_printable_sequences() {
        assert_eq!(
            encode_kitty_csi_u(
                'A' as u32,
                kitty_modifier_param(ModifiersState::SHIFT | ModifiersState::ALT)
            ),
            b"\x1b[65;4u"
        );
        assert_eq!(
            encode_kitty_csi_u('a' as u32, kitty_modifier_param(ModifiersState::ALT)),
            b"\x1b[97;3u"
        );
    }

    #[test]
    fn kitty_mode_formats_ambiguous_ctrl_sequences() {
        assert_eq!(
            encode_kitty_csi_u('_' as u32, kitty_modifier_param(ModifiersState::CONTROL)),
            b"\x1b[95;5u"
        );
        assert_eq!(
            encode_kitty_csi_u('?' as u32, kitty_modifier_param(ModifiersState::CONTROL)),
            b"\x1b[63;5u"
        );
    }

    #[test]
    fn plain_mode_control_encoding_remains_legacy_single_byte() {
        assert_eq!(encode_control_text("_"), Some(vec![0x1f]));
        assert_eq!(encode_control_text("?"), Some(vec![0x7f]));
    }

    #[test]
    fn special_keys_continue_to_use_xterm_sequences_in_kitty_mode() {
        assert_eq!(
            encode_cursor_key('A', false, xterm_modifier_param(ModifiersState::ALT)),
            b"\x1b[1;3A"
        );
        assert_eq!(input_with_kitty().kitty_keyboard_flags.bits(), 1);
    }
}
