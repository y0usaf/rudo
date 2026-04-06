//! Minimal TOML parser — handles only flat key=value pairs in [sections].
//! No arrays, inline tables, multiline strings, or escape sequences.

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum TomlValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
}

/// Parsed TOML table: section → (key → value).
/// Keys with no section header go under the empty string "".
#[derive(Debug)]
pub struct TomlTable {
    sections: HashMap<String, HashMap<String, TomlValue>>,
}

impl TomlTable {
    pub fn parse(input: &str) -> Result<Self, String> {
        let mut sections: HashMap<String, HashMap<String, TomlValue>> = HashMap::new();
        let mut current_section = String::new();

        for (line_num, raw_line) in input.lines().enumerate() {
            let line = raw_line.trim();

            // Skip empty lines and comments
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            // Section header
            if line.starts_with('[') {
                if let Some(end) = line.find(']') {
                    current_section = line[1..end].trim().to_string();
                    sections.entry(current_section.clone()).or_default();
                    continue;
                } else {
                    return Err(format!("line {}: unclosed section bracket", line_num + 1));
                }
            }

            // Key = value
            if let Some(eq_pos) = line.find('=') {
                let key = line[..eq_pos].trim().to_string();
                let val_part = line[eq_pos + 1..].trim();

                let value = parse_value(val_part)?;
                sections
                    .entry(current_section.clone())
                    .or_default()
                    .insert(key, value);
            }
            // Ignore lines that don't match
        }

        Ok(TomlTable { sections })
    }

    pub fn get_str(&self, section: &str, key: &str) -> Option<&str> {
        match self.sections.get(section)?.get(key)? {
            TomlValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn get_f64(&self, section: &str, key: &str) -> Option<f64> {
        match self.sections.get(section)?.get(key)? {
            TomlValue::Float(f) => Some(*f),
            TomlValue::Integer(i) => Some(*i as f64),
            _ => None,
        }
    }

    pub fn get_f32(&self, section: &str, key: &str) -> Option<f32> {
        self.get_f64(section, key).map(|f| f as f32)
    }

    pub fn get_i64(&self, section: &str, key: &str) -> Option<i64> {
        match self.sections.get(section)?.get(key)? {
            TomlValue::Integer(i) => Some(*i),
            _ => None,
        }
    }

    pub fn get_usize(&self, section: &str, key: &str) -> Option<usize> {
        self.get_i64(section, key).map(|i| i as usize)
    }

    pub fn get_bool(&self, section: &str, key: &str) -> Option<bool> {
        match self.sections.get(section)?.get(key)? {
            TomlValue::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    /// Get string from flat (no section / root) table.
    pub fn get_str_flat(&self, key: &str) -> Option<&str> {
        self.get_str("", key)
    }
}

fn parse_value(raw: &str) -> Result<TomlValue, String> {
    // Strip inline comment (but not inside strings)
    let val = if raw.starts_with('"') {
        raw // handle below
    } else if let Some(hash_pos) = raw.find('#') {
        raw[..hash_pos].trim()
    } else {
        raw
    };

    // Quoted string
    if val.starts_with('"') {
        if let Some(end) = val[1..].find('"') {
            return Ok(TomlValue::String(val[1..1 + end].to_string()));
        }
        return Err(format!("unclosed string: {}", val));
    }

    // Boolean
    if val == "true" {
        return Ok(TomlValue::Boolean(true));
    }
    if val == "false" {
        return Ok(TomlValue::Boolean(false));
    }

    // Float (contains a dot)
    if val.contains('.') {
        if let Ok(f) = val.parse::<f64>() {
            return Ok(TomlValue::Float(f));
        }
    }

    // Integer
    if let Ok(i) = val.parse::<i64>() {
        return Ok(TomlValue::Integer(i));
    }

    Err(format!("cannot parse value: {}", val))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let input = r##"
[font]
family = "monospace"
size = 14.0
bold_is_bright = false

[colors]
foreground = "#d4d4d4"
background = "#1e1e1e"
"##;
        let table = TomlTable::parse(input).unwrap();
        assert_eq!(table.get_str("font", "family"), Some("monospace"));
        assert_eq!(table.get_f32("font", "size"), Some(14.0));
        assert_eq!(table.get_bool("font", "bold_is_bright"), Some(false));
        assert_eq!(table.get_str("colors", "foreground"), Some("#d4d4d4"));
    }

    #[test]
    fn parse_flat() {
        let input = r##"
background = "#282828"
foreground = "#ebdbb2"
color0 = "#282828"
"##;
        let table = TomlTable::parse(input).unwrap();
        assert_eq!(table.get_str_flat("background"), Some("#282828"));
        assert_eq!(table.get_str_flat("foreground"), Some("#ebdbb2"));
        assert_eq!(table.get_str_flat("color0"), Some("#282828"));
    }

    #[test]
    fn parse_integers() {
        let input = "[scrollback]\nlines = 10000\n";
        let table = TomlTable::parse(input).unwrap();
        assert_eq!(table.get_i64("scrollback", "lines"), Some(10000));
        assert_eq!(table.get_usize("scrollback", "lines"), Some(10000));
    }

    #[test]
    fn parse_comments() {
        let input = "# comment\n[s]\nk = \"v\" # inline comment\n";
        let table = TomlTable::parse(input).unwrap();
        assert_eq!(table.get_str("s", "k"), Some("v"));
    }
}
