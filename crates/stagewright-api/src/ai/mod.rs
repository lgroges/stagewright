//! AI review of a scene with a local model (Ollama), on top of the rule-based
//! script check.
//!
//! The review is optional and never in the way: it runs only when asked (the
//! Review scene button), off the workspace lock (see `dispatch_free`), and any
//! failure (Ollama not running, the model missing, a reply that isn't the
//! expected JSON) comes back as `{ok: false, reason}` rather than an error, so the
//! rule checks carry on regardless. Findings are cached per scene in
//! `.stagewright/ai/`, keyed by the model, the prompt and the text.
//!
//! First review: grammar, typos and wrong words, in dialogue as well as action,
//! leaving period language and characters' own ways of speaking alone.

pub mod claude;

use crate::{ApiError, ApiResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use stagewright_core::check::{Edit, Fix, Issue, Severity};
use stagewright_core::project::{Character, ProjectMeta};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Bump when the prompt or schema changes, so cached reviews are redone.
const PROMPT_VERSION: u32 = 6;
pub const DEFAULT_URL: &str = "http://127.0.0.1:11434";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// The words with the mistake, exactly as in the scene.
    pub quote: String,
    /// Body line (from 0) the quote was found on.
    #[serde(default)]
    pub line: usize,
    /// What's wrong.
    pub message: String,
    /// The quote corrected; empty when there's no simple fix.
    #[serde(default)]
    pub suggestion: String,
    /// What was changed, word for word (before, after, and the paragraph), for
    /// asking the model why. Not kept.
    #[serde(skip)]
    pub change: Option<(String, String, String)>,
}

fn agent(read: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(2)))
        .timeout_global(Some(read))
        .http_status_as_error(false)
        // Ollama is on this computer (or the local network): never via a proxy.
        .proxy(None)
        .build()
        .into()
}

fn base(url: Option<&str>) -> String {
    url.filter(|u| !u.trim().is_empty())
        .unwrap_or(DEFAULT_URL)
        .trim_end_matches('/')
        .to_string()
}

/// Why Ollama couldn't be reached, in words for the writer.
fn unreachable(url: &str, e: &ureq::Error) -> String {
    match e {
        ureq::Error::Io(_) | ureq::Error::ConnectionFailed | ureq::Error::HostNotFound => {
            format!("Ollama isn't answering at {url}. Is it running?")
        }
        ureq::Error::Timeout(_) => format!("Ollama at {url} took too long to answer. Is it running?"),
        e => format!("Couldn't reach Ollama at {url} ({e}). Is it running?"),
    }
}

fn unavailable(reason: impl Into<String>) -> ApiResult {
    Ok(json!({ "ok": false, "reason": reason.into() }))
}

/// Whether the AI is ready: for `provider: claude`, whether there's a key the
/// API accepts; otherwise whether Ollama answers at `url`, and its models.
pub fn status(args: &Value) -> ApiResult {
    if args.get("provider").and_then(Value::as_str) == Some("claude") {
        return claude::status(&claude::Claude::from_env());
    }
    ollama_status(args)
}

fn ollama_status(args: &Value) -> ApiResult {
    let url = base(args.get("url").and_then(Value::as_str));
    let mut res = match agent(Duration::from_secs(4)).get(format!("{url}/api/tags")).call() {
        Ok(r) => r,
        Err(e) => return unavailable(unreachable(&url, &e)),
    };
    let v: Value = match res.body_mut().read_json() {
        Ok(v) => v,
        Err(e) => return unavailable(format!("Ollama at {url} gave an unexpected reply ({e}).")),
    };
    let models: Vec<String> = v["models"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m["name"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({ "ok": true, "url": url, "models": models }))
}

/// The model to use: the one asked for, else the best installed one for this
/// (Qwen 3.6, then Qwen 3's 30B, which is quick on a laptop, then others), else the first.
pub fn pick_model(asked: Option<&str>, installed: &[String]) -> Option<String> {
    if let Some(m) = asked.filter(|m| !m.is_empty()) {
        return Some(m.to_string());
    }
    const LIKED: &[&str] = &["qwen3.6", "qwen3:30b", "qwen3:", "gemma3:27b", "gemma3:12b"];
    LIKED
        .iter()
        .find_map(|p| installed.iter().find(|m| m.starts_with(p)))
        .or_else(|| installed.first())
        .cloned()
}

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

/// A scene is sent a few paragraphs at a time: a short answer is quicker to
/// get right, and a model that starts repeating itself can't go on for long.
pub fn chunks(paras: &[Paragraph]) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let (mut start, mut chars) = (0, 0);
    for (i, p) in paras.iter().enumerate() {
        if i > start && (i - start == 4 || chars + p.text.len() > 1600) {
            out.push(start..i);
            (start, chars) = (i, 0);
        }
        chars += p.text.len();
    }
    if start < paras.len() {
        out.push(start..paras.len());
    }
    out
}

const SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "paragraphs": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "n": {"type": "integer"},
          "text": {"type": "string"}
        },
        "required": ["n", "text"]
      }
    }
  },
  "required": ["paragraphs"]
}"#;

fn format_name(meta: &ProjectMeta) -> &'static str {
    use stagewright_core::layout::Format;
    match meta.format {
        Format::Stage => "a stage play",
        Format::Film => "a screenplay",
        Format::TvSingle => "a TV drama script",
        Format::TvMulti => "a TV comedy script",
    }
}

/// What the model is told about the scene beyond its text.
#[derive(Debug, Default)]
pub struct About<'a> {
    pub synopsis: Option<&'a str>,
    pub cast: &'a [Character],
}

/// The instructions and some of the scene (paragraphs `range`, numbered from 1
/// for the whole scene), as chat messages.
///
/// The model gives each paragraph back corrected, and Stagewright finds what it
/// changed. Tried on Qwen 3 30B against a scene with four planted mistakes, this
/// found three or four of them in each run and changed nothing in the scene
/// without; asked instead to list the errors, it found fewer, invented some, and
/// asked about the whole scene at once, returned nothing.
pub fn prompt(meta: &ProjectMeta, about: &About, paras: &[Paragraph], range: std::ops::Range<usize>) -> Value {
    let english = format!("{:?}", meta.dialect);
    let system = format!(
        // Word for word as tested: small changes here change what the model finds.
        "You are a careful copy editor for {kind} in {english} English. You will get some of the \
scene's action and dialogue as numbered paragraphs. Return each paragraph, with its number, corrected \
for grammar mistakes, typos, missing or doubled words and wrong words, and otherwise exactly as \
written: change nothing else. Leave alone: period or archaic language (thee, thou, hath, saith, \
cometh, \"we be in death\") unless it is misspelt even for its period; how a character speaks; \
sentence fragments in action (scripts are written that way); names, and words in capitals; \
{english} spelling.\nAnswer only with JSON matching this schema: {SCHEMA}",
        kind = format_name(meta),
    );
    let user = format!("{}\n\n{}", header(meta, about), numbered(paras, range));
    json!([
        { "role": "system", "content": system },
        { "role": "user", "content": user },
    ])
}

/// What the model is told about the script and the scene, before the text.
fn header(meta: &ProjectMeta, about: &About) -> String {
    let mut out = format!("The script: {}", meta.title);
    if let Some(sub) = meta.subtitle.as_deref().filter(|s| !s.trim().is_empty()) {
        out.push_str(&format!(" ({sub})"));
    }
    out.push('.');
    if let Some(syn) = about.synopsis.map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str(&format!(" This scene: {}", syn.replace('\n', " ")));
    }
    out.push_str(" Judge from the script and the scene when and where it is set.");
    // Ordinary-case names only: capitals are left alone anyway.
    let names: Vec<&str> = about
        .cast
        .iter()
        .filter_map(|c| c.short.as_deref().or(c.name.as_deref()))
        .filter(|n| n.chars().any(char::is_lowercase))
        .collect();
    if !names.is_empty() {
        out.push_str(&format!("\nCharacters: {}.", names.join(", ")));
    }
    out
}

/// Paragraphs `range`, numbered for the whole scene: "[3] DEE: Thou art come."
fn numbered(paras: &[Paragraph], range: std::ops::Range<usize>) -> String {
    range
        .map(|n| {
            let p = &paras[n];
            format!(
                "[{}] {}{}",
                n + 1,
                p.who.as_ref().map(|w| format!("{w}: ")).unwrap_or_default(),
                p.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
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

/// What the model changed in each paragraph, as findings on the body: each
/// change with a word either side (more if needed to tell it apart on its line).
/// A paragraph rewritten rather than corrected, changes to words in capitals
/// (names), and changes that run across lines are dropped.
pub fn place(body: &str, paras: &[Paragraph], raw: &Value) -> Vec<Finding> {
    let lines: Vec<&str> = body.split('\n').collect();
    let mut out: Vec<Finding> = Vec::new();
    for r in raw["paragraphs"].as_array().into_iter().flatten() {
        let Some(p) = r["n"].as_u64().and_then(|n| paras.get((n as usize).checked_sub(1)?)) else {
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
                    change: Some((was, now, p.text.clone())),
                });
            }
        }
    }
    out
}

/// Enough context for a chunk and its answer (Ollama's default, 4,096 tokens,
/// can be too small), and a cap on the answer: about the length of the text.
fn sizes(messages: &Value, text_chars: usize) -> (u32, u32) {
    let chars: usize = messages
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["content"].as_str())
        .map(str::len)
        .sum();
    let predict = (text_chars / 2 + 200) as u32;
    let ctx = ((chars / 3) as u32 + predict).next_power_of_two().clamp(8192, 32768);
    (ctx, predict)
}

