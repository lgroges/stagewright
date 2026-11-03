//! The `.scene` file format: YAML frontmatter, then a Fountain-style script body
//! extended for the stage.
//!
//! Body grammar (line-based, LF line endings):
//! - blank line                       -> [`Element::Blank`] (ends a speech)
//! - `~ SONG "Title" [A, B]`          -> [`Element::SongStart`]
//! - `~ END SONG`                     -> [`Element::SongEnd`]
//! - `~`                              -> [`Element::LyricBreak`] (stanza break)
//! - `~ text`                         -> [`Element::Lyric`]
//! - `>TEXT<`                         -> [`Element::Centered`]
//! - `> TEXT`                         -> forced [`Element::Transition`]
//! - `!text`                          -> forced [`Element::Direction`]
//! - `@Name`                          -> forced [`Element::Cue`]
//! - `===`                            -> [`Element::PageBreak`]
//! - after a blank line, `INT.`/`EXT.`/`EST.`/`INT./EXT.`/`I/E` … or `.HEADING`
//!   -> [`Element::SceneHeading`] (an optional `#12A#` at the end is the scene number)
//! - an upper-case line ending `TO:` between blank lines -> [`Element::Transition`]
//! - after a blank line, an upper-case line followed by a non-blank line -> [`Element::Cue`]
//!   (a trailing `^` marks dual dialogue)
//! - inside a speech, `(text)`        -> [`Element::Parenthetical`]
//! - inside a speech, anything else   -> [`Element::Dialogue`]
//! - anything else                    -> [`Element::Direction`]
//!
//! Parsing then serialising any file reproduces it byte for byte.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Element {
    Blank,
    Cue(Cue),
    Parenthetical(String),
    Dialogue(String),
    Direction { text: String, forced: bool },
    SongStart(SongStart),
    Lyric(String),
    LyricBreak,
    SongEnd,
    Transition { text: String, forced: bool },
    SceneHeading(SceneHeading),
    Centered(String),
    PageBreak(String),
}

