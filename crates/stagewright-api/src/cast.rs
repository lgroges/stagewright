//! Cast and places: the characters and locations of a project, where they are
//! used, and edits to them (add, change, delete, rename everywhere).
//!
//! Edits are pure functions over text (`*_plan`, `*_text`), applied either to
//! the files on disk or, while co-writing, to the shared document by the app.

use crate::ops::{load, read_text, scene_label, set_meta_keys, slugify, write_atomic};
use crate::{ApiError, ApiResult};
use serde_json::{json, Map, Value};
use stagewright_core::analysis::scene_stats;
use stagewright_core::cast::{
    list_entries, rename_character_in_body, rename_place_in_body, valid_cue_name, write_list, RenameCount,
    CHARACTER_KEYS, LOCATION_KEYS, SONG_KEYS,
};
use stagewright_core::scene::serialize_body;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const CHARACTERS: &str = "characters.yaml";
const LOCATIONS: &str = "locations.yaml";
const SONGS: &str = "songs.yaml";

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError::new("bad_value", msg)
}

fn file_text(root: &Path, name: &str) -> Result<String, ApiError> {
    let p = root.join(name);
    if p.exists() {
        read_text(&p)
    } else {
        Ok(String::new())
    }
}

fn entries(text: &str, name: &str) -> Result<Vec<Map<String, Value>>, ApiError> {
    list_entries(text).map_err(|e| ApiError::new("invalid_yaml", format!("{name}: {e}")))
}

fn str_of<'a>(m: &'a Map<String, Value>, k: &str) -> Option<&'a str> {
    m.get(k).and_then(Value::as_str)
}

/// The text a heading uses for a place: `The quay` → `THE QUAY`.
fn heading_form(name: &str) -> String {
    name.trim().to_uppercase()
}

// ---------------------------------------------------------------- the view

/// Everything the Cast view shows: characters and places, with where each is
/// used, plus names that are in the script but not yet in the lists.
pub fn cast_view(root: &Path) -> ApiResult {
    let p = load(root)?;
    let screen = p.meta.format.is_screen();
    let chars = entries(&file_text(root, CHARACTERS)?, CHARACTERS)?;
    let places = entries(&file_text(root, LOCATIONS)?, LOCATIONS)?;

    // cue -> (scene labels, speeches, silent appearances)
    let mut use_cue: BTreeMap<String, (Vec<String>, usize, usize)> = BTreeMap::new();
    let mut first_seen: Vec<String> = Vec::new();
    // heading place -> (headings, scene labels); stage location id -> scene labels
    let mut use_heading: BTreeMap<String, (usize, Vec<String>)> = BTreeMap::new();
    let mut use_location: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for sf in &p.scenes {
        let st = scene_stats(&sf.scene, &p.meta);
        let label = scene_label(sf);
        for cue in &st.present {
            let e = use_cue.entry(cue.clone()).or_default();
            e.0.push(label.clone());
            match st.speeches.get(cue) {
                Some(n) => e.1 += n,
                None => e.2 += 1,
            }
            if !first_seen.contains(cue) {
                first_seen.push(cue.clone());
            }
        }
        let mut seen_here = BTreeSet::new();
        for h in &st.headings {
            if h.location.is_empty() {
                continue;
            }
            let e = use_heading.entry(h.location.clone()).or_default();
            e.0 += 1;
            if seen_here.insert(h.location.clone()) {
                e.1.push(label.clone());
            }
        }
        if let Some(l) = &sf.scene.meta.location {
            use_location.entry(l.clone()).or_default().push(label.clone());
        }
    }

    let listed: BTreeSet<String> = chars
        .iter()
        .filter_map(|c| str_of(c, "cue"))
        .map(str::to_string)
        .collect();
    let characters: Vec<Value> = chars
        .iter()
        .map(|c| {
            let cue = str_of(c, "cue").unwrap_or_default();
            let (scenes, speeches, silent) = use_cue.get(cue).cloned().unwrap_or_default();
            json!({ "entry": c, "cue": cue, "scenes": scenes, "speeches": speeches, "silent": silent })
        })
        .collect();
    let unlisted: Vec<Value> = first_seen
        .iter()
        .filter(|c| !listed.contains(*c))
        .map(|c| {
            let (scenes, speeches, silent) = &use_cue[c];
            json!({ "cue": c, "scenes": scenes, "speeches": speeches, "silent": silent })
        })
        .collect();
    let tracks: BTreeSet<&str> = chars.iter().filter_map(|c| str_of(c, "track")).collect();

    let mut matched_headings = BTreeSet::new();
    let locations: Vec<Value> = places
        .iter()
        .map(|l| {
            let id = str_of(l, "id").unwrap_or_default();
            let name = str_of(l, "name").unwrap_or(id);
            let mut scenes: Vec<String> = use_location.get(id).cloned().unwrap_or_default();
            let mut headings = 0;
            for key in [heading_form(name), heading_form(id)] {
                if let Some((n, labels)) = use_heading.get(&key) {
                    if matched_headings.insert(key.clone()) {
                        headings += n;
                        for s in labels {
                            if !scenes.contains(s) {
                                scenes.push(s.clone());
                            }
                        }
                    }
                }
            }
            json!({ "entry": l, "id": id, "heading": heading_form(name), "scenes": scenes, "headings": headings })
        })
        .collect();
    let known_ids: BTreeSet<&str> = places.iter().filter_map(|l| str_of(l, "id")).collect();
    let mut unlisted_places: Vec<Value> = use_heading
        .iter()
        .filter(|(k, _)| !matched_headings.contains(*k))
        .map(|(k, (n, scenes))| json!({ "place": k, "kind": "heading", "headings": n, "scenes": scenes }))
        .collect();
    for (id, scenes) in &use_location {
        if !known_ids.contains(id.as_str()) {
            unlisted_places.push(json!({ "place": id, "kind": "location", "headings": 0, "scenes": scenes }));
        }
    }

    Ok(json!({
        "screen": screen,
        "characters": characters,
        "unlisted_characters": unlisted,
        "tracks": tracks,
        "locations": locations,
        "unlisted_places": unlisted_places,
    }))
}

