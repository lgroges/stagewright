//! Comments on the script (see `stagewright_core::comments`).

use crate::ops::{read_text, write_atomic};
use crate::{ApiError, ApiResult};
use serde_json::{json, Value};
use stagewright_core::comments::{self, Op, FILE};
use std::path::Path;

fn bad(e: String) -> ApiError {
    ApiError::new("bad_args", e)
}

fn op_from(args: &Value) -> Result<Op, ApiError> {
    serde_json::from_value(args.get("op").cloned().unwrap_or(Value::Null)).map_err(|e| bad(format!("op: {e}")))
}

fn text_of(root: &Path) -> Result<String, ApiError> {
    let path = root.join(FILE);
    if path.exists() {
        read_text(&path)
    } else {
        Ok(String::new())
    }
}

/// Every comment in the project.
pub fn list(root: &Path) -> ApiResult {
    parse_text(&text_of(root)?)
}

/// Comments in `text` (comments.yaml as shared when co-writing).
pub fn parse_text(text: &str) -> ApiResult {
    let list = comments::parse(text).map_err(|e| ApiError::new("invalid_yaml", e))?;
    Ok(json!({ "comments": list }))
}

/// Apply a change to comments.yaml and return every comment.
pub fn edit(root: &Path, args: &Value) -> ApiResult {
    let updated = comments::edit_text(&text_of(root)?, op_from(args)?).map_err(bad)?;
    write_atomic(&root.join(FILE), &updated)?;
    let mut v = parse_text(&updated)?;
    v["text"] = json!(updated);
    Ok(v)
}

/// The same change, made to shared text; nothing is written.
pub fn edit_text(text: &str, args: &Value) -> ApiResult {
    let updated = comments::edit_text(text, op_from(args)?).map_err(bad)?;
    let mut v = parse_text(&updated)?;
    v["text"] = json!(updated);
    Ok(v)
}
