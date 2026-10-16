//! The lifecycle convention (ADR-511) on the verbs other than `update`
//! (`tests/update_whatif.rs` has that one): without `--commit` a verb prints
//! what it would change and changes nothing; with `--commit` it applies; the
//! hidden `reconcile` applies with no flag, because another release's
//! updater and the installer call it so.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Fx {
    root: PathBuf,
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

impl Fx {
    fn new(name: &str) -> Fx {
        let root = std::env::temp_dir().join(format!("ways-lifecycle-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["home/.config", "home/.local/share", "home/.local/state", "home/.claude", "cache", "project", "other"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let src = root.join("src");
        write(&src.join("skills/wrap/SKILL.md"), "---\nx\n");
        write(&src.join("hooks/ways/meta/a.md"), "---\nx\n");
        write(&src.join("hooks/check-config-updates.sh"), "#!/bin/sh\n");
        write(
            &src.join("settings.json"),
            r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"${HOME}/.claude/hooks/ways/check-setup.sh"}]}]}}"#,
        );
        Fx { root }
    }

    fn src(&self) -> PathBuf {
        self.root.join("src")
    }

    fn ways(&self, args: &[&str]) -> Output {
        let home = self.root.join("home");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ways"));
        cmd.args(args)
            .current_dir(self.root.join("project"))
            .env("PWD", self.root.join("project"))
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("PATH", "/usr/bin:/bin")
            .env("NO_COLOR", "1");
        for var in ["CLAUDE_PROJECT_DIR", "CLAUDE_CONFIG_DIR", "CLAUDE_SESSION_ID", "WAYS_CLAUDE_BIN", "XDG_BIN_HOME", "CLICOLOR_FORCE", "FORCE_COLOR"] {
            cmd.env_remove(var);
        }
        cmd.output().unwrap()
    }

    /// Every path under `dir`, with each symlink's target.
    fn tree(&self, dir: &Path) -> Vec<(PathBuf, Option<PathBuf>)> {
        let mut out = Vec::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                out.push((p.clone(), std::fs::read_link(&p).ok()));
                if p.is_dir() && std::fs::read_link(&p).is_err() {
                    out.extend(self.tree(&p));
                }
            }
        }
        out.sort();
        out
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

