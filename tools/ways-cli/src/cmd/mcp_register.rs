//! Registration of the agent-ways MCP server with Claude Code (ADR-501).
//!
//! A user-scope MCP server lives in Claude Code's `.claude.json`, which
//! settings.json cannot declare. Running sessions rewrite that file, so every
//! change goes through Claude Code's own CLI (`claude mcp add-json` and
//! `claude mcp remove`, user scope); only the "is it current?" check reads the
//! file directly.
//!
//! The file sits beside the default target, `~/.claude.json` for `~/.claude`,
//! and inside any other target: Claude Code resolves it as
//! `$CLAUDE_CONFIG_DIR/.claude.json` when that variable is set. Each target
//! registers the `ways-mcp` it projects under `<target>/bin/`, so an update
//! reaches the registration through the symlink and withdrawal removes both.
//!
//! Registration never fails a reconcile: a missing `claude` binary, a missing
//! `ways-mcp` build, or a failed CLI call is reported and the rest proceeds.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// The name Claude Code knows the server by; tools appear as
/// `mcp__agent-ways__<tool>`.
pub const SERVER: &str = "agent-ways";
/// The projected binary, relative to a target.
const BIN: &str = "bin/ways-mcp";

/// What to do about one target's registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Registered with the command wanted: nothing to do.
    Current,
    Add,
    /// Registered with another `ways-mcp` path (a moved install): replace.
    Replace,
    /// An `agent-ways` entry that runs something other than `ways-mcp`: the
    /// operator's own, left alone.
    Foreign(String),
    Remove,
    /// Nothing registered and nothing to add or remove.
    Absent,
}

fn is_ours(command: &str) -> bool {
    Path::new(command).file_stem().and_then(|s| s.to_str()) == Some("ways-mcp")
}

pub fn plan_converge(existing: Option<&str>, desired: &str) -> Step {
    match existing {
        None => Step::Add,
        Some(cmd) if cmd == desired => Step::Current,
        Some(cmd) if is_ours(cmd) => Step::Replace,
        Some(cmd) => Step::Foreign(cmd.to_string()),
    }
}

pub fn plan_withdraw(existing: Option<&str>) -> Step {
    match existing {
        None => Step::Absent,
        Some(cmd) if is_ours(cmd) => Step::Remove,
        Some(cmd) => Step::Foreign(cmd.to_string()),
    }
}

/// The `CLAUDE_CONFIG_DIR` to run the CLI under, and the directory whose
/// config file it writes. Claude Code reads `$CLAUDE_CONFIG_DIR/.claude.json`,
/// or `~/.claude.json` when the variable is unset. The default target runs
/// without it, unless the operator's own environment already points it at
/// that target.
fn config_for(dest_root: &Path, default_root: &Path, inherited: Option<&Path>) -> (Option<PathBuf>, PathBuf) {
    let same = super::reconcile::same_path;
    if same(dest_root, default_root) && !inherited.is_some_and(|d| same(d, dest_root)) {
        (None, default_root.parent().unwrap_or(default_root).to_path_buf())
    } else {
        (Some(dest_root.to_path_buf()), dest_root.to_path_buf())
    }
}

pub(crate) fn config_dir(dest_root: &Path, default_root: &Path) -> (Option<PathBuf>, PathBuf) {
    let inherited = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from);
    config_for(dest_root, default_root, inherited.as_deref())
}

/// The file Claude Code reads in `dir`: a legacy `.config.json` wins when one
/// exists, as it does for Claude Code.
fn config_file(dir: &Path) -> PathBuf {
    let legacy = dir.join(".config.json");
    if legacy.is_file() {
        legacy
    } else {
        dir.join(".claude.json")
    }
}

/// The command the registered `agent-ways` entry runs, if any.
fn registered_command(file: &Path) -> Option<String> {
    let text = std::fs::read_to_string(file).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json.get("mcpServers")?.get(SERVER)?.get("command")?.as_str().map(str::to_string)
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// The `claude` binary: `WAYS_CLAUDE_BIN` when set (tests), else the first
/// executable `claude` on PATH.
pub(crate) fn claude_bin() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("WAYS_CLAUDE_BIN") {
        return Some(PathBuf::from(p));
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).map(|d| d.join("claude")).find(|p| is_executable(p))
    })
}

