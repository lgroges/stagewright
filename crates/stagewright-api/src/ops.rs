//! Filesystem operations behind each API command.
//!
//! Invariants kept by every operation:
//! - files are written atomically (temp file + rename);
//! - a write is refused if the file changed on disk since the client read it
//!   (`base_version` mismatch), so two windows or an external editor never
//!   silently overwrite each other;
//! - scene files are named `<order>-<slug>.scene`, order 010, 020… (act 0 starts at 000),
//!   and their `act`/`scene` frontmatter matches their position.

use crate::{ApiError, ApiResult};
use serde_json::{json, Map, Value};
use stagewright_core::analysis::{doubling_issues, scene_stats, SceneStats};
use stagewright_core::frontmatter::set_key;
use stagewright_core::layout::{eighths_label, Format};
use stagewright_core::project::{ActRef, Character, Location, ProjectError, ProjectMeta};
use stagewright_core::{Project, Scene};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const PROJECT_FILES: &[&str] = &[
    "project.yaml",
    "characters.yaml",
    "locations.yaml",
    "songs.yaml",
    "comments.yaml",
];

const STATUSES: &[&str] = &["outline", "draft", "first-edit", "second-edit", "done"];

/// Scene metadata keys the client may set; `act` and `scene` are owned by reordering.
const EDITABLE_KEYS: &[&str] = &[
    "title",
    "status",
    "location",
    "duration",
    "onstage_silent",
    "synopsis",
    "notes",
    "tags",
    "ratings",
];

// ---------------------------------------------------------------- helpers

/// FNV-1a 64-bit hash of the file's bytes, as hex. Stable across runs and platforms.
pub fn version_of(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in s.chars() {
        if ch == '\'' || ch == '’' {
            continue;
        }
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "scene".into()
    } else {
        out.chars()
            .take(48)
            .collect::<String>()
            .trim_end_matches('-')
            .to_string()
    }
}

pub(crate) fn io_err(path: &Path, e: std::io::Error) -> ApiError {
    ApiError::new("io", format!("{}: {e}", path.display()))
}

fn project_err(e: ProjectError) -> ApiError {
    ApiError::new("project", e.to_string())
}

pub(crate) fn read_text(path: &Path) -> Result<String, ApiError> {
    fs::read_to_string(path)
        .map(|s| s.replace("\r\n", "\n"))
        .map_err(|e| io_err(path, e))
}

pub(crate) fn write_atomic(path: &Path, text: &str) -> Result<(), ApiError> {
    let tmp = path.with_extension("stagewright-tmp");
    fs::write(&tmp, text).map_err(|e| io_err(&tmp, e))?;
    fs::rename(&tmp, path).map_err(|e| io_err(path, e))
}

pub(crate) fn load(root: &Path) -> Result<Project, ApiError> {
    Project::load(root).map_err(project_err)
}

fn act_dir(root: &Path, a: ActRef) -> PathBuf {
    a.dir(root)
}

const NUMBER_WORDS: &[&str] = &[
    "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Ten",
];

fn act_title(meta: &ProjectMeta, act: u32) -> String {
    meta.acts
        .iter()
        .find(|a| a.number == act)
        .and_then(|a| a.title.clone())
        .unwrap_or_else(|| match (act, meta.format) {
            (0, Format::Stage) => "Prologue".into(),
            (0, Format::Film) => "Opening".into(),
            (0, _) => "Teaser".into(),
            (n, _) => match NUMBER_WORDS.get(n as usize) {
                Some(w) => format!("Act {w}"),
                None => format!("Act {n}"),
            },
        })
}

fn episode_title(p: &Project, e: u32) -> String {
    p.episode_title(e).unwrap_or_else(|| format!("Episode {e}"))
}

/// `P`, `2.3`, or in a series `E1 2.3`.
pub(crate) fn scene_label(sf: &stagewright_core::project::SceneFile) -> String {
    let base = match (sf.act, sf.scene.meta.scene) {
        (0, Some(n)) if n > 0 => format!("0.{n}"),
        (0, _) => "P".to_string(),
        (a, Some(n)) => format!("{a}.{n}"),
        (a, None) => format!("{a}.?"),
    };
    match sf.episode {
        Some(e) => format!("E{e} {base}"),
        None => base,
    }
}

fn act_json(p: &Project, a: ActRef) -> Value {
    json!({ "episode": a.episode, "number": a.act, "title": act_title(&p.meta, a.act) })
}

fn order_prefix(act: u32, index: usize) -> String {
    let n = if act == 0 { index * 10 } else { (index + 1) * 10 };
    format!("{n:03}")
}

fn scene_number(act: u32, index: usize) -> u32 {
    if act == 0 {
        index as u32
    } else {
        index as u32 + 1
    }
}

/// `020-the-bishops-parlour.scene` -> `the-bishops-parlour`.
fn file_slug(path: &Path) -> String {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("scene");
    match stem.split_once('-') {
        Some((prefix, rest)) if prefix.chars().all(|c| c.is_ascii_digit()) => rest.to_string(),
        _ => stem.to_string(),
    }
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

pub(crate) fn find_scene<'a>(
    project: &'a Project,
    id: &str,
) -> Result<&'a stagewright_core::project::SceneFile, ApiError> {
    project
        .scene(id)
        .ok_or_else(|| ApiError::new("not_found", format!("no scene with id '{id}'")))
}

