//! A project folder: `project.yaml`, registries, and the scenes:
//! - a play or film: `acts/<NN>/<order>-<slug>.scene`
//! - a TV series:    `episodes/<EE>/acts/<NN>/<order>-<slug>.scene`

pub use crate::layout::Format;
use crate::scene::{ParseError, Scene};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RunningTimeConfig {
    #[serde(default = "default_wpm")]
    pub words_per_minute: f64,
    #[serde(default = "default_lyric_secs")]
    pub seconds_per_lyric_line: f64,
    /// Screen formats: minutes per page (default 1, or 0.5 for multi-camera).
    #[serde(default)]
    pub minutes_per_page: Option<f64>,
}

fn default_wpm() -> f64 {
    150.0
}
fn default_lyric_secs() -> f64 {
    6.0
}

impl Default for RunningTimeConfig {
    fn default() -> Self {
        Self {
            words_per_minute: default_wpm(),
            seconds_per_lyric_line: default_lyric_secs(),
            minutes_per_page: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ActMeta {
    pub number: u32,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProjectMeta {
    pub title: String,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub running_time: RunningTimeConfig,
    /// Names of the four 1–9 scene ratings (yWriter's "fields").
    #[serde(default)]
    pub rating_fields: Vec<String>,
    #[serde(default)]
    pub acts: Vec<ActMeta>,
    #[serde(default)]
    pub format: Format,
    /// A TV series: episodes, each with acts.
    #[serde(default)]
    pub series: bool,
    #[serde(default)]
    pub episodes: Vec<EpisodeMeta>,
    /// Shown on title pages, e.g. "Draft 9 (May 2024)".
    #[serde(default)]
    pub draft: Option<String>,
    /// Add (CONT'D) automatically when a character speaks again after action.
    /// Unset: on for film and TV, off for stage plays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_contd: Option<bool>,
    /// Contact details for the title page's bottom-left corner: address,
    /// phone, email, one per line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact: Option<String>,
    /// A shooting script (film and TV): camera directions and transitions
    /// are expected, and the draft is printed on the title page. Unset: a
    /// spec script.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shooting_script: bool,
    /// How exports lay the script out: `uk-stage` for stage plays, `bbc-studio`
    /// for TV; unset, the standard layout.
    #[serde(default, skip_serializing_if = "is_standard")]
    pub layout: crate::layout::Layout,
    /// Script check settings, e.g. `checks: {off: [contd-mismatch]}`.
    #[serde(default)]
    pub checks: ChecksConfig,
}

fn is_standard(l: &crate::layout::Layout) -> bool {
    *l == crate::layout::Layout::Standard
}

impl ProjectMeta {
    /// Whether (CONT'D) is added automatically in this project.
    pub fn auto_contd(&self) -> bool {
        self.auto_contd.unwrap_or(self.format.is_screen())
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ChecksConfig {
    /// Check codes switched off for this project.
    #[serde(default)]
    pub off: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EpisodeMeta {
    pub number: u32,
    #[serde(default)]
    pub title: Option<String>,
}

/// One act folder: `acts/NN`, or `episodes/EE/acts/NN` in a series.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct ActRef {
    pub episode: Option<u32>,
    pub act: u32,
}

impl ActRef {
    pub fn dir(&self, root: &Path) -> PathBuf {
        match self.episode {
            Some(e) => root
                .join("episodes")
                .join(format!("{e:02}"))
                .join("acts")
                .join(format!("{:02}", self.act)),
            None => root.join("acts").join(format!("{:02}", self.act)),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Location {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Character {
    pub id: String,
    pub cue: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// What the action calls them after they're introduced ("Joanna").
    #[serde(default)]
    pub short: Option<String>,
    /// Doubling track, e.g. `actor-3`. `null` = unassigned; `any-free` = anyone not on stage.
    #[serde(default)]
    pub track: Option<String>,
}

#[derive(Debug)]
pub struct SceneFile {
    pub path: PathBuf,
    /// Act number taken from the folder name (`acts/02` -> 2).
    pub act: u32,
    /// Episode number in a series (`episodes/03/…` -> 3).
    pub episode: Option<u32>,
    pub scene: Scene,
}

#[derive(Debug)]
pub struct Project {
    pub root: PathBuf,
    pub meta: ProjectMeta,
    pub characters: Vec<Character>,
    pub locations: Vec<Location>,
    /// Act folders found on disk, in order, including empty ones.
    pub act_refs: Vec<ActRef>,
    /// Episode folders found on disk (series only).
    pub episode_numbers: Vec<u32>,
    pub series: bool,
    /// In running order: act folder, then file name.
    pub scenes: Vec<SceneFile>,
}

#[derive(Debug)]
pub enum ProjectError {
    Io(PathBuf, std::io::Error),
    Yaml(PathBuf, serde_yaml::Error),
    Scene(PathBuf, ParseError),
    DuplicateId(String, PathBuf, PathBuf),
    BadActFolder(PathBuf),
}

impl SceneFile {
    pub fn act_ref(&self) -> ActRef {
        ActRef {
            episode: self.episode,
            act: self.act,
        }
    }
}

impl fmt::Display for ProjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectError::Io(p, e) => write!(f, "{}: {e}", p.display()),
            ProjectError::Yaml(p, e) => write!(f, "{}: {e}", p.display()),
            ProjectError::Scene(p, e) => write!(f, "{}: {e}", p.display()),
            ProjectError::DuplicateId(id, a, b) => {
                write!(f, "scene id '{id}' is used by both {} and {}", a.display(), b.display())
            }
            ProjectError::BadActFolder(p) => {
                write!(
                    f,
                    "{}: act and episode folders must be numbers, e.g. acts/01",
                    p.display()
                )
            }
        }
    }
}

impl std::error::Error for ProjectError {}

fn number_of(dir: &Path) -> Result<u32, ProjectError> {
    dir.file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| ProjectError::BadActFolder(dir.to_path_buf()))
}

fn read(path: &Path) -> Result<String, ProjectError> {
    std::fs::read_to_string(path).map_err(|e| ProjectError::Io(path.to_path_buf(), e))
}

fn sorted_entries(dir: &Path) -> Result<Vec<PathBuf>, ProjectError> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| ProjectError::Io(dir.to_path_buf(), e))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    v.sort();
    Ok(v)
}

impl Project {
    pub fn load(root: impl AsRef<Path>) -> Result<Project, ProjectError> {
        let root = root.as_ref().to_path_buf();
        let meta_path = root.join("project.yaml");
        let meta: ProjectMeta =
            serde_yaml::from_str(&read(&meta_path)?).map_err(|e| ProjectError::Yaml(meta_path, e))?;

        let chars_path = root.join("characters.yaml");
        let characters = if chars_path.exists() {
            serde_yaml::from_str(&read(&chars_path)?).map_err(|e| ProjectError::Yaml(chars_path, e))?
        } else {
            Vec::new()
        };

        let locs_path = root.join("locations.yaml");
        let locations = if locs_path.exists() {
            serde_yaml::from_str(&read(&locs_path)?).map_err(|e| ProjectError::Yaml(locs_path, e))?
        } else {
            Vec::new()
        };

        let series = meta.series || root.join("episodes").is_dir();
        let mut scenes: Vec<SceneFile> = Vec::new();
        let mut act_refs = Vec::new();
        let mut episode_numbers = Vec::new();
        let containers: Vec<(Option<u32>, PathBuf)> = if series {
            let dir = root.join("episodes");
            let mut v = Vec::new();
            for e in if dir.exists() {
                sorted_entries(&dir)?
            } else {
                Vec::new()
            } {
                if !e.is_dir() {
                    continue;
                }
                let n = number_of(&e)?;
                episode_numbers.push(n);
                v.push((Some(n), e.join("acts")));
            }
            v
        } else {
            vec![(None, root.join("acts"))]
        };
        for (episode, acts_dir) in containers {
            let act_dirs = if acts_dir.exists() {
                sorted_entries(&acts_dir)?
            } else {
                Vec::new()
            };
            for act_dir in act_dirs {
                if !act_dir.is_dir() {
                    continue;
                }
                let act = number_of(&act_dir)?;
                act_refs.push(ActRef { episode, act });
                for path in sorted_entries(&act_dir)? {
                    if path.extension().and_then(|e| e.to_str()) != Some("scene") {
                        continue;
                    }
                    // Tolerate files saved with Windows line endings.
                    let src = read(&path)?.replace("\r\n", "\n");
                    let scene = Scene::parse(&src).map_err(|e| ProjectError::Scene(path.clone(), e))?;
                    if let Some(other) = scenes.iter().find(|s| s.scene.meta.id == scene.meta.id) {
                        return Err(ProjectError::DuplicateId(
                            scene.meta.id.clone(),
                            other.path.clone(),
                            path,
                        ));
                    }
                    scenes.push(SceneFile {
                        path,
                        act,
                        episode,
                        scene,
                    });
                }
            }
        }
        Ok(Project {
            root,
            meta,
            characters,
            locations,
            act_refs,
            episode_numbers,
            series,
            scenes,
        })
    }

    /// Distinct act numbers (plays and films; in a series, across episodes).
    pub fn act_numbers(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.act_refs.iter().map(|a| a.act).collect();
        v.sort();
        v.dedup();
        v
    }

    pub fn episode_title(&self, n: u32) -> Option<String> {
        self.meta
            .episodes
            .iter()
            .find(|e| e.number == n)
            .and_then(|e| e.title.clone())
    }

    pub fn scene(&self, id: &str) -> Option<&SceneFile> {
        self.scenes.iter().find(|s| s.scene.meta.id == id)
    }

    pub fn character_by_cue(&self, cue: &str) -> Option<&Character> {
        self.characters.iter().find(|c| c.cue == cue)
    }
}
