//! Keyboard handling: XKB context, key repeat, and fallback keymap.

use std::os::raw::c_char;
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use crate::xkb_ffi as xkb;
use crate::xkb_ffi::xkb as xkb_fns;

use crate::input::{Key, KeyEvent, Modifiers, MouseButton};

// ─── Key repeat state machine ────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct RepeatState {
    pub rate: i32,
    pub delay: i32,
    pub key: Option<u32>,
    pub next_fire: Option<Instant>,
}

impl RepeatState {
    pub fn timeout_ms(&self) -> i32 {
        let Some(next) = self.next_fire else {
            return -1;
        };
        let now = Instant::now();
        if next <= now {
            0
        } else {
            let ms = next.duration_since(now).as_millis();
            ms.min(i32::MAX as u128) as i32
        }
    }

    pub fn start(&mut self, key: u32) {
        if self.rate <= 0 {
            self.stop(None);
            return;
        }
        self.key = Some(key);
        self.next_fire = Some(Instant::now() + Duration::from_millis(self.delay.max(0) as u64));
    }

    pub fn stop(&mut self, key: Option<u32>) {
        if key.is_none() || self.key == key {
            self.key = None;
            self.next_fire = None;
        }
    }

    pub fn reschedule(&mut self) {
        if self.rate > 0 {
            self.next_fire =
                Some(Instant::now() + Duration::from_nanos(1_000_000_000u64 / self.rate as u64));
        } else {
            self.next_fire = None;
        }
    }
}

// ─── XKB context ─────────────────────────────────────────────────────────────

pub struct XkbContextData {
    context: NonNull<std::ffi::c_void>,
    keymap: NonNull<std::ffi::c_void>,
    state: NonNull<std::ffi::c_void>,
}

impl XkbContextData {
    pub fn from_keymap_string(s: &[u8]) -> Option<Self> {
        let xkbh = xkb_fns();
        // SAFETY: xkb_context_new with NO_FLAGS creates a new context.
        // Returns null on failure, which NonNull::new handles.
        let context = NonNull::new(unsafe { (xkbh.xkb_context_new)(xkb::XKB_CONTEXT_NO_FLAGS) })?;
        let ptr = s.as_ptr().cast::<c_char>();
        // SAFETY: context is a valid xkb_context (NonNull). ptr points to
        // the keymap string buffer with s.len() bytes. Format and flags are
        // valid XKB constants.
        let keymap = NonNull::new(unsafe {
            (xkbh.xkb_keymap_new_from_buffer)(
                context.as_ptr(),
                ptr,
                s.len(),
                xkb::XKB_KEYMAP_FORMAT_TEXT_V1,
                xkb::XKB_KEYMAP_COMPILE_NO_FLAGS,
            )
        })?;
        // SAFETY: keymap is a valid xkb_keymap (NonNull). Returns null on
        // failure, which NonNull::new handles.
        let state = NonNull::new(unsafe { (xkbh.xkb_state_new)(keymap.as_ptr()) })?;
        Some(Self {
            context,
            keymap,
            state,
        })
    }

    pub fn modifiers(&mut self) -> Modifiers {
        let xkbh = xkb_fns();
        // SAFETY: state is a valid xkb_state pointer. Modifier names are
        // null-terminated byte constants from xkb_ffi. XKB_STATE_MODS_EFFECTIVE
        // is a valid modifier type.
        let active = |name: &[u8], state: *mut std::ffi::c_void| unsafe {
            (xkbh.xkb_state_mod_name_is_active)(
                state,
                name.as_ptr().cast(),
                xkb::XKB_STATE_MODS_EFFECTIVE,
            ) > 0
        };
        Modifiers {
            shift: active(xkb::XKB_MOD_NAME_SHIFT, self.state.as_ptr()),
            ctrl: active(xkb::XKB_MOD_NAME_CTRL, self.state.as_ptr()),
            alt: active(xkb::XKB_MOD_NAME_ALT, self.state.as_ptr()),
        }
    }

    pub fn update_modifiers(
        &mut self,
        mods_depressed: u32,
        mods_latched: u32,
        mods_locked: u32,
        group: u32,
    ) {
        let xkbh = xkb_fns();
        // SAFETY: self.state is a valid xkb_state (NonNull). The modifier
        // arguments come directly from the Wayland compositor's modifier event.
        unsafe {
            (xkbh.xkb_state_update_mask)(
                self.state.as_ptr(),
                mods_depressed,
                mods_latched,
                mods_locked,
                0,
                0,
                group,
            );
        }
    }

    pub fn key_repeats(&self, key: u32) -> bool {
        let xkbh = xkb_fns();
        // SAFETY: self.keymap is a valid xkb_keymap (NonNull). key + 8
        // converts from evdev keycode to XKB keycode.
        unsafe { (xkbh.xkb_keymap_key_repeats)(self.keymap.as_ptr(), key + 8) > 0 }
    }

