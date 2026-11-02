//! Characters and places across a project: renaming them in scene text, and
//! editing the registries (`characters.yaml`, `locations.yaml`, `songs.yaml`),
//! which are YAML lists written one entry per line.
//!
//! Everything here is a pure text transform, so the same code serves edits on
//! disk and edits to a co-written project's shared text.

use crate::scene::{Element, parse_body, serialize_body};
use serde_json::{Map, Value};

/// What a rename changed in one scene body.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct RenameCount {
    /// Character cues (`HETTIE`, `TOBIAS & HETTIE`, `@HETTIE`).
    pub cues: usize,
    /// Singers in `~ SONG "…" [HETTIE]` lines.
    pub songs: usize,
    /// Mentions in stage directions / action (`HETTIE enters.`).
    pub mentions: usize,
    /// Scene headings whose location changed.
    pub headings: usize,
}

impl RenameCount {
    pub fn total(&self) -> usize {
        self.cues + self.songs + self.mentions + self.headings
    }
    pub fn add(&mut self, o: RenameCount) {
        self.cues += o.cues;
        self.songs += o.songs;
        self.mentions += o.mentions;
        self.headings += o.headings;
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Replace whole-word, exact-case occurrences of `old` in `text`.
/// `HETTIE` matches in `HETTIE’s` and `(HETTIE)`, not in `HETTIES`.
pub fn replace_word(text: &str, old: &str, new: &str) -> (String, usize) {
    if old.is_empty() {
        return (text.to_string(), 0);
    }
    let mut out = String::with_capacity(text.len());
    let mut n = 0;
    let mut rest = text;
    let mut prev: Option<char> = None;
    while let Some(i) = rest.find(old) {
        let before = rest[..i].chars().next_back().or(prev);
        let after = rest[i + old.len()..].chars().next();
        let starts_ok = !before.is_some_and(is_word_char) || !old.starts_with(is_word_char);
        let ends_ok = !after.is_some_and(is_word_char) || !old.ends_with(is_word_char);
        if starts_ok && ends_ok {
            out.push_str(&rest[..i]);
            out.push_str(new);
            n += 1;
        } else {
            out.push_str(&rest[..i + old.len()]);
        }
        prev = rest[..i + old.len()].chars().next_back();
        rest = &rest[i + old.len()..];
    }
    out.push_str(rest);
    (out, n)
}

/// Is `name` usable as a character cue? Upper case, no markup characters.
pub fn valid_cue_name(name: &str) -> Result<(), String> {
    let t = name.trim();
    if t.is_empty() {
        return Err("a character needs a name".into());
    }
    if t != name {
        return Err("no spaces at the start or end".into());
    }
    if t.chars().any(|c| c.is_lowercase()) {
        return Err("character cues are written in capitals".into());
    }
    if !t.chars().any(|c| c.is_alphabetic()) {
        return Err("a cue needs at least one letter".into());
    }
    if t.starts_with(['(', '~', '>', '!', '#', '@', '.']) || t.contains(['&', '^', '\n', '[', ']']) || t.ends_with(')')
    {
        return Err("a cue can't contain & ^ [ ] or start with ( ~ > ! # @ .".into());
    }
    if t.len() > 60 {
        return Err("that name is too long for a cue".into());
    }
    Ok(())
}

/// Rename a character in a scene body: cues, song singers, and mentions in
/// directions. Dialogue is left alone (people say "Hettie", not the cue).
/// Fails if the result would read differently, element by element.
pub fn rename_character_in_body(body: &str, old: &str, new: &str) -> Result<(String, RenameCount), String> {
    let mut els = parse_body(body);
    let mut count = RenameCount::default();
    for el in els.iter_mut() {
        match el {
            Element::Cue(c) => {
                let (raw, n) = replace_word(&c.raw, old, new);
                if n > 0 {
                    c.raw = raw;
                    count.cues += 1;
                }
            }
            Element::SongStart(s) => {
                if let Some(open) = s.raw.rfind('[') {
                    let (singers, n) = replace_word(&s.raw[open..], old, new);
                    if n > 0 {
                        s.raw = format!("{}{}", &s.raw[..open], singers);
                        count.songs += 1;
                    }
                }
            }
            Element::Direction { text, .. } => {
                let (t, n) = replace_word(text, old, new);
                if n > 0 {
                    *text = t;
                    count.mentions += n;
                }
            }
            _ => {}
        }
    }
    let out = serialize_body(&els);
    same_shape(body, &out)?;
    Ok((out, count))
}

/// Rename the location in every scene heading that uses it
/// (`INT. KITCHEN - NIGHT` → `INT. GALLEY - NIGHT`).
pub fn rename_place_in_body(body: &str, old: &str, new: &str) -> Result<(String, RenameCount), String> {
    let mut els = parse_body(body);
    let mut count = RenameCount::default();
    for el in els.iter_mut() {
        if let Element::SceneHeading(h) = el {
            let (setting, location, _) = h.parts();
            if location != old {
                continue;
            }
            // Search after the INT./EXT. part, so a place called "INT" can't misfire.
            let from = setting.as_deref().map(str::len).unwrap_or(0).min(h.raw.len());
            let offset = usize::from(h.forced); // the leading `.` of a forced heading
            let start = h.raw[offset + from..].find(old).map(|i| offset + from + i);
            if let Some(i) = start {
                h.raw = format!("{}{}{}", &h.raw[..i], new, &h.raw[i + old.len()..]);
                count.headings += 1;
            }
        }
    }
    let out = serialize_body(&els);
    same_shape(body, &out)?;
    Ok((out, count))
}

/// The edit must not change what any line is (a cue must stay a cue …).
fn same_shape(before: &str, after: &str) -> Result<(), String> {
    let a: Vec<&str> = parse_body(before).iter().map(Element::kind).collect();
    let b: Vec<&str> = parse_body(after).iter().map(Element::kind).collect();
    if a == b {
        Ok(())
    } else {
        Err("the new name would change how the script reads (a cue would stop being a cue)".into())
    }
}

// ---------------------------------------------------------------- registries

/// Order keys are written in, per registry; others follow alphabetically.
pub const CHARACTER_KEYS: &[&str] = &["id", "cue", "name", "short", "description", "track"];
pub const LOCATION_KEYS: &[&str] = &["id", "name", "description"];
pub const SONG_KEYS: &[&str] = &["id", "title", "by", "sung_in", "heard_in", "singers", "note"];

/// `{id: hettie, cue: HETTIE, name: "Hettie"}`: one entry, one line.
pub fn flow_entry(entry: &Map<String, Value>, order: &[&str]) -> String {
    let mut keys: Vec<&String> = entry.keys().collect();
    keys.sort_by_key(|k| (order.iter().position(|o| o == k).unwrap_or(order.len()), (*k).clone()));
    let parts: Vec<String> = keys
        .into_iter()
        .map(|k| format!("{}: {}", yaml_key(k), yaml_value(&entry[k])))
        .collect();
    format!("{{{}}}", parts.join(", "))
}

fn yaml_key(k: &str) -> String {
    if !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        k.to_string()
    } else {
        serde_json::to_string(k).unwrap()
    }
}

/// Plain words stay unquoted (`actor-3`, `HETTIE`); anything else is JSON-quoted.
fn yaml_value(v: &Value) -> String {
    match v {
        Value::String(s) if plain_scalar(s) => s.clone(),
        Value::Array(items) => format!("[{}]", items.iter().map(yaml_value).collect::<Vec<_>>().join(", ")),
        Value::Object(m) => {
            let parts: Vec<String> = m
                .iter()
                .map(|(k, v)| format!("{}: {}", yaml_key(k), yaml_value(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
        other => serde_json::to_string(other).unwrap(),
    }
}

/// Safe unquoted in a YAML flow map: letters, digits, `-`, `_`, `.`, single
/// inner spaces, and nothing YAML would read as another type.
fn plain_scalar(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '))
        && s.chars().next().is_some_and(|c| c.is_alphabetic())
        && !s.ends_with(' ')
        && !s.contains("  ")
        && !matches!(
            s.to_ascii_lowercase().as_str(),
            "null" | "true" | "false" | "yes" | "no" | "on" | "off" | "y" | "n" | "~"
        )
}

/// The entries of a registry file, as JSON maps.
pub fn list_entries(text: &str) -> Result<Vec<Map<String, Value>>, String> {
    let v: serde_yaml::Value = serde_yaml::from_str(text).map_err(|e| e.to_string())?;
    let v: Value = serde_json::to_value(v).map_err(|e| e.to_string())?;
    match v {
        Value::Null => Ok(vec![]),
        Value::Array(items) => items
            .into_iter()
            .map(|i| match i {
                Value::Object(m) => Ok(m),
                _ => Err("every entry must be a map, like {id: …}".to_string()),
            })
            .collect(),
        _ => Err("the file must be a list".into()),
    }
}

/// Rewrite a registry list with `entries`, keeping the file's comments and,
/// where an entry is unchanged, its line exactly as written. Files written one
/// entry per line keep their per-line comments; any other layout is rewritten
/// in that style, after the file's leading comments.
pub fn write_list(text: &str, entries: &[Map<String, Value>], order: &[&str]) -> Result<String, String> {
    let old = list_entries(text)?;
    let lines: Vec<&str> = text.lines().collect();
    let item_lines: Vec<usize> = (0..lines.len()).filter(|&i| lines[i].starts_with("- ")).collect();
    let one_per_line = item_lines.len() == old.len()
        && lines
            .iter()
            .all(|l| l.is_empty() || l.starts_with('#') || l.starts_with("- {") || l.trim() == "[]");

    let render = |e: &Map<String, Value>, keep: Option<&str>| -> String {
        match keep {
            Some(line) => line.to_string(),
            None => format!("- {}", flow_entry(e, order)),
        }
    };

    let mut out: Vec<String> = Vec::new();
    if one_per_line {
        // Lines up to the first entry (comments) stay; entries are matched to
        // their old line by id so unchanged ones keep their spacing and comment.
        let first_item = item_lines.first().copied().unwrap_or(lines.len());
        for l in &lines[..first_item] {
            if l.trim() != "[]" {
                out.push(l.to_string());
            }
        }
        let id_of = |m: &Map<String, Value>| m.get("id").and_then(Value::as_str).map(str::to_string);
        let mut written = 0;
        for (k, &li) in item_lines.iter().enumerate() {
            // Comment lines between entries stay where they are.
            let next = item_lines.get(k + 1).copied().unwrap_or(lines.len());
            let old_entry = &old[k];
            if let Some(new_entry) = entries
                .iter()
                .find(|e| id_of(e).is_some() && id_of(e) == id_of(old_entry))
            {
                let keep = (new_entry == old_entry).then_some(lines[li]);
                let line = match keep {
                    Some(l) => l.to_string(),
                    None => {
                        let comment = trailing_comment(lines[li]);
                        format!("{}{}", render(new_entry, None), comment)
                    }
                };
                out.push(line);
                written += 1;
            }
            for l in &lines[li + 1..next] {
                if l.trim() != "[]" {
                    out.push(l.to_string());
                }
            }
        }
        // New entries go after the last entry (before trailing comments).
        let new_entries: Vec<&Map<String, Value>> = entries
            .iter()
            .filter(|e| id_of(e).is_none() || !old.iter().any(|o| id_of(o) == id_of(e)))
            .collect();
        if !new_entries.is_empty() {
            let mut insert_at = out.len();
            while insert_at > 0 && (out[insert_at - 1].starts_with('#') || out[insert_at - 1].is_empty()) && written > 0
            {
                insert_at -= 1;
            }
            for (j, e) in new_entries.iter().enumerate() {
                out.insert(insert_at + j, render(e, None));
            }
        }
        // Keep the order of `entries` (it only ever appends or removes).
    } else {
        for l in &lines {
            if l.starts_with('#') {
                out.push(l.to_string());
            } else {
                break;
            }
        }
        for e in entries {
            out.push(render(e, None));
        }
    }
    if entries.is_empty() {
        out.push("[]".into());
    }
    let mut s = out.join("\n");
    s.push('\n');
    // Never write something that reads back differently.
    let check = list_entries(&s)?;
    if check != entries {
        return Err("internal error: the list would not read back the same".into());
    }
    Ok(s)
}

/// `  # comment` after a flow map, if any.
fn trailing_comment(line: &str) -> String {
    match line.rfind('}') {
        Some(close) => {
            let rest = &line[close + 1..];
            if rest.trim_start().starts_with('#') {
                rest.to_string()
            } else {
                String::new()
            }
        }
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn whole_words_only() {
        assert_eq!(
            replace_word("HETTIE and HETTIES, HETTIE’s (HETTIE)", "HETTIE", "HATTIE").1,
            3
        );
        assert_eq!(
            replace_word("HETTIE and HETTIES, HETTIE’s (HETTIE)", "HETTIE", "HATTIE").0,
            "HATTIE and HETTIES, HATTIE’s (HATTIE)"
        );
        assert_eq!(
            replace_word("SAILOR 1 and SAILOR 12", "SAILOR 1", "DECKHAND").0,
            "DECKHAND and SAILOR 12"
        );
        assert_eq!(replace_word("Hettie", "HETTIE", "X").1, 0);
    }

    #[test]
    fn renames_cues_singers_and_directions_but_not_dialogue() {
        let body = "HETTIE enters.\n\nHETTIE\nI'm Hettie.\n\nTOBIAS & HETTIE\nTogether.\n\nHETTIE (CONT'D)\n(to HETTIE)\n~ SONG \"HETTIE's tune\" [HETTIE, TOBIAS]\n~ la\n~ END SONG\n\n@HETTIE\nForced.";
        let (out, n) = rename_character_in_body(body, "HETTIE", "HARRIET").unwrap();
        assert_eq!(
            out,
            "HARRIET enters.\n\nHARRIET\nI'm Hettie.\n\nTOBIAS & HARRIET\nTogether.\n\nHARRIET (CONT'D)\n(to HETTIE)\n~ SONG \"HETTIE's tune\" [HARRIET, TOBIAS]\n~ la\n~ END SONG\n\n@HARRIET\nForced."
        );
        assert_eq!(
            n,
            RenameCount {
                cues: 4,
                songs: 1,
                mentions: 1,
                headings: 0
            }
        );
    }

    #[test]
    fn a_rename_that_breaks_a_cue_is_refused() {
        assert!(valid_cue_name("Hettie").is_err());
        assert!(valid_cue_name("A & B").is_err());
        assert!(valid_cue_name("SAILOR 3").is_ok());
        assert!(rename_character_in_body("\nHETTIE\nHi.", "HETTIE", "> X").is_err());
    }

    #[test]
    fn renames_places_in_headings() {
        let body = "INT. KITCHEN - NIGHT\n\nThe KITCHEN is dark.\n\nEXT. KITCHEN GARDEN - DAY\n\nINT./EXT. KITCHEN - CONTINUOUS #12#\n";
        let (out, n) = rename_place_in_body(body, "KITCHEN", "GALLEY").unwrap();
        assert_eq!(
            out,
            "INT. GALLEY - NIGHT\n\nThe KITCHEN is dark.\n\nEXT. KITCHEN GARDEN - DAY\n\nINT./EXT. GALLEY - CONTINUOUS #12#\n"
        );
        assert_eq!(n.headings, 2);
    }

    const CHARS: &str = "# Cast list.\n- {id: admiral,  cue: ADMIRAL,  name: \"The Admiral\", track: null}   # not doubled\n- {id: hettie,   cue: HETTIE,   name: \"Hettie\", track: actor-4}\n# ALL is a group cue.\n";

    #[test]
    fn unchanged_lines_and_comments_survive_an_edit() {
        let mut e = list_entries(CHARS).unwrap();
        e[1].insert("description".into(), json!("a fisher: sharp, kind"));
        let out = write_list(CHARS, &e, CHARACTER_KEYS).unwrap();
        assert_eq!(
            out,
            "# Cast list.\n- {id: admiral,  cue: ADMIRAL,  name: \"The Admiral\", track: null}   # not doubled\n- {id: hettie, cue: HETTIE, name: Hettie, description: \"a fisher: sharp, kind\", track: actor-4}\n# ALL is a group cue.\n"
        );
    }

    #[test]
    fn adding_and_removing_entries() {
        let mut e = list_entries(CHARS).unwrap();
        e.push(map(
            json!({"id": "sailor-1", "cue": "SAILOR 1", "name": "Sailor 1", "track": "any-free"}),
        ));
        let out = write_list(CHARS, &e, CHARACTER_KEYS).unwrap();
        assert!(
            out.ends_with("- {id: sailor-1, cue: SAILOR 1, name: Sailor 1, track: any-free}\n# ALL is a group cue.\n"),
            "{out}"
        );
        e.remove(0);
        let out = write_list(&out, &e, CHARACTER_KEYS).unwrap();
        assert!(
            !out.contains("ADMIRAL") && out.starts_with("# Cast list.\n- {id: hettie"),
            "{out}"
        );
        let empty = write_list("# none yet\n[]\n", &[], CHARACTER_KEYS).unwrap();
        assert_eq!(empty, "# none yet\n[]\n");
        let one = write_list("# none yet\n[]\n", &e[..1], CHARACTER_KEYS).unwrap();
        assert_eq!(
            one,
            "# none yet\n- {id: hettie, cue: HETTIE, name: Hettie, track: actor-4}\n"
        );
    }

    #[test]
    fn block_style_files_are_rewritten_one_entry_per_line() {
        let block = "# chars\n- id: a\n  cue: A\n- id: b\n  cue: B\n";
        let e = list_entries(block).unwrap();
        assert_eq!(
            write_list(block, &e, CHARACTER_KEYS).unwrap(),
            "# chars\n- {id: a, cue: A}\n- {id: b, cue: B}\n"
        );
    }

    #[test]
    fn values_that_need_quotes_get_them() {
        let e = map(json!({"id": "x", "name": "yes", "description": "it's: odd", "track": "actor-1", "n": 3}));
        assert_eq!(
            flow_entry(&e, CHARACTER_KEYS),
            "{id: x, name: \"yes\", description: \"it's: odd\", track: actor-1, n: 3}"
        );
    }
}
