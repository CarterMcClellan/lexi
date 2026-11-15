//! Lexi desktop app (Tauri) — a thin webview shell over `lexi-host`.
//!
//! The reused `lexi-gui` frontend speaks the Node server's `/api/*` contract; a
//! `fetch`-shim in the webview routes those calls to the `api` command below,
//! which is dispatched in [`backend`]. No Node, no Python, no localhost server.

// Hide the extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backend;

/// The single entry point the webview `fetch`-shim calls for every `/api/*`
/// request (method + path + optional JSON body).
#[tauri::command]
async fn api(
    method: String,
    path: String,
    body: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    backend::handle(&method, &path, body).await
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![api])
        .run(tauri::generate_context!())
        .expect("error while running the Lexi desktop app");
}
