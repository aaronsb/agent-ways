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

/// The `CLAUDE_CONFIG_DIR` to run the CLI under, and the file it writes. The
/// default target runs without the variable, as Claude Code itself does.
fn config_for(dest_root: &Path, default_root: &Path) -> (Option<PathBuf>, PathBuf) {
    if super::reconcile::same_path(dest_root, default_root) {
        let file = default_root.parent().unwrap_or(default_root).join(".claude.json");
        (None, file)
    } else {
        (Some(dest_root.to_path_buf()), dest_root.join(".claude.json"))
    }
}

/// The command the registered `agent-ways` entry runs, if any.
fn registered_command(file: &Path) -> Option<String> {
    let text = std::fs::read_to_string(file).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json.get("mcpServers")?.get(SERVER)?.get("command")?.as_str().map(str::to_string)
}

/// The `claude` binary: `WAYS_CLAUDE_BIN` when set (tests), else `claude` on PATH.
fn claude_bin() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("WAYS_CLAUDE_BIN") {
        return Some(PathBuf::from(p));
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).map(|d| d.join("claude")).find(|p| p.is_file())
    })
}

fn run_claude(claude: &Path, config_dir: Option<&Path>, args: &[&str]) -> Result<(), String> {
    let mut cmd = Command::new(claude);
    cmd.args(args);
    match config_dir {
        Some(d) => cmd.env("CLAUDE_CONFIG_DIR", d),
        None => cmd.env_remove("CLAUDE_CONFIG_DIR"),
    };
    let out = cmd.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Register the server for an enabled target. Returns a line to report, or
/// `None` when nothing changed.
pub fn converge(dest_root: &Path, default_root: &Path, dry_run: bool) -> Option<String> {
    let desired = dest_root.join(BIN);
    if !desired.exists() {
        return Some(format!("mcp: {SERVER} not registered: {} is not built", desired.display()));
    }
    let desired = desired.to_string_lossy().to_string();
    let (config_dir, file) = config_for(dest_root, default_root);
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
        return Some(format!("mcp: {SERVER} not registered: `claude` is not on PATH; re-run `ways reconcile` once it is"));
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
        Err(e) => format!("mcp: ⚠ could not {verb} {SERVER}: {e}"),
    })
}

/// Remove the server's registration from a disabled target, when it is ours.
pub fn withdraw(dest_root: &Path, default_root: &Path, dry_run: bool) -> Option<String> {
    let (config_dir, file) = config_for(dest_root, default_root);
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
    registered_command(&config_for(dest_root, default_root).1)
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
    fn default_target_uses_the_sibling_file_and_no_config_dir() {
        let home = Path::new("/home/u/.claude");
        assert_eq!(config_for(home, home), (None, PathBuf::from("/home/u/.claude.json")));
        let other = Path::new("/work/claude-b");
        assert_eq!(
            config_for(other, home),
            (Some(other.to_path_buf()), PathBuf::from("/work/claude-b/.claude.json"))
        );
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