fn check_version(path: &Path, base_version: &str) -> Result<String, ApiError> {
    let current = version_of(&fs::read(path).map_err(|e| io_err(path, e))?);
    if current != base_version {
        return Err(
            ApiError::new("conflict", "the file changed on disk since it was opened")
                .with_detail(json!({ "version": current })),
        );
    }
    Ok(current)
}

/// Re-parse to be sure what we are about to write is a valid scene.
fn validated(source: &str) -> Result<Scene, ApiError> {
    Scene::parse(source).map_err(|e| ApiError::new("invalid_scene", e.to_string()))
}

pub(crate) fn set_meta_keys(source: &str, pairs: &[(&str, Value)]) -> Result<String, ApiError> {
    let mut scene = validated(source)?;
    for (k, v) in pairs {
        scene.frontmatter = set_key(&scene.frontmatter, k, v);
    }
    let out = scene.to_source();
    validated(&out)?;
    Ok(out)
}

// ---------------------------------------------------------------- views

fn summary(root: &Project, sf: &stagewright_core::project::SceneFile, st: &SceneStats) -> Value {
    let m = &sf.scene.meta;
    json!({
        "id": m.id,
        "title": st.title,
        "act": sf.act,
        "episode": sf.episode,
        "label": scene_label(sf),
        "number": m.scene,
        "status": m.status,
        "location": m.location,
        "file": rel(&root.root, &sf.path),
        "minutes": st.seconds() / 60.0,
        "manual_duration": st.manual_seconds.is_some(),
        "words": st.dialogue_words,
        "eighths": st.eighths,
        "pages": eighths_label(st.eighths),
        "headings": st.headings.iter().map(|h| &h.text).collect::<Vec<_>>(),
        "speakers": st.speeches.keys().collect::<Vec<_>>(),
    })
}

pub fn project_view(root: &Path) -> ApiResult {
    let p = load(root)?;
    let mut cues: BTreeSet<String> = p.characters.iter().map(|c| c.cue.clone()).collect();
    let mut places: BTreeSet<String> = BTreeSet::new();
    let acts: Vec<Value> = p
        .act_refs
        .iter()
        .map(|&a| {
            let scenes: Vec<Value> = p
                .scenes
                .iter()
                .filter(|s| s.act_ref() == a)
                .map(|sf| {
                    let st = scene_stats(&sf.scene, &p.meta);
                    cues.extend(st.speeches.keys().cloned());
                    places.extend(st.headings.iter().map(|h| h.location.clone()).filter(|l| !l.is_empty()));
                    summary(&p, sf, &st)
                })
                .collect();
            let mut j = act_json(&p, a);
            j["scenes"] = json!(scenes);
            j
        })
        .collect();
    let episodes: Vec<Value> = p
        .episode_numbers
        .iter()
        .map(|&e| json!({ "number": e, "title": episode_title(&p, e) }))
        .collect();
    Ok(json!({
        "root": p.root,
        "title": p.meta.title,
        "subtitle": p.meta.subtitle,
        "authors": p.meta.authors,
        "rating_fields": p.meta.rating_fields,
        "running_time": p.meta.running_time,
        "statuses": STATUSES,
        "format": p.meta.format,
        "format_label": p.meta.format.label(),
        "auto_contd": p.meta.auto_contd(),
        "shooting_script": p.meta.shooting_script,
        "series": p.series,
        "episodes": episodes,
        "acts": acts,
        "heading_locations": places,
        "characters": p.characters,
        "locations": p.locations,
        "cues": cues,
        "files": PROJECT_FILES.iter().filter(|f| p.root.join(f).exists()).collect::<Vec<_>>(),
    }))
}

pub fn scene_read(root: &Path, id: &str) -> ApiResult {
    let p = load(root)?;
    let sf = find_scene(&p, id)?;
    let bytes = fs::read(&sf.path).map_err(|e| io_err(&sf.path, e))?;
    let st = scene_stats(&sf.scene, &p.meta);
    Ok(json!({
        "id": id,
        "meta": sf.scene.meta,
        "body": stagewright_core::scene::serialize_body(&sf.scene.body)
            + if sf.scene.trailing_newline { "\n" } else { "" },
        "version": version_of(&bytes),
        "summary": summary(&p, sf, &st),
    }))
}

pub fn scene_save(root: &Path, id: &str, body: &str, base_version: &str) -> ApiResult {
    let p = load(root)?;
    let sf = find_scene(&p, id)?;
    check_version(&sf.path, base_version)?;
    let body = body.replace("\r\n", "\n");
    let source = format!("---\n{}---\n{}", sf.scene.frontmatter, body);
    validated(&source)?;
    write_atomic(&sf.path, &source)?;
    scene_read(root, id)
}