    pub fn key_event(&mut self, key: u32, pressed: bool) -> KeyEvent {
        let xkbh = xkb_fns();
        let code = key + 8;
        // SAFETY: self.state is a valid xkb_state (NonNull). code is a valid
        // XKB keycode. XKB_KEY_DOWN/XKB_KEY_UP are valid key directions.
        unsafe {
            (xkbh.xkb_state_update_key)(
                self.state.as_ptr(),
                code,
                if pressed {
                    xkb::XKB_KEY_DOWN
                } else {
                    xkb::XKB_KEY_UP
                },
            );
        }

        self.key_event_inner(code, pressed)
    }

    pub fn repeat_key_event(&mut self, key: u32) -> KeyEvent {
        self.key_event_inner(key + 8, true)
    }

    fn key_event_inner(&mut self, code: u32, pressed: bool) -> KeyEvent {
        let xkbh = xkb_fns();
        // SAFETY: self.state is a valid xkb_state (NonNull). code is a valid XKB keycode.
        let sym = unsafe { (xkbh.xkb_state_key_get_one_sym)(self.state.as_ptr(), code) };
        let named = match sym {
            s if s == xkb::XKB_KEY_Return => Some(Key::Enter),
            s if s == xkb::XKB_KEY_BackSpace => Some(Key::Backspace),
            s if s == xkb::XKB_KEY_Escape => Some(Key::Escape),
            s if s == xkb::XKB_KEY_Tab => Some(Key::Tab),
            s if s == xkb::XKB_KEY_Left => Some(Key::ArrowLeft),
            s if s == xkb::XKB_KEY_Right => Some(Key::ArrowRight),
            s if s == xkb::XKB_KEY_Up => Some(Key::ArrowUp),
            s if s == xkb::XKB_KEY_Down => Some(Key::ArrowDown),
            s if s == xkb::XKB_KEY_Home => Some(Key::Home),
            s if s == xkb::XKB_KEY_End => Some(Key::End),
            s if s == xkb::XKB_KEY_Prior => Some(Key::PageUp),
            s if s == xkb::XKB_KEY_Next => Some(Key::PageDown),
            s if s == xkb::XKB_KEY_Delete => Some(Key::Delete),
            s if s == xkb::XKB_KEY_Insert => Some(Key::Insert),
            s if s == xkb::XKB_KEY_F1 => Some(Key::F(1)),
            s if s == xkb::XKB_KEY_F2 => Some(Key::F(2)),
            s if s == xkb::XKB_KEY_F3 => Some(Key::F(3)),
            s if s == xkb::XKB_KEY_F4 => Some(Key::F(4)),
            s if s == xkb::XKB_KEY_F5 => Some(Key::F(5)),
            s if s == xkb::XKB_KEY_F6 => Some(Key::F(6)),
            s if s == xkb::XKB_KEY_F7 => Some(Key::F(7)),
            s if s == xkb::XKB_KEY_F8 => Some(Key::F(8)),
            s if s == xkb::XKB_KEY_F9 => Some(Key::F(9)),
            s if s == xkb::XKB_KEY_F10 => Some(Key::F(10)),
            s if s == xkb::XKB_KEY_F11 => Some(Key::F(11)),
            s if s == xkb::XKB_KEY_F12 => Some(Key::F(12)),
            _ => None,
        };

        let key = if let Some(key) = named {
            key
        } else {
            let mut buf = [0u8; 64];
            // SAFETY: self.state is a valid xkb_state (NonNull). buf is a
            // stack-allocated 64-byte array. xkb_state_key_get_utf8 writes at
            // most buf.len() bytes including the null terminator.
            let written = unsafe {
                (xkbh.xkb_state_key_get_utf8)(
                    self.state.as_ptr(),
                    code,
                    buf.as_mut_ptr().cast(),
                    buf.len(),
                )
            };
            if written > 1 {
                let s = std::str::from_utf8(&buf[..(written as usize - 1)]).unwrap_or("");
                if s == " " {
                    Key::Space
                } else {
                    Key::Text(s.to_string())
                }
            } else {
                Key::Unknown
            }
        };

        KeyEvent { pressed, key }
    }
}

impl Drop for XkbContextData {
    fn drop(&mut self) {
        let xkbh = xkb_fns();
        // SAFETY: state, keymap, and context are valid NonNull pointers
        // created in from_keymap_string. Each unref is called exactly once
        // in Drop, releasing the XKB resources.
        unsafe {
            (xkbh.xkb_state_unref)(self.state.as_ptr());
            (xkbh.xkb_keymap_unref)(self.keymap.as_ptr());
            (xkbh.xkb_context_unref)(self.context.as_ptr());
        }
    }
}

// ─── Fallback keymap (no XKB) ────────────────────────────────────────────────

pub fn update_fallback_modifiers(mods: &mut Modifiers, key: u32, pressed: bool) {
    match key {
        29 | 97 => mods.ctrl = pressed,
        42 | 54 => mods.shift = pressed,
        56 | 100 => mods.alt = pressed,
        _ => {}
    }
}