/// The findings as script-check issues on `body` as it is now: each one found
/// again by its quote (nearest the line it was on), with a fix that puts the
/// suggestion in. Findings whose words have since been edited away are dropped,
/// as are ones a rule already flags in the same place.
pub fn issues(body: &str, findings: &[Finding], existing: &[Issue]) -> Vec<Issue> {
    let lines: Vec<&str> = body.split('\n').collect();
    let mut out = Vec::new();
    for f in findings {
        if f.quote.is_empty() {
            continue;
        }
        let Some((line, at)) = lines
            .iter()
            .enumerate()
            .filter_map(|(n, l)| l.find(&f.quote).map(|at| (n, at)))
            .min_by_key(|(n, _)| n.abs_diff(f.line))
        else {
            continue;
        };
        let text = lines[line];
        let from = text[..at].chars().count();
        let to = from + f.quote.chars().count();
        let covered = existing.iter().any(|i| {
            i.line == line
                && matches!(i.code, "grammar" | "repeated-word")
                && (i.from == i.to || (i.from < to && from < i.to))
        });
        if covered {
            continue;
        }
        let fix = (!f.suggestion.is_empty()).then(|| Fix {
            label: format!("Change to \u{201c}{}\u{201d}", f.suggestion),
            edits: vec![Edit::Replace {
                line,
                text: format!("{}{}{}", &text[..at], f.suggestion, &text[at + f.quote.len()..]),
            }],
        });
        out.push(Issue {
            code: "ai-grammar",
            severity: Severity::Info,
            line,
            from,
            to,
            message: f.message.clone(),
            fix,
        });
    }
    out
}

fn cached(root: &Path, scene: &str, key: &str) -> Option<Value> {
    std::fs::read_to_string(cache_path(root, scene))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(|v| v["key"] == key)
}

/// Keep a review. A convenience: one that can't be saved is still shown.
fn keep(root: &Path, scene: &str, key: &str, model: &str, findings: &[Finding]) {
    let cache = cache_path(root, scene);
    if crate::ops::collab_dir(root).is_ok() {
        if let Some(dir) = cache.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(
            &cache,
            json!({ "key": key, "model": model, "findings": findings }).to_string(),
        );
    }
}

/// The review with Claude: only for a project that allows its text to go to
/// Anthropic.
fn review_claude(
    root: &Path,
    scene: &str,
    body: &str,
    project: &stagewright_core::project::Project,
    model: Option<&str>,
    claude_conn: impl FnOnce() -> claude::Claude,
) -> ApiResult {
    let meta = &project.meta;
    if !meta.ai.cloud {
        return Ok(json!({
            "ok": false,
            "needs_consent": true,
            "reason": "Reviewing with Claude sends this project's scenes to Anthropic. Allow it for this project?",
        }));
    }
    let model = model.filter(|m| !m.is_empty()).unwrap_or(claude::DEFAULT_MODEL);
    let paras = paragraphs(body);
    if paras.is_empty() {
        return Ok(json!({ "ok": true, "findings": [], "cached": false, "model": model }));
    }
    let synopsis = project
        .scenes
        .iter()
        .find(|s| s.scene.meta.id == scene)
        .and_then(|s| s.scene.meta.synopsis.as_deref());
    let about = About {
        synopsis,
        cast: &project.characters,
    };
    let request = claude::body(model, meta, &about, &paras);
    let key = cache_key(&format!("claude:{model}"), &request);
    if let Some(hit) = cached(root, scene, &key) {
        return Ok(json!({ "ok": true, "findings": hit["findings"], "cached": true, "model": model }));
    }
    match claude::review(&claude_conn(), model, body, &paras, &request) {
        Ok((findings, cost)) => {
            keep(root, scene, &key, model, &findings);
            Ok(json!({ "ok": true, "findings": findings, "cached": false, "model": model, "cost_usd": cost }))
        }
        Err(reason) => unavailable(reason),
    }
}

fn cache_path(root: &Path, scene: &str) -> PathBuf {
    let safe: String = scene
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect();
    root.join(".stagewright").join("ai").join(format!("{safe}.json"))
}

fn cache_key(model: &str, messages: &Value) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (PROMPT_VERSION, model, messages.to_string()).hash(&mut h);
    format!("{:016x}", h.finish())
}