fn validate_patch(key: &str, v: &Value, p: &Project) -> Result<(), ApiError> {
    let bad = |m: &str| Err(ApiError::new("bad_value", format!("{key}: {m}")));
    match key {
        "title" => match v.as_str() {
            Some(s) if !s.trim().is_empty() => Ok(()),
            _ => bad("must be a non-empty string"),
        },
        "status" => match v {
            Value::Null => Ok(()),
            Value::String(s) if STATUSES.contains(&s.as_str()) => Ok(()),
            _ => bad(&format!("must be one of {}, or null", STATUSES.join(", "))),
        },
        "location" | "synopsis" | "notes" => {
            if v.is_null() || v.is_string() {
                Ok(())
            } else {
                bad("must be a string or null")
            }
        }
        "duration" => match v {
            Value::Null => Ok(()),
            Value::Number(n) if n.as_f64().is_some_and(|f| f > 0.0 && f < 600.0) => Ok(()),
            _ => bad("must be minutes between 0 and 600, or null"),
        },
        "onstage_silent" | "tags" => match v.as_array() {
            Some(a) if a.iter().all(Value::is_string) => Ok(()),
            _ => bad("must be a list of strings"),
        },
        "ratings" => match v.as_object() {
            Some(m)
                if m.iter().all(|(k, n)| {
                    (p.meta.rating_fields.is_empty() || p.meta.rating_fields.contains(k))
                        && n.as_u64().is_some_and(|n| (1..=9).contains(&n))
                }) =>
            {
                Ok(())
            }
            _ => bad("must map rating names to whole numbers 1–9"),
        },
        _ => Err(ApiError::new("bad_key", format!("'{key}' cannot be edited here"))),
    }
}

pub fn scene_update_meta(root: &Path, id: &str, patch: &Map<String, Value>, base_version: &str) -> ApiResult {
    let p = load(root)?;
    let sf = find_scene(&p, id)?;
    check_version(&sf.path, base_version)?;
    for (k, v) in patch {
        if !EDITABLE_KEYS.contains(&k.as_str()) {
            return Err(ApiError::new("bad_key", format!("'{k}' cannot be edited here")));
        }
        validate_patch(k, v, &p)?;
    }
    let source = read_text(&sf.path)?;
    let pairs: Vec<(&str, Value)> = patch.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    let updated = set_meta_keys(&source, &pairs)?;
    write_atomic(&sf.path, &updated)?;

    // A new title renames the file so the folder stays readable; the id is unchanged.
    if let Some(title) = patch.get("title").and_then(Value::as_str) {
        let stem = sf.path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let prefix = stem.split_once('-').map(|(a, _)| a).unwrap_or(stem);
        let new_path = sf.path.with_file_name(format!("{prefix}-{}.scene", slugify(title)));
        if new_path != sf.path {
            if new_path.exists() {
                return Err(ApiError::new(
                    "exists",
                    format!("{} already exists", new_path.display()),
                ));
            }
            fs::rename(&sf.path, &new_path).map_err(|e| io_err(&new_path, e))?;
        }
    }
    scene_read(root, id)
}

fn new_id(p: &Project) -> String {
    let mut n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    loop {
        let mut s = String::new();
        let mut x = n;
        while x > 0 {
            let d = (x % 36) as u32;
            s.insert(0, char::from_digit(d, 36).unwrap());
            x /= 36;
        }
        let id = format!("sc-{s}");
        if p.scene(&id).is_none() {
            return id;
        }
        n += 1;
    }
}

