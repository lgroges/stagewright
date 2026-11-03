//! The script check over a project (see `stagewright_core::check`).

use crate::ops::{find_scene, load, read_text, scene_label, write_atomic};
use crate::{ApiError, ApiResult};
use serde_json::{json, Value};
use stagewright_core::analysis::scene_stats;
use stagewright_core::check::{check_cues, check_scene, Context, Person, Severity, CHECKS};
use stagewright_core::frontmatter::set_key;
use stagewright_core::layout::Format;
use stagewright_core::project::ProjectMeta;
use stagewright_core::Project;
use std::path::Path;

/// What every scene is checked against: the cast list, all spoken cues, and
/// the checks switched off in project.yaml.
fn context(p: &Project) -> Context {
    let cast = p.characters.iter().map(|c| c.cue.clone()).collect();
    let mut known: std::collections::BTreeSet<String> = p.characters.iter().map(|c| c.cue.clone()).collect();
    for sf in &p.scenes {
        known.extend(scene_stats(&sf.scene, &p.meta).speeches.keys().cloned());
    }
    let people = p
        .characters
        .iter()
        .map(|c| Person {
            cue: c.cue.clone(),
            name: c.name.clone(),
            short: c.short.clone().filter(|s| !s.trim().is_empty()),
        })
        .collect();
    Context {
        format: p.meta.format,
        cast,
        known,
        introduced: Default::default(),
        off: p.meta.checks.off.iter().cloned().collect(),
        people,
        auto_contd: p.meta.auto_contd(),
        shooting: p.meta.shooting_script,
        fade_out_here: false,
    }
}

/// Whether FADE OUT. may close scene `i` (in running order): the last scene
/// of a film, or of a TV act.
fn fade_out_here(p: &Project, i: usize) -> bool {
    match p.meta.format {
        Format::Film => i + 1 == p.scenes.len(),
        Format::TvSingle | Format::TvMulti => p
            .scenes
            .get(i + 1)
            .is_none_or(|next| next.act_ref() != p.scenes[i].act_ref()),
        Format::Stage => true,
    }
}

/// A project-level issue: not on a line of any scene.
fn project_issue(code: &str, message: String, label: &str, title: &str) -> Value {
    json!({
        "code": code,
        "severity": Severity::Info,
        "line": 0, "from": 0, "to": 0,
        "message": message,
        "fix": null,
        "scene": "", "label": label, "title": title, "text": "",
    })
}

fn checks_json(p: &Project) -> Value {
    json!(CHECKS
        .iter()
        .map(|(code, name)| json!({ "code": code, "name": name, "on": !p.meta.checks.off.iter().any(|o| o == code) }))
        .collect::<Vec<_>>())
}

fn body_of(sf: &stagewright_core::project::SceneFile) -> String {
    stagewright_core::scene::serialize_body(&sf.scene.body) + if sf.scene.trailing_newline { "\n" } else { "" }
}

/// Every issue in the project, scene by scene in running order.
pub fn check_project(root: &Path) -> ApiResult {
    let p = load(root)?;
    let mut ctx = context(&p);
    let mut out = Vec::new();
    let (mut warnings, mut infos) = (0, 0);
    for (n, sf) in p.scenes.iter().enumerate() {
        ctx.fade_out_here = fade_out_here(&p, n);
        let body = body_of(sf);
        let lines: Vec<&str> = body.split('\n').collect();
        for i in check_scene(&body, &sf.scene.meta, &mut ctx) {
            match i.severity {
                stagewright_core::check::Severity::Warning => warnings += 1,
                stagewright_core::check::Severity::Info => infos += 1,
            }
            let mut v = serde_json::to_value(&i).unwrap();
            v["scene"] = json!(sf.scene.meta.id);
            v["label"] = json!(scene_label(sf));
            v["title"] = json!(sf.scene.meta.title.clone().unwrap_or_else(|| sf.scene.meta.id.clone()));
            v["text"] = json!(lines.get(i.line).copied().unwrap_or(""));
            out.push(v);
        }
    }
    // The project itself: the cues, then what the title page will say.
    let cast_notes = check_cues(&ctx);
    for (k, note) in cast_notes.into_iter().enumerate() {
        infos += 1;
        out.insert(k, project_issue(note.code, note.message, "Cast", "characters.yaml"));
    }
    if !ctx.off.contains("title-page") {
        let title = p.meta.title.trim();
        let slug_like =
            !title.contains(' ') && (title.contains('-') || title.contains('_')) && title == title.to_lowercase();
        let mut problems = Vec::new();
        if title.is_empty() {
            problems.push("has no title".to_string());
        } else if slug_like {
            problems.push(format!("will read \"{title}\", which looks like a folder name"));
        }
        if p.meta.authors.iter().all(|a| a.trim().is_empty()) {
            problems.push("has no author".to_string());
        }
        if p.meta.contact.as_deref().is_none_or(|c| c.trim().is_empty()) {
            problems.push("has no contact details (address, phone and email, for the bottom-left corner)".to_string());
        }
        if !problems.is_empty() {
            infos += 1;
            let last = problems.pop().unwrap();
            let all = if problems.is_empty() {
                last
            } else {
                format!("{}, and {last}", problems.join(", "))
            };
            out.insert(
                0,
                project_issue(
                    "title-page",
                    format!("The title page {all}. Set title, authors and contact in project.yaml."),
                    "Project",
                    "project.yaml",
                ),
            );
        }
    }
    Ok(json!({ "issues": out, "warnings": warnings, "infos": infos, "checks": checks_json(&p) }))
}

