//! The script check over a project (see `stagewright_core::check`).

use crate::ops::{find_scene, load, read_text, scene_label, write_atomic};
use crate::{ApiError, ApiResult};
use serde_json::{json, Value};
use stagewright_core::analysis::scene_stats;
use stagewright_core::check::{check_scene, Context, CHECKS};
use stagewright_core::frontmatter::set_key;
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
    Context {
        format: p.meta.format,
        cast,
        known,
        introduced: Default::default(),
        off: p.meta.checks.off.iter().cloned().collect(),
    }
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
    for sf in &p.scenes {
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
    Ok(json!({ "issues": out, "warnings": warnings, "infos": infos, "checks": checks_json(&p) }))
}

/// One scene, with the text as it is in the editor (maybe unsaved).
pub fn check_one(root: &Path, id: &str, body: &str) -> ApiResult {
    let p = load(root)?;
    let target = find_scene(&p, id)?;
    let mut ctx = context(&p);
    // Characters introduced in earlier scenes count as introduced.
    for sf in &p.scenes {
        if sf.scene.meta.id == target.scene.meta.id {
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
