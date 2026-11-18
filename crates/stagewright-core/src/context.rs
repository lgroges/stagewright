//! The context around the script: notes on characters and locations, research
//! notes, beat sheets, and the links between them and the scenes (the "context
//! web", docs/plans/context-and-ai.md).
//!
//! ```text
//! characters/<id>.md      a character's page (characters.yaml stays the index)
//! locations/<id>.md       a location's page
//! research/<id>.md        a research note
//! plan/beats.yaml         the beat sheet (a series' sheet for a TV series)
//! plan/episodes/01.yaml   an episode's beat sheet
//! ```
//!
//! A page is Markdown with optional YAML frontmatter. Links are stored once:
//! labelled ones in a page's `links:`, plain ones as `[[id]]` in its text, and a
//! beat's scenes in its sheet. Scenes link to their characters and location
//! without anything stored. The other end of every link sees it as a backlink.

use crate::analysis::scene_stats;
use crate::frontmatter;
use crate::project::{Project, ProjectError};
use crate::scene::Element;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

// ------------------------------------------------------------------ pages

/// A labelled link from a page: "Kelley — scryer to — Dee", and the other way
/// "Dee — employs — Kelley" (`back`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub back: Option<String>,
}

/// A page's frontmatter. Characters' and locations' pages mostly use `links`;
/// research notes also have a title, tags and sources.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Where the research came from, as written ("Dee's diary, 1583").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Link>,
}

/// A Markdown page with optional frontmatter. The frontmatter's own text is
/// kept, so keys the app doesn't know and the writer's layout survive edits.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Page {
    pub frontmatter: String,
    pub meta: PageMeta,
    pub body: String,
}

impl Page {
    pub fn parse(src: &str) -> Result<Page, String> {
        let src = src.replace("\r\n", "\n");
        let Some(rest) = src.strip_prefix("---\n") else {
            return Ok(Page {
                body: src,
                ..Page::default()
            });
        };
        let end = rest
            .split_inclusive('\n')
            .scan(0, |at, l| {
                let start = *at;
                *at += l.len();
                Some((start, l))
            })
            .find(|(_, l)| l.trim_end() == "---")
            .map(|(start, _)| start)
            .ok_or("the page's frontmatter has no closing '---'")?;
        let frontmatter = rest[..end].to_string();
        let after = &rest[end..];
        let body = after
            .strip_prefix("---\n")
            .or_else(|| after.strip_prefix("---"))
            .unwrap_or("")
            .to_string();
        let meta = meta_of(&frontmatter).map_err(|e| format!("the page's frontmatter: {e}"))?;
        Ok(Page {
            frontmatter,
            meta,
            body,
        })
    }

    pub fn to_text(&self) -> String {
        if self.frontmatter.trim().is_empty() {
            return self.body.clone();
        }
        let mut fm = self.frontmatter.clone();
        if !fm.ends_with('\n') {
            fm.push('\n');
        }
        format!("---\n{fm}---\n{}", self.body)
    }

    /// Set a frontmatter key (`null` removes it), keeping the rest as written.
    pub fn set(&mut self, key: &str, value: &serde_json::Value) -> Result<(), String> {
        let updated = frontmatter::set_or_remove(&self.frontmatter, key, value);
        self.meta = meta_of(&updated)?;
        self.frontmatter = updated;
        Ok(())
    }

    /// The page's title, or `id` if it has none (characters' and locations'
    /// pages take their name from the cast list instead).
    pub fn title_or(&self, id: &str) -> String {
        self.meta
            .title
            .clone()
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| id.to_string())
    }
}

fn meta_of(frontmatter: &str) -> Result<PageMeta, String> {
    if frontmatter.trim().is_empty() {
        return Ok(PageMeta::default());
    }
    serde_yaml::from_str(frontmatter).map_err(|e| e.to_string())
}

/// `[[id]]` and `[[id|shown text]]` in a page's text: (id, shown text).
pub fn text_links(text: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find("[[") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("]]") else { break };
        let inner = &after[..close];
        if !inner.contains('\n') && !inner.trim().is_empty() {
            let (id, shown) = match inner.split_once('|') {
                Some((id, shown)) => (id.trim(), Some(shown.trim().to_string())),
                None => (inner.trim(), None),
            };
            out.push((id.to_string(), shown));
        }
        rest = &after[close + 2..];
    }
    out
}

// ------------------------------------------------------------------ beats

