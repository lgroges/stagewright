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

use super::{header, numbered, place, Finding, Paragraph};
use crate::{ApiError, ApiResult};
use serde_json::{json, Value};
use stagewright_core::project::ProjectMeta;
use std::time::Duration;

pub const DEFAULT_MODEL: &str = "claude-sonnet-5-5";

/// The models offered: id, name, and US dollars per million tokens in and out.
pub const MODELS: &[(&str, &str, f64, f64)] = &[
    ("claude-sonnet-5-5", "Sonnet 5.5", 2.0, 10.0),
    ("claude-opus-5-5", "Opus 5.5", 4.0, 20.0),
    ("claude-haiku-5-5", "Haiku 5.5", 0.10, 0.50),
];

const API_URL: &str = "https://api.anthropic.com";
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

/// Whether there's a key and the API accepts it.
pub fn status(c: &Claude) -> ApiResult {
    let models: Vec<Value> = MODELS
        .iter()
        .map(|(id, name, ..)| json!({ "id": id, "name": name }))
        .collect();
    let Some(key) = c.key.as_deref() else {
        return Ok(json!({ "ok": false, "has_key": false, "models": models, "reason": "No API key yet." }));
    };
    let res = agent(Duration::from_secs(20))
        .get(format!("{}/v1/models", c.url))
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .call();
    let mut res = match res {
        Ok(r) => r,
        Err(e) => {
            let reason = format!("Couldn't reach Anthropic ({e}).");
            return Ok(json!({ "ok": false, "has_key": true, "stored": c.stored, "models": models, "reason": reason }));
        }
    };
    let code = res.status().as_u16();
    let body: Value = res.body_mut().read_json().unwrap_or(Value::Null);
    if code == 200 {
        Ok(json!({ "ok": true, "has_key": true, "stored": c.stored, "models": models }))
    } else {
        Ok(
            json!({ "ok": false, "has_key": true, "stored": c.stored, "models": models, "reason": failure(code, &body) }),
        )
    }
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
    let (.., inp, out) = MODELS.iter().find(|(id, ..)| *id == model)?;
    let u = &reply["usage"];
    let sent = u["input_tokens"].as_f64()?
        + u["cache_creation_input_tokens"].as_f64().unwrap_or(0.0) * 1.25
        + u["cache_read_input_tokens"].as_f64().unwrap_or(0.0) * 0.1;
    Some((sent * inp + u["output_tokens"].as_f64()? * out) / 1e6)
}

/// Put the model's reason after each finding's change: the change in the same
/// paragraph whose words match, or the paragraph's only change.
pub fn add_reasons(findings: &mut [Finding], paras: &[Paragraph], raw: &Value) {
    let norm = |s: &str| {
        s.replace(['\u{2019}', '\u{2018}'], "'")
            .replace(['\u{201c}', '\u{201d}'], "\"")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    };
    for f in findings.iter_mut() {
        let Some((was, now, text)) = &f.change else { continue };
        let Some(p) = raw["paragraphs"].as_array().into_iter().flatten().find(|p| {
            p["n"]
                .as_u64()
                .and_then(|n| paras.get((n as usize).checked_sub(1)?))
                .is_some_and(|q| &q.text == text)
        }) else {
            continue;
        };
        let changes: Vec<&Value> = p["changes"].as_array().into_iter().flatten().collect();
        // The diff joins tokens with spaces ("He ’s"); compare without them.
        let squash = |s: &str| norm(s).replace(' ', "");
        let found = changes
            .iter()
            .find(|c| {
                squash(c["from"].as_str().unwrap_or("")).contains(&squash(was))
                    && squash(c["to"].as_str().unwrap_or("")).contains(&squash(now))
            })
            .or_else(|| (changes.len() == 1).then(|| &changes[0]));
        let Some(why) = found
            .and_then(|c| c["why"].as_str())
            .map(str::trim)
            .filter(|w| !w.is_empty())
        else {
            continue;
        };
        let mut why = why.to_string();
        if let Some(first) = why.chars().next() {
            why.replace_range(..first.len_utf8(), &first.to_uppercase().to_string());
        }
        if !why.ends_with(['.', '!', '?']) {
            why.push('.');
        }
        f.message = format!("{} {why}", f.message);
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
    let Some(key) = c.key.as_deref() else {
        return Err("Add your Anthropic API key under Production → Checks… first.".into());
    };
    let url = format!("{}/v1/messages", c.url);
    let mut res = agent(Duration::from_secs(180))
        .post(url)
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .send_json(request)
        .map_err(|e| format!("Couldn't reach Anthropic ({e}). Are you online?"))?;
    let code = res.status().as_u16();
    let reply: Value = res
        .body_mut()
        .read_json()
        .map_err(|e| format!("Anthropic gave an unexpected reply ({e})."))?;
    if code != 200 {
        return Err(failure(code, &reply));
    }
    if reply["stop_reason"] == "max_tokens" {
        return Err("The scene is too long for one review; try splitting it.".into());
    }
    let raw = answer(&reply).ok_or_else(|| format!("{model} didn't answer in the expected form; try again."))?;
    let mut findings = place(body, paras, &raw);
    add_reasons(&mut findings, paras, &raw);
    Ok((findings, cost(model, &reply)))
}
