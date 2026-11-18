//! The AI copy-edit through the Claude API (Anthropic), beside Ollama.
//!
//! One request a scene: the whole scene as numbered paragraphs, and an answer
//! listing only the paragraphs corrected, each with its changes and why. The
//! word diff in `place` still decides where each change is; the model's reason
//! is matched to it. The prompt is the one tested on 2026-11-05 (Sonnet 5.5:
//! 9 of 9 planted mistakes found, nothing changed in the clean scenes).
//!
//! The text goes to Anthropic only for a project that allows it
//! (`ai: {cloud: true}`). The API key is kept in the system's credential store
//! (Windows, macOS), or read from ANTHROPIC_API_KEY.

use super::text::{paragraph, place, Paragraph};
use super::{header, numbered, Finding};
use crate::{ApiError, ApiResult};
use serde_json::{json, Value};
use stagewright_core::project::ProjectMeta;
use std::time::Duration;

pub const DEFAULT_MODEL: &str = "claude-sonnet-5-5";

/// The models' prices: US dollars per million tokens in and out. (The app
/// offers these models.)
pub const PRICES: &[(&str, f64, f64)] = &[
    ("claude-sonnet-5-5", 2.0, 10.0),
    ("claude-opus-5-5", 4.0, 20.0),
    ("claude-haiku-5-5", 0.10, 0.50),
];

const API_URL: &str = "https://api.anthropic.com";
const API_VERSION: &str = "2023-06-01";
const KEY_SERVICE: &str = "Stagewright";
const KEY_USER: &str = "anthropic-api-key";

/// Where the API is and the key to use.
pub struct Claude {
    pub url: String,
    pub key: Option<String>,
    /// The key is the saved one (not ANTHROPIC_API_KEY).
    pub stored: bool,
}

impl Claude {
    /// The real API (or, for tests, STAGEWRIGHT_CLAUDE_URL), with the saved key
    /// or ANTHROPIC_API_KEY.
    pub fn from_env() -> Claude {
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let saved = stored_key();
        Claude {
            url: env("STAGEWRIGHT_CLAUDE_URL")
                .unwrap_or_else(|| API_URL.into())
                .trim_end_matches('/')
                .into(),
            stored: saved.is_some(),
            key: saved.or_else(|| env("ANTHROPIC_API_KEY")),
        }
    }
}

#[cfg(any(windows, target_os = "macos"))]
fn entry() -> Option<keyring::Entry> {
    keyring::Entry::new(KEY_SERVICE, KEY_USER).ok()
}

#[cfg(any(windows, target_os = "macos"))]
fn stored_key() -> Option<String> {
    entry()?.get_password().ok()
}

#[cfg(not(any(windows, target_os = "macos")))]
fn stored_key() -> Option<String> {
    let _ = (KEY_SERVICE, KEY_USER);
    None
}

/// Save the API key (`key`), or remove it (empty). Args: `key`.
pub fn set_key(args: &Value) -> ApiResult {
    let key = args.get("key").and_then(Value::as_str).unwrap_or("").trim();
    if !key.is_empty() && !key.starts_with("sk-ant-") {
        return Err(ApiError::new(
            "bad_args",
            "That doesn't look like an Anthropic API key (they start sk-ant-).",
        ));
    }
    #[cfg(any(windows, target_os = "macos"))]
    {
        let e = entry().ok_or_else(|| ApiError::new("keyring", "The system's credential store isn't available."))?;
        let done = if key.is_empty() {
            match e.delete_credential() {
                Err(keyring::Error::NoEntry) => Ok(()),
                other => other,
            }
        } else {
            e.set_password(key)
        };
        done.map_err(|err| ApiError::new("keyring", format!("Couldn't save the key: {err}")))?;
        Ok(json!({ "ok": true, "has_key": !key.is_empty() }))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = key;
        Err(ApiError::new(
            "unsupported",
            "Saving the key isn't supported on this system: set ANTHROPIC_API_KEY instead.",
        ))
    }
}

