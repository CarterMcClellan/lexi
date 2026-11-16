//! Python bindings for the `lexi-host` runtime: send prompts (buffered,
//! streaming, background), detached jobs, image generation and uploads.
//!
//! These are the private `lexi._lexi._*` functions; the public, documented
//! signatures are the Python wrappers in `python/lexi/`. Each call:
//! - seeds `~/.lexi-cli` (or `$LEXI_HOME`) with missing defaults, like the CLI;
//! - runs on one shared tokio runtime with the GIL released, checking for
//!   Ctrl-C while it waits;
//! - returns the CLI's `--json` shapes (`docs/spec/cli-machine-v0.md`), built by
//!   the same `lexi-host` code, and raises `lexi.LexiError` on failure.

use std::future::Future;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use lexi_host::{ChatMode, ChatRequest, HostError, PreparedChat, StreamEvent};
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::{from_py, to_py};

pyo3::create_exception!(
    lexi,
    LexiError,
    PyException,
    "A Lexi operation failed (unknown provider, missing API key, provider error, …)."
);

fn lexi_err(e: impl std::fmt::Display) -> PyErr {
    LexiError::new_err(e.to_string())
}

fn runtime() -> &'static tokio::runtime::Runtime {
    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("lexi-runtime")
            .build()
            .expect("failed to start the lexi runtime")
    })
}

/// How often a blocked call wakes to check for Ctrl-C.
const SIGNAL_CHECK: Duration = Duration::from_millis(100);

/// Run `fut` on the shared runtime and wait for it without holding the GIL.
/// A pending Python signal (Ctrl-C) cancels the task and is raised.
fn block_on<T, F>(py: Python<'_>, fut: F) -> PyResult<T>
where
    T: Send + 'static,
    F: Future<Output = Result<T, HostError>> + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    let task = runtime().spawn(async move {
        let _ = tx.send(fut.await);
    });
    let mut rx = rx;
    loop {
        // Receiver is Send but not Sync: move it through the GIL-free closure.
        let (got, back) = py.allow_threads(move || (rx.recv_timeout(SIGNAL_CHECK), rx));
        rx = back;
        match got {
            Ok(res) => return res.map_err(lexi_err),
            Err(RecvTimeoutError::Timeout) => {
                if let Err(e) = py.check_signals() {
                    task.abort();
                    return Err(e);
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(lexi_err("the lexi runtime task ended unexpectedly"))
            }
        }
    }
}

fn ensure_home() -> PyResult<()> {
    lexi_host::ensure_home().map(|_| ()).map_err(lexi_err)
}

// --- chat options -----------------------------------------------------------

/// The keyword options shared by `respond` / `stream` / `submit` / `prepare`
/// (the Python wrappers drop `None`s before passing them here).
#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct ChatOpts {
    text: Option<String>,
    prompt: Option<String>,
    role: Option<String>,
    model: Option<String>,
    images: Vec<String>,
    params: Map<String, Value>,
    tools: Vec<String>,
    set: Value,
    unset: Vec<String>,
    validate: Option<bool>,
    cite: bool,
    raw: bool,
}

