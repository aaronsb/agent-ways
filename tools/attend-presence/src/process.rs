//! What attend reads about another process: its parent and its command
//! line. Linux reads `/proc` directly; other Unix systems ask `ps`, and
//! Windows asks PowerShell.

#[cfg(not(windows))]
use std::process::Command;

/// Whether `/proc` is mounted. Without it (a minimal container), Linux
/// asks `ps` like the other Unix systems.
#[cfg(target_os = "linux")]
fn proc_mounted() -> bool {
    std::path::Path::new("/proc/self/stat").exists()
}

/// The parent pid of `pid`, or `None` when it cannot be read.
pub fn parent_pid(pid: u32) -> Option<u32> {
    #[cfg(target_os = "linux")]
    if proc_mounted() {
        return parse_stat_ppid(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?);
    }
    parent_pid_by_command(pid)
}

/// The fourth field of `/proc/<pid>/stat`. The second field, the command
/// name, is in parentheses and may itself hold spaces or `)`, so the
/// fields are counted from the last `)`.
#[cfg(any(target_os = "linux", test))]
fn parse_stat_ppid(stat: &str) -> Option<u32> {
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(1)?.parse::<u32>().ok().filter(|&p| p > 0)
}

#[cfg(not(windows))]
fn parent_pid_by_command(pid: u32) -> Option<u32> {
    let output = Command::new("ps").args(["-p", &pid.to_string(), "-o", "ppid="]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout).trim().parse::<u32>().ok().filter(|&p| p > 0)
}

#[cfg(windows)]
fn parent_pid_by_command(pid: u32) -> Option<u32> {
    let script = format!("(Get-CimInstance Win32_Process -Filter 'ProcessId={pid}').ParentProcessId");
    let output = powershell(&script)?;
    output.trim().parse::<u32>().ok().filter(|&p| p > 0)
}

/// The command line of `pid` as arguments, or `None` when the process is
/// gone or cannot be read. Linux reads `/proc/<pid>/cmdline` exactly;
/// elsewhere `ps -ww` prints the line untruncated and it is split on
/// whitespace. On Windows the one argument is the executable's path.
pub fn argv(pid: u32) -> Option<Vec<String>> {
    #[cfg(target_os = "linux")]
    if proc_mounted() {
        let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
        return Some(
            raw.split(|&b| b == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).into_owned())
                .collect(),
        );
    }
    argv_by_command(pid)
}

#[cfg(not(windows))]
fn argv_by_command(pid: u32) -> Option<Vec<String>> {
    let out = Command::new("ps").args(["-ww", "-p", &pid.to_string(), "-o", "args="]).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).split_whitespace().map(str::to_string).collect())
}

#[cfg(windows)]
fn argv_by_command(pid: u32) -> Option<Vec<String>> {
    let path = powershell(&format!("(Get-Process -Id {pid} -ErrorAction SilentlyContinue).Path"))?;
    let path = path.trim();
    (!path.is_empty()).then(|| vec![path.to_string()])
}

#[cfg(windows)]
fn powershell(script: &str) -> Option<String> {
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .ok()?;
    output.status.success().then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The working directory of `pid`, or `None` when it cannot be read.
/// Linux reads the `/proc/<pid>/cwd` link; other Unix systems ask `lsof`.
pub fn cwd(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    if proc_mounted() {
        return std::fs::read_link(format!("/proc/{pid}/cwd")).ok().map(|p| p.to_string_lossy().into_owned());
    }
    cwd_by_command(pid)
}

#[cfg(not(windows))]
fn cwd_by_command(pid: u32) -> Option<String> {
    // `-b` skips the kernel calls that can block on a hung network mount;
    // `-w` drops the warnings `-b` then prints.
    let out = Command::new("lsof").args(["-b", "-w", "-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"]).output().ok()?;
    String::from_utf8_lossy(&out.stdout).lines().find_map(|l| l.strip_prefix('n').map(str::to_string))
}

#[cfg(windows)]
fn cwd_by_command(_pid: u32) -> Option<String> {
    None
}

/// Whether a process holds `pid`. A cheap test before [`start_time`]: on Unix
/// a signal-0 `kill`, which asks `ps` nothing; elsewhere the start time is
/// read.
pub fn is_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = libc::pid_t::try_from(pid) else { return false };
        // SAFETY: signal 0 delivers nothing; the call only checks the pid.
        let ret = unsafe { libc::kill(pid, 0) };
        ret == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        start_time(pid).is_some()
    }
}

/// When `pid` started, as an opaque string that differs between two
/// processes that had the same pid. `None` when the process is gone.
pub fn start_time(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    if proc_mounted() {
        // Field 22 (starttime, in clock ticks since boot), counted from
        // the last `)` like the parent pid.
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let rest = &stat[stat.rfind(')')? + 1..];
        return rest.split_whitespace().nth(19).map(str::to_string);
    }
    start_time_by_command(pid)
}

