// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `ways hook pre-compact`: the classic PreCompact hook.
//!
//! Claude Code adds what this hook prints to a compaction's instructions.
//! When the compaction already carries instructions (an operator's
//! `/compact <focus>`, or the attend mod's relay compaction), the hook prints
//! nothing, so the handoff is not given twice. Otherwise it prints the
//! repository's handoff as instructions when the handoff is fresh, and the
//! wrap skill's handoff outline when it is not.
//!
//! The hook always exits 0 and never blocks: a block during context-limit
//! recovery fails the request.
//!
//! References:
//! - ADR-417 item 3: a PreCompact hook steers each compaction with the wrap's handoff.

use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

use ways_core::handoff;

/// How long attend has to name the session's last wrap id.
const ATTEND_WAIT: Duration = Duration::from_secs(3);

/// Read the payload on stdin and print the instructions. Every failure ends
/// in the outline or in nothing, never in an error.
pub(super) fn run() {
    let mut raw = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut raw);
    let out = respond(&raw, OsStr::new("attend"), &local_today);
    if !out.is_empty() {
        use std::io::Write;
        let mut stdout = std::io::stdout().lock();
        let _ = writeln!(stdout, "{out}");
        let _ = stdout.flush();
    }
}

/// The instructions for the payload `raw`, asking the program `attend` for
/// the wrap id and `today` for the local date (`YYYY-MM-DD`).
pub(super) fn respond(raw: &str, attend: &OsStr, today: &dyn Fn() -> String) -> String {
    let payload: serde_json::Value = serde_json::from_str(raw).unwrap_or_default();
    let text_at = |key: &str| payload.get(key).and_then(serde_json::Value::as_str).unwrap_or("");
    if !text_at("custom_instructions").trim().is_empty() {
        return String::new();
    }
    let cwd = text_at("cwd");
    if cwd.is_empty() {
        return handoff::OUTLINE.to_string();
    }
    let Some(root) = handoff::repo_root(Path::new(cwd)) else { return handoff::OUTLINE.to_string() };
    let path = handoff::path(&root);
    let Ok(text) = std::fs::read_to_string(&path) else { return handoff::OUTLINE.to_string() };
    let fresh = match wrap_id(attend) {
        Some(id) => handoff::is_fresh_for(&text, &id),
        None => handoff::head_short(&root).is_some_and(|head| handoff::is_fresh_by_head(&text, &head, &today())),
    };
    if fresh {
        handoff::instructions(&text, &path)
    } else {
        handoff::OUTLINE.to_string()
    }
}

/// The wrap id attend's ledger recorded for this session's last wrap, by
/// `attend keepwarm wrap-id`. attend finds the session by process ancestry.
/// `None` when attend is absent, fails, prints nothing, or does not answer
/// within [`ATTEND_WAIT`].
fn wrap_id(attend: &OsStr) -> Option<String> {
    let mut cmd = std::process::Command::new(attend);
    cmd.args(["keepwarm", "wrap-id"]);
    let out = crate::cmd::proc::output_within(cmd, ATTEND_WAIT).ok()?;
    let id = out.trim();
    (!id.is_empty()).then(|| id.to_string())
}

