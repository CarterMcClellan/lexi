//! Python bindings (PyO3) — the native `lexi._lexi` module.
//!
//! Two layers, both dict-in/dict-out (Python objects convert to/from
//! `serde_json::Value` via `pythonize`):
//! - this file: the pure `lexi-core` engine (compose request bodies, extract
//!   responses, validate definitions), pinned by the shared conformance suite;
//! - [`host`]: the `lexi-host` runtime (send prompts, stream, jobs, media),
//!   wrapped by the public functions in `python/lexi/`.

// PyO3 0.22's #[pyfunction] expansion converts the error return with `.into()`,
// which clippy flags as a useless PyErr->PyErr conversion on every function.
// It is generated code, not ours.
#![allow(clippy::useless_conversion)]

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pythonize::{depythonize, pythonize};
use serde_json::Value;

mod host;

// --- conversions ----------------------------------------------------------

fn from_py(obj: &Bound<'_, PyAny>) -> PyResult<Value> {
    depythonize(obj).map_err(|e| PyValueError::new_err(format!("{e}")))
}

fn opt_from_py(obj: Option<&Bound<'_, PyAny>>) -> PyResult<Option<Value>> {
    match obj {
        Some(o) if !o.is_none() => Ok(Some(from_py(o)?)),
        _ => Ok(None),
    }
}

fn to_py<'py>(py: Python<'py>, v: &Value) -> PyResult<Bound<'py, PyAny>> {
    pythonize(py, v).map_err(|e| PyValueError::new_err(format!("{e}")))
}

fn to_msg(obj: &Bound<'_, PyAny>) -> PyResult<lexi_core::Message> {
    Ok(lexi_core::message_from_dict(&from_py(obj)?))
}

// --- request composition --------------------------------------------------

#[pyfunction]
#[pyo3(signature = (api_def, prompt_payload=None, overrides=None, tool_presets=None,
                    set_paths=None, unset_paths=None, skip_validation=false))]
#[allow(clippy::too_many_arguments)]
fn compose_request_body<'py>(
    py: Python<'py>,
    api_def: &Bound<'py, PyAny>,
    prompt_payload: Option<&Bound<'py, PyAny>>,
    overrides: Option<&Bound<'py, PyAny>>,
    tool_presets: Option<Vec<String>>,
    set_paths: Option<&Bound<'py, PyAny>>,
    unset_paths: Option<Vec<String>>,
    skip_validation: bool,
) -> PyResult<Bound<'py, PyAny>> {
    let api = from_py(api_def)?;
    let pp = opt_from_py(prompt_payload)?.unwrap_or(Value::Null);
    let ov = opt_from_py(overrides)?.unwrap_or(Value::Null);
    let presets = tool_presets.unwrap_or_default();
    let sp: Vec<(String, Value)> = match opt_from_py(set_paths)? {
        Some(Value::Array(pairs)) => pairs
            .iter()
            .map(|p| (p[0].as_str().unwrap_or("").to_string(), p[1].clone()))
            .collect(),
        _ => Vec::new(),
    };
    let up = unset_paths.unwrap_or_default();
    let body = lexi_core::compose_request_body(&api, &pp, &ov, &presets, &sp, &up, skip_validation)
        .map_err(PyValueError::new_err)?;
    to_py(py, &body)
}

// --- message IR -----------------------------------------------------------

#[pyfunction]
fn message_from_dict<'py>(
    py: Python<'py>,
    data: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    to_py(py, &to_msg(data)?.to_value())
}

#[pyfunction]
fn normalize_prompt_payload<'py>(
    py: Python<'py>,
    payload: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    to_py(
        py,
        &lexi_core::normalize_prompt_payload(&from_py(payload)?).to_value(),
    )
}

#[pyfunction]
#[pyo3(signature = (message, content_mapping=None))]
fn lower_message<'py>(
    py: Python<'py>,
    message: &Bound<'py, PyAny>,
    content_mapping: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let msg = to_msg(message)?;
    let cm = opt_from_py(content_mapping)?;
    to_py(py, &lexi_core::lower_message(&msg, cm.as_ref()))
}

#[pyfunction]
fn to_prompt_bindings<'py>(
    py: Python<'py>,
    message: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let b = lexi_core::to_prompt_bindings(&to_msg(message)?).map_err(PyValueError::new_err)?;
    to_py(py, &b)
}

// --- response extraction --------------------------------------------------

#[pyfunction]
#[pyo3(signature = (response, text_extract=None))]
fn extract_text(
    response: &Bound<'_, PyAny>,
    text_extract: Option<&Bound<'_, PyAny>>,
) -> PyResult<String> {
    let resp = from_py(response)?;
    Ok(lexi_core::extract_text(
        &resp,
        opt_from_py(text_extract)?.as_ref(),
    ))
}