#[cfg(not(windows))]
fn start_time_by_command(pid: u32) -> Option<String> {
    let out = Command::new("ps").args(["-p", &pid.to_string(), "-o", "lstart="]).output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).split_whitespace().collect::<Vec<_>>().join("_");
    (out.status.success() && !s.is_empty()).then_some(s)
}

#[cfg(windows)]
fn start_time_by_command(pid: u32) -> Option<String> {
    let s = powershell(&format!("(Get-Process -Id {pid} -ErrorAction SilentlyContinue).StartTime.Ticks"))?;
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// Whether `pid` is alive and running Claude Code: some argument of its
/// command line names `claude`. The full line is read, not `comm`:
/// background sessions exec the versioned binary directly, so `comm` shows
/// the version string rather than `claude`.
pub fn is_claude(pid: u32) -> bool {
    argv(pid).is_some_and(|args| {
        args.iter().any(|a| if cfg!(windows) { a.to_lowercase().contains("claude") } else { a.contains("claude") })
    })
}

/// Whether `pid` runs the Claude Code program itself (see
/// [`is_claude_argv`]).
pub fn is_claude_program(pid: u32) -> bool {
    argv(pid).is_some_and(|args| is_claude_argv(&args))
}

/// Whether a command line runs the Claude Code program itself, not one that
/// merely mentions it. The program (`argv[0]`) is `claude` or a path under a
/// `claude/versions/` install; or it is node, bun or deno and the script is
/// Claude Code's entry point: an npm-global `claude` (`node /usr/bin/claude`),
/// `@anthropic-ai/claude-code/`, or the Agent SDK's bundled
/// `@anthropic-ai/claude-agent-sdk/cli.js`. A script that only lives under a
/// `.claude/` folder (an operator's hook or statusline), a shell sourcing a
/// snapshot under `~/.claude/`, and a wrapper such as `timeout claude` are not.
pub fn is_claude_argv(args: &[String]) -> bool {
    let unix = |s: &str| s.replace('\\', "/");
    let base = |s: &str| {
        let b = std::path::Path::new(&unix(s)).file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        b.trim_end_matches(".exe").trim_end_matches(".cmd").to_string()
    };
    let Some(program) = args.first() else { return false };
    if base(program) == "claude" || unix(program).contains("/claude/versions/") {
        return true;
    }
    if !matches!(base(program).as_str(), "node" | "bun" | "deno") {
        return false;
    }
    // The script is the first argument that is not an interpreter flag.
    let Some(script) = args.iter().skip(1).find(|a| !a.starts_with('-')) else { return false };
    let script = unix(script);
    base(&script) == "claude"
        || script.contains("@anthropic-ai/claude-code/")
        || script.contains("@anthropic-ai/claude-agent-sdk/cli.js")
}

/// The nearest ancestor of `pid` that is the Claude Code program itself,
/// within 15 hops. A shell whose command line merely mentions `claude`
/// (the Bash tool sources a snapshot under `~/.claude/`) is skipped.
pub fn claude_ancestor(pid: u32) -> Option<u32> {
    const SHELLS: &[&str] = &["sh", "bash", "zsh", "fish", "dash"];
    let mut cur = parent_pid(pid)?;
    for _ in 0..15 {
        let program = argv(cur).and_then(|a| a.into_iter().next()).unwrap_or_default();
        let base = std::path::Path::new(&program).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if !SHELLS.contains(&base.as_str()) && is_claude(cur) {
            return Some(cur);
        }
        cur = parent_pid(cur)?;
    }
    None
}

/// Whether `ancestor` is in `pid`'s ancestry, `pid` itself included,
/// within 15 hops.
pub fn has_ancestor(pid: u32, ancestor: u32) -> bool {
    let mut cur = pid;
    for _ in 0..15 {
        if cur == ancestor {
            return true;
        }
        if cur <= 1 {
            break;
        }
        match parent_pid(cur) {
            Some(ppid) if ppid != cur => cur = ppid,
            _ => break,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_parent_survives_a_command_name_with_parens_and_spaces() {
        assert_eq!(parse_stat_ppid("42 (a) b) S 7 42 42 0"), Some(7));
        assert_eq!(parse_stat_ppid("1 (init) S 0 1 1"), None);
        assert_eq!(parse_stat_ppid("garbage"), None);
    }

    #[cfg(unix)]
    #[test]
    fn this_process_has_a_parent_and_a_command_line() {
        let me = std::process::id();
        let parent = parent_pid(me).expect("a test process has a parent");
        assert!(has_ancestor(me, parent));
        assert!(has_ancestor(me, me));
        assert!(!argv(me).expect("own command line").is_empty());
        let started = start_time(me).expect("own start time");
        assert_eq!(start_time(me).as_deref(), Some(started.as_str()), "stable");
        assert_ne!(start_time(parent), Some(started), "another process differs");
    }
}
