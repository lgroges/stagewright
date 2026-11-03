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
    ("caps-after-intro", "A character in capitals after their introduction"),
    ("heading-blank-after", "A scene heading with no blank line after it"),
    ("action-run-on", "Action lines running together without a blank line"),
    ("heading-description", "Description inside a scene heading"),
    ("contd-typed", "(CONT'D) typed by hand, or spelt (CONT.)"),
    ("long-action", "An action paragraph longer than 5 lines"),
    ("heading-then-dialogue", "A scene heading followed straight by dialogue"),
    (
        "cue-extension",
        "(V.O.) or (O.S.) as a parenthetical, or written (O.O.V.)",
    ),
    (
        "parenthetical-caps",
        "A parenthetical in capitals (film and single-camera TV)",
    ),
    ("transition", "CUT TO: in a spec script, or FADE OUT. part-way through"),
    ("camera-spec", "Camera directions in a spec script"),
    ("novelistic", "Action describing thoughts or feelings we can't see"),
    ("cue-full-name", "A cue using a character's full name"),
    ("cue-title", "A cue with a title written out (MISTER, DOCTOR)"),
    ("cue-variant", "Two cues that look like the same character"),
    ("repeated-word", "The same word twice in a row (\"are are\")"),
    ("age-at-intro", "A character introduced without an age, like (30s)"),
    ("grammar", "Grammar in action and stage directions"),
    (
        "heading-vague",
        "A scene heading that doesn't name the place (A CHURCHYARD)",
    ),
    ("scene-long", "A film or TV scene longer than 3 pages"),
    ("title-page", "Title page details missing"),
];

/// Camera and technical directions a spec script leaves to the director.
/// (MONTAGE, SERIES OF SHOTS, INTERCUT and FLASHBACK are storytelling, so allowed.)
pub const CAMERA_DIRECTIONS: &[&str] = &[
    "ANGLE ON",
    "CLOSE ON",
    "CLOSE UP",
    "CLOSE-UP",
    "EXTREME CLOSE UP",
    "EXTREME CLOSE-UP",
    "ECU",
    "MCU",
    "MEDIUM SHOT",
    "WIDE SHOT",
    "ESTABLISHING SHOT",
    "TWO SHOT",
    "OVER THE SHOULDER",
    "POV",
    "REVERSE ANGLE",
    "INSERT",
    "BACK TO SCENE",
    "AERIAL SHOT",
    "TRACKING SHOT",
    "MOVING SHOT",
    "HANDHELD",
    "PAN TO",
    "PANS TO",
    "TILT UP",
    "TILT DOWN",
    "PUSH IN",
    "PULL BACK",
    "DOLLY IN",
    "DOLLY OUT",
    "ZOOM IN",
    "ZOOM OUT",
    "CRANE UP",
    "CRANE DOWN",
    "RACK FOCUS",
    "SLOW MOTION",
    "FREEZE FRAME",
    "SPLIT SCREEN",
];

/// Words for what's inside a character's head, which the camera can't show.
const NOVELISTIC: &[&str] = &[
    "thinks",
    "thinking about",
    "remembers",
    "remembering",
    "recalls",
    "realises",
    "realizes",
    "realising",
    "realizing",
    "wonders",
    "wondering",
    "wishes",
    "knows",
    "decides",
    "intrigued",
    "curious",
    "senses",
    "suspects",
    "believes",
    "understands",
    "hopes",
    "longs",
    "yearns",
    "is aware",
    "can't help but",
    "deep down",
];

/// Extensions sometimes written as a parenthetical, and how they're written.
const EXTENSIONS: &[(&str, &str)] = &[
    ("V.O.", "V.O."),
    ("VO", "V.O."),
    ("V/O", "V.O."),
    ("VOICE OVER", "V.O."),
    ("VOICEOVER", "V.O."),
    ("VOICE-OVER", "V.O."),
    ("O.S.", "O.S."),
    ("OS", "O.S."),
    ("OFF", "O.S."),
    ("OFF SCREEN", "O.S."),
    ("OFF-SCREEN", "O.S."),
    ("OFFSCREEN", "O.S."),
    ("O.C.", "O.C."),
    ("O.O.V.", "O.O.V."),
    ("O.O.V", "O.O.V."),
    ("OOV", "O.O.V."),
];

/// Titles written out in a cue, and their abbreviations (British style, no full stop).
const TITLES: &[(&str, &str)] = &[
    ("MISTER", "MR"),
    ("MISSUS", "MRS"),
    ("MISSES", "MRS"),
    ("DOCTOR", "DR"),
    ("PROFESSOR", "PROF"),
];

/// Words that qualify a cue rather than name someone: YOUNG NELL, OLD MAN.
const DESCRIPTORS: &[&str] = &[
    "YOUNG",
    "YOUNGER",
    "OLD",
    "OLDER",
    "LITTLE",
    "BIG",
    "THE",
    "A",
    "AN",
    "MR",
    "MRS",
    "MS",
    "MISS",
    "DR",
    "PROF",
    "SIR",
    "LADY",
    "LORD",
    "DAME",
    "FATHER",
    "MOTHER",
    "SISTER",
    "BROTHER",
    "AUNT",
    "UNCLE",
    "CAPTAIN",
    "SERGEANT",
    "INSPECTOR",
    "DETECTIVE",
    "OFFICER",
    "KING",
    "QUEEN",
    "PRINCE",
    "PRINCESS",
    "SAINT",
    "ST",
    "REVEREND",
];

/// A character as the action may mention them.
#[derive(Debug, Clone, Default)]
pub struct Person {
    /// The cue, in capitals: how they're introduced ("JOANNA WESTON").
    pub cue: String,
    /// Their name as written in the cast list ("Joanna Weston").
    pub name: Option<String>,
    /// What the action calls them after the introduction ("Joanna").
    pub short: Option<String>,
}

impl Person {
    /// How to write them after they've been introduced.
    pub fn later(&self) -> String {
        self.short
            .clone()
            .or_else(|| self.name.clone())
            .unwrap_or_else(|| title_case(&self.cue))
    }
}