// ---------------------------------------------------------------- characters (pure)

/// The incoming fields of an edit, cleaned: trimmed, empty ones removed.
fn cleaned(entry: &Map<String, Value>) -> Map<String, Value> {
    let mut m = Map::new();
    for (k, v) in entry {
        match v {
            Value::String(s) if s.trim().is_empty() => {}
            Value::Null => {}
            Value::String(s) => {
                m.insert(k.clone(), json!(s.trim()));
            }
            other => {
                m.insert(k.clone(), other.clone());
            }
        }
    }
    m
}

fn unique_id(base: &str, taken: &BTreeSet<String>) -> String {
    let base = if base.is_empty() || base == "scene" {
        "character".to_string()
    } else {
        base.to_string()
    };
    let mut id = base.clone();
    let mut n = 2;
    while taken.contains(&id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

/// Add a character (`id` absent) or change one (`id` given; its cue must stay
/// the same — renaming a cue goes through `character_rename`). Fields not in
/// `entry` are kept; empty ones are removed. Returns the new file text.
pub fn character_save_text(text: &str, id: Option<&str>, entry: &Map<String, Value>) -> Result<String, ApiError> {
    let mut list = entries(text, CHARACTERS)?;
    let incoming = cleaned(entry);
    let ids: BTreeSet<String> = list
        .iter()
        .filter_map(|c| str_of(c, "id"))
        .map(str::to_string)
        .collect();
    match id {
        Some(id) => {
            let i = list
                .iter()
                .position(|c| str_of(c, "id") == Some(id))
                .ok_or_else(|| ApiError::new("not_found", format!("no character '{id}' in {CHARACTERS}")))?;
            let cue = str_of(&list[i], "cue").unwrap_or_default().to_string();
            if let Some(new_cue) = str_of(&incoming, "cue") {
                if new_cue != cue {
                    return Err(bad("change a cue with Rename, so the script changes too"));
                }
            }
            let mut merged = Map::new();
            for (k, v) in &list[i] {
                // Keys the edit sends empty are removed; others are kept.
                if entry.contains_key(k) && !incoming.contains_key(k) && k != "id" && k != "cue" {
                    continue;
                }
                merged.insert(k.clone(), incoming.get(k).cloned().unwrap_or_else(|| v.clone()));
            }
            for (k, v) in &incoming {
                merged.entry(k.clone()).or_insert_with(|| v.clone());
            }
            merged.insert("id".into(), json!(id));
            merged.insert("cue".into(), json!(cue));
            list[i] = merged;
        }
        None => {
            let cue = str_of(&incoming, "cue")
                .ok_or_else(|| bad("a character needs a cue"))?
                .to_string();
            valid_cue_name(&cue).map_err(bad)?;
            if list.iter().any(|c| str_of(c, "cue") == Some(cue.as_str())) {
                return Err(bad(format!("{cue} is already in the list")));
            }
            let mut m = Map::new();
            let wanted = str_of(&incoming, "id").map(slugify).unwrap_or_else(|| slugify(&cue));
            m.insert("id".into(), json!(unique_id(&wanted, &ids)));
            m.insert("cue".into(), json!(cue));
            for (k, v) in &incoming {
                if k != "id" && k != "cue" {
                    m.insert(k.clone(), v.clone());
                }
            }
            if !m.contains_key("name") {
                m.insert("name".into(), json!(title_case(&cue)));
            }
            list.push(m);
        }
    }
    write_list(text, &list, CHARACTER_KEYS).map_err(|e| ApiError::new("internal", e))
}

/// `SAILOR 1` → `Sailor 1`, `MRS O'NEILL` → `Mrs O'neill` (a starting point to edit).
fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn character_delete_text(text: &str, id: &str) -> Result<String, ApiError> {
    let mut list = entries(text, CHARACTERS)?;
    let before = list.len();
    list.retain(|c| str_of(c, "id") != Some(id));
    if list.len() == before {
        return Err(ApiError::new(
            "not_found",
            format!("no character '{id}' in {CHARACTERS}"),
        ));
    }
    write_list(text, &list, CHARACTER_KEYS).map_err(|e| ApiError::new("internal", e))
}

/// One scene as a rename sees it.
#[derive(serde::Deserialize, serde::Serialize, Clone)]
pub struct SceneText {
    pub id: String,
    pub body: String,
    #[serde(default)]
    pub onstage_silent: Vec<String>,
}

/// What a rename would change, scene by scene and file by file.
pub struct Plan {
    pub scenes: Vec<SceneText>,
    pub files: BTreeMap<String, String>,
    pub count: RenameCount,
    pub silent: usize,
    pub scene_count: usize,
    /// The new name is already used in the script: this joins two characters.
    pub merge: bool,
}

impl Plan {
    fn json(&self, apply: bool) -> Value {
        json!({
            "applied": apply,
            "scenes": self.scene_count,
            "cues": self.count.cues,
            "songs": self.count.songs,
            "mentions": self.count.mentions,
            "headings": self.count.headings,
            "silent": self.silent,
            "files": self.files.keys().collect::<Vec<_>>(),
            "merge": self.merge,
        })
    }
}

/// Rename a character everywhere: cues, song singers, mentions in directions,
/// on-stage-silent lists, characters.yaml and songs.yaml.
pub fn character_rename_plan(
    old: &str,
    new: &str,
    scenes: &[SceneText],
    files: &BTreeMap<String, String>,
) -> Result<Plan, ApiError> {
    valid_cue_name(new).map_err(bad)?;
    if old == new {
        return Err(bad("that is the same name"));
    }
    let mut out = Plan {
        scenes: vec![],
        files: BTreeMap::new(),
        count: RenameCount::default(),
        silent: 0,
        scene_count: 0,
        merge: false,
    };
    for s in scenes {
        let (body, n) = rename_character_in_body(&s.body, old, new).map_err(bad)?;
        let uses_new = s.onstage_silent.iter().any(|c| c == new)
            || stagewright_core::scene::parse_body(&s.body).iter().any(|e| match e {
                stagewright_core::Element::Cue(c) => c.speakers().contains(&new),
                _ => false,
            });
        out.merge |= uses_new;
        let had_silent = s.onstage_silent.iter().any(|c| c == old);
        let mut silent: Vec<String> = s
            .onstage_silent
            .iter()
            .map(|c| if c == old { new.to_string() } else { c.clone() })
            .collect();
        silent.dedup();
        if n.total() > 0 || had_silent {
            out.count.add(n);
            out.silent += usize::from(had_silent);
            out.scene_count += 1;
            out.scenes.push(SceneText {
                id: s.id.clone(),
                body,
                onstage_silent: silent,
            });
        }
    }
    if let Some(text) = files.get(CHARACTERS) {
        let mut list = entries(text, CHARACTERS)?;
        let has_old = list.iter().any(|c| str_of(c, "cue") == Some(old));
        let has_new = list.iter().any(|c| str_of(c, "cue") == Some(new));
        if has_old && has_new {
            return Err(bad(format!(
                "{old} and {new} are both in the character list; delete one from the list first"
            )));
        }
        out.merge |= has_new;
        if has_old {
            for c in list.iter_mut() {
                if str_of(c, "cue") == Some(old) {
                    c.insert("cue".into(), json!(new));
                }
            }
            out.files.insert(
                CHARACTERS.into(),
                write_list(text, &list, CHARACTER_KEYS).map_err(|e| ApiError::new("internal", e))?,
            );
        }
    }
    if let Some(text) = files.get(SONGS) {
        let mut list = entries(text, SONGS)?;
        let mut changed = false;
        for song in list.iter_mut() {
            if let Some(Value::Array(singers)) = song.get_mut("singers") {
                for s in singers.iter_mut() {
                    if s.as_str() == Some(old) {
                        *s = json!(new);
                        changed = true;
                    }
                }
            }
        }
        if changed {
            out.files.insert(
                SONGS.into(),
                write_list(text, &list, SONG_KEYS).map_err(|e| ApiError::new("internal", e))?,
            );
        }
    }
    // A rename of a character only in the list (never in the script) is still a rename.
    if out.scene_count == 0 && out.files.is_empty() {
        return Err(ApiError::new(
            "not_found",
            format!("{old} isn't in the script or the lists"),
        ));
    }
    Ok(out)
}

// ---------------------------------------------------------------- places (pure)

pub fn location_save_text(text: &str, id: Option<&str>, entry: &Map<String, Value>) -> Result<String, ApiError> {
    let mut list = entries(text, LOCATIONS)?;
    let incoming = cleaned(entry);
    let ids: BTreeSet<String> = list
        .iter()
        .filter_map(|c| str_of(c, "id"))
        .map(str::to_string)
        .collect();
    match id {
        Some(id) => {
            let i = list
                .iter()
                .position(|c| str_of(c, "id") == Some(id))
                .ok_or_else(|| ApiError::new("not_found", format!("no place '{id}' in {LOCATIONS}")))?;
            let mut merged = Map::new();
            for (k, v) in &list[i] {
                if entry.contains_key(k) && !incoming.contains_key(k) && k != "id" {
                    continue;
                }
                merged.insert(k.clone(), incoming.get(k).cloned().unwrap_or_else(|| v.clone()));
            }
            for (k, v) in &incoming {
                merged.entry(k.clone()).or_insert_with(|| v.clone());
            }
            merged.insert("id".into(), json!(id));
            list[i] = merged;
        }
        None => {
            let name = str_of(&incoming, "name")
                .ok_or_else(|| bad("a place needs a name"))?
                .to_string();
            if list
                .iter()
                .any(|l| str_of(l, "name").map(heading_form) == Some(heading_form(&name)))
            {
                return Err(bad(format!("{name} is already in the list")));
            }
            let mut m = Map::new();
            let base = str_of(&incoming, "id").map(slugify).unwrap_or_else(|| slugify(&name));
            let base = if base == "scene" { "place".to_string() } else { base };
            let mut nid = base.clone();
            let mut n = 2;
            while ids.contains(&nid) {
                nid = format!("{base}-{n}");
                n += 1;
            }
            m.insert("id".into(), json!(nid));
            for (k, v) in &incoming {
                if k != "id" {
                    m.insert(k.clone(), v.clone());
                }
            }
            list.push(m);
        }
    }
    write_list(text, &list, LOCATION_KEYS).map_err(|e| ApiError::new("internal", e))
}

pub fn location_delete_text(text: &str, id: &str) -> Result<String, ApiError> {
    let mut list = entries(text, LOCATIONS)?;
    let before = list.len();
    list.retain(|c| str_of(c, "id") != Some(id));
    if list.len() == before {
        return Err(ApiError::new("not_found", format!("no place '{id}' in {LOCATIONS}")));
    }
    write_list(text, &list, LOCATION_KEYS).map_err(|e| ApiError::new("internal", e))
}

/// Rename a place in every scene heading (`KITCHEN` → `GALLEY`), and the
/// matching entry's name in locations.yaml (`id` stays, so scenes that refer
/// to it by id keep working).
pub fn place_rename_plan(
    old: &str,
    new_name: &str,
    scenes: &[SceneText],
    files: &BTreeMap<String, String>,
) -> Result<Plan, ApiError> {
    let new_name = new_name.trim();
    if new_name.is_empty() || new_name.contains(" - ") || new_name.contains('\n') {
        return Err(bad("a place name can't be empty or contain \" - \""));
    }
    let (old_h, new_h) = (heading_form(old), heading_form(new_name));
    let mut out = Plan {
        scenes: vec![],
        files: BTreeMap::new(),
        count: RenameCount::default(),
        silent: 0,
        scene_count: 0,
        merge: false,
    };
    if old_h != new_h {
        for s in scenes {
            let (body, n) = rename_place_in_body(&s.body, &old_h, &new_h).map_err(bad)?;
            if n.total() > 0 {
                out.count.add(n);
                out.scene_count += 1;
                out.scenes.push(SceneText {
                    id: s.id.clone(),
                    body,
                    onstage_silent: s.onstage_silent.clone(),
                });
            }
        }
    }
    if let Some(text) = files.get(LOCATIONS) {
        let mut list = entries(text, LOCATIONS)?;
        let mut changed = false;
        for l in list.iter_mut() {
            let name = str_of(l, "name").or_else(|| str_of(l, "id")).unwrap_or_default();
            if heading_form(name) == old_h || str_of(l, "id") == Some(old) {
                l.insert("name".into(), json!(new_name));
                changed = true;
            }
        }
        if changed {
            out.files.insert(
                LOCATIONS.into(),
                write_list(text, &list, LOCATION_KEYS).map_err(|e| ApiError::new("internal", e))?,
            );
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- on disk

fn disk_inputs(root: &Path) -> Result<(Vec<SceneText>, BTreeMap<String, String>), ApiError> {
    let p = load(root)?;
    let scenes = p
        .scenes
        .iter()
        .map(|sf| SceneText {
            id: sf.scene.meta.id.clone(),
            body: serialize_body(&sf.scene.body) + if sf.scene.trailing_newline { "\n" } else { "" },
            onstage_silent: sf.scene.meta.onstage_silent.clone(),
        })
        .collect();
    let mut files = BTreeMap::new();
    for name in [CHARACTERS, LOCATIONS, SONGS] {
        if root.join(name).exists() {
            files.insert(name.to_string(), read_text(&root.join(name))?);
        }
    }
    Ok((scenes, files))
}

fn write_plan(root: &Path, plan: &Plan) -> Result<(), ApiError> {
    let p = load(root)?;
    for s in &plan.scenes {
        let sf = p
            .scenes
            .iter()
            .find(|x| x.scene.meta.id == s.id)
            .ok_or_else(|| ApiError::new("not_found", format!("scene '{}'", s.id)))?;
        let mut source = format!("---\n{}---\n{}", sf.scene.frontmatter, s.body);
        if s.onstage_silent != sf.scene.meta.onstage_silent {
            source = set_meta_keys(&source, &[("onstage_silent", json!(s.onstage_silent))])?;
        }
        stagewright_core::Scene::parse(&source).map_err(|e| ApiError::new("invalid_scene", e.to_string()))?;
        write_atomic(&sf.path, &source)?;
    }
    for (name, text) in &plan.files {
        write_atomic(&root.join(name), text)?;
    }
    Ok(())
}

fn with_view(root: &Path, mut extra: Value) -> ApiResult {
    let view = cast_view(root)?;
    extra["cast"] = view;
    Ok(extra)
}

fn edit_file(root: &Path, name: &str, f: impl FnOnce(&str) -> Result<String, ApiError>) -> ApiResult {
    let text = file_text(root, name)?;
    let text = if text.trim().is_empty() {
        "[]\n".to_string()
    } else {
        text
    };
    let updated = f(&text)?;
    write_atomic(&root.join(name), &updated)?;
    with_view(root, json!({ "files": { name: updated } }))
}

pub fn character_save(root: &Path, id: Option<&str>, entry: &Map<String, Value>) -> ApiResult {
    edit_file(root, CHARACTERS, |t| character_save_text(t, id, entry))
}

pub fn character_delete(root: &Path, id: &str) -> ApiResult {
    edit_file(root, CHARACTERS, |t| character_delete_text(t, id))
}

pub fn location_save(root: &Path, id: Option<&str>, entry: &Map<String, Value>) -> ApiResult {
    edit_file(root, LOCATIONS, |t| location_save_text(t, id, entry))
}

pub fn location_delete(root: &Path, id: &str) -> ApiResult {
    edit_file(root, LOCATIONS, |t| location_delete_text(t, id))
}

/// Preview (`apply` false) or carry out a rename on disk.
pub fn character_rename(root: &Path, old: &str, new: &str, apply: bool) -> ApiResult {
    let (scenes, files) = disk_inputs(root)?;
    let plan = character_rename_plan(old, new, &scenes, &files)?;
    if apply {
        write_plan(root, &plan)?;
    }
    with_view(root, plan.json(apply))
}

pub fn place_rename(root: &Path, old: &str, new: &str, apply: bool) -> ApiResult {
    let (scenes, files) = disk_inputs(root)?;
    let plan = place_rename_plan(old, new, &scenes, &files)?;
    if apply {
        write_plan(root, &plan)?;
    }
    with_view(root, plan.json(apply))
}

/// Co-writing: the same rename over text the app supplies (shared scenes and
/// files); nothing is written. Returns the changed scenes and files.
pub fn rename_shared(
    kind: &str,
    old: &str,
    new: &str,
    scenes: &[SceneText],
    files: &BTreeMap<String, String>,
) -> ApiResult {
    let plan = match kind {
        "character" => character_rename_plan(old, new, scenes, files)?,
        "place" => place_rename_plan(old, new, scenes, files)?,
        _ => return Err(ApiError::new("bad_args", "kind is 'character' or 'place'")),
    };
    let mut v = plan.json(true);
    v["changed_scenes"] = json!(plan.scenes);
    v["changed_files"] = json!(plan.files);
    Ok(v)
}

/// Co-writing: an add/change/delete over a registry's shared text.
pub fn registry_edit_text(file: &str, text: &str, op: &str, id: Option<&str>, entry: &Map<String, Value>) -> ApiResult {
    let text = if text.trim().is_empty() { "[]\n" } else { text };
    let updated = match (file, op) {
        (CHARACTERS, "save") => character_save_text(text, id, entry)?,
        (CHARACTERS, "delete") => character_delete_text(text, id.unwrap_or_default())?,
        (LOCATIONS, "save") => location_save_text(text, id, entry)?,
        (LOCATIONS, "delete") => location_delete_text(text, id.unwrap_or_default())?,
        _ => return Err(ApiError::new("bad_args", "unknown registry edit")),
    };
    Ok(json!({ "text": updated }))
}
