//! Page layout for every format: line widths and indents in 12pt Courier
//! (10 characters an inch, 6 lines an inch), the industry pagination rules,
//! and page counts in eighths.
//!
//! Formats:
//! - `stage`    — A4, character names centred, directions in italics.
//! - `film`     — US Letter screenplay: action 1.5"–7.5", dialogue 2.5"–6",
//!   character 3.7", parenthetical 3.1"; about a minute a page.
//! - `tv-single`— as film, plus act headers on new pages and "END OF ACT …".
//! - `tv-multi` — sitcom style: action in capitals, dialogue double-spaced,
//!   every scene starts a new page with a scene letter and its cast;
//!   about 30 seconds a page.
//!
//! Layouts (`layout:` in project.yaml), alternatives for some formats:
//! - `uk-stage`   — stage plays as the BBC stage guide sets them: the name at the
//!   left margin with a colon and the speech beside it, directions in capitals,
//!   ACT I SCENE 1, every scene on a new page, CURTAIN.
//! - `bbc-studio` — TV as studio scripts (BBC taped drama and sitcom): numbered
//!   scenes, NAME: underlined over the speech, action in capitals (bracketed in
//!   drama), a transition after every scene, END OF EPISODE.

use crate::scene::{Element, Scene};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    #[default]
    Stage,
    Film,
    TvSingle,
    TvMulti,
}

impl Format {
    pub fn is_screen(self) -> bool {
        self != Format::Stage
    }
    pub fn is_tv(self) -> bool {
        matches!(self, Format::TvSingle | Format::TvMulti)
    }
    pub fn label(self) -> &'static str {
        match self {
            Format::Stage => "Stage play",
            Format::Film => "Film screenplay",
            Format::TvSingle => "TV, single-camera",
            Format::TvMulti => "TV, multi-camera",
        }
    }
}

/// How a format is laid out on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Layout {
    #[default]
    Standard,
    UkStage,
    BbcStudio,
}

