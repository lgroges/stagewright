use crate::{dispatch, Workspace};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

/// Copy the sample play fixture into a temp dir so tests can mutate it.
fn stage_sample() -> (tempfile::TempDir, Workspace) {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/stage-sample");
    let tmp = tempfile::tempdir().unwrap();
    copy_dir(&src, tmp.path());
    let ws = Workspace::new(Some(tmp.path().to_path_buf()));
    (tmp, ws)
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            fs::copy(e.path(), dest).unwrap();
        }
    }
}

fn call(ws: &mut Workspace, cmd: &str, args: Value) -> Value {
    dispatch(ws, cmd, args).unwrap_or_else(|e| panic!("{cmd} failed: {e}"))
}

fn act_ids(view: &Value, act: u64) -> Vec<String> {
    view["acts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["number"] == act)
        .unwrap()["scenes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_string())
        .collect()
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into())
        .collect();
    v.sort();
    v
}

#[test]
fn project_view_lists_acts_scenes_and_cues() {
    let (_t, mut ws) = stage_sample();
    let v = call(&mut ws, "project_view", json!({}));
    assert_eq!(v["title"], "Harbour Lights");
    assert_eq!(v["acts"].as_array().unwrap().len(), 3);
    assert_eq!(v["acts"][0]["title"], "Prologue");
    assert_eq!(act_ids(&v, 1).len(), 6);
    let cues: Vec<&str> = v["cues"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    assert!(cues.contains(&"MORDECAI") && cues.contains(&"MUSICIAN 1"));
    assert_eq!(v["rating_fields"].as_array().unwrap().len(), 4);
}

#[test]
fn read_then_save_round_trips_and_bumps_version() {
    let (_t, mut ws) = stage_sample();
    let r = call(&mut ws, "scene_read", json!({"id": "sc-1-3"}));
    let body = r["body"].as_str().unwrap().to_string();
    let path = _t.path().join("acts/01/030-the-cliff-path.scene");
    let before = fs::read_to_string(&path).unwrap();
    // Saving the unchanged body writes the identical file.
    let same = call(
        &mut ws,
        "scene_save",
        json!({"id": "sc-1-3", "body": body, "base_version": r["version"]}),
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), before);
    assert_eq!(same["version"], r["version"]);
    // An edit changes the version and lands on disk.
    let edited = body.replacen("MORDECAI\n", "MORDECAI\n(brightly)\n", 1);
    let s = call(
        &mut ws,
        "scene_save",
        json!({"id": "sc-1-3", "body": edited, "base_version": r["version"]}),
    );
    assert_ne!(s["version"], r["version"]);
    assert!(fs::read_to_string(&path).unwrap().contains("(brightly)"));
}

#[test]
fn save_with_stale_version_is_a_conflict() {
    let (_t, mut ws) = stage_sample();
    let r = call(&mut ws, "scene_read", json!({"id": "sc-0-0"}));
    let path = _t.path().join("acts/00/000-prologue.scene");
    fs::write(&path, fs::read_to_string(&path).unwrap() + "\nEdited elsewhere.\n").unwrap();
    let err = dispatch(
        &mut ws,
        "scene_save",
        json!({"id": "sc-0-0", "body": "x\n", "base_version": r["version"]}),
    )
    .unwrap_err();
    assert_eq!(err.code, "conflict");
    let disk_version = err.detail.unwrap()["version"].clone();
    // The client can choose to overwrite by sending the disk version.
    call(
        &mut ws,
        "scene_save",
        json!({"id": "sc-0-0", "body": "x\n", "base_version": disk_version}),
    );
    assert!(fs::read_to_string(&path).unwrap().ends_with("---\nx\n"));
}

#[test]
fn crlf_body_is_saved_as_lf() {
    let (_t, mut ws) = stage_sample();
    let r = call(&mut ws, "scene_read", json!({"id": "sc-0-0"}));
    call(
        &mut ws,
        "scene_save",
        json!({"id": "sc-0-0", "body": "A\r\n\r\nADMIRAL\r\nHi.\r\n", "base_version": r["version"]}),
    );
    let text = fs::read_to_string(_t.path().join("acts/00/000-prologue.scene")).unwrap();
    assert!(!text.contains('\r'));
}

#[test]
fn update_meta_keeps_comments_and_renames_file_on_title_change() {
    let (_t, mut ws) = stage_sample();
    let r = call(&mut ws, "scene_read", json!({"id": "sc-1-4"}));
    let u = call(
        &mut ws,
        "scene_update_meta",
        json!({"id": "sc-1-4", "base_version": r["version"], "patch": {
            "title": "The map room", "duration": 12.5, "location": "chart-room",
            "onstage_silent": ["CONSTABLE"], "ratings": {"tension": 7}, "synopsis": "The King plans a war.\nThe Queen doesn't."
        }}),
    );
    assert_eq!(u["meta"]["title"], "The map room");
    assert_eq!(u["meta"]["duration"], 12.5);
    assert_eq!(u["meta"]["ratings"]["tension"], 7);
    assert_eq!(u["meta"]["synopsis"], "The King plans a war.\nThe Queen doesn't.");
    assert_eq!(u["summary"]["manual_duration"], true);
    let dir = _t.path().join("acts/01");
    assert!(
        files_in(&dir).contains(&"040-the-map-room.scene".to_string()),
        "{:?}",
        files_in(&dir)
    );
    let text = fs::read_to_string(dir.join("040-the-map-room.scene")).unwrap();
    assert!(text.contains("duration: 12.5  # manual override, minutes"), "{text}");
    assert!(text.contains("source: first draft notebook"));
}

#[test]
fn update_meta_rejects_bad_values_and_keys() {
    let (_t, mut ws) = stage_sample();
    let r = call(&mut ws, "scene_read", json!({"id": "sc-1-1"}));
    let v = r["version"].clone();
    for patch in [
        json!({"status": "finished"}),
        json!({"duration": -3}),
        json!({"ratings": {"tension": 12}}),
        json!({"ratings": {"spice": 3}}),
        json!({"title": "  "}),
        json!({"act": 4}),
        json!({"id": "sc-x"}),
    ] {
        let e = dispatch(
            &mut ws,
            "scene_update_meta",
            json!({"id": "sc-1-1", "base_version": v, "patch": patch}),
        )
        .unwrap_err();
        assert!(e.code == "bad_value" || e.code == "bad_key", "{patch}: {e}");
    }
}

#[test]
fn reorder_within_an_act_renames_and_renumbers() {
    let (_t, mut ws) = stage_sample();
    let v = call(&mut ws, "project_view", json!({}));
    let mut ids = act_ids(&v, 2);
    ids.swap(0, 3); // put "Outside the lighthouse" first
    let v2 = call(&mut ws, "scenes_reorder", json!({"act": 2, "ids": ids}));
    assert_eq!(act_ids(&v2, 2), ids);
    let dir = _t.path().join("acts/02");
    assert_eq!(
        files_in(&dir),
        vec![
            "010-outside-the-lighthouse.scene",
            "020-mordecais-lodgings.scene",
            "030-the-storm.scene",
            "040-hetties-cottage.scene"
        ]
    );
    let r = call(&mut ws, "scene_read", json!({"id": "sc-2-4"}));
    assert_eq!(r["meta"]["scene"], 1);
    assert_eq!(r["meta"]["act"], 2);
}

#[test]
fn moving_a_scene_between_acts_renumbers_both() {
    let (_t, mut ws) = stage_sample();
    let v = call(&mut ws, "project_view", json!({}));
    let mut act2 = act_ids(&v, 2);
    act2.insert(0, "sc-1-6".into()); // last scene of act 1 opens act 2
    let v2 = call(&mut ws, "scenes_reorder", json!({"act": 2, "ids": act2}));
    assert_eq!(act_ids(&v2, 1).len(), 5);
    assert_eq!(act_ids(&v2, 2).len(), 5);
    assert_eq!(act_ids(&v2, 2)[0], "sc-1-6");
    let r = call(&mut ws, "scene_read", json!({"id": "sc-1-6"}));
    assert_eq!(
        (r["meta"]["act"].clone(), r["meta"]["scene"].clone()),
        (json!(2), json!(1))
    );
    assert!(files_in(&_t.path().join("acts/02")).contains(&"010-the-morning-after.scene".to_string()));
    // The project still loads, and every scene still round-trips.
    let p = stagewright_core::Project::load(_t.path()).unwrap();
    for sf in &p.scenes {
        assert_eq!(sf.scene.to_source(), fs::read_to_string(&sf.path).unwrap());
    }
}

#[test]
fn reorder_refuses_to_drop_a_scene() {
    let (_t, mut ws) = stage_sample();
    let v = call(&mut ws, "project_view", json!({}));
    let mut ids = act_ids(&v, 2);
    ids.pop();
    let e = dispatch(&mut ws, "scenes_reorder", json!({"act": 2, "ids": ids})).unwrap_err();
    assert_eq!(e.code, "bad_args");
    assert_eq!(files_in(&_t.path().join("acts/02")).len(), 4);
}

#[test]
fn create_scene_after_another_and_trash_it() {
    let (_t, mut ws) = stage_sample();
    let v = call(
        &mut ws,
        "scene_create",
        json!({"act": 1, "after": "sc-1-2", "title": "A new interlude"}),
    );
    let id = v["created"].as_str().unwrap().to_string();
    let ids = act_ids(&v, 1);
    assert_eq!(ids.len(), 7);
    assert_eq!(ids[2], id);
    assert!(files_in(&_t.path().join("acts/01")).contains(&"030-a-new-interlude.scene".to_string()));
    let r = call(&mut ws, "scene_read", json!({"id": id}));
    assert_eq!(r["body"], "");
    assert_eq!(r["meta"]["scene"], 3);

    let v2 = call(&mut ws, "scene_trash", json!({"id": id}));
    assert_eq!(act_ids(&v2, 1).len(), 6);
    assert_eq!(files_in(&_t.path().join(".trash")).len(), 1);
    assert!(files_in(&_t.path().join("acts/01")).contains(&"030-the-cliff-path.scene".to_string()));
}

#[test]
fn create_act_then_a_scene_in_it() {
    let (_t, mut ws) = stage_sample();
    let v = call(&mut ws, "act_create", json!({}));
    assert_eq!(v["created_act"], 3);
    let v2 = call(&mut ws, "scene_create", json!({"act": 3, "title": "Epilogue"}));
    assert_eq!(act_ids(&v2, 3).len(), 1);
    assert!(_t.path().join("acts/03/010-epilogue.scene").exists());
}

#[test]
fn stats_report_matches_cli_numbers() {
    let (_t, mut ws) = stage_sample();
    let s = call(&mut ws, "stats", json!({}));
    assert!((s["total_minutes"].as_f64().unwrap() - 8.16).abs() < 0.01);
    assert_eq!(s["scenes"][4]["label"], "1.4");
    assert_eq!(s["scenes"][4]["speeches"]["DUKE"], 5);
    let issues = s["issues"].as_array().unwrap();
    assert!(issues
        .iter()
        .any(|i| i["kind"] == "unassigned" && i["role"] == "ADMIRAL"));
    assert!(!issues.iter().any(|i| i["kind"] == "clash"));
}

#[test]
fn project_files_validate_on_save() {
    let (_t, mut ws) = stage_sample();
    let f = call(&mut ws, "file_read", json!({"name": "characters.yaml"}));
    let bad = dispatch(
        &mut ws,
        "file_save",
        json!({"name": "characters.yaml", "text": "- {cue: X}\n", "base_version": f["version"]}),
    )
    .unwrap_err();
    assert_eq!(bad.code, "invalid_yaml"); // missing id
    let text = f["text"].as_str().unwrap().to_string() + "- {id: monk, cue: MONK, track: null}\n";
    call(
        &mut ws,
        "file_save",
        json!({"name": "characters.yaml", "text": text, "base_version": f["version"]}),
    );
    let v = call(&mut ws, "project_view", json!({}));
    assert!(v["characters"].as_array().unwrap().iter().any(|c| c["cue"] == "MONK"));
    let e = dispatch(&mut ws, "file_read", json!({"name": "../secret"})).unwrap_err();
    assert_eq!(e.code, "bad_args");
}

#[test]
fn new_project_from_scratch() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("my-play");
    let mut ws = Workspace::default();
    let v = call(&mut ws, "project_create", json!({"root": root, "title": "My Play"}));
    assert_eq!(v["title"], "My Play");
    assert_eq!(act_ids(&v, 1).len(), 1);
    assert!(root.join("acts/01/010-scene-1.scene").exists());
    let again = dispatch(&mut ws, "project_create", json!({"root": root, "title": "x"})).unwrap_err();
    assert_eq!(again.code, "exists");
}