fn run_claude(claude: &Path, config_dir: Option<&Path>, args: &[&str]) -> Result<(), String> {
    claude_stdout(claude, config_dir, args).map(|_| ())
}

/// How long one `claude` call may run before it is killed. The calls are
/// local (the CLI's own config files and a folder marketplace), so a call
/// past this is hung, and a repair must not hang with it.
pub(crate) const CLAUDE_TIMEOUT: Duration = Duration::from_secs(60);

/// Run the `claude` CLI under `config_dir` (unset for the default target) and
/// answer its stdout, or its stderr when it fails or times out.
pub(crate) fn claude_stdout(claude: &Path, config_dir: Option<&Path>, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new(claude);
    cmd.args(args);
    match config_dir {
        Some(d) => cmd.env("CLAUDE_CONFIG_DIR", d),
        None => cmd.env_remove("CLAUDE_CONFIG_DIR"),
    };
    output_within(cmd, CLAUDE_TIMEOUT)
}

/// Run `cmd` with its output captured, killing it once `limit` has passed.
/// Its stdout on success; its stderr, its exit code, or the timeout otherwise.
pub(crate) fn output_within(mut cmd: Command, limit: Duration) -> Result<String, String> {
    use std::io::Read;
    use std::process::Stdio;
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    // Read both pipes on threads, so a child that fills one never blocks.
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            buf
        })
    };
    let out = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {}s", limit.as_secs()));
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if status.success() {
        Ok(String::from_utf8_lossy(&stdout).into_owned())
    } else {
        let e = String::from_utf8_lossy(&stderr).trim().to_string();
        Err(if e.is_empty() { format!("exit {}", status.code().unwrap_or(-1)) } else { e })
    }
}

/// Register the server for an enabled target. Returns a line to report, or
/// `None` when nothing changed.
pub fn converge(source_root: &Path, dest_root: &Path, default_root: &Path, dry_run: bool) -> Option<String> {
    // Built means present in the source; a dry run has not linked it yet.
    if !source_root.join(BIN).exists() {
        return Some(format!("mcp: {SERVER} not registered: {} is not built", source_root.join(BIN).display()));
    }
    let desired = dest_root.join(BIN).to_string_lossy().to_string();
    let (config_dir, dir) = config_dir(dest_root, default_root);
    let file = config_file(&dir);
    let step = plan_converge(registered_command(&file).as_deref(), &desired);
    let verb = match &step {
        Step::Current => return None,
        Step::Foreign(cmd) => {
            return Some(format!(
                "mcp: {} already names an `{SERVER}` server running {cmd}; left as it is",
                file.display()
            ))
        }
        Step::Add => "register",
        Step::Replace => "re-register",
        Step::Remove | Step::Absent => unreachable!("converge plans no removal"),
    };
    if dry_run {
        return Some(format!("mcp: would {verb} {SERVER} → {desired}"));
    }
    let Some(claude) = claude_bin() else {
        return Some(format!("mcp: {SERVER} not registered: `claude` is not on PATH; re-run `ways manage repair` once it is"));
    };
    let spec = serde_json::json!({ "type": "stdio", "command": desired, "args": [] }).to_string();
    let result = (|| {
        if step == Step::Replace {
            run_claude(&claude, config_dir.as_deref(), &["mcp", "remove", "--scope", "user", SERVER])?;
        }
        run_claude(&claude, config_dir.as_deref(), &["mcp", "add-json", "--scope", "user", SERVER, &spec])
    })();
    Some(match result {
        Ok(()) => format!("mcp: {verb}ed {SERVER} → {desired}"),
        // Registered in a file this check did not read (a variant name some
        // builds use): Claude Code has it, so there is nothing to do.
        Err(e) if e.contains("already exists") => {
            format!("mcp: {SERVER} is registered in a config file other than {}; left as it is", file.display())
        }
        Err(e) => format!("mcp: ⚠ could not {verb} {SERVER}: {e}"),
    })
}

