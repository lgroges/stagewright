//! The scene as paragraphs for a model, and what a model changed in them.
//!
//! Models are asked to give paragraphs back corrected (they're better at that
//! than at listing errors); a word-by-word diff against the original turns each
//! change into a `Finding` placed on the body's lines.

use super::{Change, Finding};
use serde_json::Value;

/// One paragraph of the scene as the model sees it: a run of action, a speech
/// (its dialogue, without parentheticals), or a song's lyrics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paragraph {
    /// Who speaks or sings it; `None` for action.
    pub who: Option<String>,
    pub text: String,
    /// Where its text is: (body line, start in `text`, column in the line), a
    /// line's text running from its start to the next one's (less the joiner).
    pub lines: Vec<(usize, usize, usize)>,
}

impl Paragraph {
    /// The body line and column of the `text` range `from..to`, if it's on one line.
    fn locate(&self, from: usize, to: usize) -> Option<(usize, usize)> {
        let k = self.lines.iter().rposition(|&(_, start, _)| start <= from)?;
        let (line, start, col) = self.lines[k];
        let end = self.lines.get(k + 1).map_or(self.text.len(), |&(_, next, _)| next);
        (to <= end).then_some((line, col + from - start))
    }
}

/// The scene's action and dialogue, paragraph by paragraph. Headings, cues,
/// transitions and the like aren't the model's business.
pub fn paragraphs(body: &str) -> Vec<Paragraph> {
    use stagewright_core::scene::{parse_body, Element};
    let lines: Vec<&str> = body.split('\n').collect();
    let mut out: Vec<Paragraph> = Vec::new();
    let mut cur: Option<Paragraph> = None;
    let mut who: Option<String> = None;
    let flush = |cur: &mut Option<Paragraph>, out: &mut Vec<Paragraph>| {
        if let Some(p) = cur.take().filter(|p| !p.text.trim().is_empty()) {
            out.push(p);
        }
    };
    for (i, el) in parse_body(body).iter().enumerate() {
        let (kind_who, joiner, text) = match el {
            Element::Direction { text, .. } => (None, " ", text.trim()),
            Element::Dialogue(text) => (who.clone(), " ", text.trim()),
            Element::Lyric(text) => (Some(who.clone().unwrap_or_else(|| "SONG".into())), " / ", text.trim()),
            Element::Parenthetical(_) => continue,
            Element::Cue(c) => {
                flush(&mut cur, &mut out);
                who = Some(c.name.clone());
                continue;
            }
            Element::SongStart(s) => {
                flush(&mut cur, &mut out);
                who = Some(if s.singers.is_empty() {
                    "SONG".into()
                } else {
                    s.singers.join(" & ")
                });
                continue;
            }
            _ => {
                flush(&mut cur, &mut out);
                if !matches!(el, Element::Blank) {
                    who = None;
                }
                continue;
            }
        };
        let Some(col) = lines.get(i).and_then(|l| l.find(text)) else {
            continue;
        };
        match cur.as_mut().filter(|p| p.who == kind_who) {
            Some(p) => {
                p.text.push_str(joiner);
                p.lines.push((i, p.text.len(), col));
                p.text.push_str(text);
            }
            None => {
                flush(&mut cur, &mut out);
                cur = Some(Paragraph {
                    who: kind_who,
                    text: text.to_string(),
                    lines: vec![(i, 0, col)],
                });
            }
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// Words and punctuation, as byte ranges.
fn tokens(s: &str) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut it = s.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        if c.is_whitespace() {
            continue;
        }
        let mut end = i + c.len_utf8();
        if c.is_alphanumeric() {
            while let Some(&(j, d)) = it.peek() {
                let inner = matches!(d, '\'' | '\u{2019}' | '-')
                    && s[j + d.len_utf8()..].chars().next().is_some_and(char::is_alphanumeric);
                if d.is_alphanumeric() || inner {
                    end = j + d.len_utf8();
                    it.next();
                } else {
                    break;
                }
            }
        }
        out.push(i..end);
    }
    out
}

/// Tokens compare alike whichever apostrophes and quotes the model used.
fn same(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        s.replace(['\u{2019}', '\u{2018}'], "'")
            .replace(['\u{201c}', '\u{201d}'], "\"")
    };
    norm(a) == norm(b)
}