/// `set` as `{path: value}` or `[(path, value), …]`.
fn set_paths(v: &Value) -> PyResult<Vec<(String, Value)>> {
    match v {
        Value::Null => Ok(Vec::new()),
        Value::Object(m) => Ok(m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
        Value::Array(pairs) => pairs
            .iter()
            .map(|p| match p.as_array().map(Vec::as_slice) {
                Some([Value::String(k), v]) => Ok((k.clone(), v.clone())),
                _ => Err(PyValueError::new_err(
                    "set: expected a dict or (path, value) pairs",
                )),
            })
            .collect(),
        _ => Err(PyValueError::new_err(
            "set: expected a dict or (path, value) pairs",
        )),
    }
}

fn parse_opts(opts: &Bound<'_, PyAny>) -> PyResult<ChatOpts> {
    serde_json::from_value(from_py(opts)?).map_err(|e| PyValueError::new_err(e.to_string()))
}

/// Resolve the prompt and compose the chat (no network I/O).
fn prepare(provider: &str, opts: &ChatOpts, mode: ChatMode) -> PyResult<PreparedChat> {
    ensure_home()?;
    let mut payload = match (&opts.text, &opts.prompt) {
        (Some(_), Some(_)) => {
            return Err(PyValueError::new_err(
                "pass either text or prompt (a saved prompt's name), not both",
            ))
        }
        (Some(t), None) => json!({ "prompt": t, "role": "user" }),
        (None, name) => lexi_host::config::resolve_prompt(name.as_deref()).map_err(lexi_err)?,
    };
    if let Some(role) = &opts.role {
        payload["role"] = Value::from(role.as_str());
    }
    let req = ChatRequest {
        prompt: payload,
        model: opts.model.clone(),
        images: lexi_host::resolve_images(&opts.images).map_err(lexi_err)?,
        overrides: opts.params.clone(),
        tool_presets: opts.tools.clone(),
        set_paths: set_paths(&opts.set)?,
        unset_paths: opts.unset.clone(),
        skip_validation: !opts.validate.unwrap_or(true),
    };
    lexi_host::prepare_chat(provider, &req, mode).map_err(lexi_err)
}

fn mode_from(name: &str) -> PyResult<ChatMode> {
    match name {
        "buffered" => Ok(ChatMode::Buffered),
        "stream" => Ok(ChatMode::Stream),
        "background" => Ok(ChatMode::Background),
        other => Err(PyValueError::new_err(format!(
            "unknown chat mode {other:?}"
        ))),
    }
}

// --- chat -------------------------------------------------------------------

#[pyfunction]
fn _prepare<'py>(
    py: Python<'py>,
    provider: &str,
    opts: &Bound<'py, PyAny>,
    mode: &str,
) -> PyResult<Bound<'py, PyAny>> {
    let chat = prepare(provider, &parse_opts(opts)?, mode_from(mode)?)?;
    to_py(
        py,
        &json!({ "provider": chat.provider, "model": chat.model, "body": chat.body }),
    )
}

#[pyfunction]
fn _respond<'py>(
    py: Python<'py>,
    provider: &str,
    opts: &Bound<'py, PyAny>,
    background: bool,
) -> PyResult<Bound<'py, PyAny>> {
    let opts = parse_opts(opts)?;
    let mode = if background {
        ChatMode::Background
    } else {
        ChatMode::Buffered
    };
    let chat = prepare(provider, &opts, mode)?;
    let (cite, raw) = (opts.cite, opts.raw);
    let out = block_on(py, async move {
        let res = lexi_host::run_chat(&chat, |_| {}).await?;
        Ok(chat.result_value(&res, cite, raw))
    })?;
    to_py(py, &out)
}

#[pyfunction]
fn _submit<'py>(
    py: Python<'py>,
    provider: &str,
    opts: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let chat = prepare(provider, &parse_opts(opts)?, ChatMode::Background)?;
    let out = block_on(py, async move {
        let st = lexi_host::submit_chat(&chat).await?;
        Ok(st.to_value(&chat.provider))
    })?;
    to_py(py, &out)
}

enum Event {
    Delta(String),
    Activity(String),
    Done(Value),
    Failed(String),
}

/// An iterator over a streamed response's events — `activity`, `delta`, then
/// `provider_done` (the NDJSON events of `lexi respond --stream --json`).
/// A failure raises `LexiError` instead of a `provider_done` with `error`.
#[pyclass(module = "lexi")]
struct ChatStream {
    provider: String,
    rx: Mutex<Option<Receiver<Event>>>,
    started: bool,
    task: tokio::task::AbortHandle,
}

impl Drop for ChatStream {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[pymethods]
impl ChatStream {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__<'py>(&mut self, py: Python<'py>) -> PyResult<Option<Bound<'py, PyAny>>> {
        if !self.started {
            self.started = true;
            return to_py(
                py,
                &json!({ "event": "provider_start", "provider": self.provider }),
            )
            .map(Some);
        }
        let mut guard = self.rx.lock().unwrap();
        let Some(mut rx) = guard.take() else {
            return Ok(None);
        };
        let event = loop {
            let (got, back) = py.allow_threads(move || (rx.recv_timeout(SIGNAL_CHECK), rx));
            rx = back;
            match got {
                Ok(ev) => break ev,
                Err(RecvTimeoutError::Timeout) => {
                    if let Err(e) = py.check_signals() {
                        self.task.abort();
                        return Err(e);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(lexi_err("the stream ended unexpectedly"));
                }
            }
        };
        let p = &self.provider;
        let value = match event {
            Event::Delta(text) => json!({ "event": "delta", "provider": p, "text": text }),
            Event::Activity(msg) => json!({ "event": "activity", "provider": p, "message": msg }),
            Event::Done(mut done) => {
                done["event"] = Value::from("provider_done");
                done
            }
            Event::Failed(e) => return Err(lexi_err(e)),
        };
        if matches!(value["event"].as_str(), Some("delta" | "activity")) {
            *guard = Some(rx); // more to come
        }
        to_py(py, &value).map(Some)
    }

