//! The AI copy-edit of a scene, on top of the rule-based script check: grammar,
//! typos and wrong words, in dialogue as well as action, leaving period language
//! and characters' own ways of speaking alone.
//!
//! Two providers: a local model through Ollama (`ollama`), or Claude through
//! Anthropic's API (`claude`), for a project that allows it. Both give paragraphs
//! back corrected, and `text` turns what they changed into findings.
//!
//! The review is optional and never in the way: it runs only when asked (the
//! Review scene button), off the workspace lock (see `free_command`), and any
//! failure (the AI not running or not reachable, a reply that isn't the expected
//! JSON) comes back as `{ok: false, reason}` rather than an error, so the rule
//! checks carry on regardless. Reviews are kept per scene in `.stagewright/ai/`,
//! keyed by the provider, the model, the prompt and the text.

pub mod claude;
mod ollama;
mod text;

use crate::{ApiError, ApiResult};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use stagewright_core::check::{Edit, Fix, Issue, Severity};
use stagewright_core::project::{Character, Project, ProjectMeta};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use text::{paragraphs, Paragraph};

/// Bump when the prompts or schemas change, so kept reviews are redone.
const PROMPT_VERSION: u32 = 6;

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
    /// What the model changed, for finding its reason. Not kept.
    #[serde(skip)]
    pub change: Option<Change>,
}

/// What a model changed, word for word, and in which paragraph (from 0).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Change {
    pub was: String,
    pub now: String,
    pub paragraph: usize,
}

impl Finding {
    /// Put the model's reason after the change: "“a” → “an”? An goes before a vowel."
    fn add_reason(&mut self, why: &str) {
        let why = why.trim();
        let mut chars = why.chars();
        let Some(first) = chars.next() else { return };
        let mut why: String = first.to_uppercase().chain(chars).collect();
        if !why.ends_with(['.', '!', '?']) {
            why.push('.');
        }
        self.message = format!("{} {why}", self.message);
    }
}

/// Why a review ended early: a bad request (an error), or an answer for the
/// writer, such as `{ok: false, reason}` (not an error: the checks carry on).
enum Stop {
    Error(ApiError),
    Answer(Value),
}

impl From<ApiError> for Stop {
    fn from(e: ApiError) -> Stop {
        Stop::Error(e)
    }
}

/// A reason the AI couldn't review the scene.
impl From<String> for Stop {
    fn from(reason: String) -> Stop {
        Stop::Answer(json!({ "ok": false, "reason": reason }))
    }
}

/// Whether the AI is ready: for `provider: claude`, whether there's a key the
/// API accepts; otherwise whether Ollama answers at `url`, and its models.
pub fn status(args: &Value) -> ApiResult {
    if args.get("provider").and_then(Value::as_str) == Some("claude") {
        return claude::status(&claude::Claude::from_env());
    }
    ollama::status(args)
}

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

