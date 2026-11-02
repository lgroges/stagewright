//! The script check: rules that find likely mistakes and departures from
//! convention, scene by scene, with a safe fix where there is one.
//!
//! Every rule has a code (used to switch it off in project.yaml:
//! `checks: {off: [contd-mismatch]}`), a severity, and a message written for
//! the writer, not the parser. Positions are body lines (0-based) and columns
//! in characters, so the editor can underline the exact words.

use crate::cast::replace_word;
use crate::layout::Format;
use crate::scene::{Element, SceneMeta, parse_body, strip_extension};
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Probably a mistake: the script reads differently from what was meant.
    Warning,
    /// A convention, or worth a look.
    Info,
}

/// One change a fix makes, by body line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Edit {
    Replace { line: usize, text: String },
    InsertBefore { line: usize, text: String },
    InsertAfter { line: usize, text: String },
    Delete { line: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fix {
    pub label: String,
    pub edits: Vec<Edit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Issue {
    pub code: &'static str,
    pub severity: Severity,
    pub line: usize,
    /// Columns in characters (not bytes); `from == to` marks the whole line.
    pub from: usize,
    pub to: usize,
    pub message: String,
    pub fix: Option<Fix>,
}

/// Every check, with a short name for settings and the "switch off" action.
pub const CHECKS: &[(&str, &str)] = &[
    ("lonely-cue", "A cue with a blank line after it"),
    ("cue-not-caps", "A cue not written in capitals"),
    ("cue-unknown", "A cue that isn't in the cast list"),
    ("song-unclosed", "A song with no ~ END SONG"),
    ("song-stray-end", "~ END SONG with no song open"),
    ("lyric-outside-song", "A lyric outside a song"),
    ("parenthetical-unclosed", "A parenthetical with no closing bracket"),
    ("dual-orphan", "Dual dialogue (^) with nothing beside it"),
    ("contd-mismatch", "(CONT'D) after a different speaker"),
    ("silent-speaks", "On stage but silent, yet speaks"),
    ("heading-no-time", "A scene heading with no time of day"),
    ("heading-time", "A misspelt time of day"),
    ("heading-setting", "INT/EXT written without a full stop"),
    (
        "screen-scene-start",
        "A film or TV scene that doesn't start with a heading",
    ),
    ("intro-caps", "A character's first appearance not in capitals"),
];

/// What the check knows about the rest of the project.
#[derive(Debug, Clone, Default)]
pub struct Context {
    pub format: Format,
    /// Cues in characters.yaml.
    pub cast: BTreeSet<String>,
    /// Every cue spoken anywhere, plus the cast.
    pub known: BTreeSet<String>,
    /// Characters already introduced in earlier scenes (screen formats).
    pub introduced: BTreeSet<String>,
    pub off: BTreeSet<String>,
}

const GROUP_CUES: &[&str] = &["ALL"];

pub const TIMES: &[&str] = &[
    "DAY",
    "NIGHT",
    "MORNING",
    "AFTERNOON",
    "EVENING",
    "DAWN",
    "DUSK",
    "SUNRISE",
    "SUNSET",
    "MIDNIGHT",
    "NOON",
    "CONTINUOUS",
    "LATER",
    "MOMENTS LATER",
    "SAME",
    "SAME TIME",
    "PRE-DAWN",
    "MAGIC HOUR",
];

const TIME_ALIASES: &[(&str, &str)] = &[
    ("NITE", "NIGHT"),
    ("NIGHTTIME", "NIGHT"),
    ("NIGHT TIME", "NIGHT"),
    ("DAYTIME", "DAY"),
    ("EVE", "EVENING"),
    ("MORN", "MORNING"),
    ("CONT", "CONTINUOUS"),
    ("CONT'D", "CONTINUOUS"),
    ("CONTINUED", "CONTINUOUS"),
    ("CONTINOUS", "CONTINUOUS"),
];

/// Levenshtein distance, by characters.
pub fn distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// The closest of `candidates` to `word`, if it is close enough to be a typo.
fn near<'a>(word: &str, candidates: impl IntoIterator<Item = &'a String>) -> Option<&'a String> {
    let n = word.chars().count();
    if n < 3 {
        return None;
    }
    let limit = if n <= 5 { 1 } else { 2 };
    candidates
        .into_iter()
        .filter(|c| c.as_str() != word)
        .map(|c| (distance(word, c), c))
        .filter(|(d, _)| *d <= limit)
        .min_by_key(|(d, c)| (*d, (*c).clone()))
        .map(|(_, c)| c)
}