#[test]
fn commands_need_an_open_project() {
    let mut ws = Workspace::default();
    assert_eq!(
        dispatch(&mut ws, "project_view", json!({})).unwrap_err().code,
        "no_project"
    );
    assert_eq!(
        dispatch(&mut ws, "nope", json!({})).unwrap_err().code,
        "unknown_command"
    );
}

#[test]
fn snapshot_has_every_scene_and_file() {
    let (_t, mut ws) = stage_sample();
    let s = call(&mut ws, "project_snapshot", json!({}));
    assert_eq!(s["scenes"].as_array().unwrap().len(), 11);
    assert_eq!(s["files"].as_array().unwrap().len(), 4);
    let pro = &s["scenes"][0];
    assert_eq!(pro["id"], "sc-0-0");
    assert!(pro["body"]
        .as_str()
        .unwrap()
        .starts_with("A cramped office at the Admiralty."));
    // The snapshot version is the one scene_read reports.
    let r = call(&mut ws, "scene_read", json!({"id": "sc-0-0"}));
    assert_eq!(pro["version"], r["version"]);
}

#[test]
fn create_scene_with_a_given_id_and_ensure_act() {
    let (_t, mut ws) = stage_sample();
    call(&mut ws, "act_ensure", json!({"act": 5}));
    let v = call(
        &mut ws,
        "scene_create",
        json!({"act": 5, "title": "Shared", "id": "sc-shared-1"}),
    );
    assert_eq!(act_ids(&v, 5), vec!["sc-shared-1"]);
    let dup = dispatch(
        &mut ws,
        "scene_create",
        json!({"act": 5, "title": "x", "id": "sc-shared-1"}),
    )
    .unwrap_err();
    assert_eq!(dup.code, "exists");
    let bad = dispatch(
        &mut ws,
        "scene_create",
        json!({"act": 5, "title": "x", "id": "../evil"}),
    )
    .unwrap_err();
    assert_eq!(bad.code, "bad_args");
}

