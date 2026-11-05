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
const PROMPT_VERSION: u32 = 1;
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

/// The model to use: the one asked for, else a Qwen 3.6 if installed, else the first.
pub fn pick_model(asked: Option<&str>, installed: &[String]) -> Option<String> {
    if let Some(m) = asked.filter(|m| !m.is_empty()) {
        return Some(m.to_string());
    }
    installed
        .iter()
        .find(|m| m.starts_with("qwen3.6"))
        .or_else(|| installed.first())
        .cloned()
}

const SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "findings": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "quote": {"type": "string"},
          "problem": {"type": "string"},
          "suggestion": {"type": "string"}
        },
        "required": ["quote", "problem", "suggestion"]
      }
    }
  },
  "required": ["findings"]
}"#;

fn format_name(meta: &ProjectMeta) -> &'static str {
    use stagewright_core::layout::Format;
    match meta.format {
        Format::Stage => "a stage play",
        Format::Film => "a screenplay",
        Format::TvSingle => "a single-camera TV drama",
        Format::TvMulti => "a multi-camera TV comedy",
    }
}

/// The instructions and the scene, as chat messages.
pub fn prompt(meta: &ProjectMeta, cast: &[Character], body: &str) -> Value {
    let english = format!("{:?}", meta.dialect).to_lowercase();
    let names: Vec<String> = cast
        .iter()
        .flat_map(|c| [Some(c.cue.clone()), c.name.clone(), c.short.clone()])
        .flatten()
        .collect();
    let system = format!(
        "You are a careful copy editor for {kind} written in {english} English. \
Find grammar mistakes, typos, missing or doubled words and wrong words in the scene. \
\n\nReport only real errors. Do NOT report:\n\
- period or archaic language, in dialogue or in quoted texts (thee, thou, hath, saith, cometh), unless it is misspelt for its own period;\n\
- the way a character speaks (dialect, slang, fragments, deliberate repetition);\n\
- sentence fragments in action or stage directions (scripts are written that way);\n\
- names written in capitals, character cues, scene headings or formatting;\n\
- style preferences or rewording that isn't fixing an error;\n\
- spelling that is correct in {english} English.\n\n\
For each error give: \"quote\", a few words copied exactly from the scene, character for character, \
that contain the error (short, and unique in the scene if you can); \"problem\", one short sentence \
saying what's wrong; \"suggestion\", the quote corrected (or an empty string if there's no simple fix). \
If there are no errors, return an empty list. Answer only with JSON matching this schema: {SCHEMA}",
        kind = format_name(meta),
    );
    let mut user = format!("The script: {}", meta.title);
    if let Some(sub) = meta.subtitle.as_deref().filter(|s| !s.trim().is_empty()) {
        user.push_str(&format!(" ({sub})"));
    }
    user.push_str(". Judge from the scene when and where it is set.\n\n");
    if !names.is_empty() {
        user.push_str(&format!("Character names (not errors): {}.\n\n", names.join(", ")));
    }
    user.push_str("The scene:\n\n");
    user.push_str(body);
    json!([
        { "role": "system", "content": system },
        { "role": "user", "content": user },
    ])
}

