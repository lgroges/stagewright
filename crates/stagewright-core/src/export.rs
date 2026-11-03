//! Whole-script exports: a paginated PDF in the project's format, and Fountain.

use crate::analysis::scene_stats;
use crate::layout::{
    Block, BlockKind, Format, Line, Page, Profile, centered_slot, header_block, paginate, scene_blocks, text_block,
};
use crate::pdf::{PdfPage, PdfSettings, render};
use crate::project::{ActRef, Project, SceneFile};
use crate::scene::{Element, serialize_body};

const WORDS: &[&str] = &[
    "ZERO", "ONE", "TWO", "THREE", "FOUR", "FIVE", "SIX", "SEVEN", "EIGHT", "NINE", "TEN",
];

fn act_heading(p: &Project, act: u32) -> String {
    if let Some(t) = p
        .meta
        .acts
        .iter()
        .find(|a| a.number == act)
        .and_then(|a| a.title.clone())
    {
        return t.to_uppercase();
    }
    match (act, p.meta.format) {
        (0, Format::Stage) => "PROLOGUE".into(),
        (0, Format::Film) => "OPENING".into(),
        (0, _) => "TEASER".into(),
        (n, _) => format!("ACT {}", WORDS.get(n as usize).copied().unwrap_or("?")),
    }
}

fn scene_letter(i: usize) -> String {
    // A..Z, then AA, BB … (multi-camera convention)
    let c = (b'A' + (i % 26) as u8) as char;
    c.to_string().repeat(i / 26 + 1)
}

/// The scenes to print: everything, or one episode of a series.
fn selection(p: &Project, episode: Option<u32>) -> Vec<(ActRef, Vec<&SceneFile>)> {
    p.act_refs
        .iter()
        .filter(|a| episode.is_none() || a.episode == episode)
        .map(|a| (*a, p.scenes.iter().filter(|s| s.act_ref() == *a).collect::<Vec<_>>()))
        .filter(|(_, s)| !s.is_empty())
        .collect()
}

/// Whether a scene already opens (or closes) with a transition, say FADE IN:.
fn scene_says(sf: &SceneFile, last: bool, word: &str) -> bool {
    let text_of = |e: &Element| match e {
        Element::Transition { text, .. } | Element::Direction { text, .. } | Element::Centered(text) => {
            Some(text.clone())
        }
        _ => None,
    };
    let mut els = sf.scene.body.iter().filter(|e| !matches!(e, Element::Blank));
    let el = if last {
        // Past a closing THE END the writer may have typed.
        els.rev()
            .find(|e| text_of(e).is_none_or(|t| !crate::scene::is_end_line(&t) || word == "THE END"))
    } else {
        els.next()
    };
    let Some(text) = el.and_then(text_of) else { return false };
    text.trim_start_matches('>').trim().to_uppercase().starts_with(word)
}

/// FADE IN: at the left, as the guides set it.
fn fade_in(prof: &Profile) -> Block {
    text_block("FADE IN:", &prof.action, 0)
}

fn fade_out(prof: &Profile) -> Block {
    text_block("FADE OUT.", &prof.transition, 1)
}