#[test]
fn collab_state_round_trips_and_is_git_ignored() {
    let (_t, mut ws) = stage_sample();
    assert_eq!(
        call(&mut ws, "collab_read", json!({})),
        json!({"config": null, "state": null, "base": null})
    );
    let cfg = json!({"relay": "ws://127.0.0.1:4444", "room": "abcdefghijklmnopqrstuvwx", "name": "Liam"});
    call(
        &mut ws,
        "collab_write",
        json!({"config": cfg, "state": "AAEC", "base": {"scenes": {}}}),
    );
    let r = call(&mut ws, "collab_read", json!({}));
    assert_eq!(r["base"], json!({"scenes": {}}));
    assert_eq!(r["config"], cfg);
    assert_eq!(r["state"], "AAEC");
    assert_eq!(
        fs::read_to_string(_t.path().join(".stagewright/.gitignore")).unwrap(),
        "*\n"
    );
    // The project still loads with the extra folder present.
    call(&mut ws, "project_view", json!({}));
    call(&mut ws, "collab_write", json!({"config": null}));
    assert_eq!(call(&mut ws, "collab_read", json!({}))["config"], Value::Null);
}

#[test]
fn join_into_an_empty_folder() {
    let (src, mut a) = stage_sample();
    let snap = call(&mut a, "project_snapshot", json!({}));
    let files: serde_json::Map<String, Value> = snap["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["name"].as_str().unwrap().to_string(), f["text"].clone()))
        .collect();
    let dest = tempfile::tempdir().unwrap();
    let root = dest.path().join("joined");
    let mut b = Workspace::default();
    let v = call(
        &mut b,
        "project_join",
        json!({"root": root, "files": files, "config": {"room": "r"}}),
    );
    assert_eq!(v["title"], "Harbour Lights");
    assert_eq!(
        fs::read_to_string(root.join("characters.yaml")).unwrap(),
        fs::read_to_string(src.path().join("characters.yaml")).unwrap()
    );
    assert_eq!(call(&mut b, "collab_read", json!({}))["config"]["room"], "r");
    // Refuses a folder that already has things in it.
    let again = dispatch(
        &mut b,
        "project_join",
        json!({"root": root, "files": {"project.yaml": "title: x\n"}}),
    )
    .unwrap_err();
    assert_eq!(again.code, "exists");
}

fn fixture(name: &str) -> (tempfile::TempDir, Workspace) {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name);
    let tmp = tempfile::tempdir().unwrap();
    copy_dir(&src, tmp.path());
    let ws = Workspace::new(Some(tmp.path().to_path_buf()));
    (tmp, ws)
}

