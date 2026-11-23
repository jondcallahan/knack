//! The `---` header a SKILL.md opens with, read as flat `key: value` pairs.
//!
//! Skills only use flat keys (`name`, `description`, `license`,
//! `allowed-tools`, and whatever an agent adds), so this reads those and keeps
//! the rest of the file as the body. Folded (`>`) and literal (`|`) values are
//! joined into one line, which is how they are shown.

use std::path::Path;

/// The keys of a header, in file order, the raw top-level blocks, and the text after it.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Frontmatter {
    pub keys: Vec<(String, String)>,
    pub raw_entries: Vec<(String, String)>,
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
    if lines.next().map(str::trim_end) != Some("---") {
        return Frontmatter {
            keys: Vec::new(),
            raw_entries: Vec::new(),
            body: text.trim_matches('\n').to_string(),
        };
    }
    let mut keys: Vec<(String, String)> = Vec::new();
    let mut raw_entries: Vec<(String, String)> = Vec::new();
    let mut consumed = text.find('\n').map_or(text.len(), |i| i + 1);
    let mut closed = false;
    for line in lines.by_ref() {
        consumed += line.len() + 1;
        let indented = line.starts_with(' ') || line.starts_with('\t');
        if !indented && line.trim_end() == "---" {
            closed = true;
            break;
        }
        match line.split_once(':') {
            Some((key, value)) if !indented && !key.trim().is_empty() => {
                let key = key.trim().to_string();
                let mut value = value.trim().to_string();
                if matches!(value.as_str(), ">" | "|" | ">-" | "|-") {
                    value.clear();
                }
                keys.push((key.clone(), unquote(&value)));
                raw_entries.push((key, line.trim_end_matches('\r').to_string()));
            }
            _ => {
                if let Some((_, raw)) = raw_entries.last_mut() {
                    raw.push('\n');
                    raw.push_str(line.trim_end_matches('\r'));
                }
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
            raw_entries: Vec::new(),
            body: text.trim_matches('\n').to_string(),
        };
    }
    let body = text.get(consumed.min(text.len())..).unwrap_or("");
    Frontmatter {
        keys,
        raw_entries,
        body: body.trim_matches('\n').to_string(),
    }
}

fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"' {
        serde_json::from_str::<String>(value)
            .unwrap_or_else(|_| value[1..value.len() - 1].to_string())
    } else if bytes.len() >= 2 && bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\'' {
        value[1..value.len() - 1].replace("''", "'")
    } else {
        value.to_string()
    }
}

/// A SKILL.md from its header pairs and body. Form-owned values are serialized
/// as quoted YAML strings.
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

/// Composes `SKILL.md` with form-owned keys plus any untouched top-level YAML
/// blocks (such as nested `metadata` mappings) preserved verbatim.
pub fn compose_preserving(
    owned_keys: &[(String, String)],
    original: &Frontmatter,
    body: &str,
    skip_keys: &[&str],
) -> String {
    let mut out = String::from("---\n");
    for (key, value) in owned_keys {
        out.push_str(key);
        out.push_str(": ");
        out.push_str(&quote(value));
        out.push('\n');
    }
    for (key, raw_block) in &original.raw_entries {
        if !skip_keys.contains(&key.as_str()) {
            out.push_str(raw_block);
            out.push('\n');
        }
    }
    out.push_str("---\n\n");
    out.push_str(body.trim());
    out.push('\n');
    out
}

fn quote(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| format!("\"{value}\""))
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

/// A folder and Agent Skills `name` from free text: lower-case ASCII
/// alphanumeric words joined by `-`.
pub fn slug(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
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
    fn compose_round_trips_quoted_yaml_strings() {
        let keys = vec![
            ("name".to_string(), "my-skill".to_string()),
            ("description".to_string(), "Use it: always".to_string()),
        ];
        let text = compose(&keys, "Hello");
        assert_eq!(
            text,
            "---\nname: \"my-skill\"\ndescription: \"Use it: always\"\n---\n\nHello\n"
        );
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

    #[test]
    fn preserves_nested_yaml_mappings_when_composing() {
        let src = "---\nname: My Skill\ndescription: Old\nmetadata:\n  author: acme\n  version: \"1.0\"\n---\n\n# Body\n";
        let fm = parse(src);
        let out = compose_preserving(
            &[
                ("name".into(), "my-skill".into()),
                ("description".into(), "New".into()),
            ],
            &fm,
            &fm.body,
            &["name", "description", "license", "allowed-tools"],
        );
        assert!(out.contains("name: \"my-skill\"\n"));
        assert!(out.contains("description: \"New\"\n"));
        assert!(out.contains("metadata:\n  author: acme\n  version: \"1.0\"\n"));
    }

    #[test]
    fn quotes_yaml_booleans_numbers_nulls_and_dates() {
        let keys = vec![
            ("name".into(), "123".into()),
            ("description".into(), "true".into()),
            ("amount".into(), "1_000.00".into()),
            ("other".into(), "2026-11-23".into()),
            ("safe".into(), "my-skill".into()),
        ];
        let out = compose(&keys, "Body");
        assert!(out.contains("name: \"123\"\n"));
        assert!(out.contains("description: \"true\"\n"));
        assert!(out.contains("amount: \"1_000.00\"\n"));
        assert!(out.contains("other: \"2026-11-23\"\n"));
        assert!(out.contains("safe: \"my-skill\"\n"));
        assert_eq!(parse(&out).keys, keys);
    }

    #[test]
    fn ignores_indented_dashes_inside_block_scalar() {
        let src = "---\nname: demo\nmetadata:\n  notes: |\n    line 1\n    ---\n    line 2\n---\n\n# Real body\n";
        let fm = parse(src);
        assert_eq!(fm.body, "# Real body");
        assert!(fm.raw_entries.iter().any(|(k, v)| k == "metadata" && v.contains("line 2")));
    }

    #[test]
    fn recognizes_top_level_closing_dashes_with_trailing_whitespace() {
        for src in [
            "---\nname: demo\ndescription: hi\n---   \n\n# Body\n",
            "--- \t\r\nname: demo\ndescription: hi\n--- \t\r\n\n# Body\n",
        ] {
            let fm = parse(src);
            assert_eq!(fm.get("name"), Some("demo"));
            assert_eq!(fm.get("description"), Some("hi"));
            assert_eq!(fm.body, "# Body");
        }
    }
}
