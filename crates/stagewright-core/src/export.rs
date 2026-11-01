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
    for (aref, scenes) in &acts {
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
        for sf in scenes {
            let mut blocks = scene_blocks(&sf.scene, prof);
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
            if let Some(first) = blocks.first_mut() {
                first.space_before = if out.is_empty() { 0 } else { 2 };
            }
            out.extend(blocks);
        }
        if f.is_tv() {
            let mut b = text_block(&format!("END OF {heading}"), &centered_slot(prof), 2);
            b.lines.iter_mut().for_each(|l| l.underline = true);
            out.push(b);
        }
    }
    if f == Format::Stage || f == Format::Film {
        out.push(text_block(
            if f == Format::Stage { "THE END" } else { "FADE OUT." },
            &centered_slot(prof),
            2,
        ));
    }
    out
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
    if let Some(d) = &p.meta.draft {
        while page.len() < prof.lines_per_page - 2 {
            page.push(Line::blank());
        }
        page.push(Line {
            text: d.clone(),
            col: 0,
            style: crate::layout::Style::Plain,
            underline: false,
        });
    }
    page
}

/// The script as pages (title page first).
pub fn pages(p: &Project, episode: Option<u32>) -> (Profile, Vec<Page>) {
    let prof = Profile::for_format(p.meta.format);
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
    if let Some(d) = &p.meta.draft {
        out.push_str(&format!("Draft date: {d}\n"));
    }
    out.push('\n');
    for (aref, scenes) in selection(p, episode) {
        out.push_str(&format!("# {}\n\n", act_heading(p, aref.act)));
        for sf in scenes {
            if let Some(t) = &sf.scene.meta.title {
                out.push_str(&format!("## {t}\n\n"));
            }
            let body = serialize_body(&sf.scene.body);
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