#[test]
fn repair_reports_until_commit_and_reconcile_still_applies() {
    let fx = Fx::new("repair");
    let src = fx.src();
    let dest = fx.root.join("other");
    let (s, d) = (src.to_str().unwrap(), dest.to_str().unwrap());

    let before = fx.tree(&dest);
    let out = fx.ways(&["manage", "repair", "--source", s, "--dest", d]);
    assert!(out.status.success(), "{}", text(&out));
    assert_eq!(fx.tree(&dest), before, "the default changes nothing");
    let t = text(&out);
    assert!(t.contains("ways manage repair: what-if") && t.contains("would link skills") && t.contains("`ways manage repair --commit`"), "{t}");
    assert!(!t.contains('\x1b'), "plain in a pipe");
    assert!(t.contains(&format!("── settings.json — {d}")) && t.contains("+ add       SessionStart:"), "the merge is previewed: {t}");
    assert!(!dest.join("settings.json").exists(), "and not written");

    let out = fx.ways(&["manage", "repair", "--commit", "--source", s, "--dest", d]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(std::fs::read_link(dest.join("skills")).is_ok(), "--commit links: {}", text(&out));

    // The fixed name applies with no flag, and reports with --dry-run.
    let fresh = fx.root.join("home/.claude");
    let f = fresh.to_str().unwrap();
    let out = fx.ways(&["reconcile", "--dry-run", "--source", s, "--dest", f]);
    assert!(out.status.success() && std::fs::read_link(fresh.join("skills")).is_err(), "{}", text(&out));
    let out = fx.ways(&["reconcile", "--source", s, "--dest", f]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(std::fs::read_link(fresh.join("skills")).is_ok(), "reconcile applies with no flag: {}", text(&out));
}

#[test]
fn target_verbs_report_until_commit() {
    let fx = Fx::new("target");
    let src = fx.src();
    // The app the target verbs project from is $XDG_DATA_HOME/agent-ways.
    let app = fx.root.join("home/.local/share/agent-ways");
    std::os::unix::fs::symlink(&src, &app).unwrap();
    let other = fx.root.join("other");
    let o = other.to_str().unwrap();
    let config = fx.root.join("home/.config/agent-ways/config.yaml");
    let recorded = || std::fs::read_to_string(&config).unwrap_or_default();

    let out = fx.ways(&["manage", "target", "add", o]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    assert!(t.contains(&format!("ways manage target add: what-if for {o}")) && t.contains(&format!("`ways manage target add {o} --commit`")), "{t}");
    for section in ["── Roots", "── settings.json"] {
        assert!(t.contains(section), "{section}: {t}");
    }
    assert!(t.contains("  + skills"), "a root to link carries the add mark: {t}");
    let plan = text(&fx.ways(&["manage", "target", "plan", o]));
    assert!(plan.starts_with(&format!("ways manage target plan: {o}\n")) && plan.contains("── Roots"), "{plan}");
    assert!(std::fs::read_link(other.join("skills")).is_err() && !recorded().contains(o), "add changed nothing: {t}");

    let out = fx.ways(&["manage", "target", "add", o, "--commit"]);
    assert!(out.status.success(), "{}", text(&out));
    assert!(std::fs::read_link(other.join("skills")).is_ok() && recorded().contains(o), "{}", text(&out));
    let after_add = recorded();

    for verb in ["disable", "remove"] {
        let out = fx.ways(&["manage", "target", verb, o]);
        let t = text(&out);
        assert!(out.status.success(), "{verb}: {t}");
        assert!(t.contains(&format!("ways manage target {verb}: what-if")) && t.contains("would unlink"), "{verb}: {t}");
        assert_eq!(recorded(), after_add, "{verb} kept the record");
        assert!(std::fs::read_link(other.join("skills")).is_ok(), "{verb} kept the links");
    }
    let out = fx.ways(&["manage", "target", "disable", o, "--commit"]);
    assert!(out.status.success() && std::fs::read_link(other.join("skills")).is_err(), "{}", text(&out));
    let out = fx.ways(&["manage", "target", "enable", o]);
    assert!(out.status.success() && std::fs::read_link(other.join("skills")).is_err(), "enable reports: {}", text(&out));
    assert!(text(&out).contains("would link"), "{}", text(&out));
}

#[test]
fn uninstall_and_reinstall_take_commit_and_change_nothing_without_it() {
    let fx = Fx::new("uninstall");
    let app = fx.root.join("home/.local/share/agent-ways");
    std::os::unix::fs::symlink(fx.src(), &app).unwrap();
    let out = fx.ways(&["manage", "uninstall"]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    assert!(t.contains("Nothing changed. Run `ways manage uninstall --commit` to apply."), "{t}");
    assert!(t.starts_with("ways manage uninstall: what-if\n") && t.contains("── Delete") && t.contains("  - "), "{t}");
    assert!(app.exists() && fx.src().join("skills/wrap/SKILL.md").is_file(), "the app is kept");
    // --commit and --what-if conflict; --yes is --commit's alias.
    let out = fx.ways(&["manage", "uninstall", "--commit", "--what-if"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    let out = fx.ways(&["manage", "reinstall", "--yes", "--purge"]);
    assert_eq!(out.status.code(), Some(2), "--yes parses and --purge is refused: {}", text(&out));
    assert!(text(&out).contains("ways manage uninstall --purge --commit"), "{}", text(&out));
}

/// Run `ways` with stdout and stderr on a pty, and return what it drew.
#[cfg(target_os = "linux")]
fn on_pty(fx: &Fx, args: &[&str], env: &[(&str, &str)]) -> String {
    use std::io::Read;
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::process::Stdio;
    let (mut m, mut s) = (0, 0);
    let ws = libc::winsize { ws_row: 40, ws_col: 120, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: openpty fills two descriptors we then own.
    assert_eq!(unsafe { libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null(), &ws) }, 0);
    // SAFETY: fresh descriptors from openpty.
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(m), OwnedFd::from_raw_fd(s)) };
    let home = fx.root.join("home");
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ways"));
    cmd.args(args)
        .current_dir(fx.root.join("project"))
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CACHE_HOME", fx.root.join("cache"))
        .env("PATH", "/usr/bin:/bin")
        .env("TERM", "xterm-256color");
    for var in ["NO_COLOR", "CLICOLOR_FORCE", "FORCE_COLOR", "COLORTERM", "CLAUDE_CONFIG_DIR", "XDG_BIN_HOME"] {
        cmd.env_remove(var);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::from(slave.try_clone().unwrap())).stderr(Stdio::from(slave));
    let mut child = cmd.spawn().unwrap();
    // The command keeps its copies of the slave until dropped, and a read of
    // the master only ends once every copy is closed.
    drop(cmd);
    let mut file = std::fs::File::from(master);
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    // Reading the master ends with EIO once the child has closed the slave.
    while let Ok(n) = file.read(&mut buf) {
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
    }
    child.wait().unwrap();
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(target_os = "linux")]
#[test]
fn no_color_and_a_dumb_terminal_draw_no_escapes_on_a_terminal() {
    let fx = Fx::new("pty");
    let (s, d) = (fx.src(), fx.root.join("other"));
    let args = ["manage", "repair", "--source", s.to_str().unwrap(), "--dest", d.to_str().unwrap()];
    let coloured = on_pty(&fx, &args, &[]);
    assert!(coloured.contains('\x1b') && coloured.contains("what-if"), "a terminal gets colour: {coloured:?}");
    for env in [[("NO_COLOR", "1")], [("TERM", "dumb")]] {
        let plain = on_pty(&fx, &args, &env);
        assert!(plain.contains("ways manage repair: what-if"), "{env:?}: {plain:?}");
        assert!(!plain.contains('\x1b'), "{env:?} draws no escape, attributes included: {plain:?}");
    }
}

#[test]
fn a_repair_from_a_checkout_does_not_call_the_installed_app_a_dev_build() {
    let fx = Fx::new("source-links");
    // The installed app, and a dev checkout projected with --source.
    let app = fx.root.join("home/.local/share/agent-ways");
    std::fs::create_dir_all(app.join("bin")).unwrap();
    std::fs::create_dir_all(app.join(".git")).unwrap();
    std::fs::write(app.join("bin/ways"), "").unwrap();
    let src = fx.src();
    std::fs::create_dir_all(src.join(".git")).unwrap();
    write(&src.join("tools/suite-bins"), "ways\n");
    let dest = fx.root.join("other");
    std::fs::create_dir_all(dest.join("bin")).unwrap();
    std::os::unix::fs::symlink(app.join("bin/ways"), dest.join("bin/ways")).unwrap();
    let out = fx.ways(&["manage", "repair", "--source", src.to_str().unwrap(), "--dest", dest.to_str().unwrap()]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    assert!(!t.contains("Development setup"), "a link into the installed app is the install's own: {t}");
}