/// A beat: a step in the story's plan, carried out by one or more scenes.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Beat {
    pub id: String,
    pub title: String,
    /// What happens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// What it's for in the story.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<String>,
    /// Where it should land, as a share of the sheet's length (0–1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<f64>,
    /// The scenes that carry it out (ids), in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenes: Vec<String>,
    /// A series sheet's beat: the episodes that carry it out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub episodes: Vec<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BeatSheet {
    /// The template it started from (`save-the-cat` …), for information only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(default)]
    pub beats: Vec<Beat>,
}

/// Where a sheet is kept, in the project: the play's, film's or series'
/// (`None`), or an episode's.
pub fn sheet_file(episode: Option<u32>) -> String {
    match episode {
        None => "plan/beats.yaml".into(),
        Some(n) => format!("plan/episodes/{n:02}.yaml"),
    }
}

pub fn sheet_path(root: &Path, episode: Option<u32>) -> PathBuf {
    root.join(sheet_file(episode))
}

/// An id that can be a file name: letters, digits, `-` and `_`.
pub fn is_plain_id(id: &str) -> bool {
    !id.is_empty() && id.chars().count() <= 80 && id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_')
}

/// The templates a sheet can start from: (id, name).
pub const TEMPLATES: &[(&str, &str)] = &[
    ("save-the-cat", "Save the Cat (15 beats)"),
    ("three-act", "Three acts"),
    ("story-circle", "Story Circle (8 steps)"),
    ("freytag", "Freytag's pyramid"),
];

/// A template's beats: (title, purpose, target).
fn template_beats(name: &str) -> Option<&'static [(&'static str, &'static str, f64)]> {
    Some(match name {
        // Blake Snyder's page numbers for a 110-page screenplay, as shares.
        "save-the-cat" => &[
            (
                "Opening Image",
                "A snapshot of the hero's world before the story changes it.",
                0.0,
            ),
            ("Theme Stated", "Someone hints at what the story is about.", 0.045),
            ("Set-Up", "The hero, their world and what's missing.", 0.01),
            ("Catalyst", "The event that upsets everything.", 0.11),
            ("Debate", "The hero hesitates: should I go?", 0.12),
            (
                "Break into Two",
                "The hero chooses to act and enters the new world.",
                0.23,
            ),
            (
                "B Story",
                "A second story, often a relationship, that carries the theme.",
                0.27,
            ),
            (
                "Fun and Games",
                "The premise played out: the promise of the premise.",
                0.28,
            ),
            ("Midpoint", "A false victory or a false defeat; the stakes go up.", 0.5),
            ("Bad Guys Close In", "Pressure from outside, doubt from within.", 0.51),
            ("All Is Lost", "The lowest point, often with a whiff of death.", 0.68),
            ("Dark Night of the Soul", "The hero takes the loss in.", 0.69),
            ("Break into Three", "The answer, often from the B Story.", 0.77),
            ("Finale", "The hero puts what they've learned into action.", 0.78),
            ("Final Image", "The opposite of the opening image: the change.", 1.0),
        ],
        "three-act" => &[
            ("Set-up", "The world, the characters and what they want.", 0.0),
            ("Inciting incident", "What sets the story going.", 0.12),
            ("First turning point", "The way back closes; act two begins.", 0.25),
            ("Midpoint", "A reversal that changes what the story is about.", 0.5),
            ("Second turning point", "The crisis that leads to the end.", 0.75),
            ("Climax", "The decisive confrontation.", 0.9),
            ("Resolution", "The new order.", 0.97),
        ],
        // Dan Harmon's eight steps, evenly round the circle.
        "story-circle" => &[
            ("You", "A character in a zone of comfort.", 0.0),
            ("Need", "But they want something.", 0.125),
            ("Go", "They enter an unfamiliar situation.", 0.25),
            ("Search", "Adapt to it.", 0.375),
            ("Find", "Get what they wanted.", 0.5),
            ("Take", "Pay a heavy price for it.", 0.625),
            ("Return", "Then return to their familiar situation.", 0.75),
            ("Change", "Having changed.", 0.875),
        ],
        "freytag" => &[
            ("Exposition", "The situation and the characters.", 0.0),
            ("Inciting incident", "The conflict begins.", 0.12),
            ("Rising action", "Complications build.", 0.2),
            ("Climax", "The turning point of the play.", 0.5),
            ("Falling action", "The consequences unfold.", 0.6),
            ("Resolution", "The conflict is settled.", 0.85),
            ("Dénouement", "What's left, tied up.", 0.95),
        ],
        _ => return None,
    })
}

