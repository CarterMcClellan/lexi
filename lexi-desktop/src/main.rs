//! Lexi desktop app (Tauri) — a thin webview shell over `lexi-host`.
//!
//! The reused web frontend speaks the old Node server's `/api/*` contract; a
//! `fetch`-shim in the webview routes those calls to the `api` command below,
//! which is dispatched in [`backend`]. No Node, no Python, no localhost server.

// Hide the extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod backend;

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Mutex, OnceLock};

use futures_util::future::{abortable, AbortHandle};

/// Requests that can be stopped, by the id the webview gave them.
fn running() -> &'static Mutex<HashMap<String, AbortHandle>> {
    static RUNNING: OnceLock<Mutex<HashMap<String, AbortHandle>>> = OnceLock::new();
    RUNNING.get_or_init(Default::default)
}

/// Run `fut`; with a `request_id`, [`cancel_request`] can stop it — the
/// future is dropped, which closes its connection to the provider.
async fn cancellable<T>(
    request_id: Option<String>,
    fut: impl Future<Output = Result<T, String>>,
) -> Result<T, String> {
    let Some(id) = request_id else {
        return fut.await;
    };
    let (fut, handle) = abortable(fut);
    let lock = || running().lock().unwrap_or_else(|p| p.into_inner());
    lock().insert(id.clone(), handle);
    let out = fut.await;
    lock().remove(&id);
    out.unwrap_or_else(|_| Err("stopped".into()))
}

/// The single entry point the webview `fetch`-shim calls for every `/api/*`
/// request (method + path + optional JSON body). A request sent with an
/// abort signal carries a `request_id`, so Stop can cancel it.
#[tauri::command]
async fn api(
    method: String,
    path: String,
    body: Option<serde_json::Value>,
    request_id: Option<String>,
) -> Result<serde_json::Value, String> {
    cancellable(request_id, backend::handle(&method, &path, body)).await
}

/// Streaming chat: the shim passes a channel the SSE blocks are pushed through.
#[tauri::command]
async fn api_stream(
    body: serde_json::Value,
    on_event: tauri::ipc::Channel<String>,
    request_id: Option<String>,
) -> Result<(), String> {
    cancellable(request_id, backend::stream(body, on_event)).await
}

/// Stop a running request (the Stop button); whether it was still running.
#[tauri::command]
fn cancel_request(request_id: String) -> bool {
    let handle = running()
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(&request_id);
    handle.map(|h| h.abort()).is_some()
}

fn main() {
    // Seed missing defaults on first launch (never overwrites existing files).
    if let Err(e) = lexi_host::ensure_home() {
        eprintln!("warning: could not prepare the Lexi home: {e}");
    }
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![api, api_stream, cancel_request])
        .run(tauri::generate_context!())
        .expect("error while running the Lexi desktop app");
}

#[cfg(test)]
mod tests {
    use super::{cancel_request, cancellable};

    #[tokio::test]
    async fn stop_ends_a_request_that_never_answers() {
        let id = "test-hung-request".to_string();
        let waiting = tokio::spawn(cancellable::<()>(Some(id.clone()), std::future::pending()));
        // Wait until it's registered, then stop it.
        while !cancel_request(id.clone()) {
            tokio::task::yield_now().await;
        }
        let out = tokio::time::timeout(std::time::Duration::from_secs(5), waiting)
            .await
            .expect("stopped promptly")
            .unwrap();
        assert_eq!(out, Err("stopped".to_string()));
        // Gone from the list: stopping it again finds nothing.
        assert!(!cancel_request(id));
        // Without an id a request just runs.
        assert_eq!(cancellable(None, async { Ok::<_, String>(7) }).await, Ok(7));
    }
}
