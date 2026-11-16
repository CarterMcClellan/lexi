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

pub struct Io<'a> {
    pub out: Writer<'a>,
    pub err: Writer<'a>,
    /// Machine mode: emit JSON instead of human text.
    pub json: bool,
}

impl<'a> Io<'a> {
    pub fn new(out: Writer<'a>, err: Writer<'a>, json: bool) -> Self {
        Self { out, err, json }
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