pub fn fallback_key_event(key: u32, pressed: bool, mods: Modifiers) -> KeyEvent {
    KeyEvent {
        pressed,
        key: fallback_key(key, mods),
    }
}

pub fn fallback_key_is_repeatable(key: u32) -> bool {
    !matches!(key, 29 | 42 | 54 | 56 | 97 | 100)
}

fn fallback_key(key: u32, mods: Modifiers) -> Key {
    let shift = mods.shift;
    match key {
        1 => Key::Escape,
        14 => Key::Backspace,
        15 => Key::Tab,
        28 => Key::Enter,
        57 => Key::Space,
        102 => Key::Home,
        103 => Key::ArrowUp,
        104 => Key::PageUp,
        105 => Key::ArrowLeft,
        106 => Key::ArrowRight,
        107 => Key::End,
        108 => Key::ArrowDown,
        109 => Key::PageDown,
        110 => Key::Insert,
        111 => Key::Delete,
        59 => Key::F(1),
        60 => Key::F(2),
        61 => Key::F(3),
        62 => Key::F(4),
        63 => Key::F(5),
        64 => Key::F(6),
        65 => Key::F(7),
        66 => Key::F(8),
        67 => Key::F(9),
        68 => Key::F(10),
        87 => Key::F(11),
        88 => Key::F(12),
        _ => match fallback_text(key, shift) {
            Some(" ") => Key::Space,
            Some(s) => Key::Text(s.to_string()),
            None => Key::Unknown,
        },
    }
}

fn fallback_text(key: u32, shift: bool) -> Option<&'static str> {
    Some(match (key, shift) {
        (2, false) => "1",
        (2, true) => "!",
        (3, false) => "2",
        (3, true) => "@",
        (4, false) => "3",
        (4, true) => "#",
        (5, false) => "4",
        (5, true) => "$",
        (6, false) => "5",
        (6, true) => "%",
        (7, false) => "6",
        (7, true) => "^",
        (8, false) => "7",
        (8, true) => "&",
        (9, false) => "8",
        (9, true) => "*",
        (10, false) => "9",
        (10, true) => "(",
        (11, false) => "0",
        (11, true) => ")",
        (12, false) => "-",
        (12, true) => "_",
        (13, false) => "=",
        (13, true) => "+",
        (16, false) => "q",
        (16, true) => "Q",
        (17, false) => "w",
        (17, true) => "W",
        (18, false) => "e",
        (18, true) => "E",
        (19, false) => "r",
        (19, true) => "R",
        (20, false) => "t",
        (20, true) => "T",
        (21, false) => "y",
        (21, true) => "Y",
        (22, false) => "u",
        (22, true) => "U",
        (23, false) => "i",
        (23, true) => "I",
        (24, false) => "o",
        (24, true) => "O",
        (25, false) => "p",
        (25, true) => "P",
        (26, false) => "[",
        (26, true) => "{",
        (27, false) => "]",
        (27, true) => "}",
        (30, false) => "a",
        (30, true) => "A",
        (31, false) => "s",
        (31, true) => "S",
        (32, false) => "d",
        (32, true) => "D",
        (33, false) => "f",
        (33, true) => "F",
        (34, false) => "g",
        (34, true) => "G",
        (35, false) => "h",
        (35, true) => "H",
        (36, false) => "j",
        (36, true) => "J",
        (37, false) => "k",
        (37, true) => "K",
        (38, false) => "l",
        (38, true) => "L",
        (39, false) => ";",
        (39, true) => ":",
        (40, false) => "'",
        (40, true) => "\"",
        (41, false) => "`",
        (41, true) => "~",
        (43, false) => "\\",
        (43, true) => "|",
        (44, false) => "z",
        (44, true) => "Z",
        (45, false) => "x",
        (45, true) => "X",
        (46, false) => "c",
        (46, true) => "C",
        (47, false) => "v",
        (47, true) => "V",
        (48, false) => "b",
        (48, true) => "B",
        (49, false) => "n",
        (49, true) => "N",
        (50, false) => "m",
        (50, true) => "M",
        (51, false) => ",",
        (51, true) => "<",
        (52, false) => ".",
        (52, true) => ">",
        (53, false) => "/",
        (53, true) => "?",
        _ => return None,
    })
}

// ─── Pointer button mapping ──────────────────────────────────────────────────

const BTN_LEFT: u32 = 272;
const BTN_RIGHT: u32 = 273;
const BTN_MIDDLE: u32 = 274;

pub fn map_pointer_button(button: u32) -> MouseButton {
    match button {
        BTN_LEFT => MouseButton::Left,
        BTN_MIDDLE => MouseButton::Middle,
        BTN_RIGHT => MouseButton::Right,
        other => MouseButton::Other(other as u16),
    }
}
