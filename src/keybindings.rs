//! Configurable local keybindings for terminal actions.

use crate::input::{Key, KeyEvent, Modifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalAction {
    Copy,
    Paste,
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

#[derive(Debug, Clone)]
pub struct KeybindingsConfig {
    pub copy: Vec<KeyBinding>,
    pub paste: Vec<KeyBinding>,
    pub zoom_in: Vec<KeyBinding>,
    pub zoom_out: Vec<KeyBinding>,
    pub zoom_reset: Vec<KeyBinding>,
}

impl Default for KeybindingsConfig {
    fn default() -> Self {
        Self {
            copy: parse_binding_list("ctrl+shift+c").expect("default copy binding must parse"),
            paste: parse_binding_list("ctrl+shift+v").expect("default paste binding must parse"),
            zoom_in: parse_binding_list("ctrl+=, ctrl+plus")
                .expect("default zoom_in binding must parse"),
            zoom_out: parse_binding_list("ctrl+-, ctrl+minus")
                .expect("default zoom_out binding must parse"),
            zoom_reset: parse_binding_list("ctrl+0")
                .expect("default zoom_reset binding must parse"),
        }
    }
}

impl KeybindingsConfig {
    pub fn matches(&self, action: LocalAction, event: &KeyEvent, modifiers: Modifiers) -> bool {
        let bindings = match action {
            LocalAction::Copy => &self.copy,
            LocalAction::Paste => &self.paste,
            LocalAction::ZoomIn => &self.zoom_in,
            LocalAction::ZoomOut => &self.zoom_out,
            LocalAction::ZoomReset => &self.zoom_reset,
        };

        bindings
            .iter()
            .any(|binding| binding.matches(event, modifiers))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    modifiers: BindingModifiers,
    key: BindingKey,
}

impl KeyBinding {
    pub fn parse(spec: &str) -> Result<Self, String> {
        let spec = spec.trim();
        if spec.is_empty() {
            return Err("empty keybinding".to_string());
        }

        let parts: Vec<_> = spec.split('+').map(str::trim).collect();
        if parts.is_empty() {
            return Err("empty keybinding".to_string());
        }

        let key_part = parts.last().copied().unwrap_or_default();
        if key_part.is_empty() {
            return Err(format!(
                "missing key in binding '{spec}' (use names like 'plus' for '+')"
            ));
        }

        let mut modifiers = BindingModifiers::default();
        for modifier in &parts[..parts.len().saturating_sub(1)] {
            if modifier.is_empty() {
                return Err(format!(
                    "invalid modifier in binding '{spec}' (use names like 'plus' for '+')"
                ));
            }
            modifiers.apply(modifier)?;
        }

        Ok(Self {
            modifiers,
            key: BindingKey::parse(key_part)?,
        })
    }

    pub fn matches(&self, event: &KeyEvent, modifiers: Modifiers) -> bool {
        event.pressed
            && self.key.matches(&event.key)
            && self
                .modifiers
                .matches(modifiers, self.key.allows_implicit_shift())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct BindingModifiers {
    ctrl: bool,
    shift: bool,
    alt: bool,
}

impl BindingModifiers {
    fn apply(&mut self, modifier: &str) -> Result<(), String> {
        match modifier.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => self.ctrl = true,
            "shift" => self.shift = true,
            "alt" => self.alt = true,
            other => return Err(format!("unknown modifier '{other}'")),
        }
        Ok(())
    }

    fn matches(&self, modifiers: Modifiers, allow_extra_shift: bool) -> bool {
        self.ctrl == modifiers.ctrl
            && self.alt == modifiers.alt
            && (self.shift == modifiers.shift
                || (allow_extra_shift && !self.shift && modifiers.shift))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BindingKey {
    Text(String),
    Named(NamedKey),
}

impl BindingKey {
    fn parse(token: &str) -> Result<Self, String> {
        let token = token.trim();
        let lower = token.to_ascii_lowercase();

        let named = match lower.as_str() {
            "escape" | "esc" => Some(NamedKey::Escape),
            "enter" | "return" => Some(NamedKey::Enter),
            "backspace" => Some(NamedKey::Backspace),
            "tab" => Some(NamedKey::Tab),
            "space" => Some(NamedKey::Space),
            "up" | "arrowup" => Some(NamedKey::ArrowUp),
            "down" | "arrowdown" => Some(NamedKey::ArrowDown),
            "left" | "arrowleft" => Some(NamedKey::ArrowLeft),
            "right" | "arrowright" => Some(NamedKey::ArrowRight),
            "home" => Some(NamedKey::Home),
            "end" => Some(NamedKey::End),
            "pageup" | "pgup" => Some(NamedKey::PageUp),
            "pagedown" | "pgdown" => Some(NamedKey::PageDown),
            "delete" | "del" => Some(NamedKey::Delete),
            "insert" | "ins" => Some(NamedKey::Insert),
            _ => None,
        };
        if let Some(named) = named {
            return Ok(Self::Named(named));
        }

        if let Some(function) = parse_function_key(&lower) {
            return Ok(Self::Named(NamedKey::Function(function)));
        }

        let text = match lower.as_str() {
            "plus" => "+",
            "minus" => "-",
            "equal" | "equals" => "=",
            "comma" => ",",
            "period" | "dot" => ".",
            "slash" => "/",
            "backslash" => "\\",
            "semicolon" => ";",
            "apostrophe" | "quote" => "'",
            "grave" | "backtick" => "`",
            "leftbracket" | "lbracket" => "[",
            "rightbracket" | "rbracket" => "]",
            _ => token,
        };

        if text.chars().count() == 1 {
            return Ok(Self::Text(normalize_text_key(text)));
        }

        Err(format!("unknown key '{token}'"))
    }

    fn allows_implicit_shift(&self) -> bool {
        matches!(self, Self::Text(text) if text.chars().count() == 1
            && !text.chars().next().unwrap_or_default().is_ascii_alphabetic())
    }

    fn matches(&self, key: &Key) -> bool {
        match (self, key) {
            (Self::Text(expected), Key::Text(actual)) => normalize_text_key(actual) == *expected,
            (Self::Named(NamedKey::Escape), Key::Escape)
            | (Self::Named(NamedKey::Enter), Key::Enter)
            | (Self::Named(NamedKey::Backspace), Key::Backspace)
            | (Self::Named(NamedKey::Tab), Key::Tab)
            | (Self::Named(NamedKey::Space), Key::Space)
            | (Self::Named(NamedKey::ArrowUp), Key::ArrowUp)
            | (Self::Named(NamedKey::ArrowDown), Key::ArrowDown)
            | (Self::Named(NamedKey::ArrowLeft), Key::ArrowLeft)
            | (Self::Named(NamedKey::ArrowRight), Key::ArrowRight)
            | (Self::Named(NamedKey::Home), Key::Home)
            | (Self::Named(NamedKey::End), Key::End)
            | (Self::Named(NamedKey::PageUp), Key::PageUp)
            | (Self::Named(NamedKey::PageDown), Key::PageDown)
            | (Self::Named(NamedKey::Delete), Key::Delete)
            | (Self::Named(NamedKey::Insert), Key::Insert) => true,
            (Self::Named(NamedKey::Function(expected)), Key::F(actual)) => expected == actual,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NamedKey {
    Escape,
    Enter,
    Backspace,
    Tab,
    Space,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Insert,
    Function(u8),
}

fn parse_function_key(token: &str) -> Option<u8> {
    let suffix = token.strip_prefix('f')?;
    let num = suffix.parse::<u8>().ok()?;
    (1..=12).contains(&num).then_some(num)
}

fn normalize_text_key(text: &str) -> String {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(ch), None) if ch.is_ascii_alphabetic() => ch.to_ascii_lowercase().to_string(),
        _ => text.to_string(),
    }
}

pub fn parse_binding_list(spec: &str) -> Result<Vec<KeyBinding>, String> {
    let mut bindings = Vec::new();
    for binding in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        bindings.push(KeyBinding::parse(binding)?);
    }

    if bindings.is_empty() {
        return Err("expected at least one keybinding".to_string());
    }

    Ok(bindings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(ctrl: bool, shift: bool, alt: bool) -> Modifiers {
        Modifiers { ctrl, shift, alt }
    }

    #[test]
    fn parses_basic_binding() {
        let binding = KeyBinding::parse("ctrl+shift+c").unwrap();
        assert!(binding.matches(
            &KeyEvent {
                pressed: true,
                key: Key::Text("C".to_string()),
            },
            mods(true, true, false)
        ));
        assert!(!binding.matches(
            &KeyEvent {
                pressed: true,
                key: Key::Text("c".to_string()),
            },
            mods(true, false, false)
        ));
    }

    #[test]
    fn parses_named_symbol_aliases() {
        let plus = KeyBinding::parse("ctrl+plus").unwrap();
        assert!(plus.matches(
            &KeyEvent {
                pressed: true,
                key: Key::Text("+".to_string()),
            },
            mods(true, true, false)
        ));

        let minus = KeyBinding::parse("ctrl+minus").unwrap();
        assert!(minus.matches(
            &KeyEvent {
                pressed: true,
                key: Key::Text("-".to_string()),
            },
            mods(true, false, false)
        ));
    }

    #[test]
    fn parses_multiple_bindings() {
        let bindings = parse_binding_list("ctrl+=, ctrl+plus").unwrap();
        assert_eq!(bindings.len(), 2);
    }

    #[test]
    fn supports_named_keys() {
        let binding = KeyBinding::parse("alt+pageup").unwrap();
        assert!(binding.matches(
            &KeyEvent {
                pressed: true,
                key: Key::PageUp,
            },
            mods(false, false, true)
        ));
    }

    #[test]
    fn rejects_missing_key() {
        assert!(KeyBinding::parse("ctrl+").is_err());
    }

    #[test]
    fn default_config_matches_existing_shortcuts() {
        let bindings = KeybindingsConfig::default();
        assert!(bindings.matches(
            LocalAction::Copy,
            &KeyEvent {
                pressed: true,
                key: Key::Text("c".to_string()),
            },
            mods(true, true, false)
        ));
        assert!(bindings.matches(
            LocalAction::Paste,
            &KeyEvent {
                pressed: true,
                key: Key::Text("v".to_string()),
            },
            mods(true, true, false)
        ));
        assert!(bindings.matches(
            LocalAction::ZoomIn,
            &KeyEvent {
                pressed: true,
                key: Key::Text("=".to_string()),
            },
            mods(true, false, false)
        ));
        assert!(bindings.matches(
            LocalAction::ZoomIn,
            &KeyEvent {
                pressed: true,
                key: Key::Text("+".to_string()),
            },
            mods(true, true, false)
        ));
    }
}
