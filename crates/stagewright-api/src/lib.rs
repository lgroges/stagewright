//! One JSON command dispatcher shared by the desktop app (Tauri) and the
//! browser dev server (`stagewright serve`), so both behave identically.
//!
//! Every command takes a JSON object and returns a JSON value or an
//! [`ApiError`]. Scenes are addressed by their stable `id`, never by path,
//! so renames and reorders cannot break references.

mod ops;

pub use ops::{slugify, version_of};

use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Debug, Serialize, PartialEq)]
pub struct ApiError {
    pub code: &'static str,
    pub message: String,
    /// Extra data for the client, e.g. the on-disk version after a conflict.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<Value>,
}

impl ApiError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        ApiError {
            code,
            message: message.into(),
            detail: None,
        }
    }
    pub fn with_detail(mut self, detail: Value) -> Self {
        self.detail = Some(detail);
        self
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

pub type ApiResult = Result<Value, ApiError>;

/// The open project, if any.
#[derive(Debug, Default)]
pub struct Workspace {
    pub root: Option<PathBuf>,
}

impl Workspace {
    pub fn new(root: Option<PathBuf>) -> Self {
        Workspace { root }
    }

    fn root(&self) -> Result<PathBuf, ApiError> {
        self.root
            .clone()
            .ok_or_else(|| ApiError::new("no_project", "no project is open"))
    }
}

fn arg_str(args: &Value, key: &str) -> Result<String, ApiError> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| ApiError::new("bad_args", format!("missing string argument '{key}'")))
}

fn arg_opt_str(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(str::to_string)
}

fn arg_opt_u32(args: &Value, key: &str) -> Option<u32> {
    args.get(key)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
}

/// `{act, episode?}` -> the act folder they name.
fn arg_act(args: &Value) -> Result<stagewright_core::project::ActRef, ApiError> {
    Ok(stagewright_core::project::ActRef {
        episode: arg_opt_u32(args, "episode"),
        act: arg_u32(args, "act")?,
    })
}

fn arg_u32(args: &Value, key: &str) -> Result<u32, ApiError> {
    args.get(key)
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| ApiError::new("bad_args", format!("missing number argument '{key}'")))
}

/// Run one command. `args` must be a JSON object (or null for no arguments).
pub fn dispatch(ws: &mut Workspace, cmd: &str, args: Value) -> ApiResult {
    match cmd {
        "project_open" => {
            let root = PathBuf::from(arg_str(&args, "root")?);
            let view = ops::project_view(&root)?;
            ws.root = Some(root);
            Ok(view)
        }
        "project_create" => {
            let root = PathBuf::from(arg_str(&args, "root")?);
            let title = arg_str(&args, "title")?;
            let format = match args.get("format") {
                Some(f) if !f.is_null() => serde_json::from_value(f.clone())
                    .map_err(|_| ApiError::new("bad_args", "format must be stage, film, tv-single or tv-multi"))?,
                _ => stagewright_core::layout::Format::Stage,
            };
            let series = args.get("series").and_then(Value::as_bool).unwrap_or(false);
            ops::project_create(&root, &title, format, series)?;
            let view = ops::project_view(&root)?;
            ws.root = Some(root);
            Ok(view)
        }
        "project_view" => ops::project_view(&ws.root()?),
        "project_close" => {
            ws.root = None;
            Ok(Value::Null)
        }
        "scene_read" => ops::scene_read(&ws.root()?, &arg_str(&args, "id")?),
        "scene_save" => ops::scene_save(
            &ws.root()?,
            &arg_str(&args, "id")?,
            &arg_str(&args, "body")?,
            &arg_str(&args, "base_version")?,
        ),
        "scene_update_meta" => {
            let patch = args
                .get("patch")
                .and_then(Value::as_object)
                .ok_or_else(|| ApiError::new("bad_args", "missing object argument 'patch'"))?;
            ops::scene_update_meta(
                &ws.root()?,
                &arg_str(&args, "id")?,
                patch,
                &arg_str(&args, "base_version")?,
            )
        }
        "scene_create" => ops::scene_create(
            &ws.root()?,
            arg_act(&args)?,
            arg_opt_str(&args, "after").as_deref(),
            &arg_opt_str(&args, "title").unwrap_or_else(|| "New scene".into()),
            arg_opt_str(&args, "id").as_deref(),
        ),
        "scene_trash" => ops::scene_trash(&ws.root()?, &arg_str(&args, "id")?),
        "scenes_reorder" => {
            let ids: Vec<String> = args
                .get("ids")
                .and_then(Value::as_array)
                .ok_or_else(|| ApiError::new("bad_args", "missing array argument 'ids'"))?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| ApiError::new("bad_args", "ids must be strings"))
                })
                .collect::<Result<_, _>>()?;
            ops::scenes_reorder(&ws.root()?, arg_act(&args)?, &ids)
        }
        "act_create" => ops::act_create(&ws.root()?, arg_opt_u32(&args, "episode")),
        "episode_create" => ops::episode_create(&ws.root()?, arg_opt_str(&args, "title").as_deref()),
        "act_delete" => ops::act_delete(&ws.root()?, arg_act(&args)?),
        "episode_delete" => ops::episode_delete(&ws.root()?, arg_u32(&args, "episode")?),
        "act_remove_empty" => ops::act_remove_empty(&ws.root()?, arg_act(&args)?),
        // Pure text transforms, for co-writing (which shares project.yaml as text).
        "yaml_remove_act" => ops::yaml_remove_act(
            &arg_str(&args, "text")?,
            arg_u32(&args, "act")?,
            args.get("series").and_then(Value::as_bool).unwrap_or(false),
        )
        .map(|text| serde_json::json!({ "text": text })),
        "yaml_remove_episode" => ops::yaml_remove_episode(&arg_str(&args, "text")?, arg_u32(&args, "episode")?)
            .map(|text| serde_json::json!({ "text": text })),
        "stats" => ops::stats(&ws.root()?),
        "file_read" => ops::file_read(&ws.root()?, &arg_str(&args, "name")?),
        "file_save" => ops::file_save(
            &ws.root()?,
            &arg_str(&args, "name")?,
            &arg_str(&args, "text")?,
            &arg_str(&args, "base_version")?,
        ),
        "project_snapshot" => ops::project_snapshot(&ws.root()?),
        "act_ensure" => ops::act_ensure(&ws.root()?, arg_act(&args)?),
        "collab_read" => ops::collab_read(&ws.root()?),
        "collab_write" => ops::collab_write(&ws.root()?, &args),
        "project_join" => {
            let root = PathBuf::from(arg_str(&args, "root")?);
            let files = args
                .get("files")
                .and_then(Value::as_object)
                .ok_or_else(|| ApiError::new("bad_args", "missing object argument 'files'"))?;
            let config = args.get("config").cloned().unwrap_or(Value::Null);
            ops::project_join(&root, files, &config)?;
            let view = ops::project_view(&root)?;
            ws.root = Some(root);
            Ok(view)
        }
        "export" => ops::export(
            &ws.root()?,
            &arg_str(&args, "kind")?,
            arg_opt_u32(&args, "episode"),
            arg_opt_str(&args, "path").as_deref().map(std::path::Path::new),
        ),
        "ping" => Ok(json!({ "ok": true, "root": ws.root })),
        _ => Err(ApiError::new("unknown_command", format!("unknown command '{cmd}'"))),
    }
}
