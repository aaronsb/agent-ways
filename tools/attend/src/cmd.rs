//! Per-subcommand implementations.
//!
//! Extracted from `main.rs` in issue #51. Each submodule owns the
//! `cmd_*` function plus any helpers that are specific to that
//! command. Shared helpers used across multiple commands live in
//! `crate::util`. The top-level `cli_main()` is a thin dispatcher
//! that matches on argv and forwards to these modules. The message verbs'
//! modules here only render: their bodies live in `crate::verbs`.

pub(crate) mod cleanup;
pub(crate) mod config_cmd;
pub(crate) mod channels;
pub(crate) mod inbox;
pub(crate) mod keepwarm;
pub(crate) mod peers;
pub(crate) mod permissions;
pub(crate) mod run;
pub(crate) mod scene;
pub(crate) mod send;
pub(crate) mod sensors;
pub(crate) mod status;
pub(crate) mod tune;
pub(crate) mod whoami;

/// Print `value` as the one JSON document `--json` puts on stdout (ADR-185).
pub(crate) fn print_json<T: serde::Serialize>(value: &T) {
    match serde_json::to_string(value) {
        Ok(s) => println!("{s}"),
        Err(e) => eprintln!("[attend] could not encode the result as JSON: {e}"),
    }
}

/// The `--json` document for a verb that refused: `{"error": <kind and
/// fields>, "message": <one line>}`. The human wording still goes to stderr
/// and the exit code is unchanged.
pub(crate) fn print_json_error<T: serde::Serialize>(error: &T, message: &str) {
    print_json(&serde_json::json!({ "error": error, "message": message }));
}
