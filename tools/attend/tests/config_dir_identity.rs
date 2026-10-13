//! attend resolves its session under the Claude Code config directory it
//! runs in: `$CLAUDE_CONFIG_DIR/sessions`, then `~/.claude/sessions`. A
//! session in a non-default target (ADR-184) resolves to itself, and its
//! record wins over one in `~/.claude` naming the same process.

mod common;
use common::Fixture;

fn machine(f: &Fixture, config_dir: &std::path::Path) -> String {
    let out = f.attend_with(&["peer", "whoami", "--machine"], &[("CLAUDE_CONFIG_DIR", config_dir.as_os_str())]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn record(dir: &std::path::Path, sid: &str, cwd: &str) {
    let sessions = dir.join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let pid = std::process::id();
    std::fs::write(sessions.join(format!("{pid}.json")), format!(r#"{{"pid":{pid},"sessionId":"{sid}","cwd":"{cwd}"}}"#)).unwrap();
}

#[test]
fn a_session_under_a_non_default_config_dir_resolves_to_itself() {
    let f = Fixture::new("config-dir-self");
    // Only the target's config directory knows this session.
    std::fs::remove_file(f.home.join(".claude").join("sessions").join(format!("{}.json", std::process::id()))).unwrap();
    let target = f.home.join("claude-b");
    record(&target, "sess-target", "/work/b");
    let out = machine(&f, &target);
    assert!(out.contains("session_id=sess-target"), "{out}");
    assert!(out.contains("resolved=true"), "{out}");
}

#[test]
fn the_config_dirs_record_wins_over_home() {
    let f = Fixture::new("config-dir-wins");
    let target = f.home.join("claude-b");
    record(&target, "sess-target", "/work/b");
    let out = machine(&f, &target);
    assert!(out.contains("session_id=sess-target"), "{out}");
    assert!(!out.contains(&format!("session_id={}", f.sid)), "not the ~/.claude session: {out}");
}