impl Element {
    /// Stable name for each element type, shared with the editor's classifier.
    pub fn kind(&self) -> &'static str {
        match self {
            Element::Blank => "blank",
            Element::Cue(_) => "cue",
            Element::Parenthetical(_) => "parenthetical",
            Element::Dialogue(_) => "dialogue",
            Element::Direction { .. } => "direction",
            Element::SongStart(_) => "song_start",
            Element::Lyric(_) => "lyric",
            Element::LyricBreak => "lyric_break",
            Element::SongEnd => "song_end",
            Element::Transition { .. } => "transition",
            Element::SceneHeading(_) => "scene_heading",
            Element::Centered(_) => "centered",
            Element::PageBreak(_) => "page_break",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    /// As written, without extension, e.g. `TOBIAS & HETTIE`.
    pub name: String,
    /// e.g. `CONT'D`, `V.O.`.
    pub extension: Option<String>,
    pub forced: bool,
    /// Second half of a dual-dialogue pair (`NAME ^`).
    pub dual: bool,
    /// The line exactly as written.
    pub raw: String,
}

/// `INT. LIGHTHOUSE - NIGHT #12#`
#[derive(Debug, Clone, PartialEq)]
pub struct SceneHeading {
    /// The heading without a leading `.` or the scene number.
    pub text: String,
    /// `12A` from `#12A#`.
    pub number: Option<String>,
    pub forced: bool,
    pub raw: String,
}

impl SceneHeading {
    /// `INT. LIGHTHOUSE - NIGHT` -> (`INT.`, `LIGHTHOUSE`, `NIGHT`).
    pub fn parts(&self) -> (Option<String>, String, Option<String>) {
        let t = self.text.trim();
        let upper = t.to_uppercase();
        let mut setting = None;
        let mut rest = t;
        for p in HEADING_PREFIXES {
            if upper.starts_with(p) && t.is_char_boundary(p.len()) {
                let after = &t[p.len()..];
                if after.is_empty() || after.starts_with('.') || after.starts_with(' ') {
                    let after = after.strip_prefix('.').unwrap_or(after);
                    setting = Some(format!("{p}."));
                    rest = after.trim_start();
                    break;
                }
            }
        }
        match rest.rsplit_once(" - ") {
            Some((loc, time)) => (setting, loc.trim().to_string(), Some(time.trim().to_string())),
            None => (setting, rest.trim().to_string(), None),
        }
    }
}

/// A closing line: THE END, END OF ACT ONE, END OF SHOW (with or without > <).
pub fn is_end_line(text: &str) -> bool {
    let t = text
        .trim()
        .trim_start_matches('>')
        .trim_end_matches('<')
        .trim()
        .trim_end_matches('.')
        .to_uppercase();
    t == "THE END" || t.starts_with("END OF ")
}

/// Longest first, so `INT./EXT` wins over `INT`.
const HEADING_PREFIXES: &[&str] = &["INT./EXT", "INT/EXT", "I/E", "INT", "EXT", "EST"];

fn is_heading_text(line: &str) -> bool {
    if let Some(rest) = line.strip_prefix('.') {
        return rest.chars().next().is_some_and(|c| c.is_alphanumeric());
    }
    let head: String = line.chars().take(9).collect::<String>().to_uppercase();
    HEADING_PREFIXES.iter().any(|p| {
        head.starts_with(p)
            && matches!(
                head.get(p.len()..).and_then(|r| r.chars().next()),
                Some('.') | Some(' ')
            )
    })
}

fn parse_heading(line: &str) -> SceneHeading {
    let (forced, body) = match line.strip_prefix('.') {
        Some(b) => (true, b),
        None => (false, line),
    };
    let trimmed = body.trim_end();
    let (text, number) = match trimmed.strip_suffix('#').and_then(|t| t.rfind('#').map(|i| (t, i))) {
        Some((t, i)) if i > 0 && !t[i + 1..].is_empty() && !t[i + 1..].contains(' ') => {
            (t[..i].trim_end().to_string(), Some(t[i + 1..].to_string()))
        }
        _ => (body.to_string(), None),
    };
    SceneHeading {
        text,
        number,
        forced,
        raw: line.to_string(),
    }
}

fn is_transition_text(line: &str) -> bool {
    let t = line.trim_end();
    t.ends_with("TO:") && t.chars().any(|c| c.is_ascii_uppercase()) && !t.chars().any(|c| c.is_lowercase())
}

fn is_page_break(line: &str) -> bool {
    line.len() >= 3 && line.bytes().all(|b| b == b'=')
}

impl Cue {
    /// Individual speakers: `TOBIAS & HETTIE` -> [`TOBIAS`, `HETTIE`].
    pub fn speakers(&self) -> Vec<&str> {
        self.name.split('&').map(str::trim).filter(|s| !s.is_empty()).collect()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SongStart {
    pub title: Option<String>,
    pub singers: Vec<String>,
    /// The line exactly as written, so spacing survives a round trip.
    pub raw: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct SceneMeta {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub act: Option<u32>,
    #[serde(default)]
    pub scene: Option<u32>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    /// Manual running time in minutes; overrides the estimate when set.
    #[serde(default)]
    pub duration: Option<f64>,
    #[serde(default)]
    pub onstage_silent: Vec<String>,
    #[serde(default)]
    pub synopsis: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Ratings 1–9 keyed by the project's rating field names.
    #[serde(default)]
    pub ratings: BTreeMap<String, u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    /// Frontmatter text between the `---` fences, kept verbatim (comments included).
    pub frontmatter: String,
    pub meta: SceneMeta,
    pub body: Vec<Element>,
    pub trailing_newline: bool,
}

#[derive(Debug)]
pub enum ParseError {
    MissingFrontmatter,
    UnclosedFrontmatter,
    Yaml(serde_yaml::Error),
    CrLf,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::MissingFrontmatter => write!(f, "file must start with a '---' frontmatter fence"),
            ParseError::UnclosedFrontmatter => write!(f, "frontmatter has no closing '---' fence"),
            ParseError::Yaml(e) => write!(f, "frontmatter YAML: {e}"),
            ParseError::CrLf => write!(f, "CRLF line endings are not supported; save with LF"),
        }
    }
}

impl std::error::Error for ParseError {}

impl Scene {
    pub fn parse(src: &str) -> Result<Scene, ParseError> {
        if src.contains('\r') {
            return Err(ParseError::CrLf);
        }
        let rest = src.strip_prefix("---\n").ok_or(ParseError::MissingFrontmatter)?;
        let end = find_closing_fence(rest).ok_or(ParseError::UnclosedFrontmatter)?;
        let frontmatter = rest[..end].to_string();
        let after = &rest[end..];
        let body_src = after
            .strip_prefix("---\n")
            .or_else(|| after.strip_prefix("---"))
            .unwrap_or("");
        let meta: SceneMeta = serde_yaml::from_str(&frontmatter).map_err(ParseError::Yaml)?;
        let trailing_newline = body_src.ends_with('\n');
        let body = parse_body(body_src.strip_suffix('\n').unwrap_or(body_src));
        Ok(Scene {
            frontmatter,
            meta,
            body,
            trailing_newline,
        })
    }