/// Remove the server's registration from a disabled target, when it is ours.
pub fn withdraw(dest_root: &Path, default_root: &Path, dry_run: bool) -> Option<String> {
    let (config_dir, dir) = config_dir(dest_root, default_root);
    let file = config_file(&dir);
    match plan_withdraw(registered_command(&file).as_deref()) {
        Step::Absent | Step::Foreign(_) => None,
        _ if dry_run => Some(format!("mcp: would remove {SERVER}")),
        _ => {
            let Some(claude) = claude_bin() else {
                return Some(format!(
                    "mcp: ⚠ {SERVER} left registered in {}: `claude` is not on PATH",
                    file.display()
                ));
            };
            Some(match run_claude(&claude, config_dir.as_deref(), &["mcp", "remove", "--scope", "user", SERVER]) {
                Ok(()) => format!("mcp: removed {SERVER}"),
                Err(e) => format!("mcp: ⚠ could not remove {SERVER}: {e}"),
            })
        }
    }
}

/// For `ways status`: the command registered for `dest_root`, if any.
pub fn status(dest_root: &Path, default_root: &Path) -> Option<String> {
    registered_command(&config_file(&config_dir(dest_root, default_root).1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converge_adds_keeps_replaces_and_respects_foreign_entries() {
        let want = "/t/bin/ways-mcp";
        assert_eq!(plan_converge(None, want), Step::Add);
        assert_eq!(plan_converge(Some(want), want), Step::Current);
        assert_eq!(plan_converge(Some("/old/bin/ways-mcp"), want), Step::Replace);
        assert_eq!(plan_converge(Some("/usr/bin/node"), want), Step::Foreign("/usr/bin/node".into()));
    }

    #[test]
    fn withdraw_removes_only_ours() {
        assert_eq!(plan_withdraw(None), Step::Absent);
        assert_eq!(plan_withdraw(Some("/t/bin/ways-mcp")), Step::Remove);
        assert_eq!(plan_withdraw(Some("/usr/bin/node")), Step::Foreign("/usr/bin/node".into()));
    }

    #[test]
    fn default_target_uses_the_home_file_and_no_config_dir() {
        let home = Path::new("/home/u/.claude");
        assert_eq!(config_for(home, home, None), (None, PathBuf::from("/home/u")));
        let other = Path::new("/work/claude-b");
        assert_eq!(config_for(other, home, None), (Some(other.to_path_buf()), other.to_path_buf()));
    }

    #[test]
    fn an_inherited_config_dir_naming_the_default_target_is_honoured() {
        let home = Path::new("/home/u/.claude");
        assert_eq!(config_for(home, home, Some(home)), (Some(home.to_path_buf()), home.to_path_buf()));
        // One naming somewhere else leaves the default target's file alone.
        assert_eq!(config_for(home, home, Some(Path::new("/elsewhere"))), (None, PathBuf::from("/home/u")));
    }

    #[cfg(unix)]
    #[test]
    fn a_hung_call_is_killed_at_its_limit() {
        let mut slow = Command::new("sh");
        slow.args(["-c", "sleep 5"]);
        let started = Instant::now();
        let e = output_within(slow, Duration::from_millis(200)).unwrap_err();
        assert!(e.contains("timed out"), "{e}");
        assert!(started.elapsed() < Duration::from_secs(3), "killed, not waited out");
        let mut fine = Command::new("sh");
        fine.args(["-c", "echo out; echo err >&2"]);
        assert_eq!(output_within(fine, Duration::from_secs(5)).unwrap(), "out\n");
        let mut failing = Command::new("sh");
        failing.args(["-c", "echo why >&2; exit 3"]);
        assert_eq!(output_within(failing, Duration::from_secs(5)).unwrap_err(), "why");
    }

    #[test]
    fn a_legacy_config_file_is_the_one_read() {
        let dir = std::env::temp_dir().join(format!("ways-mcp-legacy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(config_file(&dir), dir.join(".claude.json"));
        std::fs::write(dir.join(".config.json"), "{}").unwrap();
        assert_eq!(config_file(&dir), dir.join(".config.json"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reads_the_registered_command() {
        let dir = std::env::temp_dir().join(format!("ways-mcp-register-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join(".claude.json");
        assert_eq!(registered_command(&f), None);
        std::fs::write(&f, r#"{"mcpServers":{"agent-ways":{"type":"stdio","command":"/t/bin/ways-mcp","args":[]}}}"#).unwrap();
        assert_eq!(registered_command(&f).as_deref(), Some("/t/bin/ways-mcp"));
        std::fs::write(&f, r#"{"mcpServers":{"other":{"command":"x"}}}"#).unwrap();
        assert_eq!(registered_command(&f), None);
        std::fs::remove_dir_all(&dir).ok();
    }
}