/// `THE MINISTER` -> `The Minister`.
fn title_case(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `SPRING. THE SUN SHINES.` -> `Spring. The sun shines.`
fn sentence_case(s: &str) -> String {
    if s.chars().any(|c| c.is_lowercase()) {
        return s.to_string();
    }
    let mut out = String::new();
    let mut start = true;
    for c in s.to_lowercase().chars() {
        if start && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            start = false;
        } else {
            out.push(c);
        }
        if matches!(c, '.' | '!' | '?') {
            start = true;
        }
    }
    out
}

/// Every whole-word occurrence of `form` in `text`, as character columns and
/// whether it was written in capitals. `any_case` also matches other casings.
fn occurrences(text: &[char], form: &str, any_case: bool) -> Vec<(usize, usize, bool)> {
    let f: Vec<char> = form.chars().collect();
    let mut out = Vec::new();
    if f.is_empty() || f.len() > text.len() {
        return out;
    }
    let word = |c: Option<&char>| c.is_some_and(|c| c.is_alphanumeric() || *c == '_');
    let same = |a: char, b: char| a == b || (any_case && a.to_uppercase().eq(b.to_uppercase()));
    for i in 0..=text.len() - f.len() {
        if (0..f.len()).all(|k| same(text[i + k], f[k]))
            && !word(i.checked_sub(1).map(|j| &text[j]))
            && !word(text.get(i + f.len()))
        {
            let caps = !text[i..i + f.len()].iter().any(|c| c.is_lowercase());
            out.push((i, i + f.len(), caps));
        }
    }
    out
}

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
    /// Characters with their names and short names, for introductions.
    /// Cues not in the cast list are added by `check_scene` from `known`.
    pub people: Vec<Person>,
    /// (CONT'D) is added automatically in this project.
    pub auto_contd: bool,
    /// A shooting script: camera directions and CUT TO: are expected.
    pub shooting: bool,
    /// FADE OUT. may close this scene: the last scene of a film, or of a TV act.
    pub fade_out_here: bool,
    /// The export layout: the BBC studio one writes parentheticals in capitals
    /// and a transition after every scene.
    pub layout: crate::layout::Layout,
    /// The English the grammar check expects.
    pub english: crate::grammar::English,
}

/// Words that can rightly come twice in a row ("I had had enough").
const DOUBLES: &[&str] = &["had", "that"];

/// Whether the text after a character's introduction gives their age:
/// "(40s)", "(2)", "(late twenties)", ", 50s,".
fn gives_age(after: &str) -> bool {
    let t = after.trim_start();
    if let Some(inner) = t.strip_prefix('(') {
        let inner = inner.split(')').next().unwrap_or("").to_lowercase();
        return inner.chars().any(|c| c.is_ascii_digit())
            || [
                "teen", "twent", "thirt", "fort", "fift", "sixt", "sevent", "eight", "ninet", "baby", "infant",
            ]
            .iter()
            .any(|w| inner.contains(w));
    }
    t.strip_prefix(',')
        .is_some_and(|r| r.trim_start().starts_with(|c: char| c.is_ascii_digit()))
}

/// A note about the cast as a whole, not one line of a scene.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CastNote {
    pub code: &'static str,
    pub cue: String,
    pub message: String,
}

fn words(cue: &str) -> Vec<&str> {
    cue.split_whitespace().collect()
}