/// Today's local date as `YYYY-MM-DD`, by `date +%F` as the wrap skill's
/// shell writes it; the UTC date when `date` cannot say.
fn local_today() -> String {
    std::process::Command::new("date")
        .arg("+%F")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|d| d.len() == 10)
        .unwrap_or_else(|| agent_fmt::when::utc_date(agent_fmt::when::now_secs()))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::test_dir::TestDir;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    const ID: &str = "20261126T2140Z-3f9a1c2e";
    const TODAY: &str = "2026-11-26";

    fn today() -> String {
        TODAY.to_string()
    }

    fn git(dir: &Path, args: &[&str]) -> String {
        let o = std::process::Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    /// A repository with one commit, and a scratch directory beside it for
    /// stub programs. Returns the guard, the repository and its short HEAD.
    fn repo(tag: &str) -> (TestDir, PathBuf, String) {
        let top = TestDir::at(std::env::temp_dir().join(format!("ways-pre-compact-{tag}-{}", std::process::id()))).resolved();
        let root = top.join("repo");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "init"]);
        let head = git(&root, &["rev-parse", "--short", "HEAD"]);
        (top, root, head)
    }

    fn write_handoff(root: &Path, first_line: &str) -> PathBuf {
        let path = handoff::path(root);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("{first_line}\nContinue the demo.\n\nTASKS — recreate these.\n#1 [OPEN] one\n\nDO NEXT — start #1.\n")).unwrap();
        path
    }

    /// A stub attend that prints `out` and exits `code` for `keepwarm wrap-id`.
    fn stub_attend(dir: &Path, out: &str, code: i32) -> PathBuf {
        let bin = dir.join("attend");
        std::fs::write(
            &bin,
            format!("#!/bin/sh\n[ \"$1 $2\" = \"keepwarm wrap-id\" ] || exit 2\nprintf '%s' '{out}'\nexit {code}\n"),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        bin
    }

    fn payload(cwd: &Path, custom: Option<&str>) -> String {
        serde_json::json!({
            "session_id": "3f9a1c2e-0000-0000-0000-000000000000",
            "hook_event_name": "PreCompact",
            "trigger": "auto",
            "custom_instructions": custom,
            "cwd": cwd,
        })
        .to_string()
    }

    #[test]
    fn custom_instructions_print_nothing() {
        let (top, root, head) = repo("custom");
        write_handoff(&root, &format!("{TODAY} {head} wrap={ID}"));
        let attend = stub_attend(&top, ID, 0);
        let out = respond(&payload(&root, Some("keep the parser work")), attend.as_os_str(), &today);
        assert_eq!(out, "");
        let blank = respond(&payload(&root, Some("  \n")), attend.as_os_str(), &today);
        assert!(blank.starts_with(handoff::LEAD), "blank instructions count as none: {blank}");
    }

    #[test]
    fn a_handoff_carrying_the_recorded_wrap_id_is_given() {
        let (top, root, _) = repo("byid");
        // Another day and commit on the first line: the id alone decides.
        let path = write_handoff(&root, &format!("2026-01-01 abcdef12 wrap={ID}"));
        let attend = stub_attend(&top, &format!("{ID}\n"), 0);
        let out = respond(&payload(&root, None), attend.as_os_str(), &today);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(out, handoff::instructions(&text, &path));
    }

    #[test]
    fn a_handoff_from_another_wrap_gives_the_outline() {
        let (top, root, head) = repo("staleid");
        write_handoff(&root, &format!("{TODAY} {head} wrap=20261126T2100Z-3f9a1c2e"));
        let attend = stub_attend(&top, ID, 0);
        assert_eq!(respond(&payload(&root, None), attend.as_os_str(), &today), handoff::OUTLINE);
    }

    #[test]
    fn without_attend_head_and_date_decide() {
        let (top, root, head) = repo("nohead");
        let path = write_handoff(&root, &format!("{TODAY} {head} wrap={ID}"));
        let text = std::fs::read_to_string(&path).unwrap();
        let missing = top.join("no-such-attend");
        assert_eq!(respond(&payload(&root, None), missing.as_os_str(), &today), handoff::instructions(&text, &path));
        // attend present but with no recorded wrap: the same fallback.
        let none = stub_attend(&top, "", 1);
        assert_eq!(respond(&payload(&root, None), none.as_os_str(), &today), handoff::instructions(&text, &path));
        let empty = stub_attend(&top, "", 0);
        assert_eq!(respond(&payload(&root, None), empty.as_os_str(), &today), handoff::instructions(&text, &path));
        // Another day fails the fallback.
        let tomorrow = || "2026-11-27".to_string();
        assert_eq!(respond(&payload(&root, None), missing.as_os_str(), &tomorrow), handoff::OUTLINE);
    }

    #[test]
    fn no_handoff_gives_the_outline() {
        let (top, root, _) = repo("nofile");
        let attend = stub_attend(&top, ID, 0);
        assert_eq!(respond(&payload(&root, None), attend.as_os_str(), &today), handoff::OUTLINE);
    }

    #[test]
    fn bad_input_gives_the_outline() {
        let (top, _, _) = repo("bad");
        let attend = stub_attend(&top, ID, 0);
        assert_eq!(respond("", attend.as_os_str(), &today), handoff::OUTLINE);
        assert_eq!(respond("{not json", attend.as_os_str(), &today), handoff::OUTLINE);
        let outside = top.join("not-a-repo");
        std::fs::create_dir_all(&outside).unwrap();
        assert_eq!(respond(&payload(&outside, None), attend.as_os_str(), &today), handoff::OUTLINE);
    }

    #[test]
    fn a_hung_attend_is_killed_and_the_fallback_decides() {
        let (top, root, head) = repo("hung");
        let path = write_handoff(&root, &format!("{TODAY} {head} wrap={ID}"));
        let bin = top.join("attend");
        std::fs::write(&bin, "#!/bin/sh\nexec sleep 30\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let start = std::time::Instant::now();
        let out = respond(&payload(&root, None), bin.as_os_str(), &today);
        assert!(start.elapsed() < Duration::from_secs(10), "{:?}", start.elapsed());
        assert_eq!(out, handoff::instructions(&std::fs::read_to_string(&path).unwrap(), &path));
    }
}
