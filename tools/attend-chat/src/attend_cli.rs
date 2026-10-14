//! The attend CLI verbs the chat's cleanup items run (ADR-404 D3e, ADR-405
//! amendment of 2026-10-13): `attend message delete` and `attend message
//! tombstone`. attend-chat has no crate dependency on attend, so it runs
//! the `attend` on `PATH` as a child process with `--json` and reads the
//! report. An attend too old to have a verb is told apart from one that
//! refused: the first needs updating, the second gave its reason.

use std::path::Path;

use crate::groups::BASE_CHANNEL_NAME;
use crate::tabs::Tab;

/// The attend the verbs run: the one installed beside this program, as the
/// suite installs them together, else `attend` on `PATH`.
pub fn program() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("attend")))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| "attend".into())
}

/// Which messages a delete reaches: `#open`, a channel, or the direct
/// messages between the person and one agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Open,
    Channel(String),
    /// The thread with the agent of this membership id (`--dm`).
    Dm(String),
}

impl Scope {
    /// The scope of a channel's menu: `#open` for the base channel.
    pub fn channel(name: &str) -> Scope {
        if name == BASE_CHANNEL_NAME {
            Scope::Open
        } else {
            Scope::Channel(name.to_string())
        }
    }

    /// The scope of the tab shown: `#open`, a channel, or the operator's DMs
    /// with an agent. The operator view and an agent's own DMs have none.
    pub fn of(tab: &Tab) -> Option<Scope> {
        match tab {
            Tab::Channel(c) => Some(Scope::channel(c)),
            Tab::Dm(m) => Some(Scope::Dm(m.clone())),
            Tab::Merged | Tab::Review(_) => None,
        }
    }

    /// The scope's flags for `attend message delete`. A thread is always
    /// the person's (`--as-person`): attend would otherwise take the caller
    /// to be a Claude Code session the chat was started under, and the
    /// delete would reach that session's thread with the agent.
    pub fn args(&self) -> Vec<String> {
        match self {
            Scope::Open => vec!["--open".into()],
            Scope::Channel(c) => vec!["--channel".into(), c.clone()],
            Scope::Dm(m) => vec!["--dm".into(), m.clone(), "--as-person".into()],
        }
    }

    /// The scope as a menu id carries it.
    pub fn key(&self) -> String {
        match self {
            Scope::Open => BASE_CHANNEL_NAME.into(),
            Scope::Channel(c) => format!("#{c}"),
            Scope::Dm(m) => format!("dm:{m}"),
        }
    }

    /// The scope a menu id carries ([`Scope::key`]).
    pub fn from_key(key: &str) -> Option<Scope> {
        if key == BASE_CHANNEL_NAME {
            return Some(Scope::Open);
        }
        if let Some(c) = key.strip_prefix('#') {
            return Some(Scope::channel(c));
        }
        key.strip_prefix("dm:").map(|m| Scope::Dm(m.to_string()))
    }
}

/// Delete older than: the submenu's choices and the `--older-than` each
/// passes. Custom… takes a duration or a local date instead ([`custom`]).
pub const OLDER_THAN: [(&str, &str); 3] = [("Current sessions", "current"), ("Today (local midnight)", "today"), ("All", "all")];

/// What Custom… takes: a duration such as `2h` or `3d`, or a local date
/// `YYYY-MM-DD`, as attend reads it; `Err` says what it takes.
pub fn custom(text: &str) -> Result<String, String> {
    let t = text.trim();
    if agent_fmt::when::parse_span(t).is_some() || agent_fmt::when::parse_local_date(t).is_some() {
        Ok(t.to_string())
    } else {
        Err("not a duration or a date YYYY-MM-DD".into())
    }
}

/// How a question reads `--older-than`'s value: `today` as from before
/// today, a date as from before it, a duration as older than it; `all` adds
/// nothing.
pub fn older_words(older: &str) -> String {
    match older {
        "all" => String::new(),
        "current" => " older than the current sessions".into(),
        "today" => " from before today".into(),
        d if agent_fmt::when::parse_local_date(d).is_some() => format!(" from before {d}"),
        d => format!(" older than {d}"),
    }
}

/// `attend message delete` for `scope` and `older`, reporting only with
/// `dry_run`.
pub fn delete_argv(scope: &Scope, older: &str, dry_run: bool) -> Vec<String> {
    let mut out: Vec<String> = vec!["message".into(), "delete".into()];
    out.extend(scope.args());
    out.extend(["--older-than".into(), older.to_string()]);
    if dry_run {
        out.push("--dry-run".into());
    }
    out.push("--json".into());
    out
}

/// `attend message delete --undo <batch>`.
pub fn undo_argv(batch: &str) -> Vec<String> {
    ["message", "delete", "--undo", batch, "--json"].map(String::from).to_vec()
}