/// Notes on the cues themselves: full names, titles written out, and two
/// cues for what looks like one character.
pub fn check_cues(ctx: &Context) -> Vec<CastNote> {
    let mut out = Vec::new();
    let plain = |c: &str| !c.contains(['&', ',', '#', '/']) && !c.chars().any(|ch| ch.is_ascii_digit());
    // Full names: a cast entry whose cue is its whole (several-word) name.
    for p in &ctx.people {
        let w = words(&p.cue);
        let full = p.name.as_ref().is_some_and(|n| n.to_uppercase() == p.cue);
        if w.len() < 2 || !full || !plain(&p.cue) || DESCRIPTORS.contains(&w[0]) {
            continue;
        }
        // Suggest a name nobody else's cue uses: the first, the last, then the middle ones.
        let mut options: Vec<&str> = vec![w[0], w[w.len() - 1]];
        options.extend(&w[1..w.len() - 1]);
        // A first name another cue starts with is taken; a shared surname is
        // free if the other character can go by their own first name.
        let others: Vec<&str> = ctx
            .people
            .iter()
            .map(|q| q.cue.as_str())
            .filter(|q| *q != p.cue)
            .collect();
        let first_taken = |o: &str, except: &str| {
            others
                .iter()
                .chain(std::iter::once(&p.cue.as_str()))
                .any(|q| *q != except && (words(q).first() == Some(&o) || *q == o))
        };
        let free: Vec<&str> = options
            .into_iter()
            .filter(|o| {
                !first_taken(o, &p.cue)
                    && others
                        .iter()
                        .filter(|q| words(q).contains(o))
                        .all(|q| !first_taken(words(q)[0], q))
            })
            .collect();
        let suggestion = match free.as_slice() {
            [] => String::new(),
            [one] => format!(" ({one})"),
            [a, b, ..] => format!(" ({a} or {b})"),
        };
        out.push(CastNote {
            code: "cue-full-name",
            cue: p.cue.clone(),
            message: format!(
                "{}: cues usually use just the first or last name{suggestion}, with the full name in capitals where they're introduced. Rename it in Cast.",
                p.cue
            ),
        });
    }
    // Titles written out: MISTER JONES -> MR JONES.
    let mut cues: Vec<&String> = ctx.known.iter().filter(|c| !GROUP_CUES.contains(&c.as_str())).collect();
    cues.sort();
    for c in &cues {
        let w = words(c);
        if w.len() < 2 {
            continue;
        }
        if let Some((long, short)) = TITLES.iter().find(|(t, _)| w[..w.len() - 1].contains(t)) {
            let fixed = replace_word(c, long, short).0;
            out.push(CastNote {
                code: "cue-title",
                cue: (*c).clone(),
                message: format!("{c}: titles are abbreviated in cues ({fixed}). Rename it in Cast."),
            });
        }
    }
    // One character, two cues: DEE and JOHN DEE. Both in the cast list means
    // the writer has said they're different people.
    for a in &cues {
        let longer: Vec<&str> = cues
            .iter()
            .filter(|b| {
                let (wa, wb) = (words(a), words(b));
                let inside = wa.len() < wb.len() && wa.iter().all(|x| wb.contains(x));
                let extra_descriptive = wb.iter().filter(|x| !wa.contains(x)).all(|x| DESCRIPTORS.contains(x));
                plain(a)
                    && plain(b)
                    && inside
                    && !extra_descriptive
                    && !(ctx.cast.contains(*a) && ctx.cast.contains(**b))
            })
            .map(|b| b.as_str())
            .collect();
        if !longer.is_empty() {
            out.push(CastNote {
                code: "cue-variant",
                cue: (*a).clone(),
                message: format!(
                    "{a} looks like the same character as {}: keep to one cue throughout.",
                    longer.join(" or ")
                ),
            });
        }
    }
    out.retain(|n| !ctx.off.contains(n.code));
    out
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
    // Everyone the action may mention: the cast list, plus cues not in it.
    let mut people = ctx.people.clone();
    for cue in &ctx.known {
        if !people.iter().any(|p| &p.cue == cue) && !GROUP_CUES.contains(&cue.as_str()) {
            people.push(Person {
                cue: cue.clone(),
                ..Default::default()
            });
        }
    }
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
                if let Some(ext) = &c.extension
                    && ext.to_uppercase().contains("CONT")
                    && !prev_speakers.is_empty()
                    && prev_speakers == speakers
                {
                    let without = text.replacen(&format!(" ({ext})"), "", 1);
                    let spelled = text.replacen(&format!("({ext})"), "(CONT'D)", 1);
                    if ctx.auto_contd {
                        out.push(issue(
                            "contd-typed",
                            Severity::Info,
                            i,
                            (0, 0),
                            format!("({ext}) isn't needed: (CONT'D) is added automatically in this project."),
                            (without != text).then(|| Fix {
                                label: format!("Remove ({ext})"),
                                edits: vec![Edit::Replace { line: i, text: without }],
                            }),
                        ));
                    } else if ext.trim() != "CONT'D" {
                        out.push(issue(
                            "contd-typed",
                            Severity::Info,
                            i,
                            (0, 0),
                            format!("The usual spelling is (CONT'D), not ({ext})."),
                            fix("Write (CONT'D)", vec![Edit::Replace { line: i, text: spelled }]),
                        ));
                    }
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
                // Introductions: the first mention in action is the full name in
                // capitals; after that, the short name in ordinary case.
                // (Multi-camera action is printed in capitals anyway.)
                if screen && ctx.format != Format::TvMulti {
                    let off = usize::from(*forced);
                    let chars: Vec<char> = t.chars().collect();
                    // (from, to, caps, person index)
                    let mut found: Vec<(usize, usize, bool, usize)> = Vec::new();
                    for (pi, p) in people.iter().enumerate() {
                        let mut forms: Vec<(String, bool)> = vec![(p.cue.clone(), true)];
                        if let Some(n) = &p.name {
                            forms.push((n.clone(), false));
                            forms.push((n.to_uppercase(), false));
                        }
                        if let Some(s) = &p.short {
                            forms.push((s.clone(), true));
                        }
                        for (form, any_case) in forms {
                            for (a, b, caps) in occurrences(&chars, &form, any_case) {
                                found.push((a, b, caps, pi));
                            }
                        }
                    }
                    // Longest first at each place; drop matches inside another.
                    found.sort_by_key(|(a, b, _, _)| (*a, std::cmp::Reverse(*b)));
                    let mut kept: Vec<(usize, usize, bool, usize)> = Vec::new();
                    for m in found {
                        if kept.last().is_none_or(|k| m.0 >= k.1) {
                            kept.push(m);
                        }
                    }
                    for (a, b, caps, pi) in kept {
                        let p = &people[pi];
                        let span = (a + off, b + off);
                        let before: String = text.chars().take(span.0).collect();
                        let after: String = text.chars().skip(span.1).collect();
                        if !ctx.introduced.contains(&p.cue) {
                            ctx.introduced.insert(p.cue.clone());
                            // A possessive ("JOHN WESTON's funeral") isn't a place for an age.
                            let possessive = after.starts_with("'s") || after.starts_with("’s");
                            if !gives_age(&after) && !possessive {
                                out.push(issue(
                                    "age-at-intro",
                                    Severity::Info,
                                    i,
                                    span,
                                    format!(
                                        "First appearance of {}: screenplays usually give an age here, like (30s).",
                                        p.cue
                                    ),
                                    None,
                                ));
                            }
                            if !caps {
                                let full = p
                                    .name
                                    .as_ref()
                                    .map(|n| n.to_uppercase())
                                    .unwrap_or_else(|| p.cue.clone());
                                out.push(issue(
                                    "intro-caps",
                                    Severity::Info,
                                    i,
                                    span,
                                    format!("First appearance of {}: screenplays introduce a character with their full name in capitals.", p.cue),
                                    fix(
                                        format!("Write {full}"),
                                        vec![Edit::Replace { line: i, text: format!("{before}{full}{after}") }],
                                    ),
                                ));
                            }
                        } else if caps {
                            let later = p.later();
                            let note = if p.short.is_some() {
                                String::new()
                            } else {
                                " (give them a short name in Cast)".to_string()
                            };
                            out.push(issue(
                                "caps-after-intro",
                                Severity::Info,
                                i,
                                span,
                                format!(
                                    "{} has already been introduced: from here on, write {later}{note}.",
                                    p.cue
                                ),
                                fix(
                                    format!("Write {later}"),
                                    vec![Edit::Replace {
                                        line: i,
                                        text: format!("{before}{later}{after}"),
                                    }],
                                ),
                            ));
                        }
                    }
                }
                // Action runs on: two action lines with no blank line between.
                if screen && i > 0 && matches!(els[i - 1], Element::Direction { .. }) {
                    out.push(issue(
                        "action-run-on",
                        Severity::Info,
                        i,
                        (0, 0),
                        "This runs on from the line above as one paragraph; screenplays separate action paragraphs with a blank line.".into(),
                        fix("Add a blank line before it", vec![Edit::InsertBefore { line: i, text: String::new() }]),
                    ));
                }
                // Long action: count printed lines of the paragraph, at its first line.
                if screen && !(i > 0 && matches!(els[i - 1], Element::Direction { .. })) {
                    let width = crate::layout::Profile::for_format(ctx.format).action.width;
                    let printed: usize = els[i..]
                        .iter()
                        .take_while(|e| matches!(e, Element::Direction { .. }))
                        .map(|e| match e {
                            Element::Direction { text, .. } => crate::layout::wrap(text, width).len(),
                            _ => 0,
                        })
                        .sum();
                    if printed > 5 {
                        out.push(issue(
                            "long-action",
                            Severity::Info,
                            i,
                            (0, 0),
                            format!("This action paragraph prints at {printed} lines. Keep each paragraph to four or five lines, one beat of action each: split it where the focus shifts."),
                            None,
                        ));
                    }
                }
                if !forced {
                    fade_out(i, &t.trim().to_uppercase(), text, &els, ctx, &mut out);
                }
                let off = usize::from(*forced);
                let chars: Vec<char> = t.chars().collect();
                // Camera directions, in capitals, in a spec script.
                if screen && !ctx.shooting {
                    let mut seen: Vec<(usize, usize)> = Vec::new();
                    let mut terms: Vec<&str> = CAMERA_DIRECTIONS.to_vec();
                    terms.sort_by_key(|t| std::cmp::Reverse(t.len()));
                    for term in terms {
                        for (a, b, _) in occurrences(&chars, term, false) {
                            if seen.iter().any(|(x, y)| a < *y && *x < b) {
                                continue;
                            }
                            seen.push((a, b));
                            out.push(issue(
                                "camera-spec",
                                Severity::Info,
                                i,
                                (a + off, b + off),
                                format!("{term} is a camera direction: a spec script leaves the shots to the director. Describe what we see instead (or mark this a shooting script)."),
                                None,
                            ));
                        }
                    }
                }
                // The same word twice in a row: "heads are are bowed".
                let mut prev: Option<(usize, usize)> = None;
                let mut k = 0;
                while k < chars.len() {
                    if !chars[k].is_alphabetic() {
                        k += 1;
                        continue;
                    }
                    let start = k;
                    while k < chars.len() && (chars[k].is_alphabetic() || chars[k] == '\'' || chars[k] == '’') {
                        k += 1;
                    }
                    if let Some((ps, pe)) = prev {
                        let gap: String = chars[pe..start].iter().collect();
                        let a: String = chars[ps..pe].iter().collect::<String>().to_lowercase();
                        let b: String = chars[start..k].iter().collect::<String>().to_lowercase();
                        if a == b && !gap.is_empty() && gap.chars().all(|c| c == ' ') && !DOUBLES.contains(&a.as_str())
                        {
                            let fixed: String = chars[..pe].iter().chain(chars[k..].iter()).collect();
                            let prefix = if *forced { "!" } else { "" };
                            out.push(issue(
                                "repeated-word",
                                Severity::Info,
                                i,
                                (pe + off, k + off),
                                format!("\"{b}\" twice in a row."),
                                fix(
                                    format!("Remove the second \"{b}\""),
                                    vec![Edit::Replace {
                                        line: i,
                                        text: format!("{prefix}{fixed}"),
                                    }],
                                ),
                            ));
                        }
                    }
                    prev = Some((start, k));
                }
                // What's in a character's head can't be filmed or staged.
                for w in NOVELISTIC {
                    for (a, b, _) in occurrences(&chars, w, true) {
                        let written: String = chars[a..b].iter().collect();
                        out.push(issue(
                            "novelistic",
                            Severity::Info,
                            i,
                            (a + off, b + off),
                            format!("\"{written}\" describes what's going on inside a character; action should only show what the audience can see and hear."),
                            None,
                        ));
                    }
                }
                // Grammar (not in dialogue: see grammar.rs).
                if !ctx.off.contains("grammar") {
                    let repeated: Vec<(usize, usize)> = out
                        .iter()
                        .filter(|x| x.line == i && x.code == "repeated-word")
                        .map(|x| (x.from - off, x.to - off))
                        .collect();
                    for g in crate::grammar::check(t, ctx.english) {
                        if repeated.iter().any(|(a, b)| g.from < *b + 1 && *a < g.to + 1) {
                            continue; // already noted as a repeated word
                        }
                        let fixed = g.replacement.as_ref().map(|r| {
                            let before: String = chars[..g.from].iter().collect();
                            let after: String = chars[g.to..].iter().collect();
                            let prefix = if *forced { "!" } else { "" };
                            format!("{prefix}{before}{r}{after}")
                        });
                        let label = match &g.replacement {
                            Some(r) if r.is_empty() => "Remove it".to_string(),
                            Some(r) => format!("Write \"{r}\""),
                            None => String::new(),
                        };
                        out.push(issue(
                            "grammar",
                            Severity::Info,
                            i,
                            (g.from + off, g.to + off),
                            g.message,
                            fixed.and_then(|text| fix(label, vec![Edit::Replace { line: i, text }])),
                        ));
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
                let next_blank = blank(i + 1);
                if !next_blank {
                    out.push(issue(
                        "heading-blank-after",
                        Severity::Warning,
                        i,
                        (0, 0),
                        "A scene heading needs a blank line after it; other screenwriting apps won't read it as a heading without one.".into(),
                        fix("Add a blank line after it", vec![Edit::InsertAfter { line: i, text: String::new() }]),
                    ));
                }
                if screen
                    && matches!(
                        els[i + 1..].iter().find(|e| !matches!(e, Element::Blank)),
                        Some(Element::Cue(_))
                    )
                {
                    out.push(issue(
                        "heading-then-dialogue",
                        Severity::Info,
                        i,
                        (0, 0),
                        "Follow a scene heading with a line of action, to set the scene before anyone speaks.".into(),
                        None,
                    ));
                }
                // Description in the heading: "- MORNING. SPRING. THE SUN SHINES."
                if let Some(t) = &time {
                    let trimmed = t.trim_end_matches('.').trim();
                    if let Some(dot) = trimmed.find(". ") {
                        let keep = trimmed[..dot].trim();
                        let rest = t[dot + 1..].trim();
                        if !keep.is_empty() && !rest.is_empty() {
                            let heading = match text.rfind(t.as_str()) {
                                Some(at) => format!("{}{}{}", &text[..at], keep, &text[at + t.len()..]),
                                None => text.to_string(),
                            };
                            let action = sentence_case(rest);
                            let mut edits = vec![
                                Edit::Replace { line: i, text: heading },
                                Edit::InsertAfter {
                                    line: i,
                                    text: String::new(),
                                },
                                Edit::InsertAfter {
                                    line: i,
                                    text: action.clone(),
                                },
                            ];
                            if !next_blank {
                                edits.push(Edit::InsertAfter {
                                    line: i,
                                    text: String::new(),
                                });
                            }
                            let from = text.rfind(rest).map(|b| text[..b].chars().count()).unwrap_or(0);
                            out.push(issue(
                                "heading-description",
                                Severity::Info,
                                i,
                                (from, from + rest.chars().count()),
                                format!("Keep the heading to place and time; \"{action}\" belongs in the action."),
                                fix(format!("Move \"{action}\" into the action"), edits),
                            ));
                        }
                    }
                }
                // "EXT. A CHURCHYARD": which one?
                let (_, place, _) = h.parts();
                let up = place.to_uppercase();
                if screen && (up.starts_with("A ") || up.starts_with("AN ")) {
                    out.push(issue(
                        "heading-vague",
                        Severity::Info,
                        i,
                        cols(text, &place),
                        format!("\"{place}\" doesn't say which: name the place (a reader and the location manager will want to know)."),
                        None,
                    ));
                }
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
                    // The time itself: before any "(FLASHBACK)" or a description after ". ".
                    let written = t
                        .split(" (")
                        .next()
                        .unwrap_or(&t)
                        .split(". ")
                        .next()
                        .unwrap_or(&t)
                        .trim()
                        .trim_end_matches('.');
                    let core = written.to_uppercase();
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
                            let span = cols(text, written);
                            let replaced = replace_word(text, written, &good).0;
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
            Element::Parenthetical(t) => {
                let inner = t.trim().trim_start_matches('(').trim_end_matches(')').trim();
                let upper = inner.to_uppercase();
                let as_extension = EXTENSIONS.iter().find(|(w, _)| *w == upper).map(|(_, e)| *e);
                let cue_line = (i > 0 && matches!(els[i - 1], Element::Cue(_))).then(|| i - 1);
                // Stage plays write "(off)" under the name, so this is a screen rule.
                if let (true, Some(ext), Some(c)) = (screen, as_extension, cue_line) {
                    let ext = if ext == "O.O.V." && ctx.format != Format::TvMulti {
                        "O.S."
                    } else {
                        ext
                    };
                    let cue = lines.get(c).copied().unwrap_or("").trim_end();
                    let (name, dual) = match cue.strip_suffix('^') {
                        Some(n) => (n.trim_end(), " ^"),
                        None => (cue, ""),
                    };
                    out.push(issue(
                        "cue-extension",
                        Severity::Info,
                        i,
                        (0, 0),
                        format!(
                            "({ext}) is a technical note, so it goes next to the character's name, not on its own line."
                        ),
                        fix(
                            format!("Move ({ext}) next to the name"),
                            vec![
                                Edit::Replace {
                                    line: c,
                                    text: format!("{name} ({ext}){dual}"),
                                },
                                Edit::Delete { line: i },
                            ],
                        ),
                    ));
                } else if screen
                    && ctx.format != Format::TvMulti
                    && ctx.layout != crate::layout::Layout::BbcStudio
                    && inner.chars().filter(|c| c.is_alphabetic()).count() >= 2
                    && !inner.chars().any(|c| c.is_lowercase())
                {
                    // Keep character names in capitals: (TO NELL) -> (to NELL).
                    let names: BTreeSet<&str> = ctx.known.iter().flat_map(|k| k.split_whitespace()).collect();
                    let lowered: String = t
                        .split(' ')
                        .map(|w| {
                            let bare = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'');
                            if names.contains(bare) {
                                w.to_string()
                            } else {
                                w.to_lowercase()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    if lowered != *t {
                        out.push(issue(
                            "parenthetical-caps",
                            Severity::Info,
                            i,
                            (0, 0),
                            "Screenplay parentheticals are written in lower case.".into(),
                            fix(
                                "Write it in lower case",
                                vec![Edit::Replace {
                                    line: i,
                                    text: text.replacen(t.as_str(), &lowered, 1),
                                }],
                            ),
                        ));
                    }
                }
            }
            Element::Transition { text: t, .. } => {
                let upper = t.trim().to_uppercase();
                let film_or_single = matches!(ctx.format, Format::Film | Format::TvSingle);
                let studio = ctx.layout == crate::layout::Layout::BbcStudio;
                if upper == "CUT TO:" && film_or_single && !ctx.shooting && !studio {
                    let mut edits = vec![Edit::Delete { line: i }];
                    if i > 0 && blank(i - 1) && blank(i + 1) {
                        edits.insert(0, Edit::Delete { line: i - 1 });
                    }
                    out.push(issue(
                        "transition",
                        Severity::Info,
                        i,
                        (0, 0),
                        "Every scene cuts to the next unless you say otherwise, so a spec script leaves out CUT TO:."
                            .into(),
                        fix("Remove CUT TO:", edits),
                    ));
                }
                fade_out(i, &upper, text, &els, ctx, &mut out);
            }
            Element::Blank | Element::Centered(_) | Element::PageBreak(_) => {}
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

    // Film and TV scenes usually run a page or three.
    if matches!(ctx.format, Format::Film | Format::TvSingle | Format::TvMulti) && !ctx.off.contains("scene-long") {
        let p = crate::layout::Profile::for_format(ctx.format);
        let scene = crate::scene::Scene {
            frontmatter: String::new(),
            meta: meta.clone(),
            body: els.clone(),
            trailing_newline: false,
        };
        let eighths = crate::layout::eighths(crate::layout::scene_line_count(&scene, &p), &p);
        let limit = if ctx.format == Format::TvMulti { 6 * 8 } else { 3 * 8 };
        if eighths > limit {
            out.push(issue(
                "scene-long",
                Severity::Info,
                0,
                (0, 0),
                format!(
                    "This scene runs {} pages; most {} scenes run under {}. Could it be cut, or split?",
                    crate::layout::eighths_label(eighths),
                    if ctx.format == Format::Film { "film" } else { "TV" },
                    limit / 8
                ),
                None,
            ));
        }
    }
    out.retain(|i| !ctx.off.contains(i.code));
    out.sort_by_key(|i| (i.line, i.from));
    out
}

/// FADE OUT. ends a film or a TV act; between scenes it's FADE TO:.
fn fade_out(i: usize, upper: &str, line: &str, els: &[Element], ctx: &Context, out: &mut Vec<Issue>) {
    let upper = upper.trim_start_matches('>').trim();
    if !ctx.format.is_screen() || !(upper == "FADE OUT." || upper == "FADE OUT") {
        return;
    }
    // Anything after it but blank lines and a closing THE END / END OF ACT ONE.
    let more_after = els[i + 1..].iter().any(|e| match e {
        Element::Blank => false,
        Element::Direction { text, .. } | Element::Transition { text, .. } | Element::Centered(text) => {
            !crate::scene::is_end_line(text)
        }
        _ => true,
    });
    if !more_after && ctx.fade_out_here {
        return;
    }
    let at = line.to_uppercase().find("FADE OUT").unwrap_or(0);
    let len = if line[at..].to_uppercase().starts_with("FADE OUT.") {
        9
    } else {
        8
    };
    let fixed = format!("{}FADE TO:{}", &line[..at], &line[at + len..]);
    let where_ = if ctx.format == Format::Film {
        "the film"
    } else {
        "an act"
    };
    out.push(issue(
        "transition",
        Severity::Info,
        i,
        (0, 0),
        format!("FADE OUT. only comes at the end of {where_}; between scenes, write FADE TO:."),
        fix("Write FADE TO:", vec![Edit::Replace { line: i, text: fixed }]),
    ));
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
            // Most tests are about other rules; `ages_and_repeated_words` checks this one.
            off: ["age-at-intro".to_string()].into(),
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
            if *code == "title-page" {
                continue; // a project-level check, made in the API
            }
            assert!(src.matches(&format!("\"{code}\"")).count() >= 2, "{code}");
        }
    }

    fn people_ctx() -> Context {
        let mut c = ctx(Format::TvSingle);
        for (cue, name, short) in [
            ("JOANNA WESTON", Some("Joanna Weston"), Some("Joanna")),
            ("JANE DEE", Some("Jane Dee"), Some("Jane")),
            ("MINISTER", Some("Minister"), None),
        ] {
            c.cast.insert(cue.into());
            c.known.insert(cue.into());
            c.people.push(Person {
                cue: cue.into(),
                name: name.map(Into::into),
                short: short.map(Into::into),
            });
        }
        c.auto_contd = true;
        c
    }

    fn run(body: &str, c: &mut Context) -> Vec<Issue> {
        check_scene(body, &SceneMeta::default(), c)
    }

    #[test]
    fn introductions_then_short_names() {
        let mut c = people_ctx();
        let body = "INT. CHURCH - DAY\n\nJOANNA WESTON weeps. JANE DEE holds JOANNA WESTON's hand. Joanna smiles.\n\nA MINISTER waits. The MINISTER bows.";
        let issues = run(body, &mut c);
        let got: Vec<(&str, usize, usize, String)> = issues
            .iter()
            .filter(|i| i.code == "caps-after-intro" || i.code == "intro-caps")
            .map(|i| (i.code, i.from, i.to, i.fix.as_ref().unwrap().label.clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("caps-after-intro", 36, 49, "Write Joanna".to_string()),
                ("caps-after-intro", 22, 30, "Write Minister".to_string()),
            ]
        );
        let fix = &issues
            .iter()
            .find(|i| i.code == "caps-after-intro")
            .unwrap()
            .fix
            .as_ref()
            .unwrap()
            .edits[0];
        assert_eq!(
            fix,
            &Edit::Replace {
                line: 2,
                text: "JOANNA WESTON weeps. JANE DEE holds Joanna's hand. Joanna smiles.".into()
            }
        );
        // Someone met by their short name before the introduction: introduce them properly.
        let mut c = people_ctx();
        let issues = run("INT. CHURCH - DAY\n\nJoanna arrives.", &mut c);
        let i = issues.iter().find(|i| i.code == "intro-caps").unwrap();
        assert_eq!(
            i.fix.as_ref().unwrap().edits[0],
            Edit::Replace {
                line: 2,
                text: "JOANNA WESTON arrives.".into()
            }
        );
        // Introduced in an earlier scene: capitals now are "after".
        assert!(run("INT. HALL - DAY\n\nJANE DEE sits.", &mut c).is_empty());
        let mut c2 = people_ctx();
        c2.introduced.insert("JANE DEE".into());
        assert_eq!(
            run("INT. HALL - DAY\n\nJANE DEE sits.", &mut c2)[0].code,
            "caps-after-intro"
        );
    }

    #[test]
    fn headings_need_a_blank_line_and_no_description() {
        let mut c = people_ctx();
        let body = "EXT. ROAD - MORNING. SPRING. THE SUN SHINES.\nA procession.\nThe gate.";
        let issues = run(body, &mut c);
        let codes: Vec<&str> = issues.iter().map(|i| i.code).collect();
        assert_eq!(
            codes,
            vec!["heading-blank-after", "heading-description", "action-run-on"]
        );
        let desc = issues.iter().find(|i| i.code == "heading-description").unwrap();
        assert_eq!(
            desc.message,
            "Keep the heading to place and time; \"Spring. The sun shines.\" belongs in the action."
        );
        assert_eq!(
            desc.fix.as_ref().unwrap().edits,
            vec![
                Edit::Replace {
                    line: 0,
                    text: "EXT. ROAD - MORNING".into()
                },
                Edit::InsertAfter {
                    line: 0,
                    text: String::new()
                },
                Edit::InsertAfter {
                    line: 0,
                    text: "Spring. The sun shines.".into()
                },
                Edit::InsertAfter {
                    line: 0,
                    text: String::new()
                },
            ]
        );
        // Not a misspelt time.
        assert!(!codes.contains(&"heading-time"));
    }

    #[test]
    fn typed_contd_and_long_action() {
        let mut c = people_ctx();
        let body = "INT. CHURCH - DAY\n\nMINISTER\nI am.\n\nHe turns.\n\nMINISTER (CONT.)\nI know.";
        let i = run(body, &mut c).into_iter().find(|i| i.code == "contd-typed").unwrap();
        assert_eq!(
            i.message,
            "(CONT.) isn't needed: (CONT'D) is added automatically in this project."
        );
        assert_eq!(
            i.fix.unwrap().edits,
            vec![Edit::Replace {
                line: 7,
                text: "MINISTER".into()
            }]
        );
        c.auto_contd = false;
        let i = run(body, &mut c).into_iter().find(|i| i.code == "contd-typed").unwrap();
        assert_eq!(
            i.fix.unwrap().edits,
            vec![Edit::Replace {
                line: 7,
                text: "MINISTER (CONT'D)".into()
            }]
        );
        // Five printed lines is fine; six is too many.
        let five = format!("INT. CHURCH - DAY\n\n{}", "Words go here. ".repeat(20));
        assert!(!run(&five, &mut people_ctx()).iter().any(|i| i.code == "long-action"));
        let long = format!("INT. CHURCH - DAY\n\n{}", "Words go here. ".repeat(24));
        let i = run(&long, &mut people_ctx())
            .into_iter()
            .find(|i| i.code == "long-action")
            .unwrap();
        assert_eq!(
            i.message,
            "This action paragraph prints at 6 lines. Keep each paragraph to four or five lines, one beat of action each: split it where the focus shifts."
        );
    }

    fn only<'a>(issues: &'a [Issue], code: &str) -> Vec<&'a Issue> {
        issues.iter().filter(|i| i.code == code).collect()
    }

    #[test]
    fn headings_lead_into_action() {
        let mut c = ctx(Format::Film);
        let got = check_scene("INT. KITCHEN - DAY\n\nNELL\nHi.", &SceneMeta::default(), &mut c);
        assert_eq!(only(&got, "heading-then-dialogue").len(), 1);
        let got = check_scene(
            "INT. KITCHEN - DAY\n\nRain.\n\nNELL\nHi.",
            &SceneMeta::default(),
            &mut c,
        );
        assert!(only(&got, "heading-then-dialogue").is_empty());
    }

    #[test]
    fn extensions_go_next_to_the_name() {
        let mut c = ctx(Format::Film);
        let got = check_scene(
            "INT. HALL - DAY\n\nRain.\n\nNELL\n(V.O.)\nHi.",
            &SceneMeta::default(),
            &mut c,
        );
        let i = only(&got, "cue-extension")[0];
        assert_eq!(
            i.fix.as_ref().unwrap().edits,
            vec![
                Edit::Replace {
                    line: 4,
                    text: "NELL (V.O.)".into()
                },
                Edit::Delete { line: 5 }
            ]
        );
        // Dual dialogue keeps its ^ last; (O.O.V.) becomes (O.S.) in a screenplay.
        let got = check_scene(
            "INT. HALL - DAY\n\nRain.\n\nTOBIAS\nYes.\n\nNELL ^\n(oov)\nNo.",
            &SceneMeta::default(),
            &mut c,
        );
        assert_eq!(
            only(&got, "cue-extension")[0].fix.as_ref().unwrap().edits[0],
            Edit::Replace {
                line: 7,
                text: "NELL (O.S.) ^".into()
            }
        );
        // An ordinary parenthetical is left alone.
        let got = check_scene(
            "INT. HALL - DAY\n\nRain.\n\nNELL\n(quietly)\nHi.",
            &SceneMeta::default(),
            &mut c,
        );
        assert!(got.is_empty(), "{got:?}");
    }

    #[test]
    fn parentheticals_in_lower_case() {
        let mut c = ctx(Format::Film);
        let got = check_scene(
            "INT. HALL - DAY\n\nRain.\n\nNELL\n(TO TOBIAS)\nHi.",
            &SceneMeta::default(),
            &mut c,
        );
        assert_eq!(
            only(&got, "parenthetical-caps")[0].fix.as_ref().unwrap().edits,
            vec![Edit::Replace {
                line: 5,
                text: "(to TOBIAS)".into()
            }]
        );
        // Multi-camera and stage scripts write them in capitals.
        let mut c = ctx(Format::TvMulti);
        let got = check_scene(
            "INT. HALL - DAY\n\nRAIN.\n\nNELL\n(TO TOBIAS)\nHi.",
            &SceneMeta::default(),
            &mut c,
        );
        assert!(only(&got, "parenthetical-caps").is_empty());
    }

    #[test]
    fn transitions_in_a_spec_script() {
        let mut c = ctx(Format::Film);
        let body = "INT. HALL - DAY\n\nRain.\n\nCUT TO:\n";
        let got = check_scene(body, &SceneMeta::default(), &mut c);
        assert_eq!(
            only(&got, "transition")[0].fix.as_ref().unwrap().edits,
            vec![Edit::Delete { line: 3 }, Edit::Delete { line: 4 }]
        );
        c.shooting = true;
        assert!(only(&check_scene(body, &SceneMeta::default(), &mut c), "transition").is_empty());
        // FADE OUT. part-way through is FADE TO:; at the very end it's fine.
        let body = "INT. HALL - DAY\n\nRain.\n\n> FADE OUT.";
        let got = check_scene(body, &SceneMeta::default(), &mut c);
        assert_eq!(
            only(&got, "transition")[0].fix.as_ref().unwrap().edits,
            vec![Edit::Replace {
                line: 4,
                text: "> FADE TO:".into()
            }]
        );
        c.fade_out_here = true;
        assert!(only(&check_scene(body, &SceneMeta::default(), &mut c), "transition").is_empty());
        let body = "INT. HALL - DAY\n\nRain.\n\n> FADE OUT.\n\nINT. ROOM - DAY\n\nSun.";
        assert_eq!(
            only(&check_scene(body, &SceneMeta::default(), &mut c), "transition").len(),
            1
        );
    }

    #[test]
    fn camera_directions_and_thoughts() {
        let mut c = ctx(Format::Film);
        let body = "INT. HALL - DAY\n\nCLOSE ON the letter. Nell remembers her father.\n\nA pan of milk boils over.";
        let got = check_scene(body, &SceneMeta::default(), &mut c);
        let cam = only(&got, "camera-spec");
        assert_eq!((cam.len(), cam[0].from, cam[0].to), (1, 0, 8));
        let nov = only(&got, "novelistic");
        assert_eq!((nov.len(), nov[0].from, nov[0].to), (1, 26, 35));
        c.shooting = true;
        assert!(only(&check_scene(body, &SceneMeta::default(), &mut c), "camera-spec").is_empty());
    }

    #[test]
    fn cue_names() {
        let mut c = people_ctx();
        for (cue, name) in [
            ("JOHN DEE", "John Dee"),
            ("JOHN FRANCIS WESTON", "John Francis Weston"),
            ("YOUNG NELL", "Young Nell"),
        ] {
            c.cast.insert(cue.into());
            c.known.insert(cue.into());
            c.people.push(Person {
                cue: cue.into(),
                name: Some(name.into()),
                short: None,
            });
        }
        for k in ["DEE", "MISTER JONES", "NELL"] {
            c.known.insert(k.into());
        }
        let notes = check_cues(&c);
        let msg = |code: &str, cue: &str| {
            notes
                .iter()
                .find(|n| n.code == code && n.cue == cue)
                .map(|n| n.message.clone())
        };
        // Suggestions avoid names another cue already uses.
        assert!(msg("cue-full-name", "JOANNA WESTON").unwrap().contains("(JOANNA)"));
        assert!(msg("cue-full-name", "JOHN DEE").unwrap().contains("(DEE)"));
        assert!(
            msg("cue-full-name", "JOHN FRANCIS WESTON")
                .unwrap()
                .contains("(WESTON or FRANCIS)")
        );
        assert!(msg("cue-full-name", "YOUNG NELL").is_none());
        assert!(msg("cue-full-name", "MINISTER").is_none());
        assert!(msg("cue-title", "MISTER JONES").unwrap().contains("(MR JONES)"));
        assert_eq!(
            msg("cue-variant", "DEE").unwrap(),
            "DEE looks like the same character as JANE DEE or JOHN DEE: keep to one cue throughout."
        );
        assert!(
            msg("cue-variant", "NELL").is_none(),
            "YOUNG NELL is a different age, not a variant"
        );
        // The introduction is the full name, even when the cue is short.
        let mut c = ctx(Format::Film);
        c.people.push(Person {
            cue: "NELL".into(),
            name: Some("Nell Price".into()),
            short: None,
        });
        let got = check_scene("INT. HALL - DAY\n\nNell Price waits.", &SceneMeta::default(), &mut c);
        assert_eq!(
            only(&got, "intro-caps")[0].fix.as_ref().unwrap().label,
            "Write NELL PRICE"
        );
        let got = check_scene("INT. ROOM - DAY\n\nNELL PRICE sits.", &SceneMeta::default(), &mut c);
        assert_eq!(
            only(&got, "caps-after-intro")[0].fix.as_ref().unwrap().label,
            "Write Nell Price"
        );
    }

    #[test]
    fn ages_and_repeated_words() {
        let mut c = ctx(Format::Film);
        c.off.clear();
        let body = "INT. HALL - DAY\n\nNELL (30s) and TOBIAS wait. HETTIE, 40, sits.\n\nThe heads are are bowed. I had had enough.";
        let got = check_scene(body, &SceneMeta::default(), &mut c);
        let ages: Vec<&str> = got
            .iter()
            .filter(|i| i.code == "age-at-intro")
            .map(|i| &body.split('\n').nth(2).unwrap()[i.from..i.to])
            .collect();
        assert_eq!(ages, vec!["TOBIAS"]);
        let r = only(&got, "repeated-word");
        assert_eq!(r.len(), 1);
        assert_eq!(
            r[0].fix.as_ref().unwrap().edits,
            vec![Edit::Replace {
                line: 4,
                text: "The heads are bowed. I had had enough.".into()
            }]
        );
        // A possessive introduction isn't a place for an age.
        let mut c = ctx(Format::Film);
        c.off.clear();
        let got = check_scene(
            "INT. HALL - DAY\n\nNELL's coat hangs there.",
            &SceneMeta::default(),
            &mut c,
        );
        assert!(only(&got, "age-at-intro").is_empty());
    }

    #[test]
    fn grammar_headings_and_long_scenes() {
        let mut c = ctx(Format::TvSingle);
        let body = "EXT. A CHURCHYARD - DAY\n\nThe MINISTER has been stood waiting. He is intrigued by the stranger.\n\nNELL\nI was sat there, wasn't I.";
        let got = check_scene(body, &SceneMeta::default(), &mut c);
        assert_eq!(only(&got, "heading-vague").len(), 1);
        let g = only(&got, "grammar");
        // Action only: the dialogue's "was sat" is how she talks.
        assert_eq!(g.len(), 1, "{g:?}");
        assert_eq!(g[0].fix.as_ref().unwrap().label, "Write \"standing\"");
        assert_eq!(only(&got, "novelistic").len(), 1);
        c.off.insert("grammar".into());
        assert!(only(&check_scene(body, &SceneMeta::default(), &mut c), "grammar").is_empty());
        // Over three pages.
        let long = format!("INT. HALL - DAY\n\n{}", "Rain falls on the roof.\n\n".repeat(100));
        let got = check_scene(&long, &SceneMeta::default(), &mut ctx(Format::Film));
        assert!(only(&got, "scene-long")[0].message.starts_with("This scene runs"));
        assert!(
            only(
                &check_scene(
                    "INT. HALL - DAY\n\nRain.",
                    &SceneMeta::default(),
                    &mut ctx(Format::Film)
                ),
                "scene-long"
            )
            .is_empty()
        );
    }
}