fn script_blocks(p: &Project, prof: &Profile, episode: Option<u32>) -> Vec<Block> {
    let f = p.meta.format;
    let mut out: Vec<Block> = Vec::new();
    let mut letter = 0usize;
    let mut last_episode: Option<Option<u32>> = None;
    let acts = selection(p, episode);
    let multi_act = acts
        .iter()
        .map(|(a, _)| a.act)
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        > 1;
    for (ai, (aref, scenes)) in acts.iter().enumerate() {
        // The last act of an episode (or of the script) ends the show.
        let ends_show = acts.get(ai + 1).is_none_or(|(next, _)| next.episode != aref.episode);
        // Film: FADE IN: once at the start; TV: at the start of every act.
        let opens = (f == Format::Film && ai == 0) || f.is_tv();
        if last_episode != Some(aref.episode) {
            if let (Some(e), None) = (aref.episode, episode) {
                // Whole series in one file: a title card per episode.
                let t = p.episode_title(e).unwrap_or_else(|| format!("Episode {e}"));
                out.push(header_block(&format!("EPISODE {e}: {t}"), prof, !out.is_empty()));
                letter = 0;
            }
            last_episode = Some(aref.episode);
        }
        let heading = act_heading(p, aref.act);
        let show_acts = f.is_tv() || (f == Format::Stage && multi_act);
        if show_acts {
            out.push(header_block(
                &heading,
                prof,
                !out.is_empty() && f.is_tv() || out.len() > 1,
            ));
        }
        for (si, sf) in scenes.iter().enumerate() {
            let mut blocks = scene_blocks(&sf.scene, prof);
            let fade_in_here = opens && si == 0 && !scene_says(sf, false, "FADE IN");
            if f == Format::TvMulti {
                // Every scene on a new page, lettered, with its cast under the heading.
                out.push(header_block(&format!("SCENE {}", scene_letter(letter)), prof, true));
                letter += 1;
                let cast: Vec<String> = scene_stats(&sf.scene, &p.meta).present.into_iter().collect();
                if !cast.is_empty() {
                    let pos = blocks
                        .iter()
                        .position(|b| b.kind == BlockKind::Heading)
                        .map(|i| i + 1)
                        .unwrap_or(0);
                    // the cast list sits directly under the heading
                    let mut b = text_block(&format!("({})", cast.join(", ")), &prof.heading, 0);
                    b.space_before = 0;
                    b.kind = BlockKind::Other;
                    b.lines.iter_mut().for_each(|l| l.underline = false);
                    blocks.insert(pos, b);
                }
            } else if f == Format::Stage {
                let title = sf.scene.meta.title.clone().unwrap_or_default();
                if !title.is_empty() {
                    let mut b = text_block(&title, &centered_slot(prof), 2);
                    b.kind = BlockKind::Heading;
                    b.lines.iter_mut().for_each(|l| l.underline = true);
                    out.push(b);
                }
            }
            if fade_in_here {
                let mut b = fade_in(prof);
                b.space_before = if out.is_empty() { 0 } else { 2 };
                out.push(b);
            }
            if let Some(first) = blocks.first_mut() {
                first.space_before = if fade_in_here {
                    1
                } else if out.is_empty() {
                    0
                } else {
                    2
                };
            }
            out.extend(blocks);
        }
        let closed = scenes.last().is_some_and(|sf| scene_says(sf, true, "FADE OUT"));
        if f.is_tv() {
            if !closed {
                out.push(fade_out(prof));
            }
            let end = if ends_show {
                "END OF SHOW".to_string()
            } else {
                format!("END OF {heading}")
            };
            let mut b = text_block(&end, &centered_slot(prof), 2);
            b.lines.iter_mut().for_each(|l| l.underline = true);
            out.push(b);
        } else if f == Format::Film && ai + 1 == acts.len() && !closed {
            out.push(fade_out(prof));
        }
    }
    let typed_end = acts
        .last()
        .and_then(|(_, s)| s.last())
        .is_some_and(|sf| scene_says(sf, true, "THE END"));
    if (f == Format::Stage || f == Format::Film) && !typed_end {
        out.push(text_block("THE END", &centered_slot(prof), 2));
    }
    out
}