/// The changed runs between two token lists: (old range, new range), by
/// longest common subsequence.
fn hunks(old: &[&str], new: &[&str]) -> Vec<(std::ops::Range<usize>, std::ops::Range<usize>)> {
    let (n, m) = (old.len(), new.len());
    let mut lcs = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if same(old[i], new[j]) {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    let mut open: Option<(usize, usize)> = None;
    while i < n || j < m {
        if i < n && j < m && same(old[i], new[j]) {
            if let Some((a, b)) = open.take() {
                out.push((a..i, b..j));
            }
            i += 1;
            j += 1;
            continue;
        }
        open.get_or_insert((i, j));
        if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            j += 1;
        } else {
            i += 1;
        }
    }
    if let Some((a, b)) = open {
        out.push((a..n, b..m));
    }
    out
}

/// The paragraph a model's answer numbers `n` (from 1), with its index.
pub fn paragraph<'a>(paras: &'a [Paragraph], n: &Value) -> Option<(usize, &'a Paragraph)> {
    let i = (n.as_u64()? as usize).checked_sub(1)?;
    Some((i, paras.get(i)?))
}

/// What the model changed in each paragraph, as findings on the body: each
/// change with a word either side (more if needed to tell it apart on its line).
/// A paragraph rewritten rather than corrected, changes to words in capitals
/// (names), and changes that run across lines are dropped.
pub fn place(body: &str, paras: &[Paragraph], raw: &Value) -> Vec<Finding> {
    let lines: Vec<&str> = body.split('\n').collect();
    let mut out: Vec<Finding> = Vec::new();
    for r in raw["paragraphs"].as_array().into_iter().flatten() {
        let Some((index, p)) = paragraph(paras, &r["n"]) else {
            continue;
        };
        let mut text = r["text"].as_str().unwrap_or("").trim();
        // The model may repeat the number and the speaker it was shown.
        if let Some(rest) = text.strip_prefix('[').and_then(|t| t.split_once(']')) {
            if rest.0.trim().chars().all(|c| c.is_ascii_digit()) {
                text = rest.1.trim_start();
            }
        }
        if let Some(who) = &p.who {
            text = text
                .strip_prefix(&format!("{who}:"))
                .map(str::trim_start)
                .unwrap_or(text);
        }
        if text.is_empty() {
            continue;
        }
        let (ot, nt) = (tokens(&p.text), tokens(text));
        let old: Vec<&str> = ot.iter().map(|r| &p.text[r.clone()]).collect();
        let new: Vec<&str> = nt.iter().map(|r| &text[r.clone()]).collect();
        let hs = hunks(&old, &new);
        let changed: usize = hs.iter().map(|(a, b)| a.len().max(b.len())).sum();
        if hs.is_empty() || changed > (old.len() / 4).max(4) {
            continue; // unchanged, or rewritten rather than corrected
        }
        for (a, b) in hs {
            if a.clone()
                .any(|i| old[i].chars().any(char::is_uppercase) && !old[i].chars().any(char::is_lowercase))
            {
                continue; // a name in capitals
            }
            // A word of context either side, widened until it's unique on its line.
            let (mut ca, mut cb) = (1, 1);
            let found = loop {
                let (oa, ob) = (a.start.saturating_sub(ca), (a.end + cb).min(old.len()));
                let (na, nb) = (b.start - (a.start - oa), b.end + (ob - a.end));
                if oa >= ob {
                    break None;
                }
                let (from, to) = (ot[oa].start, ot[ob - 1].end);
                let Some((line, col)) = p.locate(from, to) else {
                    break None;
                };
                let quote = &p.text[from..to];
                let suggestion = if na < nb {
                    &text[nt[na].start..nt[nb - 1].end]
                } else {
                    ""
                };
                let unique = lines[line].matches(quote).count() == 1;
                if unique || (oa == 0 && ob == old.len()) {
                    debug_assert_eq!(&lines[line][col..col + quote.len()], quote);
                    break Some((line, quote.to_string(), suggestion.to_string()));
                }
                (ca, cb) = (ca + 1, cb + 1);
            };
            let Some((line, quote, suggestion)) = found else {
                continue;
            };
            let was = old[a.clone()].join(" ");
            let now = new[b.clone()].join(" ");
            let message = match (was.is_empty(), now.is_empty()) {
                (true, _) => format!("Add \u{201c}{now}\u{201d}?"),
                (_, true) => format!("Remove \u{201c}{was}\u{201d}?"),
                _ => format!("\u{201c}{was}\u{201d} \u{2192} \u{201c}{now}\u{201d}?"),
            };
            if !out.iter().any(|o| o.quote == quote && o.line == line) {
                out.push(Finding {
                    quote,
                    line,
                    message,
                    suggestion,
                    change: Some(Change {
                        was,
                        now,
                        paragraph: index,
                    }),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn changes_become_findings() {
        let body = "INT. HALL - DAY\n\nFour men carries a plain coffin.\nA dog and a cat by a open door.\n\nDEE\nThou art come.\n\nJOHN F W watches.\n\nRain falls on the roof.";
        let paras = paragraphs(body);
        let raw = json!({ "paragraphs": [
            // Two corrections, one needing more context to be unique on its line.
            { "n": 1, "text": "Four men carry a plain coffin. A dog and a cat by an open door." },
            // Unchanged; the number and speaker repeated.
            { "n": 2, "text": "[2] DEE: Thou art come." },
            // A name in capitals: left alone.
            { "n": 3, "text": "JOHN FRANCIS watches." },
            // Rewritten, not corrected: dropped.
            { "n": 4, "text": "Heavy rain hammers down upon the old slate tiles." },
            { "n": 9, "text": "Invented." }
        ]});
        let f = place(body, &paras, &raw);
        let got: Vec<(usize, &str, &str, &str)> = f
            .iter()
            .map(|f| (f.line, f.quote.as_str(), f.suggestion.as_str(), f.message.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    2,
                    "men carries a",
                    "men carry a",
                    "\u{201c}carries\u{201d} \u{2192} \u{201c}carry\u{201d}?"
                ),
                (
                    3,
                    "by a open",
                    "by an open",
                    "\u{201c}a\u{201d} \u{2192} \u{201c}an\u{201d}?"
                ),
            ]
        );
        // An added or removed word.
        let paras = paragraphs("The ADMIRAL stamps papers.\n\nShe turns away upon to the sight.");
        let raw = json!({ "paragraphs": [
            { "n": 1, "text": "The ADMIRAL stamps the papers." },
            { "n": 2, "text": "She turns away upon the sight." }
        ]});
        let f = place(
            "The ADMIRAL stamps papers.\n\nShe turns away upon to the sight.",
            &paras,
            &raw,
        );
        assert_eq!(
            (f[0].quote.as_str(), f[0].suggestion.as_str()),
            ("stamps papers", "stamps the papers")
        );
        assert_eq!(f[0].message, "Add \u{201c}the\u{201d}?");
        assert_eq!(
            (f[1].quote.as_str(), f[1].suggestion.as_str(), f[1].line),
            ("upon to the", "upon the", 2)
        );
    }

    #[test]
    fn the_model_sees_action_and_speeches() {
        let body = "INT. HALL - DAY\n\nRain.\n!Wind.\n\nDEE\n(quietly)\nThou art come.\nSit.\n\n> CUT TO:\n\n~ SONG \"Hymn\" [JANE]\n~ Abide with me\n~ Fast falls\n~ END SONG";
        let p = paragraphs(body);
        type Row<'a> = (Option<&'a str>, &'a str, Vec<(usize, usize, usize)>);
        let got: Vec<Row> = p
            .iter()
            .map(|p| (p.who.as_deref(), p.text.as_str(), p.lines.clone()))
            .collect();
        assert_eq!(
            got,
            vec![
                (None, "Rain. Wind.", vec![(2, 0, 0), (3, 6, 1)]),
                (Some("DEE"), "Thou art come. Sit.", vec![(7, 0, 0), (8, 15, 0)]),
                (
                    Some("JANE"),
                    "Abide with me / Fast falls",
                    vec![(13, 0, 2), (14, 16, 2)]
                ),
            ]
        );
        assert_eq!(p[0].locate(6, 11), Some((3, 1)));
        assert_eq!(p[0].locate(0, 11), None, "across lines");
    }
}