/// A new sheet from a template; beat ids from the titles, prefixed for an
/// episode's sheet (`e1-catalyst`) so ids stay unique across the project.
pub fn from_template(name: &str, episode: Option<u32>) -> Option<BeatSheet> {
    let beats = template_beats(name)?
        .iter()
        .map(|(title, purpose, target)| Beat {
            id: match episode {
                Some(n) => format!("e{n}-{}", slug(title)),
                None => slug(title),
            },
            title: title.to_string(),
            purpose: Some(purpose.to_string()),
            target: Some(*target),
            ..Beat::default()
        })
        .collect();
    Some(BeatSheet {
        template: Some(name.to_string()),
        beats,
    })
}

/// Where a beat lands, measured along its sheet's scenes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Landing {
    /// A share of the sheet's length (0–1), comparable with the beat's target.
    pub share: f64,
    /// Minutes from the start of the sheet's first scene.
    pub minutes: f64,
}

/// A sheet's scenes in running order, with when each starts: the episode's for an
/// episode's sheet, the whole script's otherwise.
pub struct Timeline<'a> {
    /// (scene id, episode, start in seconds).
    scenes: Vec<(&'a str, Option<u32>, f64)>,
    total: f64,
}

impl<'a> Timeline<'a> {
    pub fn new(project: &'a Project, episode: Option<u32>) -> Timeline<'a> {
        let mut at = 0.0;
        let mut scenes = Vec::new();
        for s in project
            .scenes
            .iter()
            .filter(|s| episode.is_none() || s.episode == episode)
        {
            scenes.push((s.scene.meta.id.as_str(), s.episode, at));
            at += scene_stats(&s.scene, &project.meta).seconds();
        }
        Timeline { scenes, total: at }
    }

    /// The start of the beat's first scene, or else of its first episode (a
    /// series sheet). `None` if it has neither, or there's no running time yet.
    pub fn lands(&self, beat: &Beat) -> Option<Landing> {
        if self.total <= 0.0 {
            return None;
        }
        let first_ep = beat.episodes.iter().min();
        let (_, _, start) = self
            .scenes
            .iter()
            .find(|(id, _, _)| beat.scenes.iter().any(|b| b == id))
            .or_else(|| {
                self.scenes
                    .iter()
                    .find(|(_, e, _)| first_ep.is_some() && e.as_ref() == first_ep)
            })?;
        Some(Landing {
            share: start / self.total,
            minutes: start / 60.0,
        })
    }
}

/// "Break into Two" → `break-into-two`.
pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

// ------------------------------------------------------------------ notes

/// Everything around the script that's stored in the project.
#[derive(Debug, Clone, Default)]
pub struct Notes {
    /// Pages by id: characters' and locations' notes, and research notes.
    pub characters: BTreeMap<String, Page>,
    pub locations: BTreeMap<String, Page>,
    pub research: BTreeMap<String, Page>,
    /// The main sheet (`None`) and episodes' sheets.
    pub sheets: BTreeMap<Option<u32>, BeatSheet>,
}

fn read(path: &Path) -> Result<String, ProjectError> {
    std::fs::read_to_string(path).map_err(|e| ProjectError::Io(path.to_path_buf(), e))
}

/// The `.md` files in `dir`, by file stem, sorted.
fn pages_in(dir: &Path) -> Result<BTreeMap<String, Page>, ProjectError> {
    let mut out = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(out);
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }
        let Some(id) = path.file_stem().and_then(|s| s.to_str()).map(str::to_string) else {
            continue;
        };
        let page = Page::parse(&read(&path)?).map_err(|m| ProjectError::Page(path.clone(), m))?;
        out.insert(id, page);
    }
    Ok(out)
}

fn sheet(path: &Path) -> Result<Option<BeatSheet>, ProjectError> {
    if !path.is_file() {
        return Ok(None);
    }
    serde_yaml::from_str(&read(path)?)
        .map(Some)
        .map_err(|e| ProjectError::Yaml(path.to_path_buf(), e))
}