fn valid_id(id: &str) -> bool {
    (2..=64).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Create a scene. `id` is given when the scene already exists in a shared
/// project (so every writer's copy uses the same id); otherwise one is made.
pub fn scene_create(root: &Path, aref: ActRef, after: Option<&str>, title: &str, id: Option<&str>) -> ApiResult {
    let p = load(root)?;
    if !p.act_refs.contains(&aref) {
        return Err(ApiError::new("not_found", format!("act {} does not exist", aref.act)));
    }
    let act = aref.act;
    let id = match id {
        Some(id) if !valid_id(id) => return Err(ApiError::new("bad_args", format!("'{id}' is not a valid scene id"))),
        Some(id) if p.scene(id).is_some() => {
            return Err(ApiError::new("exists", format!("scene '{id}' already exists")))
        }
        Some(id) => id.to_string(),
        None => new_id(&p),
    };
    let mut order: Vec<String> = p
        .scenes
        .iter()
        .filter(|s| s.act_ref() == aref)
        .map(|s| s.scene.meta.id.clone())
        .collect();
    let pos = match after {
        Some(a) => order.iter().position(|x| x == a).map(|i| i + 1).unwrap_or(order.len()),
        None => order.len(),
    };
    let fm = format!(
        "id: {id}\ntitle: {}\nact: {act}\nscene: 0\nstatus: \"draft\"\nlocation: null\nduration: null  # manual override, minutes\n",
        serde_json::to_string(title).unwrap()
    );
    // A temporary name inside the act folder; reordering gives it its real name.
    let tmp = act_dir(root, aref).join(format!("999-{}.scene", slugify(title)));
    if tmp.exists() {
        return Err(ApiError::new("exists", format!("{} already exists", tmp.display())));
    }
    write_atomic(&tmp, &format!("---\n{fm}---\n"))?;
    order.insert(pos, id.clone());
    reorder_act(root, aref, &order)?;
    let mut view = project_view(root)?;
    view["created"] = json!(id);
    Ok(view)
}

pub fn scene_trash(root: &Path, id: &str) -> ApiResult {
    let p = load(root)?;
    let sf = find_scene(&p, id)?;
    let trash = root.join(".trash");
    fs::create_dir_all(&trash).map_err(|e| io_err(&trash, e))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let ep = sf.episode.map(|e| format!("ep{e:02}-")).unwrap_or_default();
    let name = format!(
        "{stamp}-{ep}act{:02}-{}",
        sf.act,
        sf.path.file_name().unwrap().to_string_lossy()
    );
    fs::rename(&sf.path, trash.join(&name)).map_err(|e| io_err(&sf.path, e))?;
    let aref = sf.act_ref();
    let remaining: Vec<String> = p
        .scenes
        .iter()
        .filter(|s| s.act_ref() == aref && s.scene.meta.id != id)
        .map(|s| s.scene.meta.id.clone())
        .collect();
    reorder_act(root, aref, &remaining)?;
    project_view(root)
}

/// Make `ids` the complete running order of `act`. Ids may come from other acts
/// (a move); those acts are renumbered afterwards.
pub fn scenes_reorder(root: &Path, aref: ActRef, ids: &[String]) -> ApiResult {
    reorder_act(root, aref, ids)?;
    project_view(root)
}

fn reorder_act(root: &Path, aref: ActRef, ids: &[String]) -> Result<(), ApiError> {
    let p = load(root)?;
    if !p.act_refs.contains(&aref) {
        return Err(ApiError::new("not_found", format!("act {} does not exist", aref.act)));
    }
    let act = aref.act;
    let unique: BTreeSet<&String> = ids.iter().collect();
    if unique.len() != ids.len() {
        return Err(ApiError::new("bad_args", "ids contain duplicates"));
    }
    for sf in p.scenes.iter().filter(|s| s.act_ref() == aref) {
        if !ids.contains(&sf.scene.meta.id) {
            return Err(ApiError::new(
                "bad_args",
                format!(
                    "scene '{}' is in act {act} but missing from the new order",
                    sf.scene.meta.id
                ),
            ));
        }
    }
    let files: Vec<&stagewright_core::project::SceneFile> =
        ids.iter().map(|id| find_scene(&p, id)).collect::<Result<_, _>>()?;
    let target = act_dir(root, aref);

    // Phase 1: move everything to unique temporary names so renames cannot collide.
    // Temporary names keep the .scene extension, so an interrupted reorder leaves
    // every scene loadable (just oddly named) rather than hidden.
    let mut staged = Vec::new();
    for (i, sf) in files.iter().enumerate() {
        let tmp = target.join(format!("~reorder-{i:03}-{}.scene", file_slug(&sf.path)));
        fs::rename(&sf.path, &tmp).map_err(|e| io_err(&sf.path, e))?;
        staged.push((tmp, *sf));
    }
    // Phase 2: fix act/scene numbers in place, then rename to the final name.
    for (i, (tmp, sf)) in staged.iter().enumerate() {
        let source = read_text(tmp)?;
        let mut keys = vec![("act", json!(act)), ("scene", json!(scene_number(act, i)))];
        if let Some(e) = aref.episode {
            keys.push(("episode", json!(e)));
        }
        let updated = set_meta_keys(&source, &keys)?;
        write_atomic(tmp, &updated)?;
        let final_path = target.join(format!("{}-{}.scene", order_prefix(act, i), file_slug(&sf.path)));
        fs::rename(tmp, &final_path).map_err(|e| io_err(&final_path, e))?;
    }

    // Renumber any act that lost a scene in a move.
    let donors: BTreeSet<ActRef> = files.iter().map(|s| s.act_ref()).filter(|a| *a != aref).collect();
    for donor in donors {
        let rest: Vec<String> = p
            .scenes
            .iter()
            .filter(|s| s.act_ref() == donor && !ids.contains(&s.scene.meta.id))
            .map(|s| s.scene.meta.id.clone())
            .collect();
        reorder_act(root, donor, &rest)?;
    }
    Ok(())
}

pub fn act_create(root: &Path, episode: Option<u32>) -> ApiResult {
    let p = load(root)?;
    if p.series != episode.is_some() {
        return Err(ApiError::new(
            "bad_args",
            if p.series {
                "say which episode"
            } else {
                "this project has no episodes"
            },
        ));
    }
    if let Some(e) = episode.filter(|e| !p.episode_numbers.contains(e)) {
        return Err(ApiError::new("not_found", format!("episode {e} does not exist")));
    }
    let next = p
        .act_refs
        .iter()
        .filter(|a| a.episode == episode)
        .map(|a| a.act)
        .max()
        .map(|n| n + 1)
        .unwrap_or(1);
    let dir = act_dir(root, ActRef { episode, act: next });
    fs::create_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
    let mut view = project_view(root)?;
    view["created_act"] = json!(next);
    Ok(view)
}

pub fn stats(root: &Path) -> ApiResult {
    let p = load(root)?;
    let stats: Vec<SceneStats> = p.scenes.iter().map(|sf| scene_stats(&sf.scene, &p.meta)).collect();
    let scenes: Vec<Value> = p
        .scenes
        .iter()
        .zip(&stats)
        .map(|(sf, st)| {
            json!({
                "id": st.id, "label": scene_label(sf), "title": st.title, "act": sf.act, "episode": sf.episode,
                "eighths": st.eighths, "pages": eighths_label(st.eighths), "headings": st.headings,
                "minutes": st.seconds() / 60.0, "manual_duration": st.manual_seconds.is_some(),
                "estimated_minutes": st.estimated_seconds / 60.0,
                "words": st.dialogue_words, "lyric_lines": st.lyric_lines, "songs": st.songs,
                "speeches": st.speeches, "present": st.present,
            })
        })
        .collect();
    let acts: Vec<Value> = p
        .act_refs
        .iter()
        .map(|&a| {
            let here = || p.scenes.iter().zip(&stats).filter(move |(sf, _)| sf.act_ref() == a);
            let mins: f64 = here().map(|(_, st)| st.seconds()).sum::<f64>() / 60.0;
            let eighths: u32 = here().map(|(_, st)| st.eighths).sum();
            let mut j = act_json(&p, a);
            j["minutes"] = json!(mins);
            j["pages"] = json!(eighths_label(eighths.max(1)));
            j
        })
        .collect();
    let mut roles: Vec<String> = p.characters.iter().map(|c| c.cue.clone()).collect();
    let extra: BTreeSet<String> = stats
        .iter()
        .flat_map(|s| s.present.iter().cloned())
        .filter(|r| !roles.contains(r))
        .collect();
    roles.extend(extra);
    roles.retain(|r| stats.iter().any(|s| s.present.contains(r)));
    let roles: Vec<Value> = roles
        .iter()
        .map(|r| json!({ "cue": r, "track": p.character_by_cue(r).and_then(|c| c.track.clone()) }))
        .collect();
    let total_eighths: u32 = stats.iter().map(|s| s.eighths).sum();
    Ok(json!({
        "format": p.meta.format,
        "series": p.series,
        "episodes": p.episode_numbers.iter().map(|&e| json!({ "number": e, "title": episode_title(&p, e) })).collect::<Vec<_>>(),
        "total_pages": eighths_label(total_eighths.max(1)),
        "scenes": scenes,
        "acts": acts,
        "total_minutes": stats.iter().map(SceneStats::seconds).sum::<f64>() / 60.0,
        "roles": roles,
        "issues": doubling_issues(&p, &stats),
        "running_time": p.meta.running_time,
    }))
}

pub(crate) fn project_file(root: &Path, name: &str) -> Result<PathBuf, ApiError> {
    if !PROJECT_FILES.contains(&name) {
        return Err(ApiError::new("bad_args", format!("'{name}' is not a project file")));
    }
    Ok(root.join(name))
}

pub fn file_read(root: &Path, name: &str) -> ApiResult {
    let path = project_file(root, name)?;
    let text = if path.exists() {
        read_text(&path)?
    } else {
        String::new()
    };
    Ok(json!({ "name": name, "text": text, "version": version_of(text.as_bytes()) }))
}

pub fn file_save(root: &Path, name: &str, text: &str, base_version: &str) -> ApiResult {
    let path = project_file(root, name)?;
    let current = if path.exists() {
        read_text(&path)?
    } else {
        String::new()
    };
    if version_of(current.as_bytes()) != base_version {
        return Err(
            ApiError::new("conflict", "the file changed on disk since it was opened")
                .with_detail(json!({ "version": version_of(current.as_bytes()) })),
        );
    }
    let text = text.replace("\r\n", "\n");
    let check = |r: Result<(), serde_yaml::Error>| r.map_err(|e| ApiError::new("invalid_yaml", e.to_string()));
    match name {
        "project.yaml" => check(serde_yaml::from_str::<ProjectMeta>(&text).map(|_| ()))?,
        "characters.yaml" => check(serde_yaml::from_str::<Vec<Character>>(&text).map(|_| ()))?,
        "locations.yaml" => check(serde_yaml::from_str::<Vec<Location>>(&text).map(|_| ()))?,
        "comments.yaml" => stagewright_core::comments::parse(&text)
            .map(|_| ())
            .map_err(|e| ApiError::new("invalid_yaml", e))?,
        _ => check(serde_yaml::from_str::<serde_yaml::Value>(&text).map(|_| ()))?,
    }
    write_atomic(&path, &text)?;
    file_read(root, name)
}

pub fn project_create(root: &Path, title: &str, format: Format, series: bool) -> Result<(), ApiError> {
    if root.join("project.yaml").exists() {
        return Err(ApiError::new(
            "exists",
            format!("{} already contains a project", root.display()),
        ));
    }
    if root.exists() && fs::read_dir(root).map_err(|e| io_err(root, e))?.next().is_some() {
        return Err(ApiError::new("not_empty", format!("{} is not empty", root.display())));
    }
    let first = ActRef {
        episode: series.then_some(1),
        act: 1,
    };
    let dir = act_dir(root, first);
    fs::create_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
    let format_name = serde_json::to_value(format).unwrap();
    let mut project = format!(
        "title: {}\nauthors: []\nformat: {}\nrating_fields: [tension, humour, relevance, quality]\nrunning_time:\n  words_per_minute: 150\n  seconds_per_lyric_line: 6\n",
        serde_json::to_string(title).unwrap(),
        format_name.as_str().unwrap()
    );
    if series {
        project.push_str("series: true\nepisodes:\n  - {number: 1, title: \"Episode 1\"}\n");
    }
    write_atomic(&root.join("project.yaml"), &project)?;
    write_atomic(
        &root.join("characters.yaml"),
        "# One entry per character. `cue` is the name as it appears in the script.\n# `track` groups roles one actor plays (doubling).\n[]\n",
    )?;
    write_atomic(&root.join("locations.yaml"), "[]\n")?;
    write_atomic(&root.join("songs.yaml"), "[]\n")?;
    scene_create(root, first, None, "Scene 1", None)?;
    Ok(())
}

// ---------------------------------------------------------------- shared projects

/// Everything a co-writing client needs in one call: the tree, every scene
/// (metadata, body, version) and every project file.
pub fn project_snapshot(root: &Path) -> ApiResult {
    let p = load(root)?;
    let mut scenes = Vec::new();
    for sf in &p.scenes {
        let bytes = fs::read(&sf.path).map_err(|e| io_err(&sf.path, e))?;
        scenes.push(json!({
            "id": sf.scene.meta.id,
            "act": sf.act,
            "episode": sf.episode,
            "meta": sf.scene.meta,
            "body": stagewright_core::scene::serialize_body(&sf.scene.body)
                + if sf.scene.trailing_newline { "\n" } else { "" },
            "version": version_of(&bytes),
        }));
    }
    let mut files = Vec::new();
    for name in PROJECT_FILES {
        if root.join(name).exists() {
            files.push(file_read(root, name)?);
        }
    }
    Ok(json!({ "view": project_view(root)?, "scenes": scenes, "files": files }))
}

/// Make sure act folder `act` exists (shared projects create acts by number).
pub fn act_ensure(root: &Path, aref: ActRef) -> ApiResult {
    if aref.act > 99 || aref.episode.is_some_and(|e| e > 99) {
        return Err(ApiError::new("bad_args", "acts and episodes are numbered 0–99"));
    }
    let dir = act_dir(root, aref);
    fs::create_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
    project_view(root)
}

/// Add an episode to a series: `episodes/NN/acts/01`, listed in project.yaml.
pub fn episode_create(root: &Path, title: Option<&str>) -> ApiResult {
    let p = load(root)?;
    if !p.series {
        return Err(ApiError::new("bad_args", "this project is not a series"));
    }
    let next = p.episode_numbers.iter().max().map(|n| n + 1).unwrap_or(1);
    let title = title.map(str::to_string).unwrap_or_else(|| format!("Episode {next}"));
    let path = root.join("project.yaml");
    let text = read_text(&path)?;
    let mut list: Vec<Value> = p
        .meta
        .episodes
        .iter()
        .map(|e| json!({ "number": e.number, "title": e.title }))
        .collect();
    list.push(json!({ "number": next, "title": title }));
    let updated = stagewright_core::frontmatter::set_key_list(&text, "episodes", &list);
    serde_yaml::from_str::<ProjectMeta>(&updated).map_err(|e| ApiError::new("invalid_yaml", e.to_string()))?;
    let dir = act_dir(
        root,
        ActRef {
            episode: Some(next),
            act: 1,
        },
    );
    fs::create_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
    write_atomic(&path, &updated)?;
    let mut view = project_view(root)?;
    view["created_episode"] = json!(next);
    Ok(view)
}

// ---------------------------------------------------------------- deleting acts and episodes

/// A folder in `.trash/` named after what it held, e.g. `1730000000-ep02-act03`.
fn trash_folder(root: &Path, what: &str) -> Result<PathBuf, ApiError> {
    let trash = root.join(".trash");
    fs::create_dir_all(&trash).map_err(|e| io_err(&trash, e))?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut dest = trash.join(format!("{stamp}-{what}"));
    let mut n = 1;
    while dest.exists() {
        n += 1;
        dest = trash.join(format!("{stamp}-{what}-{n}"));
    }
    Ok(dest)
}

/// `ep02-act03`, or `act03` outside a series.
fn act_tag(a: ActRef) -> String {
    match a.episode {
        Some(e) => format!("ep{e:02}-act{:02}", a.act),
        None => format!("act{:02}", a.act),
    }
}

/// Rewrite a numbered list in project.yaml (`acts:` or `episodes:`): drop the
/// entry numbered `removed` and close the gap. Entries keep their other fields;
/// a default title ("Act Three", "Episode 3") follows its new number.
fn renumber_list(
    text: &str,
    key: &str,
    removed: u32,
    shift_from: u32,
    default_title: impl Fn(u32) -> String,
) -> Result<String, ApiError> {
    let doc: serde_yaml::Value =
        serde_yaml::from_str(text).map_err(|e| ApiError::new("invalid_yaml", e.to_string()))?;
    let Some(list) = doc.get(key).and_then(|v| v.as_sequence()) else {
        return Ok(text.to_string());
    };
    let mut items = Vec::new();
    for item in list {
        let mut v: Value = serde_json::to_value(item).map_err(|e| ApiError::new("invalid_yaml", e.to_string()))?;
        let Some(n) = v.get("number").and_then(Value::as_u64).map(|n| n as u32) else {
            items.push(v);
            continue;
        };
        if n == removed {
            continue;
        }
        if n > removed && n >= shift_from {
            v["number"] = json!(n - 1);
            if v.get("title").and_then(Value::as_str) == Some(default_title(n).as_str()) {
                v["title"] = json!(default_title(n - 1));
            }
        }
        items.push(v);
    }
    let updated = stagewright_core::frontmatter::set_key_list(text, key, &items);
    serde_yaml::from_str::<ProjectMeta>(&updated).map_err(|e| ApiError::new("invalid_yaml", e.to_string()))?;
    Ok(updated)
}

/// project.yaml without act `act` (acts after it move up one). Act 0, the
/// prologue or teaser, never shifts. A series' act titles are shared by every
/// episode, so a series' list is left alone.
pub fn yaml_remove_act(text: &str, act: u32, series: bool) -> Result<String, ApiError> {
    if series {
        return Ok(text.to_string());
    }
    let shift_from = if act == 0 { u32::MAX } else { act + 1 };
    renumber_list(text, "acts", act, shift_from, |n| match NUMBER_WORDS.get(n as usize) {
        Some(w) => format!("Act {w}"),
        None => format!("Act {n}"),
    })
}

/// project.yaml without episode `episode` (later episodes move up one).
pub fn yaml_remove_episode(text: &str, episode: u32) -> Result<String, ApiError> {
    renumber_list(text, "episodes", episode, episode + 1, |n| format!("Episode {n}"))
}

/// Delete an act: its scenes go to `.trash/` (as one folder), later acts in the
/// same episode move up to close the gap, and project.yaml follows.
pub fn act_delete(root: &Path, aref: ActRef) -> ApiResult {
    let p = load(root)?;
    if !p.act_refs.contains(&aref) {
        return Err(ApiError::new("not_found", format!("act {} does not exist", aref.act)));
    }
    let siblings: Vec<ActRef> = p
        .act_refs
        .iter()
        .copied()
        .filter(|a| a.episode == aref.episode)
        .collect();
    if siblings.len() <= 1 {
        return Err(ApiError::new(
            "bad_args",
            if p.series {
                "an episode needs at least one act; delete the episode instead"
            } else {
                "a project needs at least one act"
            },
        ));
    }
    let trashed = p.scenes.iter().filter(|s| s.act_ref() == aref).count();
    let dir = act_dir(root, aref);
    if trashed > 0 {
        let dest = trash_folder(root, &act_tag(aref))?;
        fs::rename(&dir, &dest).map_err(|e| io_err(&dir, e))?;
    } else {
        fs::remove_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
    }
    // Close the gap: act n+1 becomes n, and so on (act 0 never moves).
    if aref.act >= 1 {
        let mut later: Vec<ActRef> = siblings.into_iter().filter(|a| a.act > aref.act).collect();
        later.sort();
        for old in later {
            let new = ActRef {
                episode: old.episode,
                act: old.act - 1,
            };
            let (from, to) = (act_dir(root, old), act_dir(root, new));
            fs::rename(&from, &to).map_err(|e| io_err(&from, e))?;
            renumber_in_place(root, new)?;
        }
    }
    let yaml = root.join("project.yaml");
    let text = read_text(&yaml)?;
    let updated = yaml_remove_act(&text, aref.act, p.series)?;
    if updated != text {
        write_atomic(&yaml, &updated)?;
    }
    let mut view = project_view(root)?;
    view["trashed_scenes"] = json!(trashed);
    Ok(view)
}

/// Delete an episode: its folder goes to `.trash/`, later episodes move up.
pub fn episode_delete(root: &Path, episode: u32) -> ApiResult {
    let p = load(root)?;
    if !p.series {
        return Err(ApiError::new("bad_args", "this project is not a series"));
    }
    if !p.episode_numbers.contains(&episode) {
        return Err(ApiError::new("not_found", format!("episode {episode} does not exist")));
    }
    if p.episode_numbers.len() <= 1 {
        return Err(ApiError::new("bad_args", "a series needs at least one episode"));
    }
    let trashed = p.scenes.iter().filter(|s| s.episode == Some(episode)).count();
    let dir = root.join("episodes").join(format!("{episode:02}"));
    let dest = trash_folder(root, &format!("ep{episode:02}"))?;
    fs::rename(&dir, &dest).map_err(|e| io_err(&dir, e))?;
    let mut later: Vec<u32> = p.episode_numbers.iter().copied().filter(|e| *e > episode).collect();
    later.sort();
    for old in later {
        let from = root.join("episodes").join(format!("{old:02}"));
        let to = root.join("episodes").join(format!("{:02}", old - 1));
        fs::rename(&from, &to).map_err(|e| io_err(&from, e))?;
        let moved = load(root)?;
        for a in moved.act_refs.iter().copied().filter(|a| a.episode == Some(old - 1)) {
            renumber_in_place(root, a)?;
        }
    }
    let yaml = root.join("project.yaml");
    let text = read_text(&yaml)?;
    let updated = yaml_remove_episode(&text, episode)?;
    if updated != text {
        write_atomic(&yaml, &updated)?;
    }
    let mut view = project_view(root)?;
    view["trashed_scenes"] = json!(trashed);
    Ok(view)
}

/// Rewrite the act/episode/scene numbers of every scene in `aref` after its
/// folder moved, keeping the running order.
fn renumber_in_place(root: &Path, aref: ActRef) -> Result<(), ApiError> {
    let p = load(root)?;
    let ids: Vec<String> = p
        .scenes
        .iter()
        .filter(|s| s.act_ref() == aref)
        .map(|s| s.scene.meta.id.clone())
        .collect();
    reorder_act(root, aref, &ids)
}

/// Co-writing: remove an act folder that the shared document no longer has,
/// once it is empty (its scenes were trashed or moved by earlier steps). An
/// episode left with no acts goes too.
pub fn act_remove_empty(root: &Path, aref: ActRef) -> ApiResult {
    let dir = act_dir(root, aref);
    if dir.is_dir() {
        let has_scenes = fs::read_dir(&dir)
            .map_err(|e| io_err(&dir, e))?
            .filter_map(Result::ok)
            .any(|e| e.path().extension().and_then(|x| x.to_str()) == Some("scene"));
        if has_scenes {
            return Err(ApiError::new(
                "not_empty",
                format!("{} still has scenes", rel(root, &dir)),
            ));
        }
        fs::remove_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
    }
    if let Some(e) = aref.episode {
        let acts = root.join("episodes").join(format!("{e:02}")).join("acts");
        let empty = fs::read_dir(&acts).map(|mut d| d.next().is_none()).unwrap_or(false);
        if empty {
            let ep = acts.parent().unwrap();
            fs::remove_dir_all(ep).map_err(|err| io_err(ep, err))?;
        }
    }
    project_view(root)
}

const COLLAB_DIR: &str = ".stagewright";

fn collab_dir(root: &Path) -> Result<PathBuf, ApiError> {
    let dir = root.join(COLLAB_DIR);
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
        // Local, per-writer state: keep it out of git.
        fs::write(dir.join(".gitignore"), "*\n").map_err(|e| io_err(&dir, e))?;
    }
    Ok(dir)
}

