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

use crate::{ApiError, ApiResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use stagewright_core::check::{Edit, Fix, Issue, Severity};
use stagewright_core::project::{Character, ProjectMeta};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Bump when the prompt or schema changes, so cached reviews are redone.
const PROMPT_VERSION: u32 = 4;
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

/// Whether Ollama answers, and which models it has.
pub fn status(args: &Value) -> ApiResult {
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
    let mut user = format!("The script: {}", meta.title);
    if let Some(sub) = meta.subtitle.as_deref().filter(|s| !s.trim().is_empty()) {
        user.push_str(&format!(" ({sub})"));
    }
    user.push('.');
    if let Some(syn) = about.synopsis.map(str::trim).filter(|s| !s.is_empty()) {
        user.push_str(&format!(" This scene: {}", syn.replace('\n', " ")));
    }
    user.push_str(" Judge from the script and the scene when and where it is set.");
    // Ordinary-case names only: capitals are left alone anyway.
    let names: Vec<&str> = about
        .cast
        .iter()
        .filter_map(|c| c.short.as_deref().or(c.name.as_deref()))
        .filter(|n| n.chars().any(char::is_lowercase))
        .collect();
    if !names.is_empty() {
        user.push_str(&format!("\nCharacters: {}.", names.join(", ")));
    }
    let numbered: Vec<String> = range
        .map(|n| {
            let p = &paras[n];
            format!(
                "[{}] {}{}",
                n + 1,
                p.who.as_ref().map(|w| format!("{w}: ")).unwrap_or_default(),
                p.text
            )
        })
        .collect();
    user.push_str("\n\n");
    user.push_str(&numbered.join("\n\n"));
    json!([
        { "role": "system", "content": system },
        { "role": "user", "content": user },
    ])
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
fn ask(url: &str, model: &str, messages: &Value, text_chars: usize) -> Result<Value, String> {
    let (ctx, predict) = sizes(messages, text_chars);
    let mut request = json!({
        "model": model,
        "messages": messages,
        "stream": false,
        // Qwen thinks before answering by default: much slower, and not needed here.
        "think": false,
        "format": serde_json::from_str::<Value>(SCHEMA).expect("schema is JSON"),
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
/// editor), optional `model` and `url`.
pub fn review(args: &Value) -> ApiResult {
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
    let url = base(s("url"));
    // The model: as asked, or the best installed one.
    let model = match pick_model(s("model"), &[]) {
        Some(m) => m,
        None => {
            let st = status(&json!({ "url": url }))?;
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
    let cache = cache_path(&root, scene);
    if let Some(hit) = std::fs::read_to_string(&cache)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(|v| v["key"] == key.as_str())
    {
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
    // The cache is a convenience: a review that can't be saved is still shown.
    if crate::ops::collab_dir(&root).is_ok() {
        if let Some(dir) = cache.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(
            &cache,
            json!({ "key": key, "model": model, "findings": findings }).to_string(),
        );
    }
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
                loop {
                    let mut l = String::new();
                    r.read_line(&mut l).unwrap();
                    if let Some(v) = l.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                    if l == "\r\n" || l.is_empty() {
                        break;
                    }
                }
                let mut got = vec![0; len];
                r.read_exact(&mut got).unwrap();
                tx.send(String::from_utf8(got).unwrap()).unwrap();
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

    #[test]
    fn a_review_end_to_end() {
        let body = "INT. HALL - DAY\n\nJoanna turns away upon to the sight.";
        let content = json!({ "paragraphs": [{ "n": 1, "text": "Joanna turns away from the sight." }]}).to_string();
        let tags = json!({ "models": [{ "name": "llama3.2:3b" }, { "name": "qwen3.6:27b" }] }).to_string();
        let (url, got) = fake(vec![
            (200, tags),
            (200, json!({ "message": { "content": content } }).to_string()),
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