/// Character columns of the first whole-word `word` in `line`.
fn cols(line: &str, word: &str) -> (usize, usize) {
    let (marked, n) = replace_word(line, word, "\u{0}");
    if n == 0 {
        return (0, 0);
    }
    let from = marked.chars().take_while(|c| *c != '\u{0}').count();
    (from, from + word.chars().count())
}

fn issue(
    code: &'static str,
    severity: Severity,
    line: usize,
    span: (usize, usize),
    message: String,
    fix: Option<Fix>,
) -> Issue {
    Issue {
        code,
        severity,
        line,
        from: span.0,
        to: span.1,
        message,
        fix,
    }
}

fn fix(label: impl Into<String>, edits: Vec<Edit>) -> Option<Fix> {
    Some(Fix {
        label: label.into(),
        edits,
    })
}

fn is_contd(ext: &str) -> bool {
    let e = ext.trim().to_uppercase().replace('’', "'");
    e == "CONT'D" || e == "CONTD" || e == "CONT."
}

/// `Hettie` -> `HETTIE` mentions, when an all-capitals copy is a different length
/// (rare non-ASCII cases) the search is skipped.
fn find_name_not_caps(text: &str, name: &str) -> Option<(usize, usize)> {
    let upper = text.to_uppercase();
    if upper.len() != text.len() {
        return None;
    }
    let mut start = 0;
    while let Some(i) = upper[start..].find(name).map(|i| i + start) {
        let end = i + name.len();
        let before = text[..i].chars().next_back();
        let after = text[end..].chars().next();
        let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        if !word(before) && !word(after) && &text[i..end] != name {
            let from = text[..i].chars().count();
            return Some((from, from + text[i..end].chars().count()));
        }
        start = end;
    }
    None
}