impl Notes {
    pub fn load(root: &Path) -> Result<Notes, ProjectError> {
        let pages = |kind: Kind| pages_in(&root.join(kind.page_dir().unwrap_or_default()));
        let mut sheets = BTreeMap::new();
        if let Some(s) = sheet(&sheet_path(root, None))? {
            sheets.insert(None, s);
        }
        if let Ok(entries) = std::fs::read_dir(root.join("plan").join("episodes")) {
            for e in entries.flatten() {
                let path = e.path();
                let n = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.parse::<u32>().ok());
                if let (Some(n), Some(s)) = (n, sheet(&path)?) {
                    sheets.insert(Some(n), s);
                }
            }
        }
        Ok(Notes {
            characters: pages(Kind::Character)?,
            locations: pages(Kind::Location)?,
            research: pages(Kind::Research)?,
            sheets,
        })
    }

    fn pages_of(&self, kind: Kind) -> Option<&BTreeMap<String, Page>> {
        match kind {
            Kind::Character => Some(&self.characters),
            Kind::Location => Some(&self.locations),
            Kind::Research => Some(&self.research),
            Kind::Scene | Kind::Beat => None,
        }
    }

    /// A record's page, if it has one.
    pub fn page(&self, kind: Kind, id: &str) -> Option<&Page> {
        self.pages_of(kind)?.get(id)
    }

    /// Every page: (kind, id, page).
    pub fn pages(&self) -> impl Iterator<Item = (Kind, &str, &Page)> {
        Kind::WITH_PAGES.iter().flat_map(move |&k| {
            self.pages_of(k)
                .into_iter()
                .flatten()
                .map(move |(id, p)| (k, id.as_str(), p))
        })
    }

    /// Every beat, in every sheet.
    pub fn beats(&self) -> impl Iterator<Item = &Beat> {
        self.sheets.values().flat_map(|s| &s.beats)
    }

    pub fn beat(&self, id: &str) -> Option<&Beat> {
        self.beats().find(|b| b.id == id)
    }
}

// ------------------------------------------------------------------ the web

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Character,
    Location,
    Scene,
    Beat,
    Research,
}

impl Kind {
    /// The kinds that can have a page.
    pub const WITH_PAGES: [Kind; 3] = [Kind::Character, Kind::Location, Kind::Research];

    /// The folder its pages are kept in.
    pub fn page_dir(self) -> Option<&'static str> {
        match self {
            Kind::Character => Some("characters"),
            Kind::Location => Some("locations"),
            Kind::Research => Some("research"),
            Kind::Scene | Kind::Beat => None,
        }
    }

    /// Where a record's page is kept (`characters/dee.md`), if it can have one.
    pub fn page_file(self, id: &str) -> Option<String> {
        let dir = self.page_dir()?;
        is_plain_id(id).then(|| format!("{dir}/{id}.md"))
    }
}

/// A record in the web.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Node {
    pub id: String,
    pub kind: Kind,
    pub name: String,
}

/// How a link came about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum How {
    /// In a page's `links:`, with labels.
    Link,
    /// `[[id]]` in a page's text.
    Text,
    /// A beat's scenes, or a scene's characters and location (worked out).
    Plan,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
    pub back: Option<String>,
    pub how: How,
}

/// A record linked to another, seen from that other: "Dee — employs — Kelley".
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Neighbour {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    /// The link's label read from this side, if it has one.
    pub label: Option<String>,
    /// The link was made by the other record (a backlink).
    pub incoming: bool,
    pub how: How,
}

/// Every record and link in a project.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Web {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

/// Does `name` appear in `text` as whole words?
fn mentions(text: &str, name: &str) -> bool {
    let name = name.trim();
    if name.is_empty() {
        return false;
    }
    text.match_indices(name).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + name.len()..].chars().next();
        !before.is_some_and(char::is_alphanumeric) && !after.is_some_and(char::is_alphanumeric)
    })
}

