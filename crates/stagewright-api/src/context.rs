//! The context web in the API: notes on characters and locations, research
//! notes, beat sheets and the links between them (stagewright_core::context).
//!
//! Pages and sheets are ordinary project files, read and saved with
//! `file_read` and `file_save` (versioned, and shared when co-writing) under
//! names like `characters/dee.md`, `research/burial.md`, `plan/beats.yaml`.
//! The commands here work out what the app shows: every record and how it's
//! linked, where beats land, the context of a scene; and they edit text (a
//! page's links, a sheet) without touching the disk, so the app can save it
//! either way.

use crate::ops::{load, scene_label};
use crate::{ApiError, ApiResult};
use serde_json::{json, Value};
use stagewright_core::analysis::scene_stats;
use stagewright_core::context::{from_template, slug, BeatSheet, Kind, Notes, Page, Web, TEMPLATES};
use stagewright_core::layout::Format;
use stagewright_core::project::ProjectError;
use std::path::Path;

/// Folders of pages, and the record kind each holds.
const PAGE_DIRS: &[(&str, Kind)] = &[
    ("characters", Kind::Character),
    ("locations", Kind::Location),
    ("research", Kind::Research),
];

/// An id usable as a file name: letters, digits, `-` and `_`.
fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.chars().count() <= 80 && id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

/// Whether `name` is one of the context web's files (`characters/dee.md`,
/// `plan/beats.yaml`, `plan/episodes/02.yaml` …).
pub fn is_context_file(name: &str) -> bool {
    if name == "plan/beats.yaml" {
        return true;
    }
    if let Some(n) = name
        .strip_prefix("plan/episodes/")
        .and_then(|f| f.strip_suffix(".yaml"))
    {
        return n.len() == 2 && n.bytes().all(|b| b.is_ascii_digit());
    }
    PAGE_DIRS.iter().any(|(dir, _)| {
        name.strip_prefix(dir)
            .and_then(|r| r.strip_prefix('/'))
            .and_then(|f| f.strip_suffix(".md"))
            .is_some_and(safe_id)
    })
}

/// The context web's files in the project, for sharing when co-writing.
pub fn context_files(root: &Path) -> Vec<String> {
    let in_dir = |dir: &str| -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(root.join(dir))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(|f| format!("{dir}/{f}")))
            .filter(|n| is_context_file(n))
            .collect();
        names.sort();
        names
    };
    let mut out: Vec<String> = PAGE_DIRS.iter().flat_map(|(dir, _)| in_dir(dir)).collect();
    if root.join("plan/beats.yaml").is_file() {
        out.push("plan/beats.yaml".into());
    }
    out.extend(in_dir("plan/episodes"));
    out
}

/// Check a context file's text before it's saved.
pub fn validate(name: &str, text: &str) -> Result<(), ApiError> {
    let bad = |m: String| ApiError::new("invalid_yaml", m);
    if name.ends_with(".md") {
        Page::parse(text).map(drop).map_err(bad)
    } else {
        let sheet: BeatSheet = serde_yaml::from_str(text).map_err(|e| bad(e.to_string()))?;
        let mut ids = std::collections::BTreeSet::new();
        match sheet
            .beats
            .iter()
            .find(|b| !safe_id(&b.id) || !ids.insert(b.id.as_str()))
        {
            Some(b) => Err(bad(format!(
                "beat id '{}' is missing, used twice or not a plain name",
                b.id
            ))),
            None => Ok(()),
        }
    }
}

fn notes(root: &Path) -> Result<Notes, ApiError> {
    Notes::load(root).map_err(|e: ProjectError| ApiError::new("invalid_yaml", e.to_string()))
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Character => "character",
        Kind::Location => "location",
        Kind::Scene => "scene",
        Kind::Beat => "beat",
        Kind::Research => "research",
    }
}

