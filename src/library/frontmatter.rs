//! The `---` header a SKILL.md opens with, read as flat `key: value` pairs.
//!
//! Skills only use flat keys (`name`, `description`, `license`,
//! `allowed-tools`, and whatever an agent adds), so this reads those and keeps
//! the rest of the file as the body. Folded (`>`) and literal (`|`) values are
//! joined into one line, which is how they are shown.

use std::path::Path;

/// The keys of a header, in file order, and the text after it.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Frontmatter {
    pub keys: Vec<(String, String)>,
    pub body: String,
}

impl Frontmatter {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.keys
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

pub fn parse(text: &str) -> Frontmatter {
    let mut lines = text.split('\n');
    if lines.next().map(str::trim) != Some("---") {
        return Frontmatter {
            keys: Vec::new(),
            body: text.trim_matches('\n').to_string(),
        };
    }
    let mut keys: Vec<(String, String)> = Vec::new();
    let mut consumed = text.find('\n').map_or(text.len(), |i| i + 1);
    let mut closed = false;
    for line in lines.by_ref() {
        consumed += line.len() + 1;
        if line.trim() == "---" {
            closed = true;
            break;
        }
        let indented = line.starts_with(' ') || line.starts_with('\t');
        match line.split_once(':') {
            Some((key, value)) if !indented && !key.trim().is_empty() => {
                let mut value = value.trim().to_string();
                if matches!(value.as_str(), ">" | "|" | ">-" | "|-") {
                    value.clear();
                }
                keys.push((key.trim().to_string(), unquote(&value)));
            }
            _ => {
                if let Some((_, last)) = keys.last_mut() {
                    let more = line.trim();
                    if !more.is_empty() {
                        if !last.is_empty() {
                            last.push(' ');
                        }
                        last.push_str(more);
                    }
                }
            }
        }
    }
    if !closed {
        return Frontmatter {
            keys: Vec::new(),
            body: text.trim_matches('\n').to_string(),
        };
    }
    let body = text.get(consumed.min(text.len())..).unwrap_or("");
    Frontmatter {
        keys,
        body: body.trim_matches('\n').to_string(),
    }
}

fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && (bytes[0] == b'"' || bytes[0] == b'\'') && bytes[bytes.len() - 1] == bytes[0] {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}

/// A SKILL.md from its header pairs and body. A value is quoted only when YAML
/// would otherwise read it as something else.
pub fn compose(keys: &[(String, String)], body: &str) -> String {
    let mut out = String::from("---\n");
    for (key, value) in keys {
        out.push_str(key);
        out.push_str(": ");
        out.push_str(&quote(value));
        out.push('\n');
    }
    out.push_str("---\n\n");
    out.push_str(body.trim());
    out.push('\n');
    out
}

fn quote(value: &str) -> String {
    let first = value.chars().next();
    let needs = value.contains(": ")
        || value.contains(" #")
        || value.contains('\n')
        || value.ends_with(char::is_whitespace)
        || first.is_some_and(|c| c.is_whitespace() || "-?:,[]{}#&*!|>'\"%@`".contains(c));
    if needs {
        serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
    } else {
        value.to_string()
    }
}

/// `interface.display_name` from a skill's `agents/openai.yaml`, the name
/// Codex shows it by.
pub fn display_name(folder: &Path) -> Option<String> {
    let yaml = std::fs::read_to_string(folder.join("agents/openai.yaml")).ok()?;
    yaml.lines().find_map(|line| {
        let value = line.trim().strip_prefix("display_name:")?;
        let value = unquote(value.trim());
        (!value.is_empty()).then_some(value)
    })
}

/// A title for a skill: its own `# Heading` when that is the same words as
/// its name (it carries the author's casing, "GPUI Kit"), else the name with
/// each word capitalized and known acronyms upper-cased.
pub fn title(name: &str, body: &str) -> String {
    let heading = body
        .lines()
        .find_map(|line| line.strip_prefix("# "))
        .map(|h| h.trim().trim_end_matches('#').trim());
    if let Some(heading) = heading
        && slug(heading) == name.to_lowercase()
    {
        return heading.to_string();
    }
    const ACRONYMS: [&str; 10] = ["pr", "ui", "ux", "hig", "api", "cli", "mcp", "ios", "md", "ai"];
    name.split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|word| {
            if ACRONYMS.contains(&word.to_lowercase().as_str()) {
                word.to_uppercase()
            } else {
                let mut chars = word.chars();
                chars
                    .next()
                    .map(|c| c.to_uppercase().chain(chars).collect())
                    .unwrap_or_default()
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

/// A folder name from free text: lower case, words joined by `-`.
pub fn slug(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_flat_keys_and_body() {
        let fm = parse("---\nname: demo\ndescription: \"Says hi: twice\"\n---\n\n# Demo\n\nBody\n");
        assert_eq!(fm.get("name"), Some("demo"));
        assert_eq!(fm.get("description"), Some("Says hi: twice"));
        assert_eq!(fm.body, "# Demo\n\nBody");
    }

    #[test]
    fn joins_folded_values() {
        let fm = parse("---\ndescription: >\n  one\n  two\nname: x\n---\nbody");
        assert_eq!(fm.get("description"), Some("one two"));
        assert_eq!(fm.get("name"), Some("x"));
    }

    #[test]
    fn text_without_header_is_all_body() {
        let fm = parse("# Just text\n");
        assert!(fm.keys.is_empty());
        assert_eq!(fm.body, "# Just text");
    }

    #[test]
    fn compose_round_trips_and_quotes_only_when_needed() {
        let keys = vec![
            ("name".to_string(), "my-skill".to_string()),
            ("description".to_string(), "Use it: always".to_string()),
        ];
        let text = compose(&keys, "Hello");
        assert_eq!(text, "---\nname: my-skill\ndescription: \"Use it: always\"\n---\n\nHello\n");
        assert_eq!(parse(&text).keys, keys);
    }

    #[test]
    fn titles_prefer_matching_heading() {
        assert_eq!(title("gpui-kit", "# GPUI Kit\n"), "GPUI Kit");
        assert_eq!(title("gpui-kit", "# Something else\n"), "Gpui Kit");
        assert_eq!(title("babysit-pr", ""), "Babysit PR");
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("My  Great Skill!"), "my-great-skill");
    }
}