impl Layout {
    /// The layout `format` is printed in: one that doesn't apply to it is standard.
    pub fn for_format(self, format: Format) -> Layout {
        match (self, format) {
            (Layout::UkStage, Format::Stage) => Layout::UkStage,
            (Layout::BbcStudio, Format::TvSingle | Format::TvMulti) => Layout::BbcStudio,
            _ => Layout::Standard,
        }
    }
    /// The layouts offered for `format`, with their names.
    pub fn options(format: Format) -> Vec<(Layout, &'static str)> {
        match format {
            Format::Stage => vec![
                (Layout::Standard, "Standard"),
                (Layout::UkStage, "UK stage (BBC guide)"),
            ],
            Format::TvSingle | Format::TvMulti => {
                vec![
                    (Layout::Standard, "Standard"),
                    (Layout::BbcStudio, "BBC studio (taped)"),
                ]
            }
            Format::Film => vec![(Layout::Standard, "Standard")],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Plain,
    Bold,
    Italic,
}

/// Where and how one kind of element is set.
#[derive(Debug, Clone, Copy)]
pub struct Slot {
    pub indent: usize,
    pub width: usize,
    pub style: Style,
    pub caps: bool,
    pub underline: bool,
    pub align: Align,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

const fn slot(indent: usize, width: usize) -> Slot {
    Slot {
        indent,
        width,
        style: Style::Plain,
        caps: false,
        underline: false,
        align: Align::Left,
    }
}

#[derive(Debug, Clone)]
pub struct Profile {
    pub format: Format,
    /// Page size in points.
    pub page_w: f32,
    pub page_h: f32,
    /// Margins in points.
    pub left: f32,
    pub top: f32,
    /// Columns in the text area (10 per inch).
    pub cols: usize,
    /// Lines in the text area (6 per inch).
    pub lines_per_page: usize,
    /// Minutes of performance per page (screen formats).
    pub minutes_per_page: f64,
    pub heading: Slot,
    pub action: Slot,
    pub cue: Slot,
    pub paren: Slot,
    pub dialogue: Slot,
    pub lyric: Slot,
    pub transition: Slot,
    pub centered: Slot,
    /// Blank line after every dialogue line (multi-camera).
    pub double_dialogue: bool,
    /// Add (CONT'D) to cues that continue the previous speech (set from the project).
    pub auto_contd: bool,
    /// Every scene starts a new page (multi-camera, and the UK and studio layouts).
    pub scene_per_page: bool,
    pub layout: Layout,
    /// A speech too long for the rest of a page splits with (MORE)/(CONT'D);
    /// otherwise it moves to the next page whole.
    pub split_speeches: bool,
    /// Scene numbers in both margins beside each heading (shooting scripts).
    pub margin_numbers: bool,
}

impl Profile {
    pub fn for_format(format: Format) -> Profile {
        Profile::with_layout(format, Layout::Standard)
    }

    pub fn with_layout(format: Format, layout: Layout) -> Profile {
        let a4 = (595.0, 842.0);
        match layout.for_format(format) {
            Layout::Standard => {}
            Layout::UkStage => {
                return Profile {
                    format,
                    page_w: a4.0,
                    page_h: a4.1,
                    left: 90.0,
                    top: 72.0,
                    cols: 58,
                    lines_per_page: 58,
                    minutes_per_page: 1.0,
                    heading: Slot {
                        caps: true,
                        style: Style::Bold,
                        ..slot(0, 58)
                    },
                    action: Slot {
                        caps: true,
                        ..slot(20, 38)
                    },
                    cue: Slot {
                        caps: true,
                        ..slot(0, 15)
                    },
                    paren: slot(16, 42),
                    dialogue: slot(16, 42),
                    lyric: Slot {
                        style: Style::Italic,
                        ..slot(16, 42)
                    },
                    transition: Slot {
                        caps: true,
                        ..slot(20, 38)
                    },
                    centered: Slot {
                        align: Align::Center,
                        ..slot(0, 58)
                    },
                    double_dialogue: false,
                    scene_per_page: true,
                    auto_contd: false,
                    layout: Layout::UkStage,
                    split_speeches: false,
                    margin_numbers: false,
                };
            }
            Layout::BbcStudio => {
                // Wide left margin, for the camera and sound cues added in the studio.
                let sitcom = format == Format::TvMulti;
                return Profile {
                    format,
                    page_w: a4.0,
                    page_h: a4.1,
                    left: 144.0,
                    top: 72.0,
                    cols: 55,
                    lines_per_page: 58,
                    minutes_per_page: if sitcom { 0.5 } else { 1.0 },
                    heading: Slot {
                        caps: true,
                        style: Style::Bold,
                        underline: true,
                        ..slot(0, 55)
                    },
                    action: Slot {
                        caps: true,
                        ..if sitcom { slot(7, 34) } else { slot(14, 38) }
                    },
                    cue: Slot {
                        caps: true,
                        style: Style::Bold,
                        underline: true,
                        ..slot(0, 55)
                    },
                    paren: slot(0, if sitcom { 48 } else { 55 }),
                    dialogue: slot(0, if sitcom { 48 } else { 55 }),
                    lyric: Slot {
                        style: Style::Italic,
                        ..slot(4, 44)
                    },
                    transition: Slot {
                        caps: true,
                        ..slot(0, 55)
                    },
                    centered: Slot {
                        align: Align::Center,
                        ..slot(0, 55)
                    },
                    double_dialogue: sitcom,
                    scene_per_page: true,
                    auto_contd: false,
                    layout: Layout::BbcStudio,
                    split_speeches: false,
                    margin_numbers: false,
                };
            }
        }
        let letter = (612.0, 792.0);
        let a4 = (595.0, 842.0);
        match format {
            Format::Stage => Profile {
                format,
                page_w: a4.0,
                page_h: a4.1,
                left: 90.0,
                top: 72.0,
                cols: 58,
                lines_per_page: 58,
                minutes_per_page: 1.0,
                heading: Slot {
                    caps: true,
                    style: Style::Bold,
                    ..slot(0, 58)
                },
                action: Slot {
                    style: Style::Italic,
                    ..slot(6, 52)
                },
                cue: Slot {
                    caps: true,
                    align: Align::Center,
                    ..slot(0, 58)
                },
                paren: Slot {
                    style: Style::Italic,
                    align: Align::Center,
                    ..slot(10, 38)
                },
                dialogue: slot(4, 50),
                lyric: Slot {
                    style: Style::Italic,
                    ..slot(12, 40)
                },
                transition: Slot {
                    caps: true,
                    align: Align::Right,
                    ..slot(0, 58)
                },
                centered: Slot {
                    align: Align::Center,
                    ..slot(0, 58)
                },
                double_dialogue: false,
                scene_per_page: false,
                auto_contd: false,
                layout: Layout::Standard,
                split_speeches: true,
                margin_numbers: false,
            },
            Format::Film | Format::TvSingle | Format::TvMulti => {
                let multi = format == Format::TvMulti;
                Profile {
                    format,
                    page_w: letter.0,
                    page_h: letter.1,
                    left: 108.0,
                    top: 72.0,
                    cols: 60,
                    lines_per_page: 54,
                    minutes_per_page: if multi { 0.5 } else { 1.0 },
                    heading: Slot {
                        caps: true,
                        underline: multi,
                        ..slot(0, 60)
                    },
                    action: Slot {
                        caps: multi,
                        ..slot(0, 60)
                    },
                    cue: Slot {
                        caps: true,
                        ..slot(22, 38)
                    },
                    paren: slot(16, 25),
                    dialogue: slot(10, 35),
                    lyric: Slot {
                        style: Style::Italic,
                        ..slot(10, 35)
                    },
                    transition: Slot {
                        caps: true,
                        align: Align::Right,
                        ..slot(0, 60)
                    },
                    centered: Slot {
                        align: Align::Center,
                        ..slot(0, 60)
                    },
                    double_dialogue: multi,
                    scene_per_page: multi,
                    auto_contd: false,
                    layout: Layout::Standard,
                    split_speeches: true,
                    margin_numbers: false,
                }
            }
        }
    }
}

/// One printed line.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub text: String,
    /// Column of the first character.
    pub col: usize,
    pub style: Style,
    pub underline: bool,
    /// A scene number printed in both margins beside this line (shooting scripts).
    pub number: Option<String>,
}

impl Line {
    pub fn blank() -> Line {
        Line {
            text: String::new(),
            col: 0,
            style: Style::Plain,
            underline: false,
            number: None,
        }
    }
    pub fn is_blank(&self) -> bool {
        self.text.is_empty()
    }
}

/// Wrap `text` into lines no wider than `width` characters.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cur_len = 0;
    for word in text.split_whitespace() {
        let mut word: Vec<char> = word.chars().collect();
        while word.len() > width {
            // A word longer than the line: break it.
            if cur_len > 0 {
                out.push(std::mem::take(&mut cur));
                cur_len = 0;
            }
            out.push(word[..width].iter().collect());
            word = word[width..].to_vec();
        }
        let wl = word.len();
        if cur_len == 0 {
            cur = word.iter().collect();
            cur_len = wl;
        } else if cur_len + 1 + wl <= width {
            cur.push(' ');
            cur.extend(word.iter());
            cur_len += 1 + wl;
        } else {
            out.push(std::mem::take(&mut cur));
            cur = word.iter().collect();
            cur_len = wl;
        }
    }
    if cur_len > 0 || out.is_empty() {
        out.push(cur);
    }
    out
}

fn set(text: &str, s: &Slot) -> Vec<Line> {
    let text = if s.caps { text.to_uppercase() } else { text.to_string() };
    // `*emphasis*` markers are not printed.
    let text = text.replace('*', "");
    wrap(&text, s.width)
        .into_iter()
        .map(|t| {
            let len = t.chars().count();
            let col = match s.align {
                Align::Left => s.indent,
                Align::Center => s.indent + s.width.saturating_sub(len) / 2,
                Align::Right => s.indent + s.width.saturating_sub(len),
            };
            Line {
                text: t,
                col,
                style: s.style,
                underline: s.underline,
                number: None,
            }
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub enum BlockKind {
    /// Keeps with the block after it.
    Heading,
    /// Can split between lines (at least two lines each side).
    Action,
    /// A cue and what follows it; splits with (MORE) / (CONT'D).
    Speech {
        cue: String,
    },
    /// Stays with the block before it.
    Transition,
    /// Act / episode / scene-letter headers (keep with what follows).
    Title,
    Other,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub kind: BlockKind,
    pub lines: Vec<Line>,
    /// Start a new page before this block.
    pub page_break: bool,
    /// Blank lines to leave before this block (dropped at the top of a page).
    pub space_before: usize,
}

/// A scene body grouped for layout: speeches (a cue and everything under it),
/// single elements, and runs of blank lines.
enum Unit<'a> {
    Speech(&'a [Element]),
    Single(&'a Element),
    Gap(usize),
}

fn units(body: &[Element]) -> Vec<Unit<'_>> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < body.len() {
        match &body[i] {
            Element::Blank => {
                let start = i;
                while i < body.len() && matches!(body[i], Element::Blank) {
                    i += 1;
                }
                out.push(Unit::Gap(i - start));
            }
            Element::Cue(_) => {
                let start = i;
                i += 1;
                while i < body.len()
                    && matches!(
                        body[i],
                        Element::Parenthetical(_)
                            | Element::Dialogue(_)
                            | Element::Lyric(_)
                            | Element::LyricBreak
                            | Element::SongStart(_)
                            | Element::SongEnd
                    )
                {
                    i += 1;
                }
                out.push(Unit::Speech(&body[start..i]));
            }
            el => {
                out.push(Unit::Single(el));
                i += 1;
            }
        }
    }
    out
}

struct SpeechSlots {
    cue: Slot,
    paren: Slot,
    dialogue: Slot,
    lyric: Slot,
}

fn cue_text(els: &[Element], contd: bool) -> (String, String) {
    match &els[0] {
        Element::Cue(c) => {
            let mut shown = match &c.extension {
                Some(e) => format!("{} ({e})", c.name),
                None => c.name.clone(),
            };
            if contd {
                shown.push_str(" (CONT'D)");
            }
            (c.name.clone(), shown)
        }
        _ => (String::new(), String::new()),
    }
}

/// Double-spaced dialogue (multi-camera and studio sitcom): a blank line between
/// lines, and before this paragraph unless it follows the cue or a parenthetical.
fn add_double_spaced(lines: &mut Vec<Line>, new: Vec<Line>) {
    if lines.len() > 1 && lines.last().is_some_and(|l| !l.is_blank() && !l.text.starts_with('(')) {
        lines.push(Line::blank());
    }
    let n = new.len();
    for (k, l) in new.into_iter().enumerate() {
        lines.push(l);
        if k + 1 < n {
            lines.push(Line::blank());
        }
    }
}

fn speech_lines(els: &[Element], s: &SpeechSlots, double: bool, contd: bool) -> Vec<Line> {
    let mut lines = set(&cue_text(els, contd).1, &s.cue);
    for el in &els[1..] {
        match el {
            Element::Parenthetical(t) => lines.extend(set(t, &s.paren)),
            Element::Dialogue(t) | Element::Lyric(t) => {
                let slot = if matches!(el, Element::Lyric(_)) {
                    &s.lyric
                } else {
                    &s.dialogue
                };
                let new = set(t, slot);
                if double {
                    add_double_spaced(&mut lines, new);
                } else {
                    lines.extend(new);
                }
            }
            Element::LyricBreak => lines.push(Line::blank()),
            Element::SongStart(song) => {
                let label = match &song.title {
                    Some(t) => format!("SONG: \u{201c}{t}\u{201d}"),
                    None => "SONG".to_string(),
                };
                lines.extend(set(
                    &label,
                    &Slot {
                        caps: false,
                        underline: true,
                        ..s.lyric
                    },
                ));
            }
            _ => {}
        }
    }
    lines
}

/// A speech in the UK stage or BBC studio layout: parentheticals run into the
/// speech in capitals, "(PAUSE) Well…". UK stage: `NAME:` at the left margin
/// with the speech beside it (left out when the character simply carries on
/// after a direction). Studio: `NAME:` underlined, the speech below it.
fn inline_speech(els: &[Element], p: &Profile, show_name: bool) -> Vec<Line> {
    let (name, ext) = match &els[0] {
        Element::Cue(c) => (
            c.name.to_uppercase(),
            c.extension.clone().filter(|e| !e.to_uppercase().contains("CONT")),
        ),
        _ => (String::new(), None),
    };
    let mut paras: Vec<(String, Slot)> = Vec::new();
    let mut pending = String::new();
    for el in &els[1..] {
        match el {
            Element::Parenthetical(t) => {
                pending.push_str(&t.to_uppercase());
                pending.push(' ');
            }
            Element::Dialogue(t) => paras.push((format!("{pending}{t}"), p.dialogue)),
            Element::Lyric(t) => paras.push((t.clone(), p.lyric)),
            Element::LyricBreak => paras.push((String::new(), p.lyric)),
            Element::SongStart(song) => {
                let label = match &song.title {
                    Some(t) => format!("SONG: \u{201c}{t}\u{201d}"),
                    None => "SONG".to_string(),
                };
                paras.push((
                    label,
                    Slot {
                        underline: true,
                        ..p.lyric
                    },
                ));
            }
            _ => {}
        }
        if !matches!(el, Element::Parenthetical(_)) {
            pending.clear();
        }
    }
    if !pending.trim().is_empty() {
        paras.push((pending.trim_end().to_string(), p.dialogue));
    }
    let mut lines: Vec<Line> = Vec::new();
    if p.layout == Layout::UkStage {
        let label = match &ext {
            Some(e) => format!("{name} ({e}):"),
            None => format!("{name}:"),
        };
        let mut first = show_name;
        for (t, s) in &paras {
            if t.is_empty() {
                lines.push(Line::blank());
                continue;
            }
            let mut set_lines = set(t, s);
            if std::mem::take(&mut first) {
                if label.chars().count() < s.indent {
                    // The name and the first line of the speech share a line.
                    let l0 = &mut set_lines[0];
                    l0.text = format!("{label:<w$}{}", l0.text, w = s.indent);
                    l0.col = 0;
                } else {
                    lines.extend(set(&label, &Slot { width: p.cols, ..p.cue }));
                }
            }
            lines.extend(set_lines);
        }
        if lines.is_empty() && show_name {
            lines.extend(set(&label, &Slot { width: p.cols, ..p.cue }));
        }
    } else {
        let label = match &ext {
            Some(e) => format!("{name}: ({e})"),
            None => format!("{name}:"),
        };
        lines.extend(set(&label, &p.cue));
        for (t, s) in &paras {
            if t.is_empty() {
                lines.push(Line::blank());
                continue;
            }
            let new = set(t, s);
            if p.double_dialogue {
                add_double_spaced(&mut lines, new);
            } else {
                lines.extend(new);
            }
        }
    }
    lines
}

/// Two speeches side by side (dual dialogue): each in half the width.
fn dual_lines(left: &[Element], right: &[Element], p: &Profile) -> Vec<Line> {
    let half = p.cols.saturating_sub(2) / 2;
    let col = |offset: usize| SpeechSlots {
        cue: Slot {
            caps: true,
            align: Align::Center,
            ..slot(offset, half)
        },
        paren: Slot {
            indent: offset + 3,
            width: half.saturating_sub(6),
            ..p.paren
        },
        dialogue: Slot {
            indent: offset,
            width: half.saturating_sub(1),
            ..p.dialogue
        },
        lyric: Slot {
            indent: offset,
            width: half.saturating_sub(1),
            ..p.lyric
        },
    };
    let a = speech_lines(left, &col(0), p.double_dialogue, false);
    let b = speech_lines(right, &col(half + 2), p.double_dialogue, false);
    (0..a.len().max(b.len()))
        .map(|i| {
            let mut text = String::new();
            for l in [a.get(i), b.get(i)].into_iter().flatten() {
                if l.is_blank() {
                    continue;
                }
                let pad = l.col.saturating_sub(text.chars().count());
                text.push_str(&" ".repeat(pad.max(if text.is_empty() { 0 } else { 1 })));
                text.push_str(&l.text);
            }
            Line {
                text,
                col: 0,
                style: Style::Plain,
                underline: false,
                number: None,
            }
        })
        .collect()
}

/// Lay out one scene's body as blocks.
pub fn scene_blocks(scene: &Scene, p: &Profile) -> Vec<Block> {
    scene_blocks_numbered(scene, p, &mut 0)
}

/// As `scene_blocks`, numbering scene headings on from `counter` (studio
/// layouts and shooting scripts number every heading; an explicit `#12#` wins).
pub fn scene_blocks_numbered(scene: &Scene, p: &Profile, counter: &mut u32) -> Vec<Block> {
    let normal = SpeechSlots {
        cue: p.cue,
        paren: p.paren,
        dialogue: p.dialogue,
        lyric: p.lyric,
    };
    let mut blocks: Vec<Block> = Vec::new();
    let mut gap = 0usize;
    let mut last_speech: Option<(usize, &[Element])> = None; // block index, elements
    let contd: Vec<usize> = if p.auto_contd || p.layout == Layout::UkStage {
        crate::scene::continued_cues(&scene.body)
    } else {
        Vec::new()
    };
    // Which body element a speech starts at (units borrow slices of the body).
    let index_of =
        |els: &[Element]| (els.as_ptr() as usize - scene.body.as_ptr() as usize) / std::mem::size_of::<Element>();
    let push = |blocks: &mut Vec<Block>, gap: &mut usize, kind: BlockKind, lines: Vec<Line>| {
        let space_before = if blocks.is_empty() { 0 } else { (*gap).clamp(1, 2) };
        blocks.push(Block {
            kind,
            lines,
            page_break: false,
            space_before,
        });
        *gap = 0;
    };

    for unit in units(&scene.body) {
        match unit {
            Unit::Gap(n) => gap += n,
            Unit::Speech(els) => {
                let dual = matches!(&els[0], Element::Cue(c) if c.dual);
                match last_speech.filter(|(i, _)| dual && *i + 1 == blocks.len()) {
                    Some((i, left)) => {
                        blocks[i].lines = dual_lines(left, els, p);
                        last_speech = None;
                        gap = 0;
                    }
                    None => {
                        let (cue, _) = cue_text(els, false);
                        let continues = contd.contains(&index_of(els));
                        let lines = match p.layout {
                            Layout::Standard => speech_lines(els, &normal, p.double_dialogue, continues),
                            // UK stage: a speech carried on after a direction doesn't repeat the name.
                            Layout::UkStage => inline_speech(els, p, !continues),
                            Layout::BbcStudio => inline_speech(els, p, true),
                        };
                        push(&mut blocks, &mut gap, BlockKind::Speech { cue }, lines);
                        last_speech = Some((blocks.len() - 1, els));
                    }
                }
            }
            Unit::Single(el) => {
                last_speech = None;
                match el {
                    Element::SceneHeading(h) => {
                        *counter = match h.number.as_deref().map(str::parse::<u32>) {
                            Some(Ok(n)) => n,
                            _ => *counter + 1,
                        };
                        let n = h.number.clone().unwrap_or_else(|| counter.to_string());
                        let (text, margin) = match p.layout {
                            Layout::BbcStudio if p.format == Format::TvMulti => {
                                (format!("SCENE {n}. {}", h.text), None)
                            }
                            Layout::BbcStudio => (format!("SC{n}. {}", h.text), None),
                            _ if p.margin_numbers => (h.text.clone(), Some(n)),
                            _ => match &h.number {
                                Some(n) => (format!("{n}  {}", h.text), None),
                                None => (h.text.clone(), None),
                            },
                        };
                        let mut lines = set(&text, &p.heading);
                        lines[0].number = margin;
                        push(&mut blocks, &mut gap, BlockKind::Heading, lines);
                    }
                    Element::Direction { text, .. } => {
                        push(&mut blocks, &mut gap, BlockKind::Action, set(text, &p.action))
                    }
                    Element::Transition { text, .. } => {
                        push(&mut blocks, &mut gap, BlockKind::Transition, set(text, &p.transition))
                    }
                    Element::Centered(raw) => {
                        let inner = raw.trim_start_matches('>').trim_end_matches('<').trim();
                        push(&mut blocks, &mut gap, BlockKind::Other, set(inner, &p.centered));
                    }
                    Element::PageBreak(_) => {
                        gap = 0;
                        blocks.push(Block {
                            kind: BlockKind::Other,
                            lines: Vec::new(),
                            page_break: true,
                            space_before: 0,
                        });
                    }
                    // Lyrics or a song outside a speech: set like a speech without a cue.
                    Element::Lyric(t) | Element::Dialogue(t) | Element::Parenthetical(t) => {
                        push(&mut blocks, &mut gap, BlockKind::Other, set(t, &p.lyric))
                    }
                    Element::SongStart(song) => {
                        let label = song
                            .title
                            .clone()
                            .map(|t| format!("SONG: \u{201c}{t}\u{201d}"))
                            .unwrap_or("SONG".into());
                        push(
                            &mut blocks,
                            &mut gap,
                            BlockKind::Other,
                            set(
                                &label,
                                &Slot {
                                    underline: true,
                                    ..p.lyric
                                },
                            ),
                        );
                    }
                    Element::LyricBreak => gap += 1,
                    Element::Blank | Element::Cue(_) | Element::SongEnd => {}
                }
            }
        }
    }
    // Studio drama: each run of action is enclosed in square brackets.
    if p.layout == Layout::BbcStudio && p.format == Format::TvSingle {
        let mut i = 0;
        while i < blocks.len() {
            if blocks[i].kind == BlockKind::Action && !blocks[i].lines.is_empty() {
                let start = i;
                while blocks.get(i + 1).is_some_and(|b| b.kind == BlockKind::Action) {
                    i += 1;
                }
                let first = &mut blocks[start].lines[0];
                first.text = format!("[{}", first.text);
                first.col = first.col.saturating_sub(1);
                if let Some(last) = blocks[i].lines.last_mut() {
                    last.text.push(']');
                }
            }
            i += 1;
        }
    }
    blocks
}

/// Lines a scene occupies, ignoring page breaks (used for page counts).
pub fn scene_line_count(scene: &Scene, p: &Profile) -> usize {
    scene_blocks(scene, p)
        .iter()
        .map(|b| b.space_before + b.lines.len())
        .sum()
}

/// Page length in eighths of a page (the industry's breakdown unit), at least 1.
pub fn eighths(lines: usize, p: &Profile) -> u32 {
    let per_eighth = p.lines_per_page as f64 / 8.0;
    ((lines as f64 / per_eighth).ceil() as u32).max(1)
}

/// `13` eighths -> `1 5/8`.
pub fn eighths_label(e: u32) -> String {
    let (whole, part) = (e / 8, e % 8);
    match (whole, part) {
        (0, p) => format!("{p}/8"),
        (w, 0) => w.to_string(),
        (w, p) => format!("{w} {p}/8"),
    }
}

// ---------------------------------------------------------------- pagination

pub type Page = Vec<Line>;

/// Lay `blocks` out on pages. `cont` is the "(CONT'D)" text used when a speech
/// carries over a page break.
pub fn paginate(blocks: &[Block], p: &Profile) -> Vec<Page> {
    let cap = p.lines_per_page;
    let mut pages: Vec<Page> = vec![Vec::new()];

    fn room(pages: &[Page], cap: usize) -> usize {
        cap.saturating_sub(pages.last().unwrap().len())
    }
    fn new_page(pages: &mut Vec<Page>) {
        if !pages.last().unwrap().is_empty() {
            pages.push(Vec::new());
        }
    }

    let mut i = 0;
    while i < blocks.len() {
        let b = &blocks[i];
        // A page break straight after a title (ACT ONE, then SCENE A) keeps them together.
        let after_title = i > 0 && blocks[i - 1].kind == BlockKind::Title;
        if b.page_break && !after_title {
            new_page(&mut pages);
        }
        if b.lines.is_empty() {
            i += 1;
            continue;
        }
        let at_top = pages.last().unwrap().is_empty();
        let space = if at_top { 0 } else { b.space_before };
        let need = space + b.lines.len();

        // Keep-with-next: a heading needs its block and (at least) two of the next block's lines.
        let extra = match b.kind {
            BlockKind::Heading | BlockKind::Title => blocks
                .get(i + 1)
                .filter(|n| !n.page_break)
                .map(|n| n.space_before + n.lines.len().min(2))
                .unwrap_or(0),
            _ => 0,
        };

        if need + extra <= room(&pages, cap) {
            let page = pages.last_mut().unwrap();
            page.extend(std::iter::repeat_n(Line::blank(), space));
            page.extend(b.lines.iter().cloned());
            i += 1;
            continue;
        }

        let avail = room(&pages, cap).saturating_sub(space);
        match &b.kind {
            BlockKind::Action if !at_top && avail >= 2 && b.lines.len() >= 4 => {
                // Split: at least two lines on each page.
                let take = avail.min(b.lines.len() - 2);
                if take >= 2 {
                    let page = pages.last_mut().unwrap();
                    page.extend(std::iter::repeat_n(Line::blank(), space));
                    page.extend(b.lines[..take].iter().cloned());
                    pages.push(b.lines[take..].to_vec());
                    i += 1;
                    continue;
                }
            }
            BlockKind::Speech { cue } if !at_top && p.split_speeches => {
                // Cue + at least two lines here, (MORE) at the foot; at least two lines carried over.
                let body = &b.lines[1..];
                let fit = avail.saturating_sub(2); // cue line + (MORE)
                let mut take = fit.min(body.len().saturating_sub(2));
                // Never end a page on a parenthetical or a blank spacer.
                while take > 0 && (body[take - 1].is_blank() || body[take - 1].text.starts_with('(')) {
                    take -= 1;
                }
                if take >= 2 {
                    let page = pages.last_mut().unwrap();
                    page.extend(std::iter::repeat_n(Line::blank(), space));
                    page.push(b.lines[0].clone());
                    page.extend(body[..take].iter().cloned());
                    page.push(Line {
                        text: "(MORE)".into(),
                        col: p.cue.indent,
                        style: Style::Plain,
                        underline: false,
                        number: None,
                    });
                    let mut next: Page = set(&format!("{cue} (CONT'D)"), &p.cue);
                    let rest: Vec<Line> = body[take..].iter().skip_while(|l| l.is_blank()).cloned().collect();
                    next.extend(rest);
                    pages.push(next);
                    i += 1;
                    continue;
                }
            }
            _ => {}
        }

        // Whole block to the next page. A transition takes the block before it along.
        if matches!(b.kind, BlockKind::Transition) && !at_top {
            let page = pages.last_mut().unwrap();
            let carried = blocks[i - 1].lines.len().min(page.len());
            let moved: Vec<Line> = page.split_off(page.len() - carried);
            while page.last().is_some_and(Line::is_blank) {
                page.pop();
            }
            pages.push(moved);
            let page = pages.last_mut().unwrap();
            page.extend(std::iter::repeat_n(Line::blank(), b.space_before));
            page.extend(b.lines.iter().cloned());
            i += 1;
            continue;
        }
        new_page(&mut pages);
        // A block taller than a page: let it run on (rare: a very long speech or direction).
        let mut rest: &[Line] = &b.lines;
        while !rest.is_empty() {
            let r = room(&pages, cap).max(1);
            let n = r.min(rest.len());
            pages.last_mut().unwrap().extend(rest[..n].iter().cloned());
            rest = &rest[n..];
            if !rest.is_empty() {
                pages.push(Vec::new());
            }
        }
        i += 1;
    }
    for page in pages.iter_mut() {
        while page.last().is_some_and(Line::is_blank) {
            page.pop();
        }
    }
    pages.retain(|pg| !pg.is_empty());
    pages
}

/// A centred, underlined header line (act titles, scene letters).
pub fn header_block(text: &str, p: &Profile, page_break: bool) -> Block {
    let slot = Slot {
        caps: true,
        underline: true,
        align: Align::Center,
        ..slot(0, p.cols)
    };
    Block {
        kind: BlockKind::Title,
        lines: set(text, &slot),
        page_break,
        space_before: 2,
    }
}

pub fn text_block(text: &str, s: &Slot, space_before: usize) -> Block {
    Block {
        kind: BlockKind::Other,
        lines: set(text, s),
        page_break: false,
        space_before,
    }
}

pub fn centered_slot(p: &Profile) -> Slot {
    Slot {
        align: Align::Center,
        ..slot(0, p.cols)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(body: &str) -> Scene {
        Scene::parse(&format!("---\nid: t\n---\n{body}")).unwrap()
    }

    #[test]
    fn wraps_on_words_and_breaks_long_words() {
        assert_eq!(wrap("one two three", 7), vec!["one two", "three"]);
        assert_eq!(wrap("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
        assert_eq!(wrap("", 10), vec![""]);
    }

    #[test]
    fn screenplay_columns() {
        let p = Profile::for_format(Format::Film);
        let b = scene_blocks(
            &scene("INT. LIGHTHOUSE - NIGHT\n\nRain.\n\nMORDECAI\n(quietly)\nHello.\n"),
            &p,
        );
        assert_eq!(b[0].lines[0].text, "INT. LIGHTHOUSE - NIGHT");
        assert_eq!(b[2].lines[0].col, 22); // cue at 3.7"
        assert_eq!(b[2].lines[1].col, 16); // parenthetical at 3.1"
        assert_eq!(b[2].lines[2].col, 10); // dialogue at 2.5"
    }

    #[test]
    fn multicam_capitals_and_double_spacing() {
        let p = Profile::for_format(Format::TvMulti);
        let b = scene_blocks(&scene("Liam enters.\n\nLIAM\nHi.\nBye.\n"), &p);
        assert_eq!(b[0].lines[0].text, "LIAM ENTERS.");
        let speech: Vec<&str> = b[1].lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(speech, vec!["LIAM", "Hi.", "", "Bye."]);
    }

    #[test]
    fn long_speech_splits_with_more_and_contd() {
        let p = Profile::for_format(Format::Film);
        let mut body = String::from("Filler.\n\n");
        for _ in 0..22 {
            body.push_str("Line of action.\n\n");
        }
        body.push_str("KING\n");
        for i in 0..12 {
            body.push_str(&format!(
                "Sentence number {i} is long enough to fill a whole dialogue line here.\n"
            ));
        }
        let pages = paginate(&scene_blocks(&scene(&body), &p), &p);
        assert!(pages.len() >= 2);
        let i = pages
            .iter()
            .position(|pg| pg.last().unwrap().text == "(MORE)")
            .expect("a (MORE) at a page foot");
        assert_eq!(pages[i + 1][0].text, "KING (CONT'D)");
        assert!(
            pages[i].iter().any(|l| l.text == "KING"),
            "the cue starts on the same page"
        );
        for pg in &pages {
            assert!(pg.len() <= p.lines_per_page);
        }
    }

    #[test]
    fn heading_never_ends_a_page() {
        let p = Profile::for_format(Format::Film);
        let mut body = String::new();
        for _ in 0..26 {
            body.push_str("Line.\n\n");
        }
        body.push_str("INT. HALL - DAY\n\nThey run.\n");
        let pages = paginate(&scene_blocks(&scene(&body), &p), &p);
        for pg in &pages {
            assert!(pg.iter().rev().find(|l| !l.is_blank()).unwrap().text != "INT. HALL - DAY");
        }
    }

    #[test]
    fn dual_dialogue_sits_side_by_side() {
        let p = Profile::for_format(Format::Film);
        let b = scene_blocks(
            &scene("NELL\nSomeone nailed a plank.\n\nHARBOURMASTER ^\nThen someone wanted it to float.\n"),
            &p,
        );
        assert_eq!(b.len(), 1, "two speeches become one block");
        let first = &b[0].lines[0].text;
        assert!(first.contains("NELL") && first.contains("HARBOURMASTER"), "{first}");
        assert!(b[0].lines.iter().all(|l| l.text.chars().count() <= p.cols));
    }

    #[test]
    fn multicam_no_blank_after_parenthetical() {
        let p = Profile::for_format(Format::TvMulti);
        let b = scene_blocks(&scene("GUS\n(reading)\nWhy am I purple?\n"), &p);
        let t: Vec<&str> = b[0].lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(t, vec!["GUS", "(reading)", "Why am I purple?"]);
    }

    #[test]
    fn auto_contd_is_printed_on_continuing_cues() {
        let body = "NELL\nHello.\n\nShe turns.\n\nNELL\nStill here.\n";
        let mut p = Profile::for_format(Format::Film);
        let cues = |p: &Profile| -> Vec<String> {
            scene_blocks(&scene(body), p)
                .iter()
                .filter(|b| matches!(b.kind, BlockKind::Speech { .. }))
                .map(|b| b.lines[0].text.trim().to_string())
                .collect()
        };
        assert_eq!(cues(&p), vec!["NELL", "NELL"]);
        p.auto_contd = true;
        assert_eq!(cues(&p), vec!["NELL", "NELL (CONT'D)"]);
    }

    #[test]
    fn eighths_and_labels() {
        let p = Profile::for_format(Format::Film);
        assert_eq!(eighths(1, &p), 1);
        assert_eq!(eighths(54, &p), 8);
        assert_eq!(eighths(55, &p), 9);
        assert_eq!(eighths_label(13), "1 5/8");
        assert_eq!(eighths_label(3), "3/8");
        assert_eq!(eighths_label(16), "2");
    }

    fn texts(blocks: &[Block]) -> Vec<String> {
        blocks
            .iter()
            .flat_map(|b| b.lines.iter().map(|l| format!("{}{}", " ".repeat(l.col), l.text)))
            .collect()
    }

    #[test]
    fn uk_stage_puts_the_name_beside_the_speech() {
        let p = Profile::with_layout(Format::Stage, Layout::UkStage);
        let b = scene_blocks(
            &scene("Rain.\n\nHETTIE\n(quietly)\nHello.\n\nShe turns.\n\nHETTIE\nAgain.\n"),
            &p,
        );
        let t = texts(&b);
        assert_eq!(t[0], format!("{}RAIN.", " ".repeat(20)));
        assert_eq!(t[1], "HETTIE:         (QUIETLY) Hello.");
        // Carrying on after a direction: no name again.
        assert_eq!(t[3], format!("{}Again.", " ".repeat(16)));
        assert!(!p.split_speeches && p.scene_per_page);
        // The layout only applies to its format.
        assert_eq!(
            Profile::with_layout(Format::Film, Layout::UkStage).layout,
            Layout::Standard
        );
    }

    #[test]
    fn studio_drama_numbers_scenes_and_brackets_action() {
        let p = Profile::with_layout(Format::TvSingle, Layout::BbcStudio);
        let mut n = 4;
        let b = scene_blocks_numbered(
            &scene("INT. HALL - DAY\n\nRain.\n\nThunder.\n\nNELL (V.O.)\nHi.\n"),
            &p,
            &mut n,
        );
        let t = texts(&b);
        assert_eq!(t[0], "SC5. INT. HALL - DAY");
        assert_eq!(t[1], format!("{}[RAIN.", " ".repeat(13)));
        assert_eq!(t[2], format!("{}THUNDER.]", " ".repeat(14)));
        assert_eq!(t[3], "NELL: (V.O.)");
        assert_eq!(n, 5);
        let m = Profile::with_layout(Format::TvMulti, Layout::BbcStudio);
        assert_eq!(
            texts(&scene_blocks(&scene("INT. HALL - DAY\n"), &m))[0],
            "SCENE 1. INT. HALL - DAY"
        );
    }

    #[test]
    fn shooting_scripts_number_headings_in_the_margins() {
        let mut p = Profile::for_format(Format::Film);
        p.margin_numbers = true;
        let b = scene_blocks(
            &scene("INT. A - DAY\n\nX.\n\nINT. B - DAY #7#\n\nY.\n\nINT. C - DAY\n"),
            &p,
        );
        let nums: Vec<Option<String>> = b
            .iter()
            .filter(|b| b.kind == BlockKind::Heading)
            .map(|b| b.lines[0].number.clone())
            .collect();
        assert_eq!(nums, vec![Some("1".into()), Some("7".into()), Some("8".into())]);
    }
}