/// Where a record's page is kept, if it can have one.
pub fn page_file(kind: Kind, id: &str) -> Option<String> {
    let (dir, _) = PAGE_DIRS.iter().find(|(_, k)| *k == kind)?;
    safe_id(id).then(|| format!("{dir}/{id}.md"))
}

/// Everything in the web: each record (with its page's file, if it has one),
/// the beat sheets with where each beat lands, and what's broken.
pub fn index(root: &Path) -> ApiResult {
    let p = load(root)?;
    let n = notes(root)?;
    let web = Web::build(&p, &n);
    let has_page = |k: Kind, id: &str| match k {
        Kind::Character => n.characters.contains_key(id),
        Kind::Location => n.locations.contains_key(id),
        Kind::Research => true,
        _ => false,
    };
    let labels: std::collections::BTreeMap<&str, String> = p
        .scenes
        .iter()
        .map(|s| (s.scene.meta.id.as_str(), scene_label(s)))
        .collect();
    let records: Vec<Value> = web
        .nodes
        .iter()
        .map(|r| {
            json!({
                "id": r.id, "kind": kind_name(r.kind), "name": r.name,
                "label": labels.get(r.id.as_str()),
                "file": page_file(r.kind, &r.id), "has_page": has_page(r.kind, &r.id),
            })
        })
        .collect();

    // Where beats land: the start of a beat's first scene (or episode), as a
    // share of its sheet's length, in running time.
    let secs: Vec<f64> = p
        .scenes
        .iter()
        .map(|s| scene_stats(&s.scene, &p.meta).seconds())
        .collect();
    let screen = p.meta.format != Format::Stage;
    let sheets: Vec<Value> = n
        .sheets
        .iter()
        .map(|(episode, sheet)| {
            let within: Vec<usize> = (0..p.scenes.len()).filter(|&i| episode.is_none() || p.scenes[i].episode == *episode).collect();
            let total: f64 = within.iter().map(|&i| secs[i]).sum();
            let start_of = |i: usize| -> f64 { within.iter().take_while(|&&j| j != i).map(|&j| secs[j]).sum() };
            let beats: Vec<Value> = sheet
                .beats
                .iter()
                .map(|b| {
                    let first = b
                        .scenes
                        .iter()
                        .filter_map(|id| within.iter().copied().find(|&i| p.scenes[i].scene.meta.id == *id))
                        .min()
                        .or_else(|| {
                            let ep = b.episodes.iter().min()?;
                            within.iter().copied().find(|&i| p.scenes[i].episode == Some(*ep))
                        });
                    let lands = first.filter(|_| total > 0.0).map(|i| start_of(i) / total);
                    // As a page (screen, about a minute a page) or a minute in.
                    let at = lands.map(|l| {
                        let minutes = l * total / 60.0;
                        if screen {
                            let per_page = p.meta.running_time.minutes_per_page.unwrap_or(1.0).max(0.1);
                            format!("page {}", (minutes / per_page).floor() as u32 + 1)
                        } else {
                            format!("minute {}", minutes.floor() as u32)
                        }
                    });
                    let mut v = serde_json::to_value(b).unwrap_or_default();
                    // Always present for the app, though a sheet leaves them out when empty.
                    v["scenes"] = json!(b.scenes);
                    v["episodes"] = json!(b.episodes);
                    v["lands"] = json!(lands);
                    v["lands_at"] = json!(at);
                    v
                })
                .collect();
            json!({
                "episode": episode, "template": sheet.template, "beats": beats,
                "file": stagewright_core::context::sheet_path(Path::new(""), *episode).to_string_lossy().replace('\\', "/"),
            })
        })
        .collect();
    let templates: Vec<Value> = TEMPLATES
        .iter()
        .map(|(id, name)| json!({ "id": id, "name": name }))
        .collect();
    Ok(json!({
        "records": records,
        "sheets": sheets,
        "templates": templates,
        "broken": web.broken().iter().map(|(from, to)| json!({ "from": from, "to": to })).collect::<Vec<_>>(),
        "duplicates": web.duplicates(),
    }))
}

