// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The poll of a `command` sensor: its argv run directly, with no shell,
//! in its `cwd` (the project when unset) with its `env` added, killed once
//! its timeout passes. Its stdout is one JSON document (`output: json`) or
//! `magnitude|description` lines (`output: lines`). A run that cannot
//! start, times out, exits non-zero or prints what does not parse is a
//! failed poll, and the failure says which.
//!
//! References:
//! - ADR-403 §1: the command kind's block and its recorded failures.

use std::io::Read;
use std::path::Path;
use std::process::{Command as Process, Stdio};
use std::time::{Duration, Instant};

use sensor_spec::{CommandBlock, Output};

use super::hosted::{Fetched, Source};

/// The most stdout a run may print, in bytes.
const MAX_OUTPUT: u64 = 1024 * 1024;

/// A command sensor's poll, for the project at `project`.
pub struct Command {
    block: CommandBlock,
    project: String,
}

impl Command {
    pub fn new(block: CommandBlock, project: &str) -> Command {
        Command { block, project: project.to_string() }
    }
}

impl Source for Command {
    fn fetch(&mut self) -> Result<Fetched, String> {
        let out = run(&self.block, Path::new(&self.project))?;
        match self.block.output {
            Output::Json => serde_json::from_str(&out).map(Fetched::Json).map_err(|e| format!("its output is not one JSON document: {e}")),
            Output::Lines => lines(&out).map(Fetched::Lines),
        }
    }
}

/// Run `block` from `project` and return its stdout.
fn run(block: &CommandBlock, project: &Path) -> Result<String, String> {
    let Some((program, args)) = block.argv.split_first() else { return Err("its argv is empty".into()) };
    let cwd = block.cwd.as_ref().map_or(project.to_path_buf(), |c| project.join(c));
    let mut child = Process::new(program)
        .args(args)
        .envs(&block.env)
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    let reader = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(p) = pipe {
                p.take(MAX_OUTPUT + 1).read_to_end(&mut buf).ok();
            }
            buf
        })
    };
    let stdout = reader(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let stderr = reader(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let deadline = Instant::now() + Duration::from_secs(block.timeout);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                child.kill().ok();
                child.wait().ok();
                return Err(format!("{program} timed out after {}s", block.timeout));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("cannot wait for {program}: {e}")),
        }
    };
    let out = stdout.join().unwrap_or_default();
    let err = stderr.join().unwrap_or_default();
    if !status.success() {
        let first = String::from_utf8_lossy(&err).lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().to_string();
        let code = status.code().map_or("a signal".to_string(), |c| format!("status {c}"));
        return Err(match first.is_empty() {
            true => format!("{program} exited with {code}"),
            false => format!("{program} exited with {code}: {first}"),
        });
    }
    if out.len() as u64 > MAX_OUTPUT {
        return Err(format!("{program} printed more than {} KiB", MAX_OUTPUT / 1024));
    }
    String::from_utf8(out).map_err(|_| format!("{program} printed text that is not UTF-8"))
}

/// `magnitude|description` lines, blank lines skipped.
fn lines(text: &str) -> Result<Vec<(f64, String)>, String> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let (m, d) = l.split_once('|').ok_or_else(|| format!("its output line `{l}` is not magnitude|description"))?;
            let m: f64 = m.trim().parse().map_err(|_| format!("its output line `{l}` has no number before `|`"))?;
            if !(0.0..=100.0).contains(&m) {
                return Err(format!("its output line `{l}` has a magnitude outside 0 to 100"));
            }
            Ok((m, d.trim().to_string()))
        })
        .collect()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn block(script: &str, output: Output, timeout: u64) -> CommandBlock {
        CommandBlock { argv: vec!["sh".into(), "-c".into(), script.into()], env: BTreeMap::from([("X_SENSOR".into(), "from-env".into())]), cwd: None, timeout, output }
    }

    fn fetch(script: &str, output: Output, timeout: u64) -> Result<Fetched, String> {
        Command::new(block(script, output, timeout), "/").fetch()
    }

    #[test]
    fn json_output_reads_as_one_document_with_env_and_cwd() {
        let Ok(Fetched::Json(v)) = fetch(r#"printf '{"env":"%s","cwd":"%s"}' "$X_SENSOR" "$(pwd)""#, Output::Json, 10) else { panic!() };
        assert_eq!(v, serde_json::json!({"env": "from-env", "cwd": "/"}));
    }

    #[test]
    fn lines_output_reads_magnitude_and_description() {
        let Ok(Fetched::Lines(l)) = fetch("printf '2|one\\n\\n0.5 | two\\n'", Output::Lines, 10) else { panic!() };
        assert_eq!(l, vec![(2.0, "one".to_string()), (0.5, "two".to_string())]);
        assert_eq!(fetch("echo nope", Output::Lines, 10).unwrap_err(), "its output line `nope` is not magnitude|description");
        assert_eq!(fetch("echo '200|x'", Output::Lines, 10).unwrap_err(), "its output line `200|x` has a magnitude outside 0 to 100");
    }

    #[test]
    fn each_failure_says_what_failed() {
        let started = Instant::now();
        assert_eq!(fetch("sleep 5", Output::Json, 1).unwrap_err(), "sh timed out after 1s");
        assert!(started.elapsed() < Duration::from_secs(4), "the run is killed at its timeout");
        assert_eq!(fetch("echo broken >&2; exit 3", Output::Json, 10).unwrap_err(), "sh exited with status 3: broken");
        assert_eq!(fetch("exit 4", Output::Json, 10).unwrap_err(), "sh exited with status 4");
        assert!(fetch("echo '{not json'", Output::Json, 10).unwrap_err().starts_with("its output is not one JSON document: "));
        let missing = CommandBlock { argv: vec!["/nonexistent/sensor-cmd".into()], env: BTreeMap::new(), cwd: None, timeout: 10, output: Output::Json };
        assert!(Command::new(missing, "/").fetch().unwrap_err().starts_with("cannot run /nonexistent/sensor-cmd: "));
    }
}