/// Check one scene body. `ctx.introduced` is updated with characters this
/// scene introduces, so a project check can carry it from scene to scene.
pub fn check_scene(body: &str, meta: &SceneMeta, ctx: &mut Context) -> Vec<Issue> {
    let lines: Vec<&str> = if body.is_empty() {
        vec![]
    } else {
        body.split('\n').collect()
    };
    let els = parse_body(body);
    let screen = ctx.format.is_screen();
    let mut out: Vec<Issue> = Vec::new();

    let mut open_song: Option<usize> = None;
    let mut last_lyric: Option<usize> = None;
    let mut speakers: Vec<String> = Vec::new(); // of the current speech
    let mut spoken: BTreeSet<String> = BTreeSet::new();
    let blank = |i: usize| lines.get(i).is_none_or(|l| l.is_empty());

    if screen && !lines.iter().all(|l| l.trim().is_empty()) {
        let first = els.iter().position(|e| !matches!(e, Element::Blank)).unwrap_or(0);
        if !matches!(
            els.get(first),
            Some(Element::SceneHeading(_)) | Some(Element::PageBreak(_))
        ) {
            out.push(issue(
                "screen-scene-start",
                Severity::Info,
                first,
                (0, 0),
                "Film and TV scenes usually start with a scene heading (INT. or EXT.).".into(),
                None,
            ));
        }
    }

    for (i, el) in els.iter().enumerate() {
        let text = lines.get(i).copied().unwrap_or("");
        match el {
            Element::Cue(c) => {
                let prev_speakers = std::mem::take(&mut speakers); // of the speech before
                speakers = c
                    .speakers()
                    .into_iter()
                    .filter(|s| !GROUP_CUES.contains(s))
                    .map(str::to_string)
                    .collect();
                for s in &speakers {
                    spoken.insert(s.clone());
                    if !ctx.cast.is_empty() && !ctx.cast.contains(s) {
                        let span = cols(text, s);
                        match near(s, &ctx.cast) {
                            Some(m) => out.push(issue(
                                "cue-unknown",
                                Severity::Warning,
                                i,
                                span,
                                format!("{s} isn't in the cast list. Did you mean {m}?"),
                                fix(
                                    format!("Change to {m}"),
                                    vec![Edit::Replace {
                                        line: i,
                                        text: replace_word(text, s, m).0,
                                    }],
                                ),
                            )),
                            None => out.push(issue(
                                "cue-unknown",
                                Severity::Info,
                                i,
                                span,
                                format!("{s} isn't in the cast list (add them in Cast)."),
                                None,
                            )),
                        }
                    }
                }
                if let Some(ext) = &c.extension
                    && is_contd(ext)
                    && (prev_speakers.is_empty() || prev_speakers != speakers)
                {
                    let who = if prev_speakers.is_empty() {
                        "there's no speech before it in this scene".to_string()
                    } else {
                        format!("the speech before is {}'s", prev_speakers.join(" & "))
                    };
                    let cleaned = text.replacen(&format!(" ({ext})"), "", 1);
                    out.push(issue(
                        "contd-mismatch",
                        Severity::Info,
                        i,
                        (0, 0),
                        format!("({ext}) means the same character carries on, but {who}."),
                        (cleaned != text).then(|| Fix {
                            label: format!("Remove ({ext})"),
                            edits: vec![Edit::Replace { line: i, text: cleaned }],
                        }),
                    ));
                }
                if c.dual {
                    // The speech it sits beside must end just before it.
                    let before = (0..i).rev().find(|&j| !matches!(els[j], Element::Blank));
                    let ok = before.is_some_and(|j| {
                        matches!(
                            els[j],
                            Element::Dialogue(_) | Element::Parenthetical(_) | Element::SongEnd
                        )
                    });
                    if !ok {
                        let cleaned = text.trim_end().trim_end_matches('^').trim_end().to_string();
                        out.push(issue(
                            "dual-orphan",
                            Severity::Warning,
                            i,
                            (0, 0),
                            "^ puts this speech beside the one before it, but there isn't one just before.".into(),
                            fix("Remove the ^", vec![Edit::Replace { line: i, text: cleaned }]),
                        ));
                    }
                }
                if screen && ctx.format != Format::TvMulti {
                    for s in &speakers {
                        ctx.introduced.insert(s.clone());
                    }
                }
            }
            Element::Dialogue(t) => {
                if t.starts_with('(') && !t.contains(')') {
                    out.push(issue(
                        "parenthetical-unclosed",
                        Severity::Warning,
                        i,
                        (0, 0),
                        "This starts like a parenthetical but has no closing bracket, so it reads as dialogue.".into(),
                        fix(
                            "Add the )",
                            vec![Edit::Replace {
                                line: i,
                                text: format!("{})", t.trim_end()),
                            }],
                        ),
                    ));
                }
            }
            Element::Direction { text: t, forced } => {
                let core = strip_extension(t).0;
                let upper = core.to_uppercase();
                let names_a_character = ctx.known.contains(&upper) || GROUP_CUES.contains(&upper.as_str());
                if !forced && (i == 0 || blank(i - 1)) && names_a_character && core.trim() == core {
                    if core == upper && blank(i + 1) && !blank(i + 2) {
                        out.push(issue(
                            "lonely-cue",
                            Severity::Warning,
                            i,
                            (0, 0),
                            format!("{upper} has a blank line after it, so the lines below read as action, not as {upper}'s speech."),
                            fix("Remove the blank line", vec![Edit::Delete { line: i + 1 }]),
                        ));
                    } else if core != upper && !blank(i + 1) {
                        out.push(issue(
                            "cue-not-caps",
                            Severity::Warning,
                            i,
                            (0, 0),
                            format!("This looks like a cue for {upper}, but cues are written in capitals, so it reads as action."),
                            fix(format!("Write {upper}"), vec![Edit::Replace { line: i, text: text.replacen(core, &upper, 1) }]),
                        ));
                    }
                }
                // Introductions: a character's first mention in action is in capitals.
                // (Multi-camera action is printed in capitals anyway.)
                if screen && ctx.format != Format::TvMulti {
                    for name in ctx.known.clone() {
                        if ctx.introduced.contains(&name) || name.chars().count() < 2 {
                            continue;
                        }
                        let caps = cols(t, &name);
                        let lower = find_name_not_caps(t, &name);
                        match (caps.1 > 0, lower) {
                            (true, Some(l)) if l.0 < caps.0 => {}
                            (true, _) => {
                                ctx.introduced.insert(name.clone());
                                continue;
                            }
                            (false, None) => continue,
                            _ => {}
                        }
                        if let Some((from, to)) = lower {
                            let off = usize::from(*forced);
                            let span = (from + off, to + off);
                            let before: String = text.chars().take(span.0).collect();
                            let after: String = text.chars().skip(span.1).collect();
                            out.push(issue(
                                "intro-caps",
                                Severity::Info,
                                i,
                                span,
                                format!("First appearance of {name}: screenplays introduce a character in capitals."),
                                fix(
                                    format!("Write {name}"),
                                    vec![Edit::Replace {
                                        line: i,
                                        text: format!("{before}{name}{after}"),
                                    }],
                                ),
                            ));
                            ctx.introduced.insert(name.clone());
                        }
                    }
                }
            }
            Element::SongStart(s) => {
                if let Some(start) = open_song {
                    out.push(song_unclosed(start, last_lyric, &lines));
                }
                open_song = Some(i);
                last_lyric = None;
                for singer in &s.singers {
                    if !ctx.cast.is_empty() && !ctx.cast.contains(singer) && !GROUP_CUES.contains(&singer.as_str()) {
                        let span = cols(text, singer);
                        if let Some(m) = near(singer, &ctx.cast) {
                            out.push(issue(
                                "cue-unknown",
                                Severity::Warning,
                                i,
                                span,
                                format!("{singer} isn't in the cast list. Did you mean {m}?"),
                                fix(
                                    format!("Change to {m}"),
                                    vec![Edit::Replace {
                                        line: i,
                                        text: replace_word(text, singer, m).0,
                                    }],
                                ),
                            ));
                        }
                    }
                }
            }
            Element::Lyric(_) | Element::LyricBreak => {
                if open_song.is_none() {
                    // Only flag the first lyric of a run.
                    if !matches!(
                        els.get(i.wrapping_sub(1)),
                        Some(Element::Lyric(_)) | Some(Element::LyricBreak)
                    ) || i == 0
                    {
                        out.push(issue(
                            "lyric-outside-song",
                            Severity::Info,
                            i,
                            (0, 0),
                            "Lyrics outside a song. If they're a song or a reprise, start it with ~ SONG so it's in the song list.".into(),
                            fix("Start a song here", vec![Edit::InsertBefore { line: i, text: "~ SONG \"\" []".into() }]),
                        ));
                    }
                } else {
                    last_lyric = Some(i);
                }
            }
            Element::SongEnd => {
                if open_song.take().is_none() {
                    out.push(issue(
                        "song-stray-end",
                        Severity::Warning,
                        i,
                        (0, 0),
                        "~ END SONG, but no song is open.".into(),
                        fix("Remove this line", vec![Edit::Delete { line: i }]),
                    ));
                }
            }
            Element::SceneHeading(h) => {
                let (setting, _, time) = h.parts();
                if screen && time.is_none() {
                    out.push(issue(
                        "heading-no-time",
                        Severity::Info,
                        i,
                        (0, 0),
                        "No time of day: headings usually end with one, like - NIGHT.".into(),
                        None,
                    ));
                }
                if let Some(t) = time {
                    let core = t.split(" (").next().unwrap_or(&t).trim().to_uppercase();
                    if !TIMES.contains(&core.as_str()) {
                        let suggestion = TIME_ALIASES
                            .iter()
                            .find(|(a, _)| *a == core)
                            .map(|(_, b)| b.to_string())
                            .or_else(|| {
                                let times: Vec<String> = TIMES.iter().map(|t| t.to_string()).collect();
                                near(&core, &times).cloned()
                            });
                        if let Some(good) = suggestion {
                            let span = cols(text, t.split(" (").next().unwrap_or(&t).trim());
                            let replaced = replace_word(text, t.split(" (").next().unwrap_or(&t).trim(), &good).0;
                            out.push(issue(
                                "heading-time",
                                Severity::Info,
                                i,
                                span,
                                format!("{core}: did you mean {good}?"),
                                fix(
                                    format!("Change to {good}"),
                                    vec![Edit::Replace {
                                        line: i,
                                        text: replaced,
                                    }],
                                ),
                            ));
                        }
                    }
                }
                if let Some(set) = setting {
                    let raw = h.raw.trim_start_matches('.');
                    let written = &raw[..raw.len().min(set.len())];
                    let bare = set.trim_end_matches('.');
                    let missing_dot =
                        raw.to_uppercase().starts_with(bare) && !raw.to_uppercase().starts_with(&set) && bare != "I/E";
                    if missing_dot && !h.forced {
                        let span = (0, bare.chars().count());
                        let fixed = format!("{}{}", set, &raw[bare.len()..]);
                        out.push(issue(
                            "heading-setting",
                            Severity::Info,
                            i,
                            span,
                            format!("Write {set} with a full stop."),
                            fix(format!("Write {set}"), vec![Edit::Replace { line: i, text: fixed }]),
                        ));
                        let _ = written;
                    }
                }
            }
            Element::Blank
            | Element::Parenthetical(_)
            | Element::Transition { .. }
            | Element::Centered(_)
            | Element::PageBreak(_) => {}
        }
    }
    if let Some(start) = open_song {
        out.push(song_unclosed(start, last_lyric, &lines));
    }

    for s in &meta.onstage_silent {
        if spoken.contains(s) {
            let line = els
                .iter()
                .position(|e| matches!(e, Element::Cue(c) if c.speakers().contains(&s.as_str())))
                .unwrap_or(0);
            out.push(issue(
                "silent-speaks",
                Severity::Info,
                line,
                cols(lines.get(line).copied().unwrap_or(""), s),
                format!("{s} is listed as on stage but silent, but speaks in this scene (see Details)."),
                None,
            ));
        }
    }

    out.retain(|i| !ctx.off.contains(i.code));
    out.sort_by_key(|i| (i.line, i.from));
    out
}

