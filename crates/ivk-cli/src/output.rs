//! Helpers for the JSON output convention.
//!
//! Every agent-facing command emits a JSON object that includes at least:
//!
//!   - `ok`           : bool — overall success
//!   - `command`      : string — identifier of what ran (e.g. "ws.new")
//!   - `next_command` : string | null — what to do next, in shell form
//!   - `error`        : object | null — `{ code, message }` on failure
//!
//! plus command-specific payload fields.
//!
//! When `--agent` is also set, an additional `recommended_next_steps` array
//! gives a short imperative list the agent can follow without re-asking.

use serde::Serialize;

/// Common output envelope. Command-specific payloads embed via flatten.
#[derive(Serialize)]
pub struct Envelope<T: Serialize> {
    pub ok: bool,
    pub command: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_next_steps: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBlock>,
    #[serde(flatten)]
    pub data: T,
}

#[derive(Serialize)]
pub struct ErrorBlock {
    pub code: &'static str,
    pub message: String,
}

pub fn print_json<T: Serialize>(env: &Envelope<T>) {
    match serde_json::to_string_pretty(env) {
        Ok(s) => println!("{}", s),
        Err(e) => {
            // Last-resort: make the JSON shape obvious even when serialization fails.
            eprintln!(
                r#"{{ "ok": false, "command": "{}", "error": {{ "code": "json_serialize_failed", "message": "{}" }} }}"#,
                env.command, e
            );
        }
    }
}

/// Returns true if any of the args is the literal `--json`.
pub fn wants_json(args: &[&str]) -> bool {
    args.contains(&"--json")
}

/// Returns true if any of the args is the literal `--agent`.
pub fn wants_agent(args: &[&str]) -> bool {
    args.contains(&"--agent")
}
