//! Edit top-level keys in a scene's YAML frontmatter without disturbing the
//! rest of it: other keys, their order, blank lines and comments survive.
//!
//! Values are written as JSON, which is valid YAML flow syntax, so strings are
//! always quoted and lists/maps stay on one line: `tags: ["a", "b"]`.

use serde_json::Value;

/// Index range of the lines that make up `key`'s entry, if present.
fn find_key(lines: &[&str], key: &str) -> Option<(usize, usize)> {
    let prefix = format!("{key}:");
    let start = lines.iter().position(|l| l.starts_with(&prefix))?;
    let mut end = start + 1;
    while end < lines.len() {
        let l = lines[end];
        let continuation = l.starts_with(' ') || l.starts_with('\t') || l.starts_with("- ") || l == "-";
        if !continuation {
            break;
        }
        end += 1;
    }
    Some((start, end))
}

/// The ` # comment` after a plain scalar (`duration: null  # minutes`), if any.
fn trailing_comment(line: &str) -> Option<&str> {
    let after_colon = &line[line.find(':')? + 1..];
    let hash = after_colon.find(" #")?;
    let value = after_colon[..hash].trim();
    let plain = value.is_empty()
        || value == "null"
        || value == "~"
        || value == "true"
        || value == "false"
        || value.parse::<f64>().is_ok();
    if plain {
        // keep the original spacing before '#'
        let ws_start = after_colon[..hash].trim_end().len();
        Some(&after_colon[ws_start..])
    } else {
        None
    }
}

/// Set (or add) `key` to `value`. Setting an absent key to `null` is a no-op.
pub fn set_key(frontmatter: &str, key: &str, value: &Value) -> String {
    let encoded = serde_json::to_string(value).expect("JSON values always serialise");
    let lines: Vec<&str> = frontmatter.lines().collect();
    if value.is_null() && find_key(&lines, key).is_none() {
        return frontmatter.to_string();
    }
    replace_entry(frontmatter, key, |old| {
        let comment = old
            .filter(|o| o.len() == 1)
            .and_then(|o| trailing_comment(o[0]))
            .unwrap_or("");
        vec![format!("{key}: {encoded}{comment}")]
    })
}

/// Set (or add) `key` to a block list, one flow-style item per line:
///
/// ```yaml
/// episodes:
///   - {"number": 1, "title": "Pilot"}
/// ```
///
/// Used for lists people read and extend by hand, which a one-line list
/// would make awkward.
pub fn set_key_list(frontmatter: &str, key: &str, items: &[Value]) -> String {
    if items.is_empty() {
        return set_key(frontmatter, key, &Value::Array(vec![]));
    }
    replace_entry(frontmatter, key, |_| {
        std::iter::once(format!("{key}:"))
            .chain(items.iter().map(|v| format!("  - {}", flow(v))))
            .collect()
    })
}

/// A map as `{key: value, ...}` with unquoted keys; anything else as JSON.
fn flow(v: &Value) -> String {
    match v {
        Value::Object(m) => {
            let parts: Vec<String> = m
                .iter()
                .map(|(k, v)| {
                    let plain_key = !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
                    let k = if plain_key {
                        k.clone()
                    } else {
                        serde_json::to_string(k).unwrap()
                    };
                    format!("{k}: {}", serde_json::to_string(v).unwrap())
                })
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
        _ => serde_json::to_string(v).unwrap(),
    }
}

/// Replace `key`'s entry (or append one) with the lines `make` returns; it is
/// given the entry's current lines, if any.
fn replace_entry(frontmatter: &str, key: &str, make: impl FnOnce(Option<&[&str]>) -> Vec<String>) -> String {
    let had_trailing_newline = frontmatter.ends_with('\n') || frontmatter.is_empty();
    let lines: Vec<&str> = frontmatter.lines().collect();
    let mut out: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
    match find_key(&lines, key) {
        Some((start, end)) => {
            let new = make(Some(&lines[start..end]));
            out.splice(start..end, new);
        }
        None => out.extend(make(None)),
    }
    let mut s = out.join("\n");
    if had_trailing_newline {
        s.push('\n');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const FM: &str = "id: sc-1\ntitle: \"Old\"\n# a comment\nduration: null  # manual override, minutes\ntags:\n- a\n- b\nstatus: draft\n";

    #[test]
    fn replaces_scalar_and_keeps_everything_else() {
        let out = set_key(FM, "title", &json!("New title"));
        assert_eq!(out, FM.replace("\"Old\"", "\"New title\""));
    }

    #[test]
    fn keeps_trailing_comment_on_plain_scalar() {
        let out = set_key(FM, "duration", &json!(4.5));
        assert!(out.contains("duration: 4.5  # manual override, minutes\n"), "{out}");
    }

    #[test]
    fn replaces_block_list_with_flow_list() {
        let out = set_key(FM, "tags", &json!(["x"]));
        assert!(out.contains("tags: [\"x\"]\nstatus: draft\n"), "{out}");
        assert!(!out.contains("- a"));
    }

    #[test]
    fn appends_missing_key_and_ignores_absent_null() {
        let out = set_key(FM, "location", &json!("lighthouse"));
        assert!(out.ends_with("status: draft\nlocation: \"lighthouse\"\n"));
        assert_eq!(set_key(FM, "synopsis", &Value::Null), FM);
    }

    #[test]
    fn multiline_strings_stay_valid_yaml() {
        let out = set_key(FM, "notes", &json!("line one\nline \"two\""));
        let v: serde_yaml::Value = serde_yaml::from_str(&out).unwrap();
        assert_eq!(v["notes"].as_str(), Some("line one\nline \"two\""));
    }

    #[test]
    fn block_list_replaces_flow_or_block_and_parses() {
        let fm = "title: x\nepisodes:\n  - {number: 1, title: \"One\"}\ndraft: 2\n";
        let items = [
            json!({"number": 1, "title": "One"}),
            json!({"number": 2, "title": "Two: \"B\""}),
        ];
        let out = set_key_list(fm, "episodes", &items);
        assert_eq!(
            out,
            "title: x\nepisodes:\n  - {number: 1, title: \"One\"}\n  - {number: 2, title: \"Two: \\\"B\\\"\"}\ndraft: 2\n"
        );
        let v: serde_yaml::Value = serde_yaml::from_str(&out).unwrap();
        assert_eq!(v["episodes"][1]["title"].as_str(), Some("Two: \"B\""));
        let flow = set_key_list("episodes: []\n", "episodes", &items[..1]);
        assert_eq!(flow, "episodes:\n  - {number: 1, title: \"One\"}\n");
    }

    #[test]
    fn similar_prefix_is_not_matched() {
        let fm = "title_alt: \"x\"\ntitle: \"y\"\n";
        assert_eq!(set_key(fm, "title", &json!("z")), "title_alt: \"x\"\ntitle: \"z\"\n");
    }
}