    /// Stop the stream (also happens when the iterator is garbage-collected).
    fn close(&mut self) {
        self.task.abort();
        *self.rx.lock().unwrap() = None;
    }
}

#[pyfunction]
fn _stream(provider: &str, opts: &Bound<'_, PyAny>) -> PyResult<ChatStream> {
    let opts = parse_opts(opts)?;
    let chat = prepare(provider, &opts, ChatMode::Stream)?;
    let (cite, raw) = (opts.cite, opts.raw);
    let (tx, rx) = mpsc::channel();
    let task = runtime().spawn(async move {
        let res = lexi_host::run_chat(&chat, |ev| {
            let _ = tx.send(match ev {
                StreamEvent::Delta(t) => Event::Delta(t.to_string()),
                StreamEvent::Activity("") => return,
                StreamEvent::Activity(l) => Event::Activity(l.to_string()),
            });
        })
        .await;
        let _ = tx.send(match res {
            Ok(res) => Event::Done(chat.result_value(&res, cite, raw)),
            Err(e) => Event::Failed(e.to_string()),
        });
    });
    Ok(ChatStream {
        provider: provider.to_string(),
        rx: Mutex::new(Some(rx)),
        started: false,
        task: task.abort_handle(),
    })
}

// --- jobs -------------------------------------------------------------------

#[pyfunction]
fn _job_status<'py>(py: Python<'py>, provider: String, id: String) -> PyResult<Bound<'py, PyAny>> {
    ensure_home()?;
    let out = block_on(py, async move {
        let st = lexi_host::operation_status(&provider, &id).await?;
        Ok(st.to_value(&provider))
    })?;
    to_py(py, &out)
}

#[pyfunction]
fn _job_wait<'py>(
    py: Python<'py>,
    provider: String,
    id: String,
    interval: f64,
    timeout: f64,
) -> PyResult<Bound<'py, PyAny>> {
    ensure_home()?;
    if !interval.is_finite() || interval <= 0.0 || !timeout.is_finite() || timeout < 0.0 {
        return Err(PyValueError::new_err(
            "interval must be > 0 and timeout >= 0",
        ));
    }
    let polls = ((timeout / interval).ceil() as usize).max(1);
    let out = block_on(py, async move {
        let st =
            lexi_host::wait_operation(&provider, &id, Duration::from_secs_f64(interval), polls)
                .await?;
        Ok(st.to_value(&provider))
    })?;
    to_py(py, &out)
}

// --- media ------------------------------------------------------------------

#[pyfunction]
#[pyo3(signature = (provider, prompt, model, out_dir))]
fn _generate<'py>(
    py: Python<'py>,
    provider: String,
    prompt: String,
    model: Option<String>,
    out_dir: PathBuf,
) -> PyResult<Bound<'py, PyAny>> {
    ensure_home()?;
    std::fs::create_dir_all(&out_dir).map_err(lexi_err)?;
    let saved = block_on(py, {
        let provider = provider.clone();
        async move {
            lexi_host::generate_images_text(&provider, &prompt, model.as_deref(), &out_dir).await
        }
    })?;
    let artifacts: Vec<Value> = saved
        .iter()
        .map(
            |a| json!({ "path": a.path.display().to_string(), "revised_prompt": a.revised_prompt }),
        )
        .collect();
    to_py(py, &json!({ "provider": provider, "artifacts": artifacts }))
}

#[pyfunction]
fn _upload<'py>(py: Python<'py>, provider: String, path: PathBuf) -> PyResult<Bound<'py, PyAny>> {
    ensure_home()?;
    let file_id = block_on(py, {
        let provider = provider.clone();
        async move { lexi_host::upload_file(&provider, &path).await }
    })?;
    to_py(py, &json!({ "provider": provider, "file_id": file_id }))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("LexiError", m.py().get_type_bound::<LexiError>())?;
    m.add_class::<ChatStream>()?;
    m.add_function(wrap_pyfunction!(_prepare, m)?)?;
    m.add_function(wrap_pyfunction!(_respond, m)?)?;
    m.add_function(wrap_pyfunction!(_submit, m)?)?;
    m.add_function(wrap_pyfunction!(_stream, m)?)?;
    m.add_function(wrap_pyfunction!(_job_status, m)?)?;
    m.add_function(wrap_pyfunction!(_job_wait, m)?)?;
    m.add_function(wrap_pyfunction!(_generate, m)?)?;
    m.add_function(wrap_pyfunction!(_upload, m)?)?;
    Ok(())
}
