//! One definition of the ways roots (#797).
//!
//! `ways reconcile` points the projection (`~/.claude/hooks/ways`) at a dev
//! checkout, so the shipped ways a session reads are the checkout's, and the
//! app copy under `$XDG_DATA` is stale. Every reader must see the projected
//! tree and none the stale copy. Each test runs the real binary in a fixture
//! where the projection is a symlink to a tree other than the app copy.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// The marker way sits only in the projected tree; the stale way only in the
/// app copy.
const PROJECTED: &str = "dev/newway";
const STALE: &str = "stale/oldway";

struct Fx {
    root: PathBuf,
}

impl Fx {
    fn new(name: &str) -> Fx {
        let root = std::env::temp_dir().join(format!("ways-one-roots-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        for d in ["config", "cache", "state", "runtime", "proj"] {
            std::fs::create_dir_all(home.join(d)).unwrap();
        }
        std::fs::create_dir_all(home.join(".claude/hooks")).unwrap();
        let fx = Fx { root };
        let app = fx.home().join(".local/share/agent-ways/hooks/ways");
        let dev = fx.home().join("dev/hooks/ways");
        Self::way(&app, STALE, "zebra");
        Self::way(&dev, PROJECTED, "giraffe");
        for tree in [&app, &dev] {
            std::fs::copy(schema(), tree.join("frontmatter-schema.yaml")).unwrap();
        }
        std::os::unix::fs::symlink(&dev, fx.home().join(".claude/hooks/ways")).unwrap();
        fx
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn way(root: &Path, id: &str, word: &str) {
        let leaf = id.rsplit('/').next().unwrap();
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        let text = format!(
            "---\ndescription: the {leaf} way\nvocabulary: {word}\nrefire: 0.15\npattern: {word}\nrequires: [\"Bash({leaf}:*)\"]\n---\n# {leaf}\n"
        );
        std::fs::write(dir.join(format!("{leaf}.md")), text).unwrap();
    }

    /// Run the binary with `args` in the fixture; stdout and stderr together.
    fn ways(&self, args: &[&str]) -> String {
        let home = self.home();
        let out = Command::new(env!("CARGO_BIN_EXE_ways"))
            .args(args)
            .current_dir(home.join("proj"))
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("XDG_CACHE_HOME", home.join("cache"))
            .env("XDG_STATE_HOME", home.join("state"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_RUNTIME_DIR", home.join("runtime"))
            .env_remove("CLAUDE_PROJECT_DIR")
            .env_remove("CLAUDE_CONFIG_DIR")
            .output()
            .expect("run ways");
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn schema() -> PathBuf {
    // Read at run time: a test binary reused from another checkout keeps the
    // path it was built at.
    std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_else(|| env!("CARGO_MANIFEST_DIR").into())).join("../../hooks/ways/frontmatter-schema.yaml")
}

fn sees_projected_only(out: &str, what: &str) {
    assert!(out.contains("newway"), "{what} must see the projected tree:\n{out}");
    assert!(!out.contains("oldway"), "{what} must not see the app copy:\n{out}");
}

#[test]
fn show_resolves_a_way_from_the_projection() {
    let fx = Fx::new("show");
    assert!(fx.ways(&["show", "way", PROJECTED, "--session", "s1"]).contains("# newway"));
    assert!(!fx.ways(&["show", "way", STALE, "--session", "s1"]).contains("# oldway"));
}

#[test]
fn scan_matches_ways_from_the_projection() {
    let fx = Fx::new("scan");
    let out = fx.ways(&["scan", "prompt", "--query", "giraffe zebra", "--session", "s2"]);
    assert!(out.contains("# newway"), "scan must fire the projected way:\n{out}");
    assert!(!out.contains("# oldway"), "scan must not fire the app copy:\n{out}");
}

#[test]
fn lint_global_reads_the_projection() {
    let fx = Fx::new("lint");
    let out = fx.ways(&["author", "lint", "--global"]);
    assert!(out.contains("scanned 1 way files"), "{out}");
    assert!(out.contains(".claude/hooks/ways/frontmatter-schema.yaml"), "schema read through the projection:\n{out}");
}

#[test]
fn permissions_audit_reads_the_projection() {
    let fx = Fx::new("perm");
    sees_projected_only(&fx.ways(&["author", "permissions", "--global"]), "permissions");
}

#[test]
fn corpus_builds_from_the_projection() {
    let fx = Fx::new("corpus");
    fx.ways(&["corpus", "--quiet"]);
    let jsonl = std::fs::read_to_string(fx.home().join("cache/agent-ways/user/ways-corpus.jsonl")).unwrap();
    sees_projected_only(&jsonl, "corpus");
}

#[test]
fn settings_screens_list_the_projected_ways() {
    let fx = Fx::new("tui");
    let out = fx.ways(&["settings", "ways", "--depth", "none", "--snap", "100x40", "--keys", "down down down down down down down down right"]);
    assert!(out.contains("▸ dev"), "the settings tab must list the projected domain:\n{out}");
    assert!(!out.contains("stale"), "the settings tab must not list the app copy:\n{out}");
}

impl Fx {
    /// Run the binary in the fixture and return its exit code.
    fn ways_status(&self, args: &[&str]) -> Option<i32> {
        let home = self.home();
        Command::new(env!("CARGO_BIN_EXE_ways"))
            .args(args)
            .current_dir(home.join("proj"))
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("XDG_CACHE_HOME", home.join("cache"))
            .env("XDG_STATE_HOME", home.join("state"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_RUNTIME_DIR", home.join("runtime"))
            .env_remove("CLAUDE_PROJECT_DIR")
            .env_remove("CLAUDE_CONFIG_DIR")
            .output()
            .expect("run ways")
            .status
            .code()
    }
}

#[test]
fn template_global_writes_the_personal_root_not_the_projection() {
    let fx = Fx::new("template");
    let out = fx.ways(&["author", "template", "mine/newone", "-d", "a personal way", "--global"]);
    let personal = fx.home().join("config/agent-ways/ways/mine/newone/newone.md");
    assert!(personal.is_file(), "template --global must write {}:\n{out}", personal.display());
    let golden = personal.with_file_name("newone.golden.jsonl");
    assert!(
        !golden.exists(),
        "a personal root has no golden requirement, so template must not write {}:\n{out}",
        golden.display()
    );
    assert!(
        !fx.home().join("dev/hooks/ways/mine").exists(),
        "template --global must not write through the shipped projection:\n{out}"
    );
    assert!(
        !fx.home().join(".local/share/agent-ways/hooks/ways/mine").exists(),
        "template --global must not write into the app copy:\n{out}"
    );
}

#[test]
fn lint_check_fails_a_fire_bearing_way_without_refire() {
    let fx = Fx::new("norefire");
    let root = fx.home().join("config/agent-ways/ways");
    let dir = root.join("mine/bare");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("bare.md"), "---\ndescription: a bare way\nvocabulary: bare\npattern: \\bbare\\b\n---\n# bare\n").unwrap();
    let path = root.to_str().unwrap();
    let out = fx.ways(&["author", "lint", path]);
    assert!(out.contains("ERROR") && out.contains("no `refire:` field"), "{out}");
    assert_eq!(fx.ways_status(&["author", "lint", "--check", path]), Some(1), "{out}");
}

#[test]
fn lint_check_requires_a_golden_sidecar_in_a_core_root() {
    let fx = Fx::new("coregolden");
    // A root holding frontmatter-schema.yaml is a core root (paths::is_core_root).
    let root = fx.home().join("corecheck");
    let dir = root.join("mine/sem");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(schema(), root.join("frontmatter-schema.yaml")).unwrap();
    std::fs::write(
        dir.join("sem.md"),
        "---\ndescription: a semantic way\nvocabulary: semantic\nrefire: 0.15\n---\n# sem\n",
    )
    .unwrap();
    let path = root.to_str().unwrap();
    let out = fx.ways(&["author", "lint", "--check", path]);
    assert!(out.contains("ERROR") && out.contains("no `sem.golden.jsonl`"), "{out}");
    assert_eq!(fx.ways_status(&["author", "lint", "--check", path]), Some(1), "{out}");

    std::fs::write(
        dir.join("sem.golden.jsonl"),
        "{\"kind\":\"direct\",\"prompt\":\"a semantic prompt\"}\n{\"kind\":\"situational\",\"prompt\":\"a situation\"}\n",
    )
    .unwrap();
    let out = fx.ways(&["author", "lint", "--check", path]);
    assert_eq!(fx.ways_status(&["author", "lint", "--check", path]), Some(0), "{out}");
}