fn agent(read: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_global(Some(read))
        .http_status_as_error(false)
        .proxy(ureq::Proxy::try_from_env())
        .build()
        .into()
}

/// Why a request failed, for the writer.
fn failure(status: u16, body: &Value) -> String {
    let msg = body["error"]["message"].as_str().unwrap_or("").trim();
    match status {
        401 | 403 => "The API key wasn't accepted. Check it under Production → Checks….".into(),
        429 | 529 => "Anthropic is busy just now; try again in a minute.".into(),
        _ if msg.contains("credit balance") => {
            "Your Anthropic credit has run out (add some at platform.claude.com).".into()
        }
        _ if !msg.is_empty() => format!("Anthropic: {msg}"),
        _ => format!("Anthropic answered {status}."),
    }
}

impl Claude {
    /// One API call (a POST of `send`, or a GET): the reply, or why there isn't
    /// one in words for the writer.
    fn call(&self, path: &str, send: Option<&Value>) -> Result<Value, String> {
        let key = self
            .key
            .as_deref()
            .ok_or("Add your Anthropic API key under Production → Checks… first.")?;
        let url = format!("{}{path}", self.url);
        let res = match send {
            Some(body) => agent(Duration::from_secs(180))
                .post(url)
                .header("x-api-key", key)
                .header("anthropic-version", API_VERSION)
                .send_json(body),
            None => agent(Duration::from_secs(20))
                .get(url)
                .header("x-api-key", key)
                .header("anthropic-version", API_VERSION)
                .call(),
        };
        let mut res = res.map_err(|e| format!("Couldn't reach Anthropic ({e}). Are you online?"))?;
        let code = res.status().as_u16();
        let reply: Value = res
            .body_mut()
            .read_json()
            .map_err(|e| format!("Anthropic gave an unexpected reply ({e})."))?;
        if code == 200 {
            Ok(reply)
        } else {
            Err(failure(code, &reply))
        }
    }
}

/// Whether there's a key and the API accepts it.
pub fn status(c: &Claude) -> ApiResult {
    let mut v = json!({ "ok": true, "has_key": c.key.is_some(), "stored": c.stored });
    let checked = if c.key.is_some() {
        c.call("/v1/models", None).map(drop)
    } else {
        Err("No API key yet.".into())
    };
    if let Err(reason) = checked {
        v["ok"] = json!(false);
        v["reason"] = json!(reason);
    }
    Ok(v)
}

/// The instructions, word for word as tested: small changes change the results.
pub fn system(meta: &ProjectMeta) -> String {
    format!(
        "You are a careful copy editor for {kind} in {english} English. You will get a scene's action and dialogue as \
numbered paragraphs. Find grammar mistakes, typos, missing or doubled words and wrong words, and correct only those.\n\n\
Leave alone: period or archaic language (thee, thou, hath, saith, cometh, \"we be in death\") unless it is misspelt \
even for its period; how a character speaks (dialect, slang, fragments, repetition); sentence fragments in action \
(scripts are written that way); names, and words in capitals; {english} spelling; style and rewording that doesn't \
fix an error.\n\n\
Answer only with JSON, and no other text, in this form, listing only the paragraphs you corrected:\n\
{{\"paragraphs\": [{{\"n\": 2, \"text\": \"the whole paragraph, corrected, otherwise exactly as written\", \
\"changes\": [{{\"from\": \"the original words\", \"to\": \"the corrected words\", \"why\": \"what was wrong, in a few \
plain words, as a writer would put it\"}}]}}]}}\n\
If nothing needs correcting, answer {{\"paragraphs\": []}}.",
        kind = super::format_name(meta),
        english = format!("{:?}", meta.dialect),
    )
}

/// The request body for one scene.
pub fn body(model: &str, meta: &ProjectMeta, about: &super::About, paras: &[Paragraph]) -> Value {
    json!({
        "model": model,
        "max_tokens": 16000,
        "system": system(meta),
        "messages": [{ "role": "user", "content": format!("{}\n\n{}", header(meta, about), numbered(paras, 0..paras.len())) }],
    })
}

