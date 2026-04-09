//! Minimal TOML parser — handles only flat key=value pairs in [sections].
//! No arrays, inline tables, multiline strings, or escape sequences.

use std::collections::HashMap;

type Section = HashMap<String, TomlValue>;
type Sections = HashMap<String, Section>;

#[derive(Debug, Clone, PartialEq)]
pub enum TomlValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
}

/// Parsed TOML table: section → (key → value).
/// Keys with no section header go under the empty string "".
#[derive(Debug, Clone, Default)]
pub struct TomlTable {
    sections: Sections,
}

impl TomlTable {
    pub fn parse(input: &str) -> Result<Self, String> {
        let mut sections = Sections::new();
        let mut current_section = String::new();

        for (line_idx, raw_line) in input.lines().enumerate() {
            let line_num = line_idx + 1;
            let line = raw_line.trim();

            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some(section_body) = line.strip_prefix('[') {
                let Some(end) = section_body.find(']') else {
                    return Err(format!("line {}: unclosed section bracket", line_num));
                };

                let section = section_body[..end].trim();
                if section.is_empty() {
                    return Err(format!("line {}: empty section name", line_num));
                }

                let trailing = section_body[end + 1..].trim_start();
                if !trailing.is_empty() && !trailing.starts_with('#') {
                    return Err(format!(
                        "line {}: trailing content after section header",
                        line_num
                    ));
                }

                current_section.clear();
                current_section.push_str(section);
                sections.entry(current_section.clone()).or_default();
                continue;
            }

            let Some((key_part, value_part)) = line.split_once('=') else {
                return Err(format!("line {}: expected key = value", line_num));
            };

            let key = key_part.trim();
            if key.is_empty() {
                return Err(format!("line {}: empty key", line_num));
            }

            let value = parse_value(value_part.trim())
                .map_err(|err| format!("line {}: {}", line_num, err))?;
            sections
                .entry(current_section.clone())
                .or_default()
                .insert(key.to_string(), value);
        }

        Ok(Self { sections })
    }

    fn get(&self, section: &str, key: &str) -> Option<&TomlValue> {
        self.sections.get(section)?.get(key)
    }

    pub fn get_str(&self, section: &str, key: &str) -> Option<&str> {
        match self.get(section, key)? {
            TomlValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn get_f64(&self, section: &str, key: &str) -> Option<f64> {
        match self.get(section, key)? {
            TomlValue::Float(f) => Some(*f),
            TomlValue::Integer(i) => Some(*i as f64),
            _ => None,
        }
    }

    pub fn get_f32(&self, section: &str, key: &str) -> Option<f32> {
        self.get_f64(section, key).map(|f| f as f32)
    }

    pub fn get_i64(&self, section: &str, key: &str) -> Option<i64> {
        match self.get(section, key)? {
            TomlValue::Integer(i) => Some(*i),
            _ => None,
        }
    }

    pub fn get_usize(&self, section: &str, key: &str) -> Option<usize> {
        self.get_i64(section, key)
            .and_then(|i| usize::try_from(i).ok())
    }

    pub fn get_bool(&self, section: &str, key: &str) -> Option<bool> {
        match self.get(section, key)? {
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
    let value = if let Some(string_body) = raw.strip_prefix('"') {
        let Some(end) = string_body.find('"') else {
            return Err(format!("unclosed string: {}", raw));
        };

        let trailing = string_body[end + 1..].trim_start();
        if !trailing.is_empty() && !trailing.starts_with('#') {
            return Err(format!("cannot parse value: {}", raw));
        }

        return Ok(TomlValue::String(string_body[..end].to_string()));
    } else if let Some(hash_pos) = raw.find('#') {
        raw[..hash_pos].trim()
    } else {
        raw
    };

    if value == "true" {
        return Ok(TomlValue::Boolean(true));
    }
    if value == "false" {
        return Ok(TomlValue::Boolean(false));
    }

    if value.contains('.') {
        if let Ok(float) = value.parse::<f64>() {
            return Ok(TomlValue::Float(float));
        }
    }

    if let Ok(integer) = value.parse::<i64>() {
        return Ok(TomlValue::Integer(integer));
    }

    Err(format!("cannot parse value: {}", value))
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
    fn negative_integer_does_not_wrap_to_usize() {
        let input = "[scrollback]\nlines = -1\n";
        let table = TomlTable::parse(input).unwrap();
        assert_eq!(table.get_i64("scrollback", "lines"), Some(-1));
        assert_eq!(table.get_usize("scrollback", "lines"), None);
    }

    #[test]
    fn parse_comments() {
        let input = "# comment\n[s]\nk = \"v\" # inline comment\n";
        let table = TomlTable::parse(input).unwrap();
        assert_eq!(table.get_str("s", "k"), Some("v"));
    }

    #[test]
    fn reject_trailing_garbage_after_quoted_string() {
        let err = TomlTable::parse("[s]\nk = \"v\" garbage\n").unwrap_err();
        assert!(err.contains("cannot parse value"));
    }

    #[test]
    fn allow_whitespace_and_comment_after_quoted_string() {
        let input = "[s]\nk = \"v\"   # ok\n";
        let table = TomlTable::parse(input).unwrap();
        assert_eq!(table.get_str("s", "k"), Some("v"));
    }

    #[test]
    fn reject_trailing_garbage_after_section_header() {
        let err = TomlTable::parse("[s] nope\nk = 1\n").unwrap_err();
        assert!(err.contains("trailing content after section header"));
    }

    #[test]
    fn reject_empty_key() {
        let err = TomlTable::parse("[s]\n= 1\n").unwrap_err();
        assert!(err.contains("empty key"));
    }

    #[test]
    fn reject_unrecognized_non_assignment_line() {
        let err = TomlTable::parse("[s]\nthis is not valid\n").unwrap_err();
        assert!(err.contains("expected key = value"));
    }
}