/// Where the model's findings are in `body`: ones whose quote isn't there (the
/// model misquoted, or invented it) are dropped, as are non-fixes.
pub fn place(body: &str, raw: &Value) -> Vec<Finding> {
    let lines: Vec<&str> = body.split('\n').collect();
    let mut out: Vec<Finding> = Vec::new();
    for f in raw["findings"].as_array().into_iter().flatten() {
        let quote = f["quote"].as_str().unwrap_or("").trim();
        let message = f["problem"].as_str().unwrap_or("").trim();
        let mut suggestion = f["suggestion"].as_str().unwrap_or("").trim().to_string();
        if quote.is_empty() || message.is_empty() {
            continue;
        }
        let Some(line) = lines.iter().position(|l| l.contains(quote)) else {
            continue;
        };
        if suggestion == quote {
            suggestion.clear();
            // "Fine as it is" with nothing to change is the model second-guessing itself.
            if message.to_lowercase().contains("correct") {
                continue;
            }
        }
        if out.iter().any(|o| o.quote == quote) {
            continue;
        }
        out.push(Finding {
            quote: quote.to_string(),
            line,
            message: message.to_string(),
            suggestion,
        });
    }
    out
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
    let meta: ProjectMeta =
        serde_yaml::from_str(&std::fs::read_to_string(root.join("project.yaml")).unwrap_or_default())
            .map_err(|e| ApiError::new("invalid_yaml", e.to_string()))?;
    let cast: Vec<Character> = std::fs::read_to_string(root.join("characters.yaml"))
        .ok()
        .and_then(|t| serde_yaml::from_str::<Option<Vec<Character>>>(&t).ok().flatten())
        .unwrap_or_default();
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
    let messages = prompt(&meta, &cast, &body);
    let key = cache_key(&model, &messages);
    let cache = cache_path(&root, scene);
    if let Some(hit) = std::fs::read_to_string(&cache)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(|v| v["key"] == key.as_str())
    {
        return Ok(json!({ "ok": true, "findings": hit["findings"], "cached": true, "model": model }));
    }
    let mut request = json!({
        "model": model,
        "messages": messages,
        "stream": false,
        // Qwen 3.6 thinks before answering by default: slower, and no better at this.
        "think": false,
        "format": serde_json::from_str::<Value>(SCHEMA).expect("schema is JSON"),
        "options": { "temperature": 0 },
    });
    let reply = loop {
        // A long scene on a laptop can take a while (and the first call loads the model).
        let mut res = match agent(Duration::from_secs(600))
            .post(format!("{url}/api/chat"))
            .send_json(&request)
        {
            Ok(r) => r,
            Err(ureq::Error::Timeout(_)) => {
                return unavailable(format!(
                    "{model} took more than ten minutes on this scene; try a smaller model."
                ));
            }
            Err(e) => return unavailable(unreachable(&url, &e)),
        };
        let status_code = res.status().as_u16();
        let reply: Value = match res.body_mut().read_json() {
            Ok(v) => v,
            Err(e) => return unavailable(format!("Ollama gave an unexpected reply ({e}).")),
        };
        if status_code == 200 {
            break reply;
        }
        let why = reply["error"].as_str().unwrap_or("unknown error");
        // An older Ollama, or a model that can't be told not to think: ask again without.
        if why.contains("think") && request.as_object_mut().and_then(|r| r.remove("think")).is_some() {
            continue;
        }
        return unavailable(format!("Ollama couldn't run {model}: {why}"));
    };
    let content = reply["message"]["content"].as_str().unwrap_or("");
    let raw: Value = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => {
            return unavailable(format!(
                "{model} didn't answer in the expected form; try again or another model."
            ))
        }
    };
    let findings = place(&body, &raw);
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
    fn findings_must_quote_the_scene() {
        let body = "INT. HALL - DAY\n\nJoanna turns away upon to the sight.\n\nDEE\nThou art come.";
        let raw = json!({ "findings": [
            { "quote": "turns away upon to the sight", "problem": "Extra word.", "suggestion": "turns away from the sight" },
            { "quote": "words that aren't there", "problem": "Invented.", "suggestion": "x" },
            { "quote": "Thou art come", "problem": "This is correct period English.", "suggestion": "Thou art come" },
            { "quote": "turns away upon to the sight", "problem": "Twice.", "suggestion": "" }
        ]});
        let f = place(body, &raw);
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].line, f[0].suggestion.as_str()), (2, "turns away from the sight"));
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
        let m = prompt(&meta, &[], "Rain.");
        assert!(m[0]["content"]
            .as_str()
            .unwrap()
            .contains("single-camera TV drama written in british English"));
        assert!(m[1]["content"].as_str().unwrap().ends_with("Rain."));
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
        let content = json!({ "findings": [
            { "quote": "upon to the sight", "problem": "Two prepositions.", "suggestion": "from the sight" }
        ]})
        .to_string();
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
            .contains("JOANNA, Joanna Weston"));
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
        let content = json!({ "findings": [] }).to_string();
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