/// The kept review of `scene`, if it was of the same requests.
fn kept(root: &Path, scene: &str, key: &str) -> Option<Value> {
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

fn cache_path(root: &Path, scene: &str) -> PathBuf {
    let safe: String = scene
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect();
    root.join(".stagewright").join("ai").join(format!("{safe}.json"))
}

/// The key a review is kept under: the provider and model, the prompts' version
/// and everything sent.
fn cache_key(provider: &Provider, requests: &[Value]) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (PROMPT_VERSION, provider.tag(), Value::from(requests).to_string()).hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Where the review is done, and with which model.
enum Provider {
    Ollama { url: String, model: String },
    Claude { conn: claude::Claude, model: String },
}

impl Provider {
    fn model(&self) -> &str {
        match self {
            Provider::Ollama { model, .. } | Provider::Claude { model, .. } => model,
        }
    }

    fn tag(&self) -> String {
        match self {
            Provider::Ollama { model, .. } => model.clone(),
            Provider::Claude { model, .. } => format!("claude:{model}"),
        }
    }

    /// What's asked, one request per item: the cache key is made from these.
    fn requests(&self, meta: &ProjectMeta, about: &About, paras: &[Paragraph]) -> Vec<Value> {
        match self {
            Provider::Ollama { .. } => ollama::requests(meta, about, paras),
            Provider::Claude { model, .. } => vec![claude::body(model, meta, about, paras)],
        }
    }

    /// Send the requests: the findings, and what they cost (Claude), or why not.
    fn send(&self, requests: &[Value], body: &str, paras: &[Paragraph]) -> Result<(Vec<Finding>, Option<f64>), String> {
        match self {
            Provider::Ollama { url, model } => Ok((ollama::review(url, model, requests, body, paras)?, None)),
            Provider::Claude { conn, model } => claude::review(conn, model, body, paras, &requests[0]),
        }
    }
}

/// Review one scene. Args: `root`, `scene` (id), `body` (the text as in the
/// editor), `provider` (`ollama`, the default, or `claude`), optional `model`,
/// and Ollama's `url`.
pub fn review(args: &Value) -> ApiResult {
    review_with(args, claude::Claude::from_env)
}

/// `review`, with the Claude connection given (for tests).
pub fn review_with(args: &Value, claude_conn: impl FnOnce() -> claude::Claude) -> ApiResult {
    match run(args, claude_conn) {
        Ok(v) | Err(Stop::Answer(v)) => Ok(v),
        Err(Stop::Error(e)) => Err(e),
    }
}

fn run(args: &Value, claude_conn: impl FnOnce() -> claude::Claude) -> Result<Value, Stop> {
    let s = |k: &str| args.get(k).and_then(Value::as_str);
    let root = PathBuf::from(s("root").ok_or_else(|| ApiError::new("bad_args", "missing 'root'"))?);
    let scene = s("scene").ok_or_else(|| ApiError::new("bad_args", "missing 'scene'"))?;
    let body = s("body").unwrap_or("").replace("\r\n", "\n");
    if !root.join("project.yaml").is_file() {
        return Err(ApiError::new("bad_args", "not a project folder").into());
    }
    let project = Project::load(&root).map_err(|e| ApiError::new("project", e.to_string()))?;
    let meta = &project.meta;
    if meta.checks.off.iter().any(|c| c == "ai-grammar") {
        let reason = "The AI review is switched off for this project (Production → Checks).";
        return Err(Stop::Answer(json!({ "ok": false, "off": true, "reason": reason })));
    }
    let provider = if s("provider") == Some("claude") {
        if !meta.ai.cloud {
            let reason = "Reviewing with Claude sends this project's scenes to Anthropic. Allow it for this project?";
            return Err(Stop::Answer(
                json!({ "ok": false, "needs_consent": true, "reason": reason }),
            ));
        }
        let model = s("model")
            .filter(|m| !m.is_empty())
            .unwrap_or(claude::DEFAULT_MODEL)
            .to_string();
        Provider::Claude {
            conn: claude_conn(),
            model,
        }
    } else {
        let url = ollama::base(s("url"));
        let model = ollama::choose_model(&url, s("model"))?;
        Provider::Ollama { url, model }
    };
    let answer = |findings: &Value, cached: bool, cost: Option<f64>| json!({ "ok": true, "findings": findings, "cached": cached, "model": provider.model(), "cost_usd": cost });
    let paras = paragraphs(&body);
    if paras.is_empty() {
        return Ok(answer(&json!([]), false, None));
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
    let requests = provider.requests(meta, &about, &paras);
    let key = cache_key(&provider, &requests);
    if let Some(hit) = kept(&root, scene, &key) {
        return Ok(answer(&hit["findings"], true, None));
    }
    let (findings, cost) = provider.send(&requests, &body, &paras)?;
    keep(&root, scene, &key, provider.model(), &findings);
    Ok(answer(&json!(findings), false, cost))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

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

    /// A stand-in for Ollama that answers each request in turn with the given
    /// status and body, and hands back the request bodies it got.
    pub(crate) fn fake(replies: Vec<(u16, String)>) -> (String, std::sync::mpsc::Receiver<String>) {
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

    pub(crate) fn project() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("project.yaml"), "title: T\nformat: film\n").unwrap();
        std::fs::write(
            d.path().join("characters.yaml"),
            "- {id: joanna, cue: JOANNA, name: Joanna Weston}\n",
        )
        .unwrap();
        d
    }
}