/// Read this writer's sharing settings and saved shared state, if any.
pub fn collab_read(root: &Path) -> ApiResult {
    use base64::Engine;
    let dir = root.join(COLLAB_DIR);
    let config = match fs::read_to_string(dir.join("collab.json")) {
        Ok(t) => serde_json::from_str::<Value>(&t).map_err(|e| ApiError::new("invalid_json", e.to_string()))?,
        Err(_) => Value::Null,
    };
    let state = fs::read(dir.join("collab.ydoc"))
        .ok()
        .map(|b| base64::engine::general_purpose::STANDARD.encode(b));
    let base = fs::read_to_string(dir.join("base.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .unwrap_or(Value::Null);
    Ok(json!({ "config": config, "state": state, "base": base }))
}

/// Save sharing settings (`config`) and/or the shared document (`state`, base64).
/// `config: null` stops sharing on this machine (the files stay).
pub fn collab_write(root: &Path, args: &Value) -> ApiResult {
    use base64::Engine;
    if let Some(config) = args.get("config") {
        let dir = collab_dir(root)?;
        if config.is_null() {
            for f in ["collab.json", "collab.ydoc", "base.json"] {
                let _ = fs::remove_file(dir.join(f));
            }
        } else {
            let text = serde_json::to_string_pretty(config).unwrap();
            write_atomic(&dir.join("collab.json"), &text)?;
        }
    }
    // What this writer last wrote to disk, so edits made outside the app can be told apart.
    if let Some(base) = args.get("base").filter(|b| !b.is_null()) {
        let dir = collab_dir(root)?;
        write_atomic(&dir.join("base.json"), &serde_json::to_string(base).unwrap())?;
    }
    if let Some(state) = args.get("state").and_then(Value::as_str) {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(state)
            .map_err(|e| ApiError::new("bad_args", format!("state: {e}")))?;
        let dir = collab_dir(root)?;
        let path = dir.join("collab.ydoc");
        let tmp = path.with_extension("ydoc.tmp");
        fs::write(&tmp, bytes).map_err(|e| io_err(&tmp, e))?;
        fs::rename(&tmp, &path).map_err(|e| io_err(&path, e))?;
    }
    Ok(Value::Null)
}

/// Start a local copy of a shared project in an empty folder: write its project
/// files and sharing settings. Scenes arrive afterwards through the normal commands.
pub fn project_join(root: &Path, files: &Map<String, Value>, config: &Value) -> Result<(), ApiError> {
    if root.join("project.yaml").exists() {
        return Err(ApiError::new(
            "exists",
            format!("{} already contains a project", root.display()),
        ));
    }
    if root.exists() && fs::read_dir(root).map_err(|e| io_err(root, e))?.next().is_some() {
        return Err(ApiError::new("not_empty", format!("{} is not empty", root.display())));
    }
    if !files.contains_key("project.yaml") {
        return Err(ApiError::new("bad_args", "the shared project has no project.yaml"));
    }
    fs::create_dir_all(root.join("acts")).map_err(|e| io_err(root, e))?;
    for (name, text) in files {
        let text = text
            .as_str()
            .ok_or_else(|| ApiError::new("bad_args", "file texts must be strings"))?;
        // file_save validates the YAML and refuses names outside the project set.
        file_save(root, name, text, &version_of(b""))?;
    }
    collab_write(root, &json!({ "config": config }))?;
    Ok(())
}

// ---------------------------------------------------------------- export

/// Export the script as `pdf` or `fountain`. With `path`, write it there and
/// return the path; without, return the file (base64 for PDF) for the client to save.
pub fn export(root: &Path, kind: &str, episode: Option<u32>, path: Option<&Path>) -> ApiResult {
    use base64::Engine;
    let p = load(root)?;
    if let Some(e) = episode.filter(|e| !p.episode_numbers.contains(e)) {
        return Err(ApiError::new("not_found", format!("episode {e} does not exist")));
    }
    let (bytes, ext) = match kind {
        "pdf" => (stagewright_core::export::pdf(&p, episode), "pdf"),
        "fountain" => (stagewright_core::export::fountain(&p, episode).into_bytes(), "fountain"),
        _ => return Err(ApiError::new("bad_args", "kind must be pdf or fountain")),
    };
    let mut name = slugify(&p.meta.title);
    if let Some(e) = episode {
        name.push_str(&format!("-ep{e:02}"));
    }
    let name = format!("{name}.{ext}");
    let pages = if kind == "pdf" {
        Some(stagewright_core::export::page_count(&p, episode))
    } else {
        None
    };
    match path {
        Some(out) => {
            fs::write(out, &bytes).map_err(|e| io_err(out, e))?;
            Ok(json!({ "path": out, "name": name, "pages": pages }))
        }
        None => Ok(json!({
            "name": name,
            "pages": pages,
            "base64": base64::engine::general_purpose::STANDARD.encode(&bytes),
        })),
    }
}

#[cfg(test)]
mod tests;
