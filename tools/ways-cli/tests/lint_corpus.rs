//! ADR-701 §3 corpus checks through the real binary: a broken See Also
//! reference is an error in a core root, so `--check` exits 1, and a warning in
//! a user root, so the same lint exits 0.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

struct Fx {
    root: PathBuf,
}

impl Fx {
    fn new(name: &str) -> Fx {
        let root = std::env::temp_dir().join(format!("ways-lint-corpus-e2e-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["config", "cache", "state", "runtime", "proj", ".claude/hooks/ways"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        // The fallback schema for a root that carries none.
        std::fs::copy(schema(), root.join(".claude/hooks/ways/frontmatter-schema.yaml")).unwrap();
        Fx { root }
    }

    fn way(root: &Path, id: &str, see_also: &str) {
        let leaf = id.rsplit('/').next().unwrap();
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        let text = format!(
            "---\ndescription: the {leaf} way\nvocabulary: giraffe\nrefire: 0.15\npattern: giraffe\n---\n# {leaf}\n\n## See Also\n\n{see_also}\n"
        );
        std::fs::write(dir.join(format!("{leaf}.md")), text).unwrap();
    }

    /// Run `ways author lint --check <path>`; (exit code, stdout and stderr).
    fn lint(&self, path: &Path) -> (i32, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_ways"))
            .args(["author", "lint", "--check"])
            .arg(path)
            .current_dir(self.root.join("proj"))
            .env("HOME", &self.root)
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("XDG_DATA_HOME", self.root.join(".local/share"))
            .env("XDG_RUNTIME_DIR", self.root.join("runtime"))
            .env_remove("CLAUDE_PROJECT_DIR")
            .env_remove("CLAUDE_CONFIG_DIR")
            .output()
            .expect("run ways");
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        (out.status.code().unwrap_or(-1), text)
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn schema() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../hooks/ways/frontmatter-schema.yaml")
}

const BROKEN: &str = "- gone(dev) — names no way";

#[test]
fn a_broken_reference_fails_check_in_a_core_root() {
    let fx = Fx::new("core");
    let core = fx.root.join("checkout/hooks/ways");
    std::fs::create_dir_all(&core).unwrap();
    std::fs::copy(schema(), core.join("frontmatter-schema.yaml")).unwrap();
    Fx::way(&core, "dev/newway", BROKEN);

    let (code, out) = fx.lint(&core);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("ERROR: dev/newway/newway.md — See Also target `gone(dev)` names no way"), "{out}");

    // A subtree of the core root is judged the same way.
    let (code, out) = fx.lint(&core.join("dev"));
    assert_eq!(code, 1, "{out}");
}

#[test]
fn the_same_reference_is_a_warning_in_a_user_root() {
    let fx = Fx::new("user");
    let user = fx.root.join("config/agent-ways/ways");
    Fx::way(&user, "dev/newway", BROKEN);

    let (code, out) = fx.lint(&user);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("WARNING: dev/newway/newway.md — See Also target `gone(dev)` names no way"), "{out}");
    assert!(out.contains("0 errors, 1 warnings"), "{out}");
}