fn ep_act_ids(view: &Value, episode: u64, act: u64) -> Vec<String> {
    view["acts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["episode"] == episode && a["number"] == act)
        .unwrap()["scenes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn film_view_has_format_pages_and_heading_locations() {
    let (_t, mut ws) = fixture("film-sample");
    let v = call(&mut ws, "project_view", json!({}));
    assert_eq!(v["format"], "film");
    assert_eq!(v["series"], false);
    let s = &v["acts"][0]["scenes"][0];
    assert_eq!(s["headings"][0], "EXT. HARBOUR SLIPWAY - DAWN");
    assert!(s["eighths"].as_u64().unwrap() >= 1);
    // Screen formats: minutes come from pages (a minute a page).
    let mins = s["minutes"].as_f64().unwrap();
    assert!((mins - s["eighths"].as_f64().unwrap() / 8.0).abs() < 1e-9);
    let places: Vec<&str> = v["heading_locations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap())
        .collect();
    assert!(places.contains(&"HARBOUR OFFICE"));
}

#[test]
fn series_view_groups_episodes_and_acts() {
    let (_t, mut ws) = fixture("tv-series-sample");
    let v = call(&mut ws, "project_view", json!({}));
    assert_eq!(v["series"], true);
    assert_eq!(v["episodes"][0]["title"], "Valuation");
    assert_eq!(v["episodes"][1]["title"], "Appeal");
    assert_eq!(v["acts"][0]["title"], "Teaser");
    assert_eq!(v["acts"][0]["episode"], 1);
    assert_eq!(ep_act_ids(&v, 1, 1), vec!["sc-tv-1-1", "sc-tv-1-2"]);
    assert_eq!(v["acts"][1]["scenes"][0]["label"], "E1 1.1");
}

#[test]
fn moving_a_scene_between_episodes() {
    let (t, mut ws) = fixture("tv-series-sample");
    let v = call(&mut ws, "project_view", json!({}));
    let mut ids = ep_act_ids(&v, 2, 1);
    ids.insert(0, "sc-tv-1-2".into());
    let v2 = call(&mut ws, "scenes_reorder", json!({"episode": 2, "act": 1, "ids": ids}));
    assert_eq!(ep_act_ids(&v2, 2, 1), vec!["sc-tv-1-2", "sc-tv-2-1"]);
    assert_eq!(ep_act_ids(&v2, 1, 1), vec!["sc-tv-1-1"]);
    assert!(t.path().join("episodes/02/acts/01/010-the-van.scene").exists());
    let r = call(&mut ws, "scene_read", json!({"id": "sc-tv-1-2"}));
    assert_eq!(r["meta"]["scene"], 1);
    assert!(
        fs::read_to_string(t.path().join("episodes/02/acts/01/010-the-van.scene"))
            .unwrap()
            .contains("\nepisode: 2\n")
    );
}

#[test]
fn new_episode_and_acts_in_a_series() {
    let (t, mut ws) = fixture("tv-series-sample");
    let v = call(&mut ws, "episode_create", json!({"title": "Probate"}));
    assert_eq!(v["created_episode"], 3);
    assert_eq!(v["episodes"][2]["title"], "Probate");
    assert!(fs::read_to_string(t.path().join("project.yaml"))
        .unwrap()
        .contains("Probate"));
    let v2 = call(&mut ws, "act_create", json!({"episode": 3}));
    assert_eq!(v2["created_act"], 2);
    let v3 = call(
        &mut ws,
        "scene_create",
        json!({"episode": 3, "act": 2, "title": "Reading of the will"}),
    );
    assert_eq!(ep_act_ids(&v3, 3, 2).len(), 1);
    assert_eq!(dispatch(&mut ws, "act_create", json!({})).unwrap_err().code, "bad_args");
}

#[test]
fn new_projects_in_each_format() {
    for (format, series) in [
        ("film", false),
        ("tv-single", true),
        ("tv-multi", true),
        ("stage", false),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("p");
        let mut ws = Workspace::default();
        let v = call(
            &mut ws,
            "project_create",
            json!({"root": root, "title": "T", "format": format, "series": series}),
        );
        assert_eq!(v["format"], format);
        assert_eq!(v["series"], series);
        let first = if series {
            root.join("episodes/01/acts/01/010-scene-1.scene")
        } else {
            root.join("acts/01/010-scene-1.scene")
        };
        assert!(first.exists(), "{format}");
    }
    let mut ws = Workspace::default();
    let tmp = tempfile::tempdir().unwrap();
    let bad = dispatch(
        &mut ws,
        "project_create",
        json!({"root": tmp.path().join("x"), "title": "T", "format": "opera"}),
    );
    assert_eq!(bad.unwrap_err().code, "bad_args");
}

#[test]
fn export_pdf_and_fountain() {
    let (t, mut ws) = fixture("tv-series-sample");
    let r = call(&mut ws, "export", json!({"kind": "pdf", "episode": 1}));
    assert_eq!(r["name"], "the-assessors-ep01.pdf");
    assert!(r["pages"].as_u64().unwrap() >= 3);
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(r["base64"].as_str().unwrap())
        .unwrap();
    assert!(bytes.starts_with(b"%PDF-1.4"));
    let out = t.path().join("out.fountain");
    call(&mut ws, "export", json!({"kind": "fountain", "path": out}));
    let text = fs::read_to_string(&out).unwrap();
    assert!(text.starts_with("Title: The Assessors\n"));
    assert!(text.contains("INT. TRIBUNAL ROOM - DAY"));
    assert_eq!(
        dispatch(&mut ws, "export", json!({"kind": "pdf", "episode": 9}))
            .unwrap_err()
            .code,
        "not_found"
    );
}

// ---------------------------------------------------------------- deleting acts and episodes

fn trash_entries(root: &Path) -> Vec<String> {
    files_in(&root.join(".trash"))
}

fn scene_text(root: &Path, id: &str) -> String {
    let p = stagewright_core::Project::load(root).unwrap();
    fs::read_to_string(&p.scenes.iter().find(|s| s.scene.meta.id == id).unwrap().path).unwrap()
}

#[test]
fn deleting_an_act_trashes_its_scenes_and_closes_the_gap() {
    let (t, mut ws) = stage_sample();
    let v = call(&mut ws, "act_delete", json!({"act": 1}));
    assert_eq!(v["trashed_scenes"], 6);
    // Act Two is now act 1, with its scenes renumbered and files intact.
    assert_eq!(v["acts"].as_array().unwrap().len(), 2);
    assert_eq!(v["acts"][1]["number"], 1);
    assert_eq!(v["acts"][1]["title"], "Act One");
    assert_eq!(act_ids(&v, 1), vec!["sc-2-1", "sc-2-2", "sc-2-3", "sc-2-4"]);
    assert!(!t.path().join("acts/02").exists());
    assert_eq!(
        files_in(&t.path().join("acts/01")),
        vec![
            "010-hetties-cottage.scene",
            "020-mordecais-lodgings.scene",
            "030-the-storm.scene",
            "040-outside-the-lighthouse.scene"
        ]
    );
    let moved = scene_text(t.path(), "sc-2-3");
    assert!(
        moved.contains("\nact: 1\n") && moved.contains("\nscene: 3\n"),
        "{moved}"
    );
    // The six scenes are in one folder in .trash, untouched.
    let trash = trash_entries(t.path());
    assert_eq!(trash.len(), 1);
    assert!(trash[0].ends_with("-act01"), "{trash:?}");
    assert_eq!(files_in(&t.path().join(".trash").join(&trash[0])).len(), 6);
    // project.yaml: the default title follows the new number.
    let yaml = fs::read_to_string(t.path().join("project.yaml")).unwrap();
    assert!(
        yaml.contains("acts:\n  - {number: 0, title: \"Prologue\"}\n  - {number: 1, title: \"Act One\"}\n"),
        "{yaml}"
    );
    assert!(!yaml.contains("Act Two"), "{yaml}");
    // Everything still loads and round-trips.
    let p = stagewright_core::Project::load(t.path()).unwrap();
    assert_eq!(p.scenes.len(), 5);
    for sf in &p.scenes {
        assert_eq!(sf.scene.to_source(), fs::read_to_string(&sf.path).unwrap());
    }
}

#[test]
fn deleting_the_prologue_leaves_the_acts_alone() {
    let (t, mut ws) = stage_sample();
    let v = call(&mut ws, "act_delete", json!({"act": 0}));
    assert_eq!(v["trashed_scenes"], 1);
    let numbers: Vec<u64> = v["acts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["number"].as_u64().unwrap())
        .collect();
    assert_eq!(numbers, vec![1, 2]);
    assert_eq!(v["acts"][1]["title"], "Act Two");
    let yaml = fs::read_to_string(t.path().join("project.yaml")).unwrap();
    assert!(
        !yaml.contains("Prologue") && yaml.contains("{number: 2, title: \"Act Two\"}"),
        "{yaml}"
    );
}

#[test]
fn an_empty_act_is_simply_removed_and_the_last_act_is_kept() {
    let (t, mut ws) = fixture("film-sample");
    call(&mut ws, "act_create", json!({}));
    assert!(t.path().join("acts/02").exists());
    call(&mut ws, "act_delete", json!({"act": 2}));
    assert!(!t.path().join("acts/02").exists());
    assert!(!t.path().join(".trash").exists());
    let e = dispatch(&mut ws, "act_delete", json!({"act": 1})).unwrap_err();
    assert_eq!(e.code, "bad_args");
    let e = dispatch(&mut ws, "act_delete", json!({"act": 7})).unwrap_err();
    assert_eq!(e.code, "not_found");
}

#[test]
fn deleting_an_act_in_an_episode_only_moves_that_episodes_acts() {
    let (t, mut ws) = fixture("tv-series-sample");
    let v = call(&mut ws, "act_delete", json!({"episode": 1, "act": 1}));
    assert_eq!(v["trashed_scenes"], 2);
    let acts: Vec<(u64, u64)> = v["acts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| (a["episode"].as_u64().unwrap(), a["number"].as_u64().unwrap()))
        .collect();
    assert_eq!(acts, vec![(1, 0), (1, 1), (2, 1)]);
    assert!(t.path().join("episodes/01/acts/01").exists() && !t.path().join("episodes/01/acts/02").exists());
    assert!(trash_entries(t.path())[0].ends_with("-ep01-act01"));
    // The last act of an episode stays: delete the episode instead.
    let e = dispatch(&mut ws, "act_delete", json!({"episode": 2, "act": 1})).unwrap_err();
    assert_eq!(e.code, "bad_args");
}

#[test]
fn deleting_an_episode_moves_the_later_ones_up() {
    let (t, mut ws) = fixture("tv-series-sample");
    let before = call(&mut ws, "project_view", json!({}));
    let ep2: Vec<String> = before["acts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["episode"] == 2)
        .flat_map(|a| {
            a["scenes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s["id"].as_str().unwrap().to_string())
        })
        .collect();
    let v = call(&mut ws, "episode_delete", json!({"episode": 1}));
    assert!(v["trashed_scenes"].as_u64().unwrap() >= 3);
    assert_eq!(v["episodes"].as_array().unwrap().len(), 1);
    assert_eq!(v["episodes"][0]["number"], 1);
    assert_eq!(v["episodes"][0]["title"], "Appeal");
    assert!(t.path().join("episodes/01").exists() && !t.path().join("episodes/02").exists());
    for id in &ep2 {
        let text = scene_text(t.path(), id);
        assert!(text.contains("\nepisode: 1\n"), "{text}");
    }
    assert!(trash_entries(t.path())[0].ends_with("-ep01"));
    let yaml = fs::read_to_string(t.path().join("project.yaml")).unwrap();
    assert!(
        yaml.contains("episodes:\n  - {number: 1, title: \"Appeal\"}\n"),
        "{yaml}"
    );
    let e = dispatch(&mut ws, "episode_delete", json!({"episode": 1})).unwrap_err();
    assert_eq!(e.code, "bad_args");
}

#[test]
fn default_episode_titles_follow_their_numbers() {
    let text = "title: x\nseries: true\nepisodes:\n  - {number: 1, title: \"Pilot\"}\n  - {number: 2, title: \"Episode 2\"}\n  - {number: 3, title: \"Episode 3\"}\n";
    let out = crate::ops::yaml_remove_episode(text, 2).unwrap();
    assert_eq!(
        out,
        "title: x\nseries: true\nepisodes:\n  - {number: 1, title: \"Pilot\"}\n  - {number: 2, title: \"Episode 2\"}\n"
    );
    let out = crate::ops::yaml_remove_episode(text, 1).unwrap();
    assert!(
        out.contains("{number: 1, title: \"Episode 1\"}\n  - {number: 2, title: \"Episode 2\"}"),
        "{out}"
    );
    // A series' act titles are shared by every episode, so they are left alone.
    let acts = "title: x\nacts:\n  - {number: 1, title: \"Act One\"}\n  - {number: 2, title: \"The Turn\"}\n";
    assert_eq!(crate::ops::yaml_remove_act(acts, 1, true).unwrap(), acts);
    let out = crate::ops::yaml_remove_act(acts, 1, false).unwrap();
    assert_eq!(out, "title: x\nacts:\n  - {number: 1, title: \"The Turn\"}\n");
}

#[test]
fn removing_an_empty_act_folder_for_co_writing() {
    let (t, mut ws) = fixture("tv-series-sample");
    let e = dispatch(&mut ws, "act_remove_empty", json!({"episode": 2, "act": 1})).unwrap_err();
    assert_eq!(e.code, "not_empty");
    // Empty it (as earlier co-writing steps would), then it goes, and so does the episode.
    let dir = t.path().join("episodes/02/acts/01");
    for f in files_in(&dir) {
        fs::remove_file(dir.join(f)).unwrap();
    }
    let v = call(&mut ws, "act_remove_empty", json!({"episode": 2, "act": 1}));
    assert!(!t.path().join("episodes/02").exists());
    assert!(v["acts"].as_array().unwrap().iter().all(|a| a["episode"] == 1));
}
