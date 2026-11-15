//! Lexi desktop app (Tauri) — a thin webview shell over `lexi-host`.
//!
//! The frontend (a static `index.html`) calls these commands via Tauri `invoke`;
//! each delegates to `lexi-host`, which does every pure decision via `lexi-core`.
//! No localhost server, no Node, no Python.

// Hide the extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// Buffered text chat: provider + prompt (+ optional model) → assistant text.
#[tauri::command]
async fn respond(
    provider: String,
    prompt: String,
    model: Option<String>,
) -> Result<String, String> {
    lexi_host::respond_text(&provider, &prompt, "user", model.as_deref(), &[])
        .await
        .map(|r| r.text)
        .map_err(|e| e.to_string())
}

/// The configured provider names (from `~/.lexi-cli/providers/providers-config.yaml`).
#[tauri::command]
fn list_providers() -> Result<Vec<String>, String> {
    lexi_host::config::list_providers().map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![respond, list_providers])
        .run(tauri::generate_context!())
        .expect("error while running the Lexi desktop app");
}