/// One record: what it's linked to, either way, and its page's text.
pub fn record(root: &Path, id: &str) -> ApiResult {
    let p = load(root)?;
    let n = notes(root)?;
    let web = Web::build(&p, &n);
    let node = web
        .node(id)
        .ok_or_else(|| ApiError::new("not_found", format!("nothing called '{id}'")))?;
    Ok(json!({
        "id": node.id, "kind": kind_name(node.kind), "name": node.name,
        "file": page_file(node.kind, id),
        "links": neighbours(&web, id),
    }))
}

fn neighbours(web: &Web, id: &str) -> Vec<Value> {
    web.around(id)
        .into_iter()
        .map(|n| {
            json!({
                "id": n.id, "kind": kind_name(n.kind), "name": n.name, "label": n.label,
                "incoming": n.incoming, "how": n.how,
            })
        })
        .collect()
}

/// What's around a scene, for the panel beside the script and the AI: its
/// beats, characters and location, with their pages, and research linked to
/// any of those or to the scene. One hop out from the scene, plus research.
pub fn scene_context(root: &Path, id: &str) -> ApiResult {
    let p = load(root)?;
    let n = notes(root)?;
    let web = Web::build(&p, &n);
    if web.node(id).is_none_or(|r| r.kind != Kind::Scene) {
        return Err(ApiError::new("not_found", format!("no scene '{id}'")));
    }
    let page_text = |kind: Kind, rid: &str| -> Option<String> {
        let page = match kind {
            Kind::Character => n.characters.get(rid),
            Kind::Location => n.locations.get(rid),
            Kind::Research => n.research.iter().find(|r| r.id == rid).map(|r| &r.page),
            _ => None,
        };
        page.map(|pg| pg.body.trim().to_string()).filter(|t| !t.is_empty())
    };
    let beat = |bid: &str| n.sheets.values().flat_map(|s| &s.beats).find(|b| b.id == bid);
    let mut items: Vec<Value> = Vec::new();
    let mut seen = std::collections::BTreeSet::from([id.to_string()]);
    let around = web.around(id);
    let mut push = |nb: &stagewright_core::context::Neighbour, via: Option<&str>| {
        if !seen.insert(nb.id.clone()) {
            return;
        }
        let mut v = json!({
            "id": nb.id, "kind": kind_name(nb.kind), "name": nb.name, "label": nb.label,
            "text": page_text(nb.kind, &nb.id), "via": via,
        });
        if let Some(b) = beat(&nb.id) {
            v["summary"] = json!(b.summary);
            v["purpose"] = json!(b.purpose);
        }
        items.push(v);
    };
    for nb in &around {
        if nb.kind != Kind::Scene {
            push(nb, None);
        }
    }
    // Research linked to the scene's characters, location or beats.
    for nb in &around {
        if matches!(nb.kind, Kind::Character | Kind::Location | Kind::Beat) {
            for r in web.around(&nb.id).iter().filter(|r| r.kind == Kind::Research) {
                push(r, Some(&nb.name));
            }
        }
    }
    let order = |k: &str| {
        ["beat", "character", "location", "research"]
            .iter()
            .position(|x| *x == k)
            .unwrap_or(9)
    };
    items.sort_by_key(|v| order(v["kind"].as_str().unwrap_or("")));
    Ok(json!({ "scene": id, "items": items }))
}

/// A page's parts: its frontmatter (`title`, `tags`, `sources`, `links`) and
/// its text. Args: `text`.
pub fn page_parse(args: &Value) -> ApiResult {
    let page = Page::parse(args.get("text").and_then(Value::as_str).unwrap_or(""))
        .map_err(|e| ApiError::new("invalid_yaml", e))?;
    Ok(json!({ "meta": page.meta, "body": page.body }))
}

