//! `ways manage update` without `--commit` (ADR-511): in a sandboxed HOME and
//! XDG tree, an app cloned from a local origin that has moved on. The
//! what-if fetches, reports the versions, the release notes, the delta, the
//! local changes and a dev link, and leaves the checkout, its working tree
//! and every link as they were.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Sandbox {
    base: PathBuf,
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "init.defaultBranch=main"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("run git");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn exec(path: &Path, text: &str) {
    use std::os::unix::fs::PermissionsExt;
    write(path, text);
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

const RELEASES: &str = r###"[
  {"tag_name":"ways-v1.1.0","draft":false,"prerelease":false,
   "body":"## ways 1.1.0\n\n### Added\n- **A new thing.** It does a thing (#2).\n\n---\nPre-built ways CLI binaries for 4 platforms.\n\nBuild from source: `make ways`\n"},
  {"tag_name":"ways-v1.0.0","draft":false,"prerelease":false,"body":"## ways 1.0.0\n\nthe installed one\n"}
]"###;

impl Sandbox {
    fn new(name: &str) -> Sandbox {
        let base = std::env::temp_dir().join(format!("ways-update-whatif-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let origin = base.join("origin");
        write(&origin.join("Makefile"), "# agent-ways test checkout\nrelink:\n\t@touch relinked\n");
        write(&origin.join("tools/suite-bins"), "ways\n");
        write(&origin.join("tools/ways-cli/Cargo.toml"), "[package]\nname = \"ways\"\nversion = \"1.0.0\"\n");
        write(&origin.join("scripts/update.sh"), "#!/bin/sh\ngit pull --ff-only\n");
        write(&origin.join("settings.json"), "{}\n");
        write(&origin.join(".gitignore"), "bin/\n");
        git(&origin, &["init", "-q"]);
        git(&origin, &["add", "."]);
        git(&origin, &["commit", "-qm", "chore: one"]);
        let s = Sandbox { base };
        std::fs::create_dir_all(s.base.join("data")).unwrap();
        git(&s.base.join("data"), &["clone", "-q", origin.to_str().unwrap(), "agent-ways"]);
        exec(&s.app().join("bin/ways"), "#!/bin/sh\necho 'ways 1.0.0 (ways-v1.0.0-0-gabc1234)'\n");

        // The origin moves on: a feature and a version bump, merged as PR #2.
        write(&origin.join("tools/ways-cli/Cargo.toml"), "[package]\nname = \"ways\"\nversion = \"1.1.0\"\n");
        write(&origin.join("tools/ways-cli/src/main.rs"), "fn main() {}\n");
        git(&origin, &["add", "."]);
        git(&origin, &["commit", "-qm", "feat(ways): a new thing (#2)"]);
        write(&origin.join("docs/x.md"), "x\n");
        git(&origin, &["add", "."]);
        git(&origin, &["commit", "-qm", "docs: a page"]);

        // A local edit the pull would stash.
        write(&s.app().join("settings.json"), "{\"mine\": true}\n");

        // The suite on PATH: ways from the app, and a dev link into a checkout.
        let bin = s.base.join("home/.local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(s.app().join("bin/ways"), bin.join("ways")).unwrap();
        let dev = s.base.join("dev");
        std::fs::create_dir_all(dev.join(".git")).unwrap();
        exec(&dev.join("bin/way-embed"), "#!/bin/sh\n");
        // way-embed links into a target's bin/, where the repair projects it;
        // `make link` never puts it in ~/.local/bin.
        let claude_bin = s.base.join("home/.claude/bin");
        std::fs::create_dir_all(&claude_bin).unwrap();
        std::os::unix::fs::symlink(dev.join("bin/way-embed"), claude_bin.join("way-embed")).unwrap();
        write(&s.base.join("releases.json"), RELEASES);
        for d in ["config", "state", "cache", "tmp", "home/.claude"] {
            std::fs::create_dir_all(s.base.join(d)).unwrap();
        }
        s
    }

    fn app(&self) -> PathBuf {
        self.base.join("data/agent-ways")
    }

    fn ways(&self, args: &[&str], releases: &Path) -> Output {
        Command::new(env!("CARGO_BIN_EXE_ways"))
            .args(args)
            .current_dir(&self.base)
            .env("PWD", &self.base)
            .env("HOME", self.base.join("home"))
            .env("XDG_DATA_HOME", self.base.join("data"))
            .env("XDG_CONFIG_HOME", self.base.join("config"))
            .env("XDG_STATE_HOME", self.base.join("state"))
            .env("XDG_CACHE_HOME", self.base.join("cache"))
            .env("TMPDIR", self.base.join("tmp"))
            .env("WAYS_AGENT_SOCK", self.base.join("tmp/agent.sock"))
            .env("WAYS_RELEASES_JSON", releases)
            .env("NO_COLOR", "1")
            .env_remove("XDG_BIN_HOME")
            .env_remove("XDG_RUNTIME_DIR")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CLAUDE_PROJECT_DIR")
            .env_remove("CLICOLOR_FORCE")
            .env_remove("FORCE_COLOR")
            .output()
            .expect("run ways")
    }

    /// HEAD, the working tree and the links, to compare before and after.
    fn snapshot(&self) -> (String, String, Vec<(PathBuf, PathBuf)>) {
        let bin = self.base.join("home/.local/bin");
        let mut links: Vec<(PathBuf, PathBuf)> = std::fs::read_dir(&bin).unwrap().flatten().map(|e| (e.path(), std::fs::read_link(e.path()).unwrap())).collect();
        links.sort();
        (git(&self.app(), &["rev-parse", "HEAD"]), git(&self.app(), &["status", "--porcelain"]), links)
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

#[test]
fn the_default_run_reports_the_update_and_changes_nothing() {
    let s = Sandbox::new("default");
    let before = s.snapshot();
    let out = s.ways(&["manage", "update"], &s.base.join("releases.json"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}\n{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(s.snapshot(), before, "HEAD, the working tree and the links are unchanged");
    assert!(!s.app().join("relinked").exists(), "no make target ran");
    assert!(!stdout.contains('\x1b'), "plain text in a pipe");

    for want in [
        "ways manage update: what-if",
        "── Versions",
        "1.0.0 → 1.1.0",
        "── Release notes",
        "ways-v1.1.0",
        "- **A new thing.** It does a thing (#2).",
        "── Changes",
        "2 commits, 1 pull request merged",
        "feat(ways): a new thing (#2)",
        "1 other commit (--verbose lists them)",
        "── Local changes",
        "settings.json",
        "── Actions",
        "stash the local changes",
        "pull: fast-forward",
        "── Development setup",
        "make dev-install",
        "Run `ways manage update --commit` to apply.",
    ] {
        assert!(stdout.contains(want), "missing {want:?} in:\n{stdout}");
    }
    assert!(!stdout.contains("Pre-built"), "the CI tail is trimmed:\n{stdout}");
    assert!(!stdout.contains("the installed one"), "the installed release's notes are not repeated:\n{stdout}");

    // --verbose lists every commit; --dry-run is the same what-if.
    let verbose = s.ways(&["manage", "update", "--verbose"], &s.base.join("releases.json"));
    assert!(String::from_utf8_lossy(&verbose.stdout).contains("docs: a page"));
    let dry = s.ways(&["manage", "update", "--dry-run"], &s.base.join("releases.json"));
    assert!(dry.status.success() && String::from_utf8_lossy(&dry.stdout).contains("what-if"));
    assert_eq!(s.snapshot(), before);
}

#[test]
fn unreachable_release_notes_are_named_and_the_report_goes_on() {
    let s = Sandbox::new("offline");
    let out = s.ways(&["manage", "update"], &s.base.join("no-such-releases.json"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert!(stdout.contains("the release notes could not be fetched"), "{stdout}");
    assert!(stdout.contains("── Changes") && stdout.contains("── Actions"), "{stdout}");
}

#[test]
fn commit_applies_with_one_header_per_stage() {
    let s = Sandbox::new("commit");
    let before = s.snapshot().0;
    let out = s.ways(&["manage", "update", "--commit"], &s.base.join("releases.json"));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let all = format!("{}{stderr}", String::from_utf8_lossy(&out.stdout));
    assert!(out.status.success(), "{all}");
    assert_ne!(s.snapshot().0, before, "the pull moved HEAD: {all}");
    assert!(s.app().join("relinked").exists(), "relink ran: {all}");
    let order: Vec<usize> = ["── Pull", "── Binaries", "── Corpus", "── Reproject", "── Done"]
        .iter()
        .map(|h| stderr.find(h).unwrap_or_else(|| panic!("{h} missing:\n{all}")))
        .collect();
    assert!(order.windows(2).all(|w| w[0] < w[1]), "{all}");
    assert!(stderr.contains("✓ fast-forwarded"), "{all}");
    // The ways refresh fails here (no `ways` make target): a warning, and
    // the failed make's output under it, not a raw scroll of every step.
    assert!(stderr.contains("⚠ ways NOT refreshed"), "{all}");
    assert!(!stderr.contains("Already up to date") && !stderr.contains("Fast-forward"), "git's own output is kept back: {all}");
    assert!(!all.contains('\x1b'), "plain in a pipe");
}

#[test]
fn a_checkout_ahead_of_upstream_is_ahead_not_diverged() {
    let s = Sandbox::new("ahead");
    git(&s.app(), &["checkout", "-q", "--", "settings.json"]);
    git(&s.app(), &["-c", "user.name=t", "-c", "user.email=t@example.com", "pull", "-q", "--ff-only"]);
    write(&s.app().join("local.txt"), "mine\n");
    git(&s.app(), &["add", "local.txt"]);
    git(&s.app(), &["-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-qm", "chore: a local commit"]);
    let out = s.ways(&["manage", "update"], &s.base.join("releases.json"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert!(stdout.contains("ahead of origin/main by 1 commit, so the pull changes nothing"), "{stdout}");
    assert!(!stdout.contains("diverged"), "{stdout}");
    assert!(stdout.contains("ahead of the target: no new commits"), "{stdout}");
}

#[test]
fn a_ref_behind_the_install_is_reported_as_a_downgrade() {
    let s = Sandbox::new("downgrade");
    let first = git(&s.app(), &["rev-parse", "HEAD"]);
    git(&s.app(), &["checkout", "-q", "--", "settings.json"]);
    git(&s.app(), &["-c", "user.name=t", "-c", "user.email=t@example.com", "pull", "-q", "--ff-only"]);
    exec(&s.app().join("bin/ways"), "#!/bin/sh\necho 'ways 1.1.0 (ways-v1.1.0-0-gabc1234)'\n");
    let before = s.snapshot();
    let out = s.ways(&["manage", "update", "--ref", &first], &s.base.join("releases.json"));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{stdout}");
    assert_eq!(s.snapshot(), before, "a --ref what-if changes nothing");
    assert!(stdout.contains("↓ ways") && stdout.contains("1.1.0 → 1.0.0 (a downgrade: --commit rebuilds it from source)"), "{stdout}");
    assert!(stdout.contains("--commit leaves these commits behind") && stdout.contains("feat(ways): a new thing (#2)"), "{stdout}");
    assert!(!stdout.contains("the fetch failed"), "a ref that resolves is not a failed fetch: {stdout}");
}

/// The index's modification time.
fn index_mtime(app: &Path) -> std::time::SystemTime {
    std::fs::metadata(app.join(".git/index")).unwrap().modified().unwrap()
}

#[test]
fn the_what_if_leaves_the_index_alone() {
    let s = Sandbox::new("index");
    // A tracked file touched without a change: a refreshing git command
    // (porcelain `git diff HEAD`) would rewrite the index to record its stat.
    let makefile = s.app().join("Makefile");
    let text = std::fs::read_to_string(&makefile).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(&makefile, text).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let before = index_mtime(&s.app());
    let out = s.ways(&["manage", "update"], &s.base.join("releases.json"));
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(index_mtime(&s.app()), before, "the what-if rewrote .git/index");
}