/// `attend message tombstone <session>`.
pub fn tombstone_argv(session: &str) -> Vec<String> {
    ["message", "tombstone", session, "--json"].map(String::from).to_vec()
}

/// What a delete marked, or would mark on a dry run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub marked: usize,
    /// The batch `--undo` names; none on a dry run or when nothing was marked.
    pub batch: Option<String>,
    /// Each live session that kept messages it has not read: its name and
    /// how many.
    pub held: Vec<(String, usize)>,
}

impl Report {
    /// How many messages stay unread, and by how many sessions.
    pub fn kept(&self) -> (usize, usize) {
        (self.held.iter().map(|(_, n)| n).sum(), self.held.len())
    }

    /// `· 3 stay unread by 2 sessions`, or nothing when none stay.
    pub fn kept_words(&self) -> String {
        match self.kept() {
            (0, _) => String::new(),
            (n, s) => format!(" · {n} stay unread by {s} session{}", if s == 1 { "" } else { "s" }),
        }
    }
}

/// What an undo restored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Undone {
    pub unmarked: usize,
    pub already_removed: usize,
}

/// Why a verb did nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// No `attend` on `PATH`.
    NoAttend { verb: String },
    /// The installed attend lacks the verb or a flag of it.
    TooOld { verb: String },
    /// attend refused, in its own words.
    Refused { verb: String, why: String },
    /// attend ended with no report the chat can read.
    Broken { verb: String, why: String },
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::NoAttend { verb } => write!(f, "`attend {verb}` needs attend on PATH"),
            Failure::TooOld { verb } => write!(f, "`attend {verb}` is missing: needs a newer attend"),
            Failure::Refused { verb, why } => write!(f, "{}: {why}", verb.rsplit(' ').next().unwrap_or(verb)),
            Failure::Broken { verb, why } => write!(f, "attend {verb}: {why}"),
        }
    }
}

/// Run `program` with `args` and read its one JSON document. A document
/// with an `error` is attend's refusal; no document, from an attend whose
/// argument parser did not know the verb or a flag, means it is too old.
pub fn run(program: &Path, args: &[String]) -> Result<serde_json::Value, Failure> {
    let verb = args.iter().take(2).cloned().collect::<Vec<_>>().join(" ");
    let out = match output(program, args) {
        Ok(out) => out,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Failure::NoAttend { verb }),
        Err(e) => return Err(Failure::Broken { verb, why: e.to_string() }),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    match serde_json::from_str::<serde_json::Value>(stdout.trim()) {
        Ok(doc) if doc.get("error").is_some() => {
            let why = doc.get("message").and_then(|m| m.as_str()).unwrap_or("refused").to_string();
            Err(Failure::Refused { verb, why })
        }
        Ok(doc) if out.status.success() => Ok(doc),
        _ if ["unrecognized subcommand", "unexpected argument"].iter().any(|w| stderr.contains(w)) => Err(Failure::TooOld { verb }),
        _ => {
            let why = stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("no report").trim().to_string();
            Err(Failure::Broken { verb, why })
        }
    }
}

/// Run `program` with `args` to its end. A program file still held open
/// for writing (`ExecutableFileBusy`: an attend being installed, or a test's
/// fake while another thread forks) is tried again shortly.
fn output(program: &Path, args: &[String]) -> std::io::Result<std::process::Output> {
    let mut tries = 0;
    loop {
        match std::process::Command::new(program).args(args).stdin(std::process::Stdio::null()).output() {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy && tries < 50 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            other => return other,
        }
    }
}

/// The count `field` of a report, which every report of the verb carries:
/// one without it is not a report the chat can read.
fn count(doc: &serde_json::Value, field: &str, verb: &str) -> Result<usize, Failure> {
    doc.get(field).and_then(|v| v.as_u64()).map(|n| n as usize).ok_or_else(|| Failure::Broken { verb: verb.into(), why: format!("its report has no `{field}`") })
}

/// `attend message delete` for `scope` and `older`, or its dry run.
pub fn delete(program: &Path, scope: &Scope, older: &str, dry_run: bool) -> Result<Report, Failure> {
    let doc = run(program, &delete_argv(scope, older, dry_run))?;
    let held = doc
        .get("held")
        .and_then(|h| h.as_array())
        .into_iter()
        .flatten()
        .map(|h| Ok((h.get("name").and_then(|n| n.as_str()).unwrap_or("?").to_string(), count(h, "kept", "message delete")?)))
        .collect::<Result<_, Failure>>()?;
    let batch = doc.get("batch").and_then(|b| b.as_str()).map(str::to_string);
    Ok(Report { marked: count(&doc, "marked", "message delete")?, batch, held })
}

/// `attend message delete --undo <batch>`.
pub fn undo(program: &Path, batch: &str) -> Result<Undone, Failure> {
    let doc = run(program, &undo_argv(batch))?;
    Ok(Undone { unmarked: count(&doc, "unmarked", "message delete")?, already_removed: count(&doc, "already_removed", "message delete")? })
}