impl Web {
    pub fn build(project: &Project, notes: &Notes) -> Web {
        let mut web = Web::default();
        let mut add = |id: &str, kind, name: String| {
            web.nodes.push(Node {
                id: id.to_string(),
                kind,
                name,
            })
        };
        for c in &project.characters {
            add(&c.id, Kind::Character, c.name.clone().unwrap_or_else(|| c.cue.clone()));
        }
        for l in &project.locations {
            add(&l.id, Kind::Location, l.name.clone().unwrap_or_else(|| l.id.clone()));
        }
        for s in &project.scenes {
            add(
                &s.scene.meta.id,
                Kind::Scene,
                s.scene.meta.title.clone().unwrap_or_else(|| s.scene.meta.id.clone()),
            );
        }
        for b in notes.beats() {
            add(&b.id, Kind::Beat, b.title.clone());
        }
        for (id, page) in &notes.research {
            add(id, Kind::Research, page.title_or(id));
        }

        let mut link = |from: &str, to: &str, label: Option<String>, back: Option<String>, how| {
            if from != to {
                web.edges.push(Edge {
                    from: from.to_string(),
                    to: to.to_string(),
                    label,
                    back,
                    how,
                });
            }
        };
        for (_, id, page) in notes.pages() {
            for l in &page.meta.links {
                link(id, &l.to, l.label.clone(), l.back.clone(), How::Link);
            }
            for (to, _) in text_links(&page.body) {
                link(id, &to, None, None, How::Text);
            }
        }
        for b in notes.beats() {
            for s in &b.scenes {
                link(
                    &b.id,
                    s,
                    Some("carried out in".into()),
                    Some("carries out".into()),
                    How::Plan,
                );
            }
        }
        // Scenes: their location, and the characters who speak or are named.
        for s in &project.scenes {
            let meta = &s.scene.meta;
            if let Some(loc) = meta.location.as_deref().filter(|l| !l.is_empty()) {
                link(
                    &meta.id,
                    loc,
                    Some("set in".into()),
                    Some("setting of".into()),
                    How::Plan,
                );
            }
            let speakers = crate::analysis::speakers_in(&s.scene);
            let action: String = s
                .scene
                .body
                .iter()
                .filter_map(|e| match e {
                    Element::Direction { text, .. } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n");
            for c in &project.characters {
                let named = [Some(&c.cue), c.name.as_ref(), c.short.as_ref()]
                    .into_iter()
                    .flatten()
                    .any(|n| mentions(&action, n));
                if speakers.contains(&c.cue) || named {
                    link(
                        &meta.id,
                        &c.id,
                        Some("features".into()),
                        Some("appears in".into()),
                        How::Plan,
                    );
                }
            }
        }
        web
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// What `id` is linked to, either way, one entry per record (a stored link
    /// before a worked-out one).
    pub fn around(&self, id: &str) -> Vec<Neighbour> {
        let mut out: Vec<Neighbour> = Vec::new();
        let mut edges: Vec<&Edge> = self.edges.iter().filter(|e| e.from == id || e.to == id).collect();
        edges.sort_by_key(|e| e.how);
        for e in edges {
            let incoming = e.to == id;
            let other = if incoming { &e.from } else { &e.to };
            let Some(n) = self.node(other) else { continue };
            if out.iter().any(|o| &o.id == other) {
                continue;
            }
            out.push(Neighbour {
                id: n.id.clone(),
                kind: n.kind,
                name: n.name.clone(),
                label: if incoming { e.back.clone() } else { e.label.clone() },
                incoming,
                how: e.how,
            });
        }
        out
    }

    /// Links to records that don't exist: (from, to).
    pub fn broken(&self) -> Vec<(String, String)> {
        let ids: BTreeSet<&str> = self.nodes.iter().map(|n| n.id.as_str()).collect();
        let broken: BTreeSet<(&str, &str)> = self
            .edges
            .iter()
            .filter(|e| !ids.contains(e.to.as_str()))
            .map(|e| (e.from.as_str(), e.to.as_str()))
            .collect();
        broken.into_iter().map(|(f, t)| (f.into(), t.into())).collect()
    }

    /// Ids used by more than one record.
    pub fn duplicates(&self) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut out = BTreeSet::new();
        for n in &self.nodes {
            if !seen.insert(n.id.as_str()) {
                out.insert(n.id.clone());
            }
        }
        out.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_with_and_without_frontmatter() {
        let p = Page::parse("Plain notes.\n").unwrap();
        assert_eq!(
            (p.meta.clone(), p.body.as_str()),
            (PageMeta::default(), "Plain notes.\n")
        );
        assert_eq!(p.to_text(), "Plain notes.\n");
        let src = "---\ntitle: Burial, 1559\n# kept\nlinks:\n  - {to: dee, label: scryer to, back: employs}\n---\nSee [[mortlake]].\n";
        let mut p = Page::parse(src).unwrap();
        assert_eq!(p.meta.title.as_deref(), Some("Burial, 1559"));
        assert_eq!(
            p.meta.links,
            vec![Link {
                to: "dee".into(),
                label: Some("scryer to".into()),
                back: Some("employs".into())
            }]
        );
        assert_eq!(p.to_text(), src);
        p.set("tags", &serde_json::json!(["church"])).unwrap();
        assert!(p.to_text().contains("# kept") && p.meta.tags == vec!["church"]);
        p.set("tags", &serde_json::Value::Null).unwrap();
        assert!(!p.to_text().contains("tags"));
        assert!(Page::parse("---\ntitle: x\n").is_err());
    }

    #[test]
    fn links_in_text() {
        assert_eq!(
            text_links("[[dee]] met [[kelley|the scryer]]. [[]] and [[broken\nline]]"),
            vec![("dee".into(), None), ("kelley".into(), Some("the scryer".into()))]
        );
    }

    #[test]
    fn templates() {
        let s = from_template("save-the-cat", None).unwrap();
        assert_eq!(s.beats.len(), 15);
        assert_eq!((s.beats[3].id.as_str(), s.beats[3].target), ("catalyst", Some(0.11)));
        let e = from_template("story-circle", Some(2)).unwrap();
        assert_eq!(e.beats[0].id, "e2-you");
        assert!(from_template("nonsense", None).is_none());
        assert_eq!(slug("Dénouement — at last!"), "dénouement-at-last");
        assert_eq!(sheet_file(Some(3)), "plan/episodes/03.yaml");
        assert_eq!(Kind::Research.page_file("a b"), None);
        assert_eq!(Kind::Beat.page_file("x"), None);
        assert!(TEMPLATES.iter().all(|(id, _)| template_beats(id).is_some()));
    }

    #[test]
    fn the_web_around_a_record() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/stage-sample");
        let project = Project::load(&root).unwrap();
        let mut notes = Notes::default();
        notes.characters.insert(
            "tobias".into(),
            Page::parse(
                "---\nlinks:\n  - {to: hettie, label: courts, back: courted by}\n---\nA lampman. See [[signs]].\n",
            )
            .unwrap(),
        );
        notes.research.insert(
            "signs".into(),
            // Linked twice to the same missing record: reported once.
            Page::parse("---\ntitle: Harbour signs\nlinks: [{to: nowhere}]\n---\nAbout [[nowhere]].").unwrap(),
        );
        let mut sheet = from_template("three-act", None).unwrap();
        sheet.beats[1].scenes = vec!["sc-1-1".into()];
        notes.sheets.insert(None, sheet);
        let web = Web::build(&project, &notes);

        let around = |id: &str| -> Vec<(String, Option<String>, bool)> {
            web.around(id)
                .into_iter()
                .map(|n| (n.id, n.label, n.incoming))
                .collect()
        };
        let hettie = around("hettie");
        assert!(
            hettie.contains(&("tobias".into(), Some("courted by".into()), true)),
            "{hettie:?}"
        );
        // She's in the quay scene: worked out from the script.
        assert!(
            hettie.contains(&("sc-1-1".into(), Some("appears in".into()), true)),
            "{hettie:?}"
        );
        let quay = around("sc-1-1");
        assert!(
            quay.contains(&("quay".into(), Some("set in".into()), false)),
            "{quay:?}"
        );
        assert!(
            quay.contains(&("inciting-incident".into(), Some("carries out".into()), true)),
            "{quay:?}"
        );
        assert!(around("signs").contains(&("tobias".into(), None, true)));
        assert_eq!(web.node("signs").unwrap().name, "Harbour signs");
        assert_eq!(web.broken(), vec![("signs".into(), "nowhere".into())]);
        assert!(web.duplicates().is_empty());
        assert_eq!(
            notes.pages().map(|(k, id, _)| (k, id)).collect::<Vec<_>>(),
            vec![(Kind::Character, "tobias"), (Kind::Research, "signs")]
        );

        // Where beats land.
        let timeline = Timeline::new(&project, None);
        let sheet = &notes.sheets[&None];
        let at = timeline.lands(&sheet.beats[1]).unwrap();
        assert!(at.share > 0.0 && at.share < 0.5 && at.minutes > 0.0, "{at:?}");
        assert_eq!(timeline.lands(&sheet.beats[0]), None);
        let by_episode = Beat {
            episodes: vec![1],
            ..Beat::default()
        };
        assert_eq!(timeline.lands(&by_episode), None, "not a series");
    }

    #[test]
    fn mentions_are_whole_words() {
        assert!(mentions("Dee waits.", "Dee"));
        assert!(!mentions("Deep water.", "Dee"));
        assert!(mentions("JOHN DEE enters.", "JOHN DEE"));
    }
}