/// A page's text with one part changed: a frontmatter key (`links`, `title`,
/// `tags`, `sources`; `null` removes it) or the `body`. Args: `text`, `key`,
/// `value`.
pub fn page_set(args: &Value) -> ApiResult {
    let key = args.get("key").and_then(Value::as_str).unwrap_or("");
    let value = args.get("value").unwrap_or(&Value::Null);
    let mut page = Page::parse(args.get("text").and_then(Value::as_str).unwrap_or(""))
        .map_err(|e| ApiError::new("invalid_yaml", e))?;
    match key {
        "body" => page.body = value.as_str().unwrap_or("").to_string(),
        "links" | "title" | "tags" | "sources" => page.set(key, value).map_err(|e| ApiError::new("invalid_yaml", e))?,
        _ => return Err(ApiError::new("bad_args", format!("'{key}' can't be set on a page"))),
    }
    Ok(json!({ "text": page.to_text() }))
}

/// A new research note's id (from its title, unique in the project) and text.
pub fn research_new(root: &Path, title: &str) -> ApiResult {
    let p = load(root)?;
    let n = notes(root)?;
    let web = Web::build(&p, &n);
    let base = Some(slug(title))
        .filter(|s| !s.is_empty() && safe_id(s))
        .unwrap_or_else(|| "note".into());
    let id = (1..)
        .map(|k| if k == 1 { base.clone() } else { format!("{base}-{k}") })
        .find(|id| web.node(id).is_none() && !root.join("research").join(format!("{id}.md")).exists())
        .expect("an unused id");
    let text = format!(
        "---\ntitle: {}\n---\n",
        serde_json::to_string(title.trim()).unwrap_or_default()
    );
    Ok(json!({ "id": id, "file": format!("research/{id}.md"), "text": text }))
}

/// A sheet as YAML text. Args: `sheet` (as `index` gives it; extra fields such
/// as where beats land are dropped).
pub fn sheet_text(args: &Value) -> ApiResult {
    let sheet: BeatSheet = serde_json::from_value(args.get("sheet").cloned().unwrap_or_default())
        .map_err(|e| ApiError::new("bad_args", e.to_string()))?;
    let text = serde_yaml::to_string(&sheet).map_err(|e| ApiError::new("internal", e.to_string()))?;
    validate("plan/beats.yaml", &text)?;
    Ok(json!({ "text": text }))
}

