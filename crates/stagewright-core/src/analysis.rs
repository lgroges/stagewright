//! Derived production data: who is in each scene, how long it runs, and
//! whether the doubling plan holds. Nothing here is typed by hand.

use crate::layout::{Profile, eighths, scene_line_count};
use crate::project::{Project, ProjectMeta};
use crate::scene::{Element, Scene};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Group cues that are not characters.
/// Cues that name a group, not a character.
pub const GROUP_CUES: &[&str] = &["ALL"];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SceneStats {
    pub id: String,
    pub title: String,
    /// Speeches per speaker (a duet line counts once for each singer).
    pub speeches: BTreeMap<String, usize>,
    /// Speakers plus anyone listed as `onstage_silent`.
    pub present: BTreeSet<String>,
    pub dialogue_words: usize,
    pub lyric_lines: usize,
    pub songs: Vec<String>,
    pub estimated_seconds: f64,
    /// Printed length in eighths of a page, in the project's format.
    pub eighths: u32,
    /// Scene headings in this scene (screen formats), for breakdowns.
    pub headings: Vec<HeadingInfo>,
    /// Manual override from frontmatter (`duration`, minutes), if set.
    pub manual_seconds: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HeadingInfo {
    pub text: String,
    /// `INT.`, `EXT.`, `INT./EXT.` …
    pub setting: Option<String>,
    pub location: String,
    /// `NIGHT`, `DAY`, `CONTINUOUS` …
    pub time: Option<String>,
}

impl SceneStats {
    pub fn seconds(&self) -> f64 {
        self.manual_seconds.unwrap_or(self.estimated_seconds)
    }
}

/// Words a performer speaks: parentheticals removed, emphasis markers ignored.
pub fn spoken_words(line: &str) -> usize {
    let mut depth = 0usize;
    let mut kept = String::with_capacity(line.len());
    for ch in line.chars() {
        match ch {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            '*' => {}
            _ if depth == 0 => kept.push(ch),
            _ => {}
        }
    }
    kept.split_whitespace().count()
}

/// Everyone who speaks in a scene (group cues like ALL left out). Cheaper than
/// `scene_stats` when only the names are wanted: no layout.
pub fn speakers_in(scene: &Scene) -> BTreeSet<String> {
    scene
        .body
        .iter()
        .filter_map(|e| match e {
            Element::Cue(c) => Some(c.speakers()),
            _ => None,
        })
        .flatten()
        .filter(|s| !GROUP_CUES.contains(s))
        .map(str::to_string)
        .collect()
}

/// Statistics for one scene. Running time: in a stage play, from spoken words;
/// in screen formats, from printed pages (the industry's minute-a-page rule).
pub fn scene_stats(scene: &Scene, meta: &ProjectMeta) -> SceneStats {
    let cfg = &meta.running_time;
    let profile = Profile::for_format(meta.format);
    let mut headings = Vec::new();
    let mut speeches: BTreeMap<String, usize> = BTreeMap::new();
    let mut words = 0;
    let mut lyrics = 0;
    let mut songs = Vec::new();
    let mut speakers: Vec<String> = Vec::new();

    for el in &scene.body {
        match el {
            Element::Cue(c) => {
                speakers = c
                    .speakers()
                    .into_iter()
                    .filter(|s| !GROUP_CUES.contains(s))
                    .map(str::to_string)
                    .collect();
                for s in &speakers {
                    speeches.entry(s.clone()).or_insert(0);
                }
            }
            Element::Dialogue(t) => {
                words += spoken_words(t);
                for s in &speakers {
                    *speeches.entry(s.clone()).or_insert(0) += 1;
                }
            }
            Element::Lyric(_) => lyrics += 1,
            Element::SongStart(s) => songs.push(s.title.clone().unwrap_or_else(|| "(untitled)".into())),
            Element::SceneHeading(h) => {
                let (setting, location, time) = h.parts();
                headings.push(HeadingInfo {
                    text: h.text.clone(),
                    setting,
                    location,
                    time,
                });
            }
            _ => {}
        }
    }

    let mut present: BTreeSet<String> = speeches.keys().cloned().collect();
    present.extend(scene.meta.onstage_silent.iter().cloned());

    let eighths = eighths(scene_line_count(scene, &profile), &profile);
    let by_words = words as f64 / cfg.words_per_minute * 60.0 + lyrics as f64 * cfg.seconds_per_lyric_line;
    let by_pages = eighths as f64 / 8.0 * cfg.minutes_per_page.unwrap_or(profile.minutes_per_page) * 60.0;
    let estimated_seconds = if meta.format.is_screen() { by_pages } else { by_words };
    SceneStats {
        eighths,
        headings,
        id: scene.meta.id.clone(),
        title: scene.meta.title.clone().unwrap_or_else(|| scene.meta.id.clone()),
        speeches,
        present,
        dialogue_words: words,
        lyric_lines: lyrics,
        songs,
        estimated_seconds,
        manual_seconds: scene.meta.duration.map(|m| m * 60.0),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DoublingIssue {
    /// Two roles on one track are both in the same scene.
    Clash {
        scene: String,
        track: String,
        roles: Vec<String>,
    },
    /// A track ends one scene as one role and starts the next as another.
    QuickChange {
        from_scene: String,
        to_scene: String,
        track: String,
        from_role: String,
        to_role: String,
    },
    /// A role speaks but has no track (or is missing from characters.yaml).
    Unassigned { role: String, scenes: Vec<String> },
}

pub fn doubling_issues(project: &Project, stats: &[SceneStats]) -> Vec<DoublingIssue> {
    let mut tracks: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for c in &project.characters {
        if let Some(t) = c.track.as_deref().filter(|t| *t != "any-free") {
            tracks.entry(t).or_default().push(&c.cue);
        }
    }
    let mut issues = Vec::new();

    for s in stats {
        for (track, roles) in &tracks {
            let here: Vec<String> = roles
                .iter()
                .filter(|r| s.present.contains(**r))
                .map(|r| r.to_string())
                .collect();
            if here.len() > 1 {
                issues.push(DoublingIssue::Clash {
                    scene: s.id.clone(),
                    track: track.to_string(),
                    roles: here,
                });
            }
        }
    }

    for pair in stats.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        for (track, roles) in &tracks {
            for from in roles {
                for to in roles {
                    if from != to && a.present.contains(*from) && b.present.contains(*to) && !a.present.contains(*to) {
                        issues.push(DoublingIssue::QuickChange {
                            from_scene: a.id.clone(),
                            to_scene: b.id.clone(),
                            track: track.to_string(),
                            from_role: from.to_string(),
                            to_role: to.to_string(),
                        });
                    }
                }
            }
        }
    }

    let mut unassigned: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for s in stats {
        for role in &s.present {
            let tracked = project.character_by_cue(role).is_some_and(|c| c.track.is_some());
            if !tracked {
                unassigned.entry(role.clone()).or_default().push(s.id.clone());
            }
        }
    }
    issues.extend(
        unassigned
            .into_iter()
            .map(|(role, scenes)| DoublingIssue::Unassigned { role, scenes }),
    );
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spoken_words_drops_parentheticals_and_emphasis() {
        assert_eq!(
            spoken_words("Very well… (turning to the villagers) People of *Briton*!"),
            5
        );
    }
}