/// One request: the model's answer as JSON, or why there isn't one.
const REASONS_SCHEMA: &str = r#"{"type":"object","properties":{"reasons":{"type":"array","items":{"type":"object","properties":{"n":{"type":"integer"},"why":{"type":"string"}},"required":["n","why"]}}},"required":["reasons"]}"#;

/// The request for the reasons behind the changes: a separate, short one, so
/// the corrections themselves are asked for exactly as tested.
pub fn reasons_prompt(changes: &[(String, String, String)]) -> Value {
    let system = format!(
        "You explain a copy editor's suggested changes to a script's writer. For each numbered change, \
say in a few plain words what is wrong with the original words, as a writer would put it. Examples: \
“‘recieve’ is misspelt.” “The subject, ‘the actors’, is plural.” \
“‘their’ should be ‘there’, the place.” “A word is repeated.” \
“‘Off of’ is informal; ‘off’ is enough.” If the original seems fine to you, say \
“May be fine as written.” and why.\nAnswer only with JSON matching this schema: {REASONS_SCHEMA}"
    );
    let user: Vec<String> = changes
        .iter()
        .enumerate()
        .map(|(i, (was, now, para))| {
            format!(
                "[{}] In: {para}\nChange: \u{201c}{was}\u{201d} \u{2192} \u{201c}{now}\u{201d}",
                i + 1
            )
        })
        .collect();
    json!([
        { "role": "system", "content": system },
        { "role": "user", "content": user.join("\n\n") },
    ])
}

/// Add the model's reason to each finding. Best effort: without an answer, the
/// findings stand as they are.
fn explain(url: &str, model: &str, findings: &mut [Finding]) {
    for batch in findings.chunks_mut(12) {
        let changes: Vec<(String, String, String)> =
            batch.iter().map(|f| f.change.clone().unwrap_or_default()).collect();
        let messages = reasons_prompt(&changes);
        let Ok(raw) = ask_with(url, model, &messages, 60 * changes.len() as u32 + 100, REASONS_SCHEMA) else {
            return;
        };
        for r in raw["reasons"].as_array().into_iter().flatten() {
            let why = r["why"].as_str().unwrap_or("").trim();
            if let Some(f) = r["n"]
                .as_u64()
                .and_then(|n| batch.get_mut((n as usize).checked_sub(1)?))
            {
                if !why.is_empty() && !f.message.ends_with('.') {
                    let why = if why.ends_with(['.', '!', '?']) {
                        why.to_string()
                    } else {
                        format!("{why}.")
                    };
                    f.message = format!("{} {why}", f.message);
                }
            }
        }
    }
}

fn ask(url: &str, model: &str, messages: &Value, text_chars: usize) -> Result<Value, String> {
    let predict = sizes(messages, text_chars).1;
    ask_with(url, model, messages, predict, SCHEMA)
}

fn ask_with(url: &str, model: &str, messages: &Value, predict: u32, schema: &str) -> Result<Value, String> {
    let chars: usize = messages
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["content"].as_str())
        .map(str::len)
        .sum();
    let ctx = ((chars / 3) as u32 + predict).next_power_of_two().clamp(8192, 32768);
    let mut request = json!({
        "model": model,
        "messages": messages,
        "stream": false,
        // Qwen thinks before answering by default: much slower, and not needed here.
        "think": false,
        "format": serde_json::from_str::<Value>(schema).expect("schema is JSON"),
        "options": { "temperature": 0, "num_ctx": ctx, "num_predict": predict },
    });
    let reply = loop {
        // On a laptop the first call loads the model, which can take a minute.
        let mut res = match agent(Duration::from_secs(600))
            .post(format!("{url}/api/chat"))
            .send_json(&request)
        {
            Ok(r) => r,
            Err(ureq::Error::Timeout(_)) => {
                return Err(format!("{model} took more than ten minutes; try a smaller model."))
            }
            Err(e) => return Err(unreachable(url, &e)),
        };
        let status_code = res.status().as_u16();
        let reply: Value = res
            .body_mut()
            .read_json()
            .map_err(|e| format!("Ollama gave an unexpected reply ({e})."))?;
        if status_code == 200 {
            break reply;
        }
        let why = reply["error"].as_str().unwrap_or("unknown error");
        // An older Ollama, or a model that can't be told not to think: ask again without.
        if why.contains("think") && request.as_object_mut().and_then(|r| r.remove("think")).is_some() {
            continue;
        }
        return Err(format!("Ollama couldn't run {model}: {why}"));
    };
    let content = reply["message"]["content"].as_str().unwrap_or("");
    serde_json::from_str(content)
        .map_err(|_| format!("{model} didn't answer in the expected form; try again or another model."))
}