/// A new sheet from a template. Args: `template`, optional `episode`.
pub fn sheet_template(args: &Value) -> ApiResult {
    let name = args.get("template").and_then(Value::as_str).unwrap_or("");
    let episode = args.get("episode").and_then(Value::as_u64).map(|e| e as u32);
    let sheet = if name.is_empty() {
        BeatSheet::default()
    } else {
        from_template(name, episode).ok_or_else(|| ApiError::new("bad_args", format!("no template called '{name}'")))?
    };
    Ok(json!({ "sheet": sheet }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        crate::ops::tests::copy_dir(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/stage-sample"),
            d.path(),
        );
        d
    }

    #[test]
    fn context_file_names() {
        for ok in [
            "characters/dee.md",
            "research/burial-1559.md",
            "plan/beats.yaml",
            "plan/episodes/02.yaml",
        ] {
            assert!(is_context_file(ok), "{ok}");
        }
        for bad in [
            "characters/../x.md",
            "research/a b.md",
            "plan/episodes/2.yaml",
            "notes/x.md",
            "characters/x.txt",
        ] {
            assert!(!is_context_file(bad), "{bad}");
        }
        assert!(validate(
            "plan/beats.yaml",
            "beats:\n  - {id: a, title: A}\n  - {id: a, title: B}\n"
        )
        .is_err());
        assert!(validate("research/x.md", "---\ntitle: x\n").is_err());
    }

    #[test]
    fn index_records_and_where_beats_land() {
        let d = sample();
        let root = d.path();
        std::fs::create_dir_all(root.join("plan")).unwrap();
        let mut sheet = from_template("three-act", None).unwrap();
        sheet.beats[0].scenes = vec!["sc-0-0".into()];
        sheet.beats[3].scenes = vec!["sc-2-1".into()];
        std::fs::write(root.join("plan/beats.yaml"), serde_yaml::to_string(&sheet).unwrap()).unwrap();
        std::fs::create_dir_all(root.join("characters")).unwrap();
        std::fs::write(root.join("characters/hettie.md"), "Sharp. Knows [[tobias]] too well.\n").unwrap();
        let v = index(root).unwrap();
        let rec = |id: &str| {
            v["records"]
                .as_array()
                .unwrap()
                .iter()
                .find(|r| r["id"] == id)
                .cloned()
                .unwrap()
        };
        assert_eq!(rec("hettie")["has_page"], true);
        assert_eq!(rec("hettie")["file"], "characters/hettie.md");
        assert_eq!(rec("tobias")["has_page"], false);
        assert_eq!(rec("sc-1-1")["label"], "1.1");
        let beats = v["sheets"][0]["beats"].as_array().unwrap();
        assert_eq!(beats[0]["lands"], 0.0);
        let mid = beats[3]["lands"].as_f64().unwrap();
        assert!(mid > 0.3 && mid < 0.8, "{mid}");
        assert!(beats[3]["lands_at"].as_str().unwrap().starts_with("minute "));
        assert!(beats[1]["lands"].is_null());
        assert_eq!(
            (beats[1]["scenes"].clone(), beats[1]["episodes"].clone()),
            (json!([]), json!([]))
        );
        assert_eq!(v["sheets"][0]["file"], "plan/beats.yaml");
        assert_eq!(v["templates"].as_array().unwrap().len(), TEMPLATES.len());

        // Tobias's record sees Hettie's page link, and his scenes.
        let t = record(root, "tobias").unwrap();
        let links = t["links"].as_array().unwrap();
        assert!(links.iter().any(|l| l["id"] == "hettie" && l["incoming"] == true));
        assert!(links.iter().any(|l| l["id"] == "sc-1-1" && l["label"] == "appears in"));
    }

    #[test]
    fn a_scenes_context() {
        let d = sample();
        let root = d.path();
        std::fs::create_dir_all(root.join("research")).unwrap();
        std::fs::write(
            root.join("research/bunting.md"),
            "---\ntitle: Bunting\nlinks:\n  - {to: tobias}\n---\nFlags, 1890s.\n",
        )
        .unwrap();
        std::fs::create_dir_all(root.join("locations")).unwrap();
        std::fs::write(root.join("locations/quay.md"), "Granite, slippery.\n").unwrap();
        let v = scene_context(root, "sc-1-1").unwrap();
        let items = v["items"].as_array().unwrap();
        let kinds: Vec<&str> = items.iter().map(|i| i["kind"].as_str().unwrap()).collect();
        assert!(
            kinds
                .windows(2)
                .all(|w| w[0] <= w[1] || w[0] == "beat" || w[1] == "research"),
            "{kinds:?}"
        );
        let quay = items.iter().find(|i| i["id"] == "quay").unwrap();
        assert_eq!(quay["text"], "Granite, slippery.");
        let bunting = items.iter().find(|i| i["id"] == "bunting").unwrap();
        assert_eq!(
            (bunting["via"].as_str(), bunting["text"].as_str()),
            (Some("Tobias"), Some("Flags, 1890s."))
        );
        assert!(scene_context(root, "tobias").is_err());
    }

    #[test]
    fn notes_are_project_files() {
        let d = sample();
        let mut ws = crate::Workspace::new(Some(d.path().to_path_buf()));
        let call = |ws: &mut crate::Workspace, cmd: &str, args: Value| crate::dispatch(ws, cmd, args);
        let empty = call(&mut ws, "file_read", json!({ "name": "characters/tobias.md" })).unwrap();
        assert_eq!(empty["text"], "");
        let saved = call(
            &mut ws,
            "file_save",
            json!({ "name": "characters/tobias.md", "text": "A lampman.\n", "base_version": empty["version"] }),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(d.path().join("characters/tobias.md")).unwrap(),
            "A lampman.\n"
        );
        let bad = call(
            &mut ws,
            "file_save",
            json!({ "name": "characters/tobias.md", "text": "---\nx: [\n", "base_version": saved["version"] }),
        );
        assert!(bad.is_err());
        assert!(call(&mut ws, "file_read", json!({ "name": "characters/../project.yaml" })).is_err());
        // Shared when co-writing.
        let snap = call(&mut ws, "project_snapshot", json!({})).unwrap();
        assert!(snap["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["name"] == "characters/tobias.md"));
        // A broken link is noted by the project check.
        std::fs::write(d.path().join("characters/hettie.md"), "Knows [[nobody]].\n").unwrap();
        let checked = crate::checks::check_project(d.path()).unwrap();
        let notes: Vec<&Value> = checked["issues"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["code"] == "broken-link")
            .collect();
        assert_eq!(notes.len(), 1, "{notes:?}");
        assert!(notes[0]["message"]
            .as_str()
            .unwrap()
            .starts_with("Hettie links to \u{201c}nobody\u{201d}"));
        // Deleted to the trash.
        call(&mut ws, "file_trash", json!({ "name": "characters/tobias.md" })).unwrap();
        assert!(!d.path().join("characters/tobias.md").exists());
        assert!(call(&mut ws, "file_trash", json!({ "name": "project.yaml" })).is_err());
    }

    #[test]
    fn editing_text() {
        let v =
            page_set(&json!({ "text": "Notes.\n", "key": "links", "value": [{ "to": "dee", "label": "scryer to" }] }))
                .unwrap();
        assert_eq!(
            v["text"],
            "---\nlinks: [{\"label\":\"scryer to\",\"to\":\"dee\"}]\n---\nNotes.\n"
        );
        let v = page_set(&json!({ "text": v["text"], "key": "links", "value": null })).unwrap();
        assert_eq!(v["text"], "Notes.\n");
        assert!(page_set(&json!({ "text": "", "key": "colour", "value": 1 })).is_err());
        let v = page_set(&json!({ "text": "---\ntitle: T\n---\nOld.\n", "key": "body", "value": "New.\n" })).unwrap();
        assert_eq!(v["text"], "---\ntitle: T\n---\nNew.\n");
        let parts = page_parse(&json!({ "text": v["text"] })).unwrap();
        assert_eq!(
            (parts["meta"]["title"].as_str(), parts["body"].as_str()),
            (Some("T"), Some("New.\n"))
        );
        let d = sample();
        std::fs::create_dir_all(d.path().join("research")).unwrap();
        std::fs::write(d.path().join("research/harbour-signs.md"), "x").unwrap();
        let v = research_new(d.path(), "Harbour signs").unwrap();
        assert_eq!(
            (v["id"].as_str(), v["file"].as_str()),
            (Some("harbour-signs-2"), Some("research/harbour-signs-2.md"))
        );
        assert_eq!(v["text"], "---\ntitle: \"Harbour signs\"\n---\n");
        assert_eq!(
            research_new(d.path(), "Tobias").unwrap()["id"],
            "tobias-2",
            "ids are unique across kinds"
        );
        let s = sheet_template(&json!({ "template": "freytag", "episode": 3 })).unwrap();
        assert_eq!(s["sheet"]["beats"][0]["id"], "e3-exposition");
        let t = sheet_text(&json!({ "sheet": s["sheet"] })).unwrap();
        assert!(t["text"].as_str().unwrap().starts_with("template: freytag\nbeats:\n"));
        assert!(sheet_template(&json!({ "template": "nope" })).is_err());
    }
}
