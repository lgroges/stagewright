//! The AI copy-edit with a local model, through Ollama.
//!
//! The scene goes a few paragraphs at a time (a short answer is quicker to get
//! right, and a model that repeats itself can't go on for long), each given back
//! corrected; then a second, short request asks why for each change. Tested on
//! Liam's laptop with Qwen 3 and Qwen 3.6 (docs/decisions.md, 2026-11-05).

use super::text::Paragraph;
use super::{format_name, header, numbered, About, Change, Finding};
use crate::ApiResult;
use serde_json::{json, Value};
use stagewright_core::project::ProjectMeta;
use std::time::Duration;

pub const DEFAULT_URL: &str = "http://127.0.0.1:11434";

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

/// Ollama's address: as set, else the default, without a trailing slash.
pub fn base(url: Option<&str>) -> String {
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

/// The models Ollama has, or why it can't say.
fn installed(url: &str) -> Result<Vec<String>, String> {
    let mut res = agent(Duration::from_secs(4))
        .get(format!("{url}/api/tags"))
        .call()
        .map_err(|e| unreachable(url, &e))?;
    let v: Value = res
        .body_mut()
        .read_json()
        .map_err(|e| format!("Ollama at {url} gave an unexpected reply ({e})."))?;
    Ok(v["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["name"].as_str().map(str::to_string))
        .collect())
}

/// Whether Ollama answers at `url`, and its models.
pub fn status(args: &Value) -> ApiResult {
    let url = base(args.get("url").and_then(Value::as_str));
    Ok(match installed(&url) {
        Ok(models) => json!({ "ok": true, "url": url, "models": models }),
        Err(reason) => json!({ "ok": false, "reason": reason }),
    })
}

/// The model asked for, or the best one installed.
pub fn choose_model(url: &str, asked: Option<&str>) -> Result<String, String> {
    if let Some(m) = pick_model(asked, &[]) {
        return Ok(m);
    }
    pick_model(None, &installed(url)?).ok_or_else(|| "Ollama has no models installed. Try: ollama pull qwen3.6".into())
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

/// The scene's requests: its paragraphs a few at a time.
pub fn requests(meta: &ProjectMeta, about: &About, paras: &[Paragraph]) -> Vec<Value> {
    chunks(paras)
        .into_iter()
        .map(|r| prompt(meta, about, paras, r))
        .collect()
}

/// Review a scene: send each request and place what the model changed, then
/// ask why.
pub fn review(
    url: &str,
    model: &str,
    requests: &[Value],
    body: &str,
    paras: &[Paragraph],
) -> Result<Vec<Finding>, String> {
    let mut findings = Vec::new();
    for messages in requests {
        // A cap on the answer, about the length of the text: a model that starts
        // repeating itself stops there.
        let predict = (messages[1]["content"].as_str().unwrap_or("").len() / 2 + 200) as u32;
        let raw = ask(url, model, messages, predict, SCHEMA)?;
        findings.extend(super::text::place(body, paras, &raw));
    }
    explain(url, model, &mut findings, paras);
    Ok(findings)
}

const REASONS_SCHEMA: &str = r#"{"type":"object","properties":{"reasons":{"type":"array","items":{"type":"object","properties":{"n":{"type":"integer"},"why":{"type":"string"}},"required":["n","why"]}}},"required":["reasons"]}"#;

/// The request for the reasons behind the changes: a separate, short one, so
/// the corrections themselves are asked for exactly as tested.
pub fn reasons_prompt(changes: &[&Change], paras: &[Paragraph]) -> Value {
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
        .map(|(i, c)| {
            let para = paras.get(c.paragraph).map_or("", |p| p.text.as_str());
            format!(
                "[{}] In: {para}\nChange: \u{201c}{}\u{201d} \u{2192} \u{201c}{}\u{201d}",
                i + 1,
                c.was,
                c.now
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
fn explain(url: &str, model: &str, findings: &mut [Finding], paras: &[Paragraph]) {
    for batch in findings.chunks_mut(12) {
        let default = Change::default();
        let changes: Vec<&Change> = batch.iter().map(|f| f.change.as_ref().unwrap_or(&default)).collect();
        let messages = reasons_prompt(&changes, paras);
        let Ok(raw) = ask(url, model, &messages, 60 * changes.len() as u32 + 100, REASONS_SCHEMA) else {
            return;
        };
        for r in raw["reasons"].as_array().into_iter().flatten() {
            let n = r["n"].as_u64().and_then(|n| (n as usize).checked_sub(1));
            if let Some(f) = n.and_then(|n| batch.get_mut(n)) {
                f.add_reason(r["why"].as_str().unwrap_or(""));
            }
        }
    }
}

/// One request: the model's answer as JSON (matching `schema`), or why there
/// isn't one. `predict` caps the answer's length.
fn ask(url: &str, model: &str, messages: &Value, predict: u32, schema: &str) -> Result<Value, String> {
    // Room for the prompt and the answer: Ollama's default, 4,096 tokens, can be
    // too small for a long scene.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::review;
    use crate::ai::tests::{fake, project};
    use crate::ai::text::paragraphs;

    #[test]
    fn a_few_paragraphs_at_a_time() {
        let p = paragraphs("Rain.");
        let many: Vec<Paragraph> = (0..20).map(|_| p[0].clone()).collect();
        assert_eq!(chunks(&many), vec![0..4, 4..8, 8..12, 12..16, 16..20]);
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
