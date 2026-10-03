//! `ways-audit lint` resolves a relative policy URI against fixed bases, not
//! the working directory: the app under `$XDG_DATA_HOME/agent-ways` (where the
//! shipped `governance/` lives since ADR-142), the `~/.claude` projection, and
//! the project whose ways are linted.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

const URI: &str = "governance/policies/only-in-app.md";

struct Fx {
    root: PathBuf,
}

impl Fx {
    /// A sandboxed HOME whose shipped ways hold one way claiming `URI`.
    fn new(name: &str) -> Fx {
        let root = std::env::temp_dir().join(format!("ways-audit-uri-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let fx = Fx { root };
        let way = fx.home().join(".claude/hooks/ways/dom/claimed");
        std::fs::create_dir_all(&way).unwrap();
        std::fs::write(way.join("claimed.md"), "---\ndescription: a claimed way\nrefire: 0.15\n---\n# claimed\n").unwrap();
        std::fs::write(
            way.join("provenance.yaml"),
            format!(
                "policy:\n  - uri: {URI}\n    type: governance-doc\ncontrols:\n  - id: TEST-1\n    justifications:\n      - a reason\nverified: 2026-10-03\nrationale: a rationale\n"
            ),
        )
        .unwrap();
        for d in ["config", "cache", "state", "elsewhere"] {
            std::fs::create_dir_all(fx.home().join(d)).unwrap();
        }
        fx
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn place(&self, base: &Path) {
        let f = base.join(URI);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, "# policy\n").unwrap();
    }

    /// `ways-audit --global lint --json` from `cwd`: (exit code, output).
    fn lint(&self, cwd: &Path) -> (Option<i32>, String) {
        let home = self.home();
        let out = Command::new(env!("CARGO_BIN_EXE_ways-audit"))
            .args(["--global", "--json", "lint"])
            .current_dir(cwd)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("XDG_CACHE_HOME", home.join("cache"))
            .env("XDG_STATE_HOME", home.join("state"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env_remove("CLAUDE_PROJECT_DIR")
            .env_remove("CLAUDE_CONFIG_DIR")
            .output()
            .expect("run ways-audit");
        (out.status.code(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)))
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn a_uri_present_only_in_the_app_resolves() {
    let fx = Fx::new("app");
    fx.place(&fx.home().join(".local/share/agent-ways"));
    let (code, out) = fx.lint(&fx.home().join("elsewhere"));
    assert_eq!(code, Some(0), "{out}");
    assert!(!out.contains("policy URI not found"), "{out}");
}

#[test]
fn a_uri_present_only_under_the_working_directory_does_not_resolve() {
    let fx = Fx::new("cwd");
    let cwd = fx.home().join("elsewhere");
    fx.place(&cwd);
    let (code, out) = fx.lint(&cwd);
    assert_eq!(code, Some(1), "{out}");
    assert!(out.contains("policy URI not found"), "{out}");
}
