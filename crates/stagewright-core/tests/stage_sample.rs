//! Tests against the sample play in `fixtures/stage-sample` ("Harbour Lights").

use stagewright_core::analysis::{DoublingIssue, doubling_issues, scene_stats};
use stagewright_core::{Element, Project, Scene};
use std::path::PathBuf;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/stage-sample")
}

#[test]
fn every_scene_round_trips_byte_for_byte() {
    let project = Project::load(fixture()).unwrap();
    assert_eq!(project.scenes.len(), 11);
    for sf in &project.scenes {
        let src = std::fs::read_to_string(&sf.path).unwrap();
        let reparsed = Scene::parse(&src).unwrap();
        assert_eq!(reparsed.to_source(), src, "round trip changed {}", sf.path.display());
    }
}

/// Spoken words, lyric lines and speeches per role, counted independently of
/// the Rust parser (by a short Python script over the same files).
type Expected = (&'static str, usize, usize, &'static [(&'static str, usize)]);
const EXPECTED: &[Expected] = &[
    ("sc-0-0", 72, 0, &[("ADMIRAL", 3), ("MORDECAI", 2), ("VOICE", 1)]),
    ("sc-1-1", 92, 0, &[("HETTIE", 5), ("TOBIAS", 5)]),
    ("sc-1-2", 126, 0, &[("MAYOR", 4), ("MORDECAI", 3), ("ROSALIND", 4)]),
    (
        "sc-1-3",
        85,
        0,
        &[("MORDECAI", 5), ("ROSALIND", 4), ("SAILOR 1", 3), ("SAILOR 2", 2)],
    ),
    ("sc-1-4", 127, 0, &[("CONSTABLE", 2), ("DUCHESS", 4), ("DUKE", 5)]),
    ("sc-1-5", 94, 0, &[("MORDECAI", 5), ("ROSALIND", 5)]),
    ("sc-1-6", 97, 0, &[("DUCHESS", 5), ("DUKE", 2), ("MORDECAI", 4)]),
    ("sc-2-1", 55, 12, &[("CORNELIUS", 4), ("HETTIE", 2), ("MUSICIAN 1", 1)]),
    ("sc-2-2", 93, 0, &[("MAYOR", 5), ("MORDECAI", 4), ("ROSALIND", 3)]),
    (
        "sc-2-3",
        65,
        0,
        &[
            ("HETTIE", 1),
            ("MAYOR", 3),
            ("MORDECAI", 2),
            ("ROSALIND", 2),
            ("SAILOR 1", 2),
            ("SAILOR 2", 1),
        ],
    ),
    (
        "sc-2-4",
        108,
        2,
        &[("HETTIE", 0), ("MORDECAI", 3), ("ROSALIND", 4), ("TOBIAS", 4)],
    ),
];

#[test]
fn stats_match_an_independent_count() {
    let project = Project::load(fixture()).unwrap();
    assert_eq!(project.scenes.len(), EXPECTED.len());
    for (sf, (id, words, lyrics, speeches)) in project.scenes.iter().zip(EXPECTED) {
        let s = scene_stats(&sf.scene, &project.meta);
        assert_eq!(s.id, *id);
        assert_eq!(s.dialogue_words, *words, "{id}");
        assert_eq!(s.lyric_lines, *lyrics, "{id}");
        let got: Vec<(&str, usize)> = s.speeches.iter().map(|(k, v)| (k.as_str(), *v)).collect();
        assert_eq!(got, speeches.to_vec(), "{id}");
    }
}

#[test]
fn songs_are_parsed() {
    let project = Project::load(fixture()).unwrap();
    let songs: Vec<_> = project
        .scenes
        .iter()
        .flat_map(|sf| sf.scene.body.iter())
        .filter_map(|e| match e {
            Element::SongStart(s) => Some((s.title.clone(), s.singers.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        songs,
        vec![
            (Some("Lanterns".to_string()), vec!["HETTIE".to_string()]),
            (None, vec![]),
        ]
    );
}

#[test]
fn doubling_plan_has_no_clashes_and_flags_the_admiral() {
    let project = Project::load(fixture()).unwrap();
    let stats: Vec<_> = project
        .scenes
        .iter()
        .map(|sf| scene_stats(&sf.scene, &project.meta))
        .collect();
    let issues = doubling_issues(&project, &stats);
    assert!(
        !issues.iter().any(|i| matches!(i, DoublingIssue::Clash { .. })),
        "{issues:?}"
    );
    assert!(
        issues
            .iter()
            .any(|i| matches!(i, DoublingIssue::Unassigned { role, .. } if role == "ADMIRAL"))
    );
    let quick: Vec<_> = issues
        .iter()
        .filter_map(|i| match i {
            DoublingIssue::QuickChange {
                from_scene,
                to_scene,
                from_role,
                to_role,
                ..
            } => Some(format!("{from_scene}->{to_scene} {from_role}->{to_role}")),
            _ => None,
        })
        .collect();
    assert_eq!(
        quick,
        vec![
            "sc-1-1->sc-1-2 TOBIAS->MAYOR",
            "sc-1-6->sc-2-1 DUKE->CORNELIUS",
            "sc-2-3->sc-2-4 MAYOR->TOBIAS",
        ]
    );
}

#[test]
fn total_running_time_follows_the_word_rule() {
    // 1,014 spoken words at 150 a minute, plus 14 lyric lines at 6 seconds each.
    let project = Project::load(fixture()).unwrap();
    let total: f64 = project
        .scenes
        .iter()
        .map(|sf| scene_stats(&sf.scene, &project.meta).seconds())
        .sum();
    assert!(
        (total - (1014.0 / 150.0 * 60.0 + 14.0 * 6.0)).abs() < 1e-6,
        "got {total:.1} s"
    );
}
