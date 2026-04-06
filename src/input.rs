//! Platform-neutral input types.
//! Backend translate Wayland/X11/winit events into these.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl Modifiers {
    #[inline]
    pub const fn empty() -> Self {
        Self {
            shift: false,
            ctrl: false,
            alt: false,
        }
    }

    #[inline]
    pub const fn shift_key(self) -> bool {
        self.shift
    }

    #[inline]
    pub const fn control_key(self) -> bool {
        self.ctrl
    }

    #[inline]
    pub const fn alt_key(self) -> bool {
        self.alt
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    Text(String),
    Enter,
    Backspace,
    Escape,
    Tab,
    Space,
    ArrowUp,
    ArrowDown,
    ArrowRight,
    ArrowLeft,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Insert,
    F(u8),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    pub pressed: bool,
    pub key: Key,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    Other(u16),
}
