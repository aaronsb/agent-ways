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
    assert!(t.contains("plan for") && t.contains(&format!("`ways manage target add {o} --commit`")), "{t}");
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