fn song_unclosed(start: usize, last_lyric: Option<usize>, lines: &[&str]) -> Issue {
    let after = last_lyric.unwrap_or(start);
    issue(
        "song-unclosed",
        Severity::Warning,
        start,
        (0, lines.get(start).map(|l| l.chars().count()).unwrap_or(0)),
        "This song has no ~ END SONG, so everything after it reads as part of the song.".into(),
        fix(
            "End the song after its last lyric",
            vec![Edit::InsertAfter {
                line: after,
                text: "~ END SONG".into(),
            }],
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(format: Format) -> Context {
        let cast: BTreeSet<String> = ["HETTIE", "TOBIAS", "NELL", "SAILOR 1"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        Context {
            format,
            known: cast.clone(),
            cast,
            ..Default::default()
        }
    }

    fn codes(body: &str, format: Format) -> Vec<(&'static str, usize)> {
        check_scene(body, &SceneMeta::default(), &mut ctx(format))
            .iter()
            .map(|i| (i.code, i.line))
            .collect()
    }

    fn first(body: &str, format: Format) -> Issue {
        check_scene(body, &SceneMeta::default(), &mut ctx(format))
            .into_iter()
            .next()
            .expect("an issue")
    }

    #[test]
    fn clean_scenes_have_no_issues() {
        let stage = "HETTIE enters.\n\nHETTIE\n(quietly)\nHello.\n\nTOBIAS\nHi.\n\nTOBIAS & HETTIE\n~ SONG \"Lanterns\" [HETTIE, TOBIAS]\n~ Light one\n~\n~ Light two\n~ END SONG\n\n> BLACKOUT.";
        assert_eq!(codes(stage, Format::Stage), vec![]);
        let film = "INT. KITCHEN - NIGHT\n\nNELL (20s) waits.\n\nNELL\nLate.\n\nTOBIAS\nSorry.\n\nNELL ^\nHm.";
        assert_eq!(codes(film, Format::Film), vec![]);
    }

    #[test]
    fn a_cue_with_a_blank_line_after_it() {
        let i = first("Rain.\n\nHETTIE\n\nWhere is everyone?", Format::Stage);
        assert_eq!((i.code, i.line, i.severity), ("lonely-cue", 2, Severity::Warning));
        assert_eq!(i.fix.unwrap().edits, vec![Edit::Delete { line: 3 }]);
    }

    #[test]
    fn a_cue_in_mixed_case() {
        let i = first("Rain.\n\nHettie\nWhere is everyone?", Format::Stage);
        assert_eq!(i.code, "cue-not-caps");
        assert_eq!(
            i.fix.unwrap().edits,
            vec![Edit::Replace {
                line: 2,
                text: "HETTIE".into()
            }]
        );
        // An ordinary sentence starting with a name is not a cue.
        assert_eq!(codes("Hettie waits.\nShe sighs.", Format::Stage), vec![]);
    }

    #[test]
    fn unknown_cues_suggest_the_nearest_name() {
        let i = first("\nHETIE\nHi.", Format::Stage);
        assert_eq!(
            (i.code, i.severity, i.from, i.to),
            ("cue-unknown", Severity::Warning, 0, 5)
        );
        assert_eq!(i.message, "HETIE isn't in the cast list. Did you mean HETTIE?");
        assert_eq!(
            i.fix.unwrap().edits,
            vec![Edit::Replace {
                line: 1,
                text: "HETTIE".into()
            }]
        );
        let i = first("\nTOBIAS & HETIE (O.S.)\nHi.", Format::Stage);
        assert_eq!(
            i.fix.unwrap().edits,
            vec![Edit::Replace {
                line: 1,
                text: "TOBIAS & HETTIE (O.S.)".into()
            }]
        );
        let i = first("\nMARGARET\nHi.", Format::Stage);
        assert_eq!((i.severity, i.fix.is_none()), (Severity::Info, true));
        // No cast list at all: nothing to compare against.
        let mut c = ctx(Format::Stage);
        c.cast.clear();
        assert!(check_scene("\nMARGARET\nHi.", &SceneMeta::default(), &mut c).is_empty());
    }

    #[test]
    fn songs_must_open_and_close() {
        let i = first(
            "HETTIE\n~ SONG \"A\" [HETTIE]\n~ one\n~ two\n\nTOBIAS\nHi.",
            Format::Stage,
        );
        assert_eq!((i.code, i.line), ("song-unclosed", 1));
        assert_eq!(
            i.fix.unwrap().edits,
            vec![Edit::InsertAfter {
                line: 3,
                text: "~ END SONG".into()
            }]
        );
        assert_eq!(codes("Rain.\n~ END SONG", Format::Stage), vec![("song-stray-end", 1)]);
        assert_eq!(
            codes("HETTIE\n~ one\n~ two", Format::Stage),
            vec![("lyric-outside-song", 1)]
        );
        let i = first("\nHETTIE\n~ SONG \"A\" [HETIE]\n~ x\n~ END SONG", Format::Stage);
        assert_eq!((i.code, i.line), ("cue-unknown", 2));
    }

    #[test]
    fn brackets_dual_dialogue_and_contd() {
        let i = first("\nHETTIE\n(quietly\nHello.", Format::Stage);
        assert_eq!(i.code, "parenthetical-unclosed");
        assert_eq!(
            i.fix.unwrap().edits,
            vec![Edit::Replace {
                line: 2,
                text: "(quietly)".into()
            }]
        );
        let i = first("INT. HALL - DAY\n\nRain.\n\nHETTIE ^\nHi.", Format::Film);
        assert_eq!(i.code, "dual-orphan");
        assert_eq!(
            i.fix.unwrap().edits,
            vec![Edit::Replace {
                line: 4,
                text: "HETTIE".into()
            }]
        );
        let i = first("\nTOBIAS\nHi.\n\nHETTIE (CONT'D)\nOh.", Format::Stage);
        assert_eq!(i.code, "contd-mismatch");
        assert_eq!(
            i.message,
            "(CONT'D) means the same character carries on, but the speech before is TOBIAS's."
        );
        assert_eq!(
            i.fix.unwrap().edits,
            vec![Edit::Replace {
                line: 4,
                text: "HETTIE".into()
            }]
        );
        assert_eq!(
            codes("\nHETTIE\nHi.\n\nShe turns.\n\nHETTIE (CONT'D)\nOh.", Format::Stage),
            vec![]
        );
    }

    #[test]
    fn headings() {
        assert_eq!(
            codes("INT. KITCHEN\n\nRain.", Format::Film),
            vec![("heading-no-time", 0)]
        );
        assert_eq!(codes("INT. KITCHEN\n\nRain.", Format::Stage), vec![]);
        let i = first("INT. KITCHEN - NITE\n\nRain.", Format::Film);
        assert_eq!(
            (i.code, i.message.as_str(), i.from, i.to),
            ("heading-time", "NITE: did you mean NIGHT?", 15, 19)
        );
        assert_eq!(
            i.fix.unwrap().edits,
            vec![Edit::Replace {
                line: 0,
                text: "INT. KITCHEN - NIGHT".into()
            }]
        );
        let i = first("INT. KITCHEN - MORNIG (FLASHBACK)\n\nRain.", Format::Film);
        assert_eq!(
            i.fix.unwrap().edits,
            vec![Edit::Replace {
                line: 0,
                text: "INT. KITCHEN - MORNING (FLASHBACK)".into()
            }]
        );
        // A time that isn't a near miss is the writer's own.
        assert_eq!(codes("EXT. FIELD - THREE WEEKS LATER\n\nGrass.", Format::Film), vec![]);
        let i = first("INT KITCHEN - DAY\n\nRain.", Format::Film);
        assert_eq!(i.code, "heading-setting");
        assert_eq!(
            i.fix.unwrap().edits,
            vec![Edit::Replace {
                line: 0,
                text: "INT. KITCHEN - DAY".into()
            }]
        );
        assert_eq!(
            codes("Rain falls.\n\nINT. KITCHEN - DAY", Format::Film),
            vec![("screen-scene-start", 0)]
        );
    }

    #[test]
    fn characters_are_introduced_in_capitals_once() {
        let mut c = ctx(Format::Film);
        let body = "INT. KITCHEN - DAY\n\nNell waits. Later, Nell sighs.\n\nNELL\nLate.";
        let issues = check_scene(body, &SceneMeta::default(), &mut c);
        assert_eq!(issues.len(), 1);
        let i = &issues[0];
        assert_eq!((i.code, i.line, i.from, i.to), ("intro-caps", 2, 0, 4));
        assert_eq!(
            i.fix.as_ref().unwrap().edits,
            vec![Edit::Replace {
                line: 2,
                text: "NELL waits. Later, Nell sighs.".into()
            }]
        );
        // Introduced now, so a later scene is fine.
        assert!(check_scene("INT. HALL - DAY\n\nNell runs.", &SceneMeta::default(), &mut c).is_empty());
        // Introduced in capitals first: no issue.
        let mut c = ctx(Format::Film);
        assert!(
            check_scene(
                "INT. HALL - DAY\n\nNELL (30s) runs. Nell stops.",
                &SceneMeta::default(),
                &mut c
            )
            .is_empty()
        );
        // Not a convention on stage.
        assert!(codes("Nell waits.", Format::Stage).is_empty());
    }

    #[test]
    fn silent_but_speaks_and_switching_checks_off() {
        let meta = SceneMeta {
            onstage_silent: vec!["TOBIAS".into()],
            ..Default::default()
        };
        let issues = check_scene("\nTOBIAS\nHi.", &meta, &mut ctx(Format::Stage));
        assert_eq!(issues[0].code, "silent-speaks");
        let mut c = ctx(Format::Stage);
        c.off.insert("silent-speaks".into());
        assert!(check_scene("\nTOBIAS\nHi.", &meta, &mut c).is_empty());
    }

    #[test]
    fn every_rule_is_listed() {
        let src = include_str!("check.rs");
        for (code, _) in CHECKS {
            assert!(src.matches(&format!("\"{code}\"")).count() >= 2, "{code}");
        }
    }
}
