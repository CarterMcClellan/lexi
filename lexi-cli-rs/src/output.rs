//! Output for commands: human text or JSON (`--json`), to caller-supplied writers.
//!
//! Commands never print directly; they go through [`Io`], so the same command
//! code serves the terminal binary, tests, and in-process hosts (the desktop
//! app's CLI console) that capture output.

use std::fmt::Display;
use std::io::Write;

use serde_json::{json, Value};

/// Exit code: success.
pub const EXIT_OK: u8 = 0;
/// Exit code: the command ran but failed (provider/HTTP/config/IO error).
pub const EXIT_FAILURE: u8 = 1;
/// Exit code: usage error (bad arguments) — clap's convention.
pub const EXIT_USAGE: u8 = 2;

/// A writer that can be handed across threads (needed for `Send` futures).
pub type Writer<'a> = &'a mut (dyn Write + Send);

/// How a host runs commands.
#[derive(Debug, Clone, Copy)]
pub struct RunOptions {
    /// ANSI styling of stderr notes.
    pub color: bool,
    /// Allow `sh` / `!cmd` (off for in-process hosts like the desktop console).
    pub allow_shell: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            color: false,
            allow_shell: true,
        }
    }
}

pub struct Io<'a> {
    pub out: Writer<'a>,
    pub err: Writer<'a>,
    /// Machine mode: emit JSON instead of human text.
    pub json: bool,
    /// ANSI styling on stderr notes (a terminal that allows color).
    pub color: bool,
}

impl<'a> Io<'a> {
    pub fn new(out: Writer<'a>, err: Writer<'a>, json: bool) -> Self {
        Self {
            out,
            err,
            json,
            color: false,
        }
    }

    /// Emit a command result: `json` in machine mode, else the `human` text
    /// (a trailing newline is added when missing).
    pub fn result(&mut self, human: impl Display, json: Value) -> u8 {
        if self.json {
            self.json_line(&json);
        } else {
            let text = human.to_string();
            let _ = write!(self.out, "{text}");
            if !text.ends_with('\n') {
                let _ = writeln!(self.out);
            }
        }
        let _ = self.out.flush();
        EXIT_OK
    }

    /// Print one compact JSON value on its own line (a result, or one NDJSON event).
    pub fn json_line(&mut self, value: &Value) {
        let _ = writeln!(self.out, "{value}");
        let _ = self.out.flush();
    }

    /// Report a failure on stderr — `{"error": "..."}` in machine mode — and
    /// return [`EXIT_FAILURE`].
    pub fn fail(&mut self, msg: impl Display) -> u8 {
        if self.json {
            let _ = writeln!(self.err, "{}", json!({ "error": msg.to_string() }));
        } else {
            let _ = writeln!(self.err, "error: {msg}");
        }
        let _ = self.err.flush();
        EXIT_FAILURE
    }

    /// A non-fatal warning on stderr (human text in both modes, so stdout
    /// stays clean JSON).
    pub fn warn(&mut self, msg: impl Display) {
        let _ = writeln!(self.err, "warning: {msg}");
        let _ = self.err.flush();
    }
}

/// Render a titled table (rounded box, like the Python CLI's `rich` tables).
pub fn table(title: &str, headers: &[&str], rows: Vec<Vec<String>>) -> String {
    let mut t = comfy_table::Table::new();
    t.load_preset(comfy_table::presets::UTF8_FULL_CONDENSED)
        .apply_modifier(comfy_table::modifiers::UTF8_ROUND_CORNERS)
        .set_content_arrangement(comfy_table::ContentArrangement::Dynamic)
        .set_header(headers.iter().copied());
    // Wrap to the terminal; when not a terminal (piped, captured), to 100 cols.
    if t.width().is_none() {
        t.set_width(100);
    }
    for row in rows {
        t.add_row(row);
    }
    format!("{title}\n{t}")
}

/// Pretty-print a JSON value (2-space indent).
pub fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

/// A JSON scalar as display text (strings unquoted; null as empty).
pub fn scalar(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    }
}

/// Dim (de-emphasized) text: ANSI faint when `color`, plain otherwise.
pub fn dim(text: &str, color: bool) -> String {
    if color {
        format!("\x1b[2m{text}\x1b[0m")
    } else {
        text.to_string()
    }
}