#[pyfunction]
#[pyo3(signature = (response, activity=None))]
fn summarize_activity(
    response: &Bound<'_, PyAny>,
    activity: Option<&Bound<'_, PyAny>>,
) -> PyResult<String> {
    let resp = from_py(response)?;
    Ok(lexi_core::summarize_activity(
        &resp,
        opt_from_py(activity)?.as_ref(),
    ))
}

#[pyfunction]
#[pyo3(signature = (response, text_extract=None))]
fn find_text_items<'py>(
    py: Python<'py>,
    response: &Bound<'py, PyAny>,
    text_extract: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let resp = from_py(response)?;
    let items = lexi_core::find_text_items(&resp, opt_from_py(text_extract)?.as_ref());
    to_py(py, &Value::Array(items))
}

#[pyfunction]
#[pyo3(signature = (response, annotations=None, text_items=None))]
fn extract_annotations<'py>(
    py: Python<'py>,
    response: &Bound<'py, PyAny>,
    annotations: Option<&Bound<'py, PyAny>>,
    text_items: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let resp = from_py(response)?;
    let items = match opt_from_py(text_items)? {
        Some(Value::Array(a)) => a,
        _ => Vec::new(),
    };
    let out = lexi_core::extract_annotations(&resp, opt_from_py(annotations)?.as_ref(), &items);
    to_py(py, &Value::Array(out))
}

#[pyfunction]
#[pyo3(signature = (annotations, annotations_cfg=None))]
fn render_annotations(
    annotations: &Bound<'_, PyAny>,
    annotations_cfg: Option<&Bound<'_, PyAny>>,
) -> PyResult<String> {
    let anns = match from_py(annotations)? {
        Value::Array(a) => a,
        _ => Vec::new(),
    };
    Ok(lexi_core::render_annotations(
        &anns,
        opt_from_py(annotations_cfg)?.as_ref(),
    ))
}

#[pyfunction]
#[pyo3(signature = (response, artifacts=None))]
fn extract_artifacts<'py>(
    py: Python<'py>,
    response: &Bound<'py, PyAny>,
    artifacts: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let resp = from_py(response)?;
    let out = lexi_core::extract_artifacts(&resp, opt_from_py(artifacts)?.as_ref());
    to_py(py, &Value::Array(out))
}

#[pyfunction]
fn get_path(py: Python<'_>, obj: &Bound<'_, PyAny>, path: String) -> PyResult<PyObject> {
    let v = from_py(obj)?;
    match lexi_core::get_path(&v, &path) {
        Some(found) => Ok(to_py(py, found)?.unbind()),
        None => Ok(py.None()),
    }
}

// --- capabilities ---------------------------------------------------------

#[pyfunction]
fn validate_definition(def: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    Ok(lexi_core::validate_definition(&from_py(def)?))
}

#[pyfunction]
fn cli_flags<'py>(py: Python<'py>, api_def: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyAny>> {
    let flags = lexi_core::cli_flags(&from_py(api_def)?);
    to_py(py, &Value::Array(flags))
}

#[pyfunction]
fn tool_preset_names(api_def: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    Ok(lexi_core::tool_preset_names(&from_py(api_def)?))
}

#[pyfunction]
fn preview_definition<'py>(
    py: Python<'py>,
    def: &Bound<'py, PyAny>,
    prompt: &str,
    model: &str,
    role: &str,
) -> PyResult<Bound<'py, PyAny>> {
    let result = lexi_core::preview_definition(&from_py(def)?, prompt, model, role);
    to_py(py, &result)
}

#[pyfunction]
#[pyo3(signature = (api_def, model_capabilities=None))]
fn resolve_capabilities<'py>(
    py: Python<'py>,
    api_def: &Bound<'py, PyAny>,
    model_capabilities: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let api = from_py(api_def)?;
    let cap = lexi_core::resolve_capabilities(&api, opt_from_py(model_capabilities)?.as_ref());
    to_py(py, &cap)
}

#[pyfunction]
fn supports_input(cap: &Bound<'_, PyAny>, modality: String) -> PyResult<bool> {
    Ok(lexi_core::supports_input(&from_py(cap)?, &modality))
}

#[pyfunction]
fn supports_output(cap: &Bound<'_, PyAny>, modality: String) -> PyResult<bool> {
    Ok(lexi_core::supports_output(&from_py(cap)?, &modality))
}

#[pyfunction]
fn validate_message(cap: &Bound<'_, PyAny>, message: &Bound<'_, PyAny>) -> PyResult<Vec<String>> {
    Ok(lexi_core::validate_message(
        &from_py(cap)?,
        &to_msg(message)?,
    ))
}