    pub fn to_source(&self) -> String {
        let mut out = String::with_capacity(self.frontmatter.len() + 4096);
        out.push_str("---\n");
        out.push_str(&self.frontmatter);
        out.push_str("---\n");
        out.push_str(&serialize_body(&self.body));
        if self.trailing_newline {
            out.push('\n');
        }
        out
    }
}

/// Byte offset (within `rest`) of a line that is exactly `---`.
fn find_closing_fence(rest: &str) -> Option<usize> {
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches('\n') == "---" {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

fn is_cue_text(line: &str) -> bool {
    let core = strip_extension(line).0;
    !core.is_empty()
        && core.len() <= 60
        && core.chars().any(|c| c.is_ascii_uppercase())
        && !core.chars().any(|c| c.is_lowercase())
        && !core.starts_with(['(', '~', '>', '!', '#'])
}

/// `ROSALIND (CONT'D)` -> (`ROSALIND`, Some(`CONT'D`)).
pub fn strip_extension(line: &str) -> (&str, Option<&str>) {
    let t = line.trim_end();
    if t.ends_with(')')
        && let Some(open) = t.rfind(" (")
    {
        let ext = &t[open + 2..t.len() - 1];
        if !ext.chars().any(|c| c.is_lowercase()) {
            return (&t[..open], Some(ext));
        }
    }
    (t, None)
}

fn parse_cue(raw: &str, forced: bool) -> Cue {
    let line = if forced { &raw[1..] } else { raw };
    let trimmed = line.trim_end();
    let (body, dual) = match trimmed.strip_suffix('^') {
        Some(b) => (b.trim_end(), true),
        None => (trimmed, false),
    };
    let (name, ext) = strip_extension(body);
    Cue {
        name: name.to_string(),
        extension: ext.map(str::to_string),
        forced,
        dual,
        raw: raw.to_string(),
    }
}

fn parse_song_start(line: &str) -> SongStart {
    let rest = line.trim_start_matches("~ SONG").trim();
    let title = rest
        .find('"')
        .and_then(|a| rest[a + 1..].find('"').map(|b| rest[a + 1..a + 1 + b].to_string()));
    let singers = rest
        .rfind('[')
        .and_then(|a| rest[a + 1..].find(']').map(|b| &rest[a + 1..a + 1 + b]))
        .map(|s| {
            s.split([',', '&'])
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect()
        })
        .unwrap_or_default();
    SongStart {
        title,
        singers,
        raw: line.to_string(),
    }
}

/// A parenthetical opened on line `i` closes within the next three lines of
/// the speech (so it was wrapped by hand, not left unclosed).
fn closes_soon(lines: &[&str], i: usize) -> bool {
    lines[i + 1..]
        .iter()
        .take(3)
        .take_while(|l| !l.is_empty() && !l.starts_with('('))
        .any(|l| l.ends_with(')'))
}

pub fn parse_body(src: &str) -> Vec<Element> {
    let lines: Vec<&str> = if src.is_empty() {
        Vec::new()
    } else {
        src.split('\n').collect()
    };
    let mut out = Vec::with_capacity(lines.len());
    let mut in_speech = false;
    // Inside a parenthetical wrapped onto more lines: "(in Russian;" / "English subtitles)".
    let mut in_paren = false;
    for (i, &line) in lines.iter().enumerate() {
        let prev_blank = i == 0 || lines[i - 1].is_empty();
        let next_nonblank = lines.get(i + 1).is_some_and(|l| !l.is_empty());

        let el = if line.is_empty() {
            in_speech = false;
            in_paren = false;
            Element::Blank
        } else if line == "~ END SONG" {
            Element::SongEnd
        } else if line.starts_with("~ SONG") {
            Element::SongStart(parse_song_start(line))
        } else if line == "~" {
            Element::LyricBreak
        } else if let Some(l) = line.strip_prefix("~ ") {
            Element::Lyric(l.to_string())
        } else if line.len() >= 2 && line.starts_with('>') && line.ends_with('<') {
            in_speech = false;
            Element::Centered(line.to_string())
        } else if let Some(t) = line.strip_prefix("> ") {
            in_speech = false;
            Element::Transition {
                text: t.to_string(),
                forced: true,
            }
        } else if let Some(t) = line.strip_prefix('!') {
            in_speech = false;
            Element::Direction {
                text: t.to_string(),
                forced: true,
            }
        } else if line.starts_with('@') {
            in_speech = true;
            Element::Cue(parse_cue(line, true))
        } else if is_page_break(line) {
            in_speech = false;
            Element::PageBreak(line.to_string())
        } else if prev_blank && is_heading_text(line) {
            in_speech = false;
            Element::SceneHeading(parse_heading(line))
        } else if prev_blank && !next_nonblank && is_transition_text(line) {
            in_speech = false;
            Element::Transition {
                text: line.to_string(),
                forced: false,
            }
        } else if prev_blank && next_nonblank && is_cue_text(line) {
            in_speech = true;
            Element::Cue(parse_cue(line, false))
        } else if in_speech && (in_paren || (line.starts_with('(') && (line.ends_with(')') || closes_soon(&lines, i))))
        {
            in_paren = !line.ends_with(')');
            Element::Parenthetical(line.to_string())
        } else if in_speech {
            Element::Dialogue(line.to_string())
        } else {
            Element::Direction {
                text: line.to_string(),
                forced: false,
            }
        };
        out.push(el);
    }
    out
}

/// Indices of the cues that should read `(CONT'D)`: the same character (or
/// characters) as the speech before, with only action in between, in the same
/// scene. A scene heading, transition, centred text or page break starts afresh.
/// Dual dialogue, group cues (ALL) and cues already marked CONT'D are left alone.
pub fn continued_cues(body: &[Element]) -> Vec<usize> {
    let mut out = Vec::new();
    let mut last: Vec<&str> = Vec::new();
    for (i, el) in body.iter().enumerate() {
        match el {
            Element::Cue(c) => {
                let speakers: Vec<&str> = c.speakers();
                let group = speakers.iter().all(|s| *s == "ALL");
                // Any spelling already typed: (CONT'D), (CONT.), (CONTD), (CONTINUED)…
                let marked = c
                    .extension
                    .as_deref()
                    .is_some_and(|e| e.to_uppercase().contains("CONT"));
                if !c.dual && !group && !marked && !last.is_empty() && speakers == last {
                    out.push(i);
                }
                last = speakers;
            }
            Element::SceneHeading(_) | Element::Transition { .. } | Element::Centered(_) | Element::PageBreak(_) => {
                last.clear()
            }
            _ => {}
        }
    }
    out
}

pub fn serialize_body(body: &[Element]) -> String {
    let mut lines: Vec<String> = Vec::with_capacity(body.len());
    for el in body {
        lines.push(match el {
            Element::Blank => String::new(),
            Element::Cue(c) => c.raw.clone(),
            Element::SceneHeading(h) => h.raw.clone(),
            Element::Centered(t) | Element::PageBreak(t) => t.clone(),
            Element::Parenthetical(t) | Element::Dialogue(t) => t.clone(),
            Element::Direction { text, forced } => {
                if *forced {
                    format!("!{text}")
                } else {
                    text.clone()
                }
            }
            Element::SongStart(s) => s.raw.clone(),
            Element::Lyric(t) => format!("~ {t}"),
            Element::LyricBreak => "~".to_string(),
            Element::SongEnd => "~ END SONG".to_string(),
            Element::Transition { text, forced } => {
                if *forced {
                    format!("> {text}")
                } else {
                    text.clone()
                }
            }
        });
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "---\nid: sc-1\ntitle: \"Test\"\nduration: 4.5 # minutes\n---\nThe lighthouse. Rain.\n\nMORDECAI\n(reading)\nThe Admiral sends his regards.\n\nROSALIND (CONT'D)\nAnd no more ships.\n\nTOBIAS & HETTIE\nAmen.\n\nHETTIE\n(singing)\n~ SONG \"Lanterns\" [HETTIE]\n~ Oh, robin,\n~\n~ Sing me your song,\n~ END SONG\n\n> BLACKOUT.\n";

    #[test]
    fn round_trip_is_exact() {
        let s = Scene::parse(SAMPLE).unwrap();
        assert_eq!(s.to_source(), SAMPLE);
    }

    #[test]
    fn classifies_elements() {
        let s = Scene::parse(SAMPLE).unwrap();
        assert_eq!(s.meta.id, "sc-1");
        assert_eq!(s.meta.duration, Some(4.5));
        assert!(matches!(&s.body[0], Element::Direction { text, .. } if text == "The lighthouse. Rain."));
        assert!(matches!(&s.body[2], Element::Cue(c) if c.name == "MORDECAI"));
        assert!(matches!(&s.body[3], Element::Parenthetical(_)));
        assert!(matches!(&s.body[4], Element::Dialogue(_)));
        assert!(matches!(&s.body[6], Element::Cue(c) if c.extension.as_deref() == Some("CONT'D")));
        let duet = s.body.iter().find_map(|e| match e {
            Element::Cue(c) if c.name.contains('&') => Some(c.speakers()),
            _ => None,
        });
        assert_eq!(duet, Some(vec!["TOBIAS", "HETTIE"]));
        let song = s
            .body
            .iter()
            .find_map(|e| match e {
                Element::SongStart(s) => Some(s.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(song.title.as_deref(), Some("Lanterns"));
        assert_eq!(song.singers, vec!["HETTIE"]);
        assert!(s.body.contains(&Element::LyricBreak));
        assert!(matches!(s.body.last(), Some(Element::Transition { text, forced: true }) if text == "BLACKOUT."));
    }

    #[test]
    fn upper_case_dialogue_is_not_a_cue() {
        // "I…" follows a cue directly, so it is dialogue, not a new speaker.
        let body = parse_body("MORDECAI\nI…\n\nQUEEN\nWell?");
        assert!(matches!(&body[1], Element::Dialogue(t) if t == "I…"));
    }

    #[test]
    fn caps_direction_followed_by_blank_is_not_a_cue() {
        let body = parse_body("Some words.\n\nBLACKOUT.\n\nKING\nHello.");
        assert!(matches!(&body[2], Element::Direction { .. }));
    }

    const SCREEN: &str = "---\nid: s1\n---\nINT. LIGHTHOUSE - NIGHT #12A#\n\nRain lashes the windows.\n\nMORDECAI (V.O.)\nAnother night.\n\nROSALIND ^\nAnother storm.\n\nCUT TO:\n\n.FLASHBACK\n\nint./ext. car - continuous\n\n>THE END<\n\n===\n\nInterior design is not a heading.\n";

    #[test]
    fn continued_cues_follow_the_same_speaker_after_action() {
        let body = parse_body(
            "HETTIE\nHello.\n\nShe turns.\n\nHETTIE\nStill here.\n\nTOBIAS\nYes.\n\nHETTIE\nWell.\n\nHETTIE (V.O.)\nAnd again.\n\nHETTIE (CONT'D)\nTyped.\n\nShe sits.\n\nHETTIE (CONT.)\nTyped the other way.\n\nINT. HALL - DAY\n\nHETTIE\nNew scene.\n\nALL\nYes!\n\nALL\nNo!\n\nTOBIAS & HETTIE\nTogether.\n\nTOBIAS & HETTIE\nAgain.\n\nTOBIAS ^\nAt once.",
        );
        let cues: Vec<String> = continued_cues(&body)
            .into_iter()
            .map(|i| match &body[i] {
                Element::Cue(c) => c.raw.clone(),
                _ => unreachable!(),
            })
            .collect();
        assert_eq!(cues, vec!["HETTIE", "HETTIE (V.O.)", "TOBIAS & HETTIE"]);
    }

    #[test]
    fn screenplay_elements() {
        let s = Scene::parse(SCREEN).unwrap();
        assert_eq!(s.to_source(), SCREEN);
        let kinds: Vec<&str> = s.body.iter().map(Element::kind).collect();
        assert_eq!(
            kinds,
            vec![
                "scene_heading",
                "blank",
                "direction",
                "blank",
                "cue",
                "dialogue",
                "blank",
                "cue",
                "dialogue",
                "blank",
                "transition",
                "blank",
                "scene_heading",
                "blank",
                "scene_heading",
                "blank",
                "centered",
                "blank",
                "page_break",
                "blank",
                "direction"
            ]
        );
        let Element::SceneHeading(h) = &s.body[0] else { panic!() };
        assert_eq!(h.number.as_deref(), Some("12A"));
        assert_eq!(
            h.parts(),
            (Some("INT.".into()), "LIGHTHOUSE".into(), Some("NIGHT".into()))
        );
        let Element::Cue(c) = &s.body[4] else { panic!() };
        assert_eq!((c.name.as_str(), c.extension.as_deref()), ("MORDECAI", Some("V.O.")));
        let Element::Cue(d) = &s.body[7] else { panic!() };
        assert!(d.dual && d.name == "ROSALIND");
        let Element::SceneHeading(f) = &s.body[12] else {
            panic!()
        };
        assert!(f.forced && f.text == "FLASHBACK");
        let Element::SceneHeading(ie) = &s.body[14] else {
            panic!()
        };
        assert_eq!(ie.parts().0.as_deref(), Some("INT./EXT."));
    }

    #[test]
    fn rejects_crlf() {
        assert!(matches!(Scene::parse("---\r\nid: x\r\n---\r\n"), Err(ParseError::CrLf)));
    }

    #[test]
    fn parentheticals_wrapped_onto_two_lines() {
        let b = parse_body("NELL\n(in Russian;\nEnglish subtitles)\nHi.\n\nTOM\n(never closed\nSo this is dialogue.");
        let kinds: Vec<&str> = b.iter().map(Element::kind).collect();
        assert_eq!(
            kinds,
            [
                "cue",
                "parenthetical",
                "parenthetical",
                "dialogue",
                "blank",
                "cue",
                "dialogue",
                "dialogue"
            ]
        );
    }
}