/// The JSON in a reply's text, without a code fence around it if there is one.
fn answer(reply: &Value) -> Option<Value> {
    let text: String = reply["content"]
        .as_array()?
        .iter()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect();
    let t = text.trim();
    let t = t.strip_prefix("```json").or_else(|| t.strip_prefix("```")).unwrap_or(t);
    let t = t.strip_suffix("```").unwrap_or(t).trim();
    serde_json::from_str(t).ok()
}

/// What a reply cost, in US dollars, from its token counts.
pub fn cost(model: &str, reply: &Value) -> Option<f64> {
    let (_, inp, out) = PRICES.iter().find(|(id, ..)| *id == model)?;
    let u = &reply["usage"];
    let sent = u["input_tokens"].as_f64()?
        + u["cache_creation_input_tokens"].as_f64().unwrap_or(0.0) * 1.25
        + u["cache_read_input_tokens"].as_f64().unwrap_or(0.0) * 0.1;
    Some((sent * inp + u["output_tokens"].as_f64()? * out) / 1e6)
}

/// Put the model's reason after each finding's change: the change in the same
/// paragraph whose words match, or the paragraph's only change.
pub fn add_reasons(findings: &mut [Finding], paras: &[Paragraph], raw: &Value) {
    // Compared without spaces or curly quotes: the diff joins words with spaces
    // ("He ’s"), the model writes "He's".
    let squash = |s: &str| {
        s.replace(['\u{2019}', '\u{2018}'], "'")
            .replace(['\u{201c}', '\u{201d}'], "\"")
            .replace(char::is_whitespace, "")
    };
    for f in findings.iter_mut() {
        let Some(c) = &f.change else { continue };
        let Some(p) = raw["paragraphs"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|p| paragraph(paras, &p["n"]).is_some_and(|(i, _)| i == c.paragraph))
        else {
            continue;
        };
        let changes: Vec<&Value> = p["changes"].as_array().into_iter().flatten().collect();
        let (was, now) = (squash(&c.was), squash(&c.now));
        let matching = changes.iter().find(|m| {
            squash(m["from"].as_str().unwrap_or("")).contains(&was)
                && squash(m["to"].as_str().unwrap_or("")).contains(&now)
        });
        if let Some(why) = matching
            .or_else(|| changes.first().filter(|_| changes.len() == 1))
            .and_then(|m| m["why"].as_str())
        {
            f.add_reason(why);
        }
    }
}

/// Review one scene: (findings, cost in dollars), or why not.
pub fn review(
    c: &Claude,
    model: &str,
    body: &str,
    paras: &[Paragraph],
    request: &Value,
) -> Result<(Vec<Finding>, Option<f64>), String> {
    let reply = c.call("/v1/messages", Some(request))?;
    if reply["stop_reason"] == "max_tokens" {
        return Err("The scene is too long for one review; try splitting it.".into());
    }
    let raw = answer(&reply).ok_or_else(|| format!("{model} didn't answer in the expected form; try again."))?;
    let mut findings = place(body, paras, &raw);
    add_reasons(&mut findings, paras, &raw);
    Ok((findings, cost(model, &reply)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::review_with;
    use crate::ai::tests::{fake, project};

    fn claude_at(url: &str, key: Option<&str>) -> Claude {
        Claude {
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
            status(&claude_at("http://127.0.0.1:9", None)).unwrap()["has_key"],
            false
        );
        let (url, _g) = fake(vec![(200, json!({ "data": [] }).to_string())]);
        assert_eq!(status(&claude_at(&url, Some("sk-ant-x"))).unwrap()["ok"], true);
        let (url, _g) = fake(vec![(401, err("invalid x-api-key").to_string())]);
        let v = status(&claude_at(&url, Some("sk-ant-x"))).unwrap();
        assert!(v["reason"].as_str().unwrap().contains("wasn't accepted"));
        // A key that isn't one.
        assert!(set_key(&json!({ "key": "hello" })).is_err());
    }
}