/// Review one scene. Args: `root`, `scene` (id), `body` (the text as in the
/// editor), `provider` (`ollama`, the default, or `claude`), optional `model`,
/// and Ollama's `url`.
pub fn review(args: &Value) -> ApiResult {
    review_with(args, claude::Claude::from_env)
}

/// Review one scene. Args: `root`, `scene` (id), `body` (the text as in the
/// editor), optional `model` and `url`.
pub fn review_with(args: &Value, claude_conn: impl FnOnce() -> claude::Claude) -> ApiResult {
    let s = |k: &str| args.get(k).and_then(Value::as_str);
    let root = PathBuf::from(s("root").ok_or_else(|| ApiError::new("bad_args", "missing 'root'"))?);
    let scene = s("scene").ok_or_else(|| ApiError::new("bad_args", "missing 'scene'"))?;
    let body = s("body").unwrap_or("").replace("\r\n", "\n");
    if !root.join("project.yaml").is_file() {
        return Err(ApiError::new("bad_args", "not a project folder"));
    }
    let project =
        stagewright_core::project::Project::load(&root).map_err(|e| ApiError::new("project", e.to_string()))?;
    let meta = &project.meta;
    if meta.checks.off.iter().any(|c| c == "ai-grammar") {
        return Ok(
            json!({ "ok": false, "off": true, "reason": "The AI review is switched off for this project (Production → Checks)." }),
        );
    }
    if body.trim().is_empty() {
        return Ok(json!({ "ok": true, "findings": [], "cached": false, "model": Value::Null }));
    }
    if s("provider") == Some("claude") {
        return review_claude(&root, scene, &body, &project, s("model"), claude_conn);
    }
    let url = base(s("url"));
    // The model: as asked, or the best installed one.
    let model = match pick_model(s("model"), &[]) {
        Some(m) => m,
        None => {
            let st = ollama_status(&json!({ "url": url }))?;
            if st["ok"] != true {
                return Ok(st);
            }
            let installed: Vec<String> = serde_json::from_value(st["models"].clone()).unwrap_or_default();
            match pick_model(None, &installed) {
                Some(m) => m,
                None => return unavailable("Ollama has no models installed. Try: ollama pull qwen3.6"),
            }
        }
    };
    let paras = paragraphs(&body);
    if paras.is_empty() {
        return Ok(json!({ "ok": true, "findings": [], "cached": false, "model": model }));
    }
    let synopsis = project
        .scenes
        .iter()
        .find(|s| s.scene.meta.id == scene)
        .and_then(|s| s.scene.meta.synopsis.as_deref());
    let about = About {
        synopsis,
        cast: &project.characters,
    };
    let parts: Vec<(Value, usize)> = chunks(&paras)
        .into_iter()
        .map(|r| {
            let chars = paras[r.clone()].iter().map(|p| p.text.len()).sum();
            (prompt(meta, &about, &paras, r), chars)
        })
        .collect();
    let key = cache_key(&model, &Value::Array(parts.iter().map(|(m, _)| m.clone()).collect()));
    if let Some(hit) = cached(&root, scene, &key) {
        return Ok(json!({ "ok": true, "findings": hit["findings"], "cached": true, "model": model }));
    }
    let mut findings = Vec::new();
    for (messages, chars) in &parts {
        let raw = match ask(&url, &model, messages, *chars) {
            Ok(raw) => raw,
            Err(reason) => return unavailable(reason),
        };
        findings.extend(place(&body, &paras, &raw));
    }
    if !findings.is_empty() {
        explain(&url, &model, &mut findings);
    }
    keep(&root, scene, &key, &model, &findings);
    Ok(json!({ "ok": true, "findings": findings, "cached": false, "model": model }))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let many: Vec<Paragraph> = (0..20).map(|_| p[0].clone()).collect();
        assert_eq!(chunks(&many), vec![0..4, 4..8, 8..12, 12..16, 16..20]);
    }

    #[test]
    fn findings_follow_the_text() {
        let f = vec![Finding {
            quote: "upon to the sight".into(),
            line: 2,
            message: "Extra word.".into(),
            suggestion: "at the sight".into(),
            change: None,
        }];
        // A line added above: found one line further down, with its fix.
        let body = "INT. HALL - DAY\n\nRain.\nJoanna turns away upon to the sight.";
        let i = issues(body, &f, &[]);
        assert_eq!((i[0].line, i[0].from, i[0].to), (3, 18, 35));
        assert_eq!(
            i[0].fix.as_ref().unwrap().edits,
            vec![Edit::Replace {
                line: 3,
                text: "Joanna turns away at the sight.".into()
            }]
        );
        // Edited away, or already flagged by a rule: not shown.
        assert!(issues("Joanna turns away.", &f, &[]).is_empty());
        let rule = Issue {
            code: "grammar",
            severity: Severity::Warning,
            line: 3,
            from: 25,
            to: 30,
            message: String::new(),
            fix: None,
        };
        assert!(issues(body, &f, &[rule]).is_empty());
    }

    #[test]
    fn models_and_prompt() {
        let installed = vec!["llama3.2:3b".to_string(), "qwen3.6:27b".to_string()];
        assert_eq!(pick_model(None, &installed).as_deref(), Some("qwen3.6:27b"));
        assert_eq!(pick_model(Some("gemma4"), &installed).as_deref(), Some("gemma4"));
        assert_eq!(pick_model(None, &[]), None);
        let meta: ProjectMeta = serde_yaml::from_str("title: T\nformat: tv-single\n").unwrap();
        let m = prompt(
            &meta,
            &About {
                synopsis: Some("A funeral."),
                cast: &[],
            },
            &paragraphs("Rain."),
            0..1,
        );
        assert!(m[0]["content"]
            .as_str()
            .unwrap()
            .contains("a TV drama script in British English"));
        let user = m[1]["content"].as_str().unwrap();
        assert!(
            user.contains("This scene: A funeral.") && user.ends_with("\n\n[1] Rain."),
            "{user}"
        );
        assert_eq!(sizes(&m, 5), (8192, 202));
    }

    /// A stand-in for Ollama that answers each request in turn with the given
    /// status and body, and hands back the request bodies it got.
    fn fake(replies: Vec<(u16, String)>) -> (String, std::sync::mpsc::Receiver<String>) {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for (status, body) in replies {
                let (mut sock, _) = listener.accept().unwrap();
                let mut r = BufReader::new(sock.try_clone().unwrap());
                let mut len = 0;
                let mut key = String::new();
                loop {
                    let mut l = String::new();
                    r.read_line(&mut l).unwrap();
                    if let Some(v) = l.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                    if let Some(v) = l.to_ascii_lowercase().strip_prefix("x-api-key:") {
                        key = v.trim().to_string();
                    }
                    if l == "\r\n" || l.is_empty() {
                        break;
                    }
                }
                let mut got = vec![0; len];
                r.read_exact(&mut got).unwrap();
                let got = String::from_utf8(got).unwrap();
                // A request with an API key: the key, a tab, then the body.
                tx.send(if key.is_empty() { got } else { format!("{key}\t{got}") })
                    .unwrap();
                let _ = write!(
                    sock,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        (url, rx)
    }

    fn project() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("project.yaml"), "title: T\nformat: film\n").unwrap();
        std::fs::write(
            d.path().join("characters.yaml"),
            "- {id: joanna, cue: JOANNA, name: Joanna Weston}\n",
        )
        .unwrap();
        d
    }

    fn claude_at(url: &str, key: Option<&str>) -> claude::Claude {
        claude::Claude {
            url: url.into(),
            key: key.map(str::to_string),
            stored: false,
        }
    }

    #[test]
    fn claude_needs_the_projects_permission_and_a_key() {
        let d = project();
        let args = json!({ "root": d.path(), "scene": "sc-1", "body": "Rain falls.", "provider": "claude" });
        let v = review_with(&args, || panic!("not asked before permission")).unwrap();
        assert_eq!(
            (v["ok"].as_bool(), v["needs_consent"].as_bool()),
            (Some(false), Some(true))
        );
        std::fs::write(
            d.path().join("project.yaml"),
            "title: T\nformat: film\nai: {cloud: true}\n",
        )
        .unwrap();
        let v = review_with(&args, || claude_at("http://127.0.0.1:9", None)).unwrap();
        assert!(v["reason"].as_str().unwrap().contains("API key"), "{v}");
        // The option round-trips in project.yaml.
        let y = crate::checks::yaml_set_option("title: T\n", "ai", &json!({ "cloud": true })).unwrap();
        let meta: ProjectMeta = serde_yaml::from_str(&y).unwrap();
        assert!(meta.ai.cloud);
    }

    #[test]
    fn a_claude_review_end_to_end() {
        let d = project();
        std::fs::write(
            d.path().join("project.yaml"),
            "title: T\nformat: stage\nai: {cloud: true}\n",
        )
        .unwrap();
        let body = "The quay.\n\nHETTIE\nHes coming on the coach.\n\nTOBIAS straightens the sign, which fall down.";
        let answer = json!({ "paragraphs": [
            { "n": 2, "text": "He\u{2019}s coming on the coach.",
              "changes": [{ "from": "Hes", "to": "He\u{2019}s", "why": "missing apostrophe" }] },
            { "n": 3, "text": "TOBIAS straightens the sign, which falls down.",
              "changes": [{ "from": "which fall", "to": "which falls", "why": "the verb agrees with 'sign'" }] }
        ]});
        let reply = json!({
            "content": [{ "type": "thinking", "thinking": "" }, { "type": "text", "text": format!("```json\n{answer}\n```") }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 1000, "output_tokens": 500 }
        });
        let (url, got) = fake(vec![(200, reply.to_string())]);
        let args = json!({ "root": d.path(), "scene": "sc-1", "body": body, "provider": "claude" });
        let v = review_with(&args, || claude_at(&url, Some("sk-ant-test"))).unwrap();
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["model"], "claude-sonnet-5-5");
        assert!((v["cost_usd"].as_f64().unwrap() - 0.007).abs() < 1e-9);
        let msgs: Vec<&str> = v["findings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["message"].as_str().unwrap())
            .collect();
        assert_eq!(
            msgs,
            vec![
                "\u{201c}Hes\u{201d} \u{2192} \u{201c}He\u{2019}s\u{201d}? Missing apostrophe.",
                "\u{201c}fall\u{201d} \u{2192} \u{201c}falls\u{201d}? The verb agrees with 'sign'."
            ]
        );
        let sent = got.recv().unwrap();
        let (key, req) = sent.split_once('\t').unwrap();
        assert_eq!(key, "sk-ant-test");
        let req: Value = serde_json::from_str(req).unwrap();
        assert!(req["system"]
            .as_str()
            .unwrap()
            .starts_with("You are a careful copy editor for a stage play in British English."));
        assert!(req["messages"][0]["content"].as_str().unwrap().ends_with("[1] The quay.\n\n[2] HETTIE: Hes coming on the coach.\n\n[3] TOBIAS straightens the sign, which fall down."));
        // Again, unchanged: kept, not asked (nothing is listening any more).
        let again = review_with(&args, || claude_at("http://127.0.0.1:9", Some("sk-ant-test"))).unwrap();
        assert_eq!(again["cached"], true);
    }

    #[test]
    fn claude_failures_come_back_as_reasons() {
        let d = project();
        std::fs::write(d.path().join("project.yaml"), "title: T\nai: {cloud: true}\n").unwrap();
        let ask = |status: u16, body: Value| {
            let (url, _g) = fake(vec![(status, body.to_string())]);
            let args =
                json!({ "root": d.path(), "scene": "sc-1", "body": format!("Rain {status}."), "provider": "claude" });
            review_with(&args, || claude_at(&url, Some("sk-ant-x"))).unwrap()["reason"]
                .as_str()
                .unwrap()
                .to_string()
        };
        let err = |m: &str| json!({ "type": "error", "error": { "type": "x", "message": m } });
        assert!(ask(401, err("invalid x-api-key")).contains("wasn't accepted"));
        assert!(
            ask(400, err("Your credit balance is too low to access the Anthropic API.")).contains("credit has run out")
        );
        assert!(ask(529, err("Overloaded")).contains("busy"));
        assert!(ask(
            200,
            json!({ "content": [{ "type": "text", "text": "Sure! Here you go." }] })
        )
        .contains("expected form"));
        // Status: no key; a key that works; one that doesn't.
        assert_eq!(
            claude::status(&claude_at("http://127.0.0.1:9", None)).unwrap()["has_key"],
            false
        );
        let (url, _g) = fake(vec![(200, json!({ "data": [] }).to_string())]);
        assert_eq!(claude::status(&claude_at(&url, Some("sk-ant-x"))).unwrap()["ok"], true);
        let (url, _g) = fake(vec![(401, err("invalid x-api-key").to_string())]);
        let v = claude::status(&claude_at(&url, Some("sk-ant-x"))).unwrap();
        assert!(v["reason"].as_str().unwrap().contains("wasn't accepted"));
        // A key that isn't one.
        assert!(claude::set_key(&json!({ "key": "hello" })).is_err());
    }

    #[test]
    fn a_review_end_to_end() {
        let body = "INT. HALL - DAY\n\nJoanna turns away upon to the sight.";
        let content = json!({ "paragraphs": [{ "n": 1, "text": "Joanna turns away from the sight." }]}).to_string();
        let tags = json!({ "models": [{ "name": "llama3.2:3b" }, { "name": "qwen3.6:27b" }] }).to_string();
        let why = json!({ "reasons": [{ "n": 1, "why": "\u{2018}upon to\u{2019} is two prepositions" }] }).to_string();
        let (url, got) = fake(vec![
            (200, tags),
            (200, json!({ "message": { "content": content } }).to_string()),
            (200, json!({ "message": { "content": why } }).to_string()),
        ]);
        let d = project();
        let args = json!({ "root": d.path(), "scene": "sc-1", "body": body, "url": url });
        let v = review(&args).unwrap();
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(
            (v["model"].as_str(), v["cached"].as_bool()),
            (Some("qwen3.6:27b"), Some(false))
        );
        assert_eq!(v["findings"][0]["line"], 2);
        assert_eq!(
            v["findings"][0]["message"],
            "\u{201c}upon to\u{201d} \u{2192} \u{201c}from\u{201d}? \u{2018}upon to\u{2019} is two prepositions."
        );
        let _ = got.recv().unwrap(); // the tags
        let asked: Value = serde_json::from_str(&got.recv().unwrap()).unwrap();
        assert_eq!(
            (
                asked["model"].as_str(),
                asked["think"].as_bool(),
                asked["stream"].as_bool()
            ),
            (Some("qwen3.6:27b"), Some(false), Some(false))
        );
        assert!(asked["messages"][1]["content"]
            .as_str()
            .unwrap()
            .contains("Characters: Joanna Weston."));
        assert!(asked["options"]["num_ctx"].as_u64().unwrap() >= 8192);
        // Then why: the change, with its paragraph.
        let asked: Value = serde_json::from_str(&got.recv().unwrap()).unwrap();
        assert_eq!(
            asked["messages"][1]["content"],
            "[1] In: Joanna turns away upon to the sight.\nChange: \u{201c}upon to\u{201d} \u{2192} \u{201c}from\u{201d}"
        );
        // The same text again with the same model: from the cache, no request made.
        let again = review(&json!({ "root": d.path(), "scene": "sc-1", "body": body, "url": "http://127.0.0.1:9", "model": "qwen3.6:27b" })).unwrap();
        assert_eq!(
            (again["cached"].as_bool(), again["findings"].clone()),
            (Some(true), v["findings"].clone())
        );
        assert!(d.path().join(".stagewright/.gitignore").is_file(), "kept out of git");
    }

    #[test]
    fn failures_come_back_as_reasons() {
        let d = project();
        let ask = |url: &str| {
            review(&json!({ "root": d.path(), "scene": "sc-1", "body": "Rain.", "url": url, "model": "qwen3.6" }))
                .unwrap()
        };
        let (url, _g) = fake(vec![(404, json!({ "error": "model 'qwen3.6' not found" }).to_string())]);
        let v = ask(&url);
        assert_eq!(v["ok"], false);
        assert!(v["reason"].as_str().unwrap().contains("not found"), "{v}");
        let (url, _g) = fake(vec![(
            200,
            json!({ "message": { "content": "Sure! Here are the errors…" } }).to_string(),
        )]);
        let v = ask(&url);
        assert!(v["reason"].as_str().unwrap().contains("expected form"), "{v}");
        // Told not to think by a model that can't: asked again without.
        let content = json!({ "paragraphs": [] }).to_string();
        let (url, got) = fake(vec![
            (
                400,
                json!({ "error": "\"qwen9\" does not support thinking" }).to_string(),
            ),
            (200, json!({ "message": { "content": content } }).to_string()),
        ]);
        let v = review(&json!({ "root": d.path(), "scene": "sc-2", "body": "Snow.", "url": url, "model": "qwen9" }))
            .unwrap();
        assert_eq!(v["ok"], true, "{v}");
        let _ = got.recv();
        assert!(serde_json::from_str::<Value>(&got.recv().unwrap())
            .unwrap()
            .get("think")
            .is_none());
        let (url, _g) = fake(vec![(200, "<html>".to_string())]);
        assert_eq!(ask(&url)["ok"], false);
        // No models at all.
        let (url, _g) = fake(vec![(200, json!({ "models": [] }).to_string())]);
        let v = review(&json!({ "root": d.path(), "scene": "sc-1", "body": "Rain.", "url": url })).unwrap();
        assert!(v["reason"].as_str().unwrap().contains("ollama pull"), "{v}");
        // Switched off for the project.
        std::fs::write(
            d.path().join("project.yaml"),
            "title: T\nchecks:\n  off: [ai-grammar]\n",
        )
        .unwrap();
        assert_eq!(ask("http://127.0.0.1:9")["off"], true);
    }

    #[test]
    fn unreachable_ollama_is_not_an_error() {
        // Nothing listens on port 9 (discard) on test machines.
        let v = status(&json!({ "url": "http://127.0.0.1:9" })).unwrap();
        assert_eq!(v["ok"], false);
        assert!(v["reason"].as_str().unwrap().contains("Is it running?"));
    }
}