/// `attend message tombstone <session>`.
pub fn tombstone(program: &Path, session: &str) -> Result<(), Failure> {
    run(program, &tombstone_argv(session)).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn each_scope_and_age_builds_its_argv() {
        assert_eq!(delete_argv(&Scope::Open, "today", true), s(&["message", "delete", "--open", "--older-than", "today", "--dry-run", "--json"]));
        assert_eq!(delete_argv(&Scope::Channel("deploy".into()), "all", false), s(&["message", "delete", "--channel", "deploy", "--older-than", "all", "--json"]));
        assert_eq!(delete_argv(&Scope::Dm("s1".into()), "3d", false), s(&["message", "delete", "--dm", "s1", "--as-person", "--older-than", "3d", "--json"]));
        assert_eq!(undo_argv("17-4"), s(&["message", "delete", "--undo", "17-4", "--json"]));
        assert_eq!(tombstone_argv("s9"), s(&["message", "tombstone", "s9", "--json"]));
    }

    #[test]
    fn a_tab_gives_its_scope_and_a_menu_id_carries_it() {
        assert_eq!(Scope::of(&Tab::Channel("open".into())), Some(Scope::Open));
        assert_eq!(Scope::of(&Tab::Channel("deploy".into())), Some(Scope::Channel("deploy".into())));
        assert_eq!(Scope::of(&Tab::Dm("s1".into())), Some(Scope::Dm("s1".into())));
        assert_eq!(Scope::of(&Tab::Merged), None);
        assert_eq!(Scope::of(&Tab::Review("s1".into())), None);
        for scope in [Scope::Open, Scope::Channel("deploy".into()), Scope::Dm("s1".into())] {
            assert_eq!(Scope::from_key(&scope.key()), Some(scope));
        }
    }

    #[test]
    fn custom_takes_a_duration_or_a_date() {
        assert_eq!(custom(" 2h "), Ok("2h".into()));
        assert_eq!(custom("2026-10-01"), Ok("2026-10-01".into()));
        assert!(custom("yesterday").is_err() && custom("").is_err());
        assert_eq!(older_words("all"), "");
        assert_eq!(older_words("today"), " from before today");
        assert_eq!(older_words("3d"), " older than 3d");
        assert_eq!(older_words("2026-10-01"), " from before 2026-10-01");
    }

    /// A program in `dir` that prints `stdout` and `stderr` and exits `code`.
    fn fake(dir: &Path, name: &str, stdout: &str, stderr: &str, code: i32) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\nprintf '%s' '{stdout}'\nprintf '%s\\n' '{stderr}' >&2\nexit {code}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    #[cfg(unix)]
    #[test]
    fn a_report_a_refusal_and_an_old_attend_read_apart() {
        let dir = crate::test_dir::unique("attend-cli");
        let ok = fake(&dir, "ok", r##"{"scope":"#open","dry_run":true,"marked":12,"batch":null,"ids":[],"held":[{"session":"a","name":"Ada-x","kept":2},{"session":"b","name":"Bo-y","kept":1}]}"##, "", 0);
        let r = delete(&ok, &Scope::Open, "all", true).unwrap();
        assert_eq!(r, Report { marked: 12, batch: None, held: vec![("Ada-x".into(), 2), ("Bo-y".into(), 1)] });
        assert_eq!(r.kept_words(), " · 3 stay unread by 2 sessions");
        let refused = fake(&dir, "refused", r#"{"error":{"kind":"not_exited","session":"s9"},"message":"s9 has not exited"}"#, "attend message tombstone: s9 has not exited", 1);
        assert_eq!(tombstone(&refused, "s9").unwrap_err().to_string(), "tombstone: s9 has not exited", "attend's words, after the verb");
        let unread = fake(&dir, "unread", r##"{"scope":"#open","dry_run":true}"##, "", 0);
        assert_eq!(delete(&unread, &Scope::Open, "all", true).unwrap_err().to_string(), "attend message delete: its report has no `marked`", "no count is no zero");
        let old = fake(&dir, "old", "", "error: unrecognized subcommand 'delete'", 2);
        let e = delete(&old, &Scope::Open, "all", true).unwrap_err();
        assert_eq!(e.to_string(), "`attend message delete` is missing: needs a newer attend");
        let none = dir.join("no-attend");
        assert_eq!(tombstone(&none, "s9").unwrap_err().to_string(), "`attend message tombstone` needs attend on PATH");
        let crashed = fake(&dir, "crashed", "", "thread main panicked", 101);
        assert_eq!(undo(&crashed, "1").unwrap_err().to_string(), "attend message delete: thread main panicked");
        let _ = std::fs::remove_dir_all(dir);
    }
}