/// One scene, with the text as it is in the editor (maybe unsaved).
pub fn check_one(root: &Path, id: &str, body: &str) -> ApiResult {
    let p = load(root)?;
    let target = find_scene(&p, id)?;
    let mut ctx = context(&p);
    // Characters introduced in earlier scenes count as introduced.
    for (n, sf) in p.scenes.iter().enumerate() {
        if sf.scene.meta.id == target.scene.meta.id {
            ctx.fade_out_here = fade_out_here(&p, n);
            break;
        }
        check_scene(&body_of(sf), &sf.scene.meta, &mut ctx);
    }
    let issues = check_scene(&body.replace("\r\n", "\n"), &target.scene.meta, &mut ctx);
    Ok(json!({ "issues": issues }))
}

/// project.yaml with check `code` switched on or off.
pub fn yaml_set_check(text: &str, code: &str, on: bool) -> Result<String, ApiError> {
    if !CHECKS.iter().any(|(c, _)| *c == code) {
        return Err(ApiError::new("bad_args", format!("no check called '{code}'")));
    }
    let meta: ProjectMeta = serde_yaml::from_str(text).map_err(|e| ApiError::new("invalid_yaml", e.to_string()))?;
    let mut off: Vec<String> = meta.checks.off.into_iter().filter(|c| c != code).collect();
    if !on {
        off.push(code.to_string());
    }
    off.sort();
    let updated = if off.is_empty() {
        // Nothing switched off: drop the key rather than leave `checks: {off: []}`.
        set_key(text, "checks", &Value::Null).replace("checks: null\n", "")
    } else {
        set_key(text, "checks", &json!({ "off": off }))
    };
    serde_yaml::from_str::<ProjectMeta>(&updated).map_err(|e| ApiError::new("invalid_yaml", e.to_string()))?;
    Ok(updated)
}

pub fn set_check(root: &Path, code: &str, on: bool) -> ApiResult {
    let path = root.join("project.yaml");
    let text = read_text(&path)?;
    let updated = yaml_set_check(&text, code, on)?;
    write_atomic(&path, &updated)?;
    let mut v = check_project(root)?;
    v["files"] = json!({ "project.yaml": updated });
    Ok(v)
}

// ---------------------------------------------------------------- project options

/// Options the app may set in project.yaml.
const OPTIONS: &[&str] = &["auto_contd", "shooting_script"];

/// project.yaml with option `name` set (`null` removes it, back to the default).
pub fn yaml_set_option(text: &str, name: &str, value: &Value) -> Result<String, ApiError> {
    if !OPTIONS.contains(&name) {
        return Err(ApiError::new("bad_args", format!("'{name}' is not a project option")));
    }
    let updated = if value.is_null() {
        let removed = set_key(text, name, &Value::Null);
        removed.replace(&format!("{name}: null\n"), "")
    } else {
        set_key(text, name, value)
    };
    serde_yaml::from_str::<ProjectMeta>(&updated).map_err(|e| ApiError::new("invalid_yaml", e.to_string()))?;
    Ok(updated)
}

pub fn set_option(root: &Path, name: &str, value: &Value) -> ApiResult {
    let path = root.join("project.yaml");
    let text = read_text(&path)?;
    let updated = yaml_set_option(&text, name, value)?;
    write_atomic(&path, &updated)?;
    let mut v = crate::ops::project_view(root)?;
    v["files"] = json!({ "project.yaml": updated });
    Ok(v)
}
