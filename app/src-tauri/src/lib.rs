//! Desktop shell: one `api` command that forwards to the same dispatcher the
//! browser dev server uses (`stagewright-api`), plus the native folder picker
//! and self-update.

use serde_json::Value;
use stagewright_api::{dispatch, dispatch_free, ApiError, Workspace};
use std::sync::Mutex;

struct AppState(Mutex<Workspace>);

/// Runs off the main thread so file work never blocks the window.
#[tauri::command(async)]
fn api(state: tauri::State<'_, AppState>, cmd: String, args: Value) -> Result<Value, ApiError> {
    // The AI review can take minutes: it doesn't touch the workspace, so it doesn't wait for it.
    if let Some(done) = dispatch_free(&cmd, &args) {
        return done;
    }
    let mut ws = state
        .0
        .lock()
        .map_err(|_| ApiError::new("internal", "workspace lock poisoned"))?;
    dispatch(&mut ws, &cmd, args)
}

/// `stagewright-app path/to/play` opens that project straight away.
fn initial_project() -> Option<std::path::PathBuf> {
    let p = std::path::PathBuf::from(std::env::args_os().nth(1)?);
    p.join("project.yaml").is_file().then_some(p)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // Updates are checked from the window (see src/lib/updates.svelte.ts).
            #[cfg(desktop)]
            app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
            Ok(())
        })
        .manage(AppState(Mutex::new(Workspace::new(initial_project()))))
        .invoke_handler(tauri::generate_handler![api])
        .run(tauri::generate_context!())
        .expect("error while running Stagewright");
}