// --- operations -----------------------------------------------------------

#[pyfunction]
#[pyo3(signature = (response, operation=None))]
fn read_operation<'py>(
    py: Python<'py>,
    response: &Bound<'py, PyAny>,
    operation: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let resp = from_py(response)?;
    to_py(
        py,
        &lexi_core::read_operation(&resp, opt_from_py(operation)?.as_ref()),
    )
}

#[pyfunction]
fn poll_path(operation: &Bound<'_, PyAny>, operation_id: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(lexi_core::poll_path(
        &from_py(operation)?,
        &from_py(operation_id)?,
    ))
}

#[pyfunction]
#[pyo3(signature = (status, status_map=None))]
fn classify_status(
    status: &Bound<'_, PyAny>,
    status_map: Option<&Bound<'_, PyAny>>,
) -> PyResult<String> {
    Ok(lexi_core::classify_status(
        &from_py(status)?,
        opt_from_py(status_map)?.as_ref(),
    ))
}

// --- uploads --------------------------------------------------------------

#[pyfunction]
#[pyo3(signature = (response, files=None))]
fn read_upload<'py>(
    py: Python<'py>,
    response: &Bound<'py, PyAny>,
    files: Option<&Bound<'py, PyAny>>,
) -> PyResult<Bound<'py, PyAny>> {
    let resp = from_py(response)?;
    to_py(
        py,
        &lexi_core::read_upload(&resp, opt_from_py(files)?.as_ref()),
    )
}

#[pyfunction]
#[pyo3(signature = (files=None))]
fn upload_path(files: Option<&Bound<'_, PyAny>>) -> PyResult<String> {
    Ok(lexi_core::upload_path(opt_from_py(files)?.as_ref()))
}

// --- streaming ------------------------------------------------------------

#[pyfunction]
fn parse_sse_block<'py>(py: Python<'py>, block: &str) -> PyResult<Bound<'py, PyAny>> {
    to_py(py, &lexi_core::parse_sse_block(block))
}

#[pyfunction]
#[pyo3(signature = (event, data, stream_cfg))]
fn classify_event<'py>(
    py: Python<'py>,
    event: Option<&str>,
    data: &str,
    stream_cfg: &Bound<'py, PyAny>,
) -> PyResult<Bound<'py, PyAny>> {
    let cfg = from_py(stream_cfg)?;
    to_py(py, &lexi_core::classify_event(event, data, &cfg))
}

// --- module ---------------------------------------------------------------

#[pymodule]
fn _lexi(m: &Bound<'_, PyModule>) -> PyResult<()> {
    host::register(m)?;
    m.add_function(wrap_pyfunction!(compose_request_body, m)?)?;
    m.add_function(wrap_pyfunction!(message_from_dict, m)?)?;
    m.add_function(wrap_pyfunction!(normalize_prompt_payload, m)?)?;
    m.add_function(wrap_pyfunction!(lower_message, m)?)?;
    m.add_function(wrap_pyfunction!(to_prompt_bindings, m)?)?;
    m.add_function(wrap_pyfunction!(extract_text, m)?)?;
    m.add_function(wrap_pyfunction!(summarize_activity, m)?)?;
    m.add_function(wrap_pyfunction!(find_text_items, m)?)?;
    m.add_function(wrap_pyfunction!(extract_annotations, m)?)?;
    m.add_function(wrap_pyfunction!(render_annotations, m)?)?;
    m.add_function(wrap_pyfunction!(extract_artifacts, m)?)?;
    m.add_function(wrap_pyfunction!(get_path, m)?)?;
    m.add_function(wrap_pyfunction!(validate_definition, m)?)?;
    m.add_function(wrap_pyfunction!(cli_flags, m)?)?;
    m.add_function(wrap_pyfunction!(tool_preset_names, m)?)?;
    m.add_function(wrap_pyfunction!(preview_definition, m)?)?;
    m.add_function(wrap_pyfunction!(resolve_capabilities, m)?)?;
    m.add_function(wrap_pyfunction!(supports_input, m)?)?;
    m.add_function(wrap_pyfunction!(supports_output, m)?)?;
    m.add_function(wrap_pyfunction!(validate_message, m)?)?;
    m.add_function(wrap_pyfunction!(read_operation, m)?)?;
    m.add_function(wrap_pyfunction!(poll_path, m)?)?;
    m.add_function(wrap_pyfunction!(classify_status, m)?)?;
    m.add_function(wrap_pyfunction!(read_upload, m)?)?;
    m.add_function(wrap_pyfunction!(upload_path, m)?)?;
    m.add_function(wrap_pyfunction!(parse_sse_block, m)?)?;
    m.add_function(wrap_pyfunction!(classify_event, m)?)?;
    Ok(())
}