/// The contact lines for the title page.
fn contact_lines(p: &Project) -> Vec<String> {
    p.meta
        .contact
        .as_deref()
        .unwrap_or("")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// Spec film and TV scripts leave the draft off the title page.
fn shows_draft(p: &Project) -> Option<&String> {
    p.meta
        .draft
        .as_ref()
        .filter(|_| !p.meta.format.is_screen() || p.meta.shooting_script)
}

fn add_centered(page: &mut Page, prof: &Profile, text: &str, underline: bool) {
    for mut l in text_block(text, &centered_slot(prof), 0).lines {
        l.underline = underline;
        page.push(l);
    }
}

fn title_page(p: &Project, prof: &Profile, episode: Option<u32>) -> Page {
    let mut page: Page = vec![Line::blank(); prof.lines_per_page / 3];
    add_centered(&mut page, prof, &p.meta.title.to_uppercase(), true);
    if let Some(sub) = &p.meta.subtitle {
        page.push(Line::blank());
        add_centered(&mut page, prof, sub, false);
    }
    if let Some(e) = episode {
        page.push(Line::blank());
        let t = p.episode_title(e).unwrap_or_else(|| format!("Episode {e}"));
        add_centered(&mut page, prof, &format!("Episode {e}: \u{201c}{t}\u{201d}"), false);
    }
    page.push(Line::blank());
    page.push(Line::blank());
    add_centered(
        &mut page,
        prof,
        if p.meta.format.is_screen() { "Written by" } else { "by" },
        false,
    );
    page.push(Line::blank());
    add_centered(&mut page, prof, &p.meta.authors.join(" & "), false);
    // At the foot: the draft on the right, then the contact details on the left.
    let contact = contact_lines(p);
    let draft = shows_draft(p);
    let foot = contact.len() + usize::from(draft.is_some());
    if foot > 0 {
        while page.len() + foot < prof.lines_per_page {
            page.push(Line::blank());
        }
        let line = |text: &str, col: usize| Line {
            text: text.to_string(),
            col,
            style: crate::layout::Style::Plain,
            underline: false,
        };
        if let Some(d) = draft {
            page.push(line(d, prof.cols.saturating_sub(d.chars().count())));
        }
        for c in &contact {
            page.push(line(c, 0));
        }
    }
    page
}

/// The script as pages (title page first).
pub fn pages(p: &Project, episode: Option<u32>) -> (Profile, Vec<Page>) {
    let mut prof = Profile::for_format(p.meta.format);
    prof.auto_contd = p.meta.auto_contd();
    let mut out = vec![title_page(p, &prof, episode)];
    out.extend(paginate(&script_blocks(p, &prof, episode), &prof));
    (prof, out)
}

/// The script as a PDF. Pages are numbered from the second script page, as is customary.
pub fn pdf(p: &Project, episode: Option<u32>) -> Vec<u8> {
    let (prof, pages) = pages(p, episode);
    let pdf_pages: Vec<PdfPage> = pages
        .into_iter()
        .enumerate()
        .map(|(i, lines)| PdfPage {
            lines,
            number: (i >= 2).then(|| format!("{i}.")),
        })
        .collect();
    render(
        &pdf_pages,
        &PdfSettings {
            page_w: prof.page_w,
            page_h: prof.page_h,
            left: prof.left,
            top: prof.top,
            title: p.meta.title.clone(),
            author: p.meta.authors.join(" & "),
        },
    )
}

/// The script as one Fountain file (https://fountain.io), for other screenwriting tools.
pub fn fountain(p: &Project, episode: Option<u32>) -> String {
    let mut out = format!("Title: {}\n", p.meta.title);
    if let Some(e) = episode {
        out.push_str(&format!("Episode: {e}: {}\n", p.episode_title(e).unwrap_or_default()));
    }
    out.push_str("Credit: Written by\n");
    out.push_str(&format!("Author: {}\n", p.meta.authors.join(" & ")));
    if let Some(d) = shows_draft(p) {
        out.push_str(&format!("Draft date: {d}\n"));
    }
    let contact = contact_lines(p);
    if !contact.is_empty() {
        out.push_str("Contact:\n");
        for c in contact {
            out.push_str(&format!("    {c}\n"));
        }
    }
    out.push('\n');
    for (aref, scenes) in selection(p, episode) {
        out.push_str(&format!("# {}\n\n", act_heading(p, aref.act)));
        for sf in scenes {
            if let Some(t) = &sf.scene.meta.title {
                out.push_str(&format!("## {t}\n\n"));
            }
            let mut body_els = sf.scene.body.clone();
            if p.meta.auto_contd() {
                for i in crate::scene::continued_cues(&body_els) {
                    if let Element::Cue(c) = &mut body_els[i] {
                        c.raw = format!("{} (CONT'D)", c.raw.trim_end());
                    }
                }
            }
            let body = serialize_body(&body_els);
            out.push_str(body.trim_end_matches('\n'));
            out.push_str("\n\n");
        }
    }
    out
}

/// Total printed pages of the script (not counting the title page).
pub fn page_count(p: &Project, episode: Option<u32>) -> usize {
    pages(p, episode).1.len() - 1
}

/// Every scene's body contains only things Fountain understands except our
/// `~ SONG`/`~ END SONG` markers, which Fountain reads as lyrics.
pub fn fountain_warnings(p: &Project) -> Vec<String> {
    let mut w = Vec::new();
    for sf in &p.scenes {
        if sf.scene.body.iter().any(|e| matches!(e, Element::SongStart(_))) {
            w.push(format!("{}: song markers export as lyric lines", sf.scene.meta.id));
        }
    }
    w
}
