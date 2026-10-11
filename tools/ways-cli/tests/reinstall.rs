//! `ways manage reinstall` (ADR-509 §3), run in a sandboxed HOME and XDG
//! tree against a local origin. The app is a clone of a small agent-ways
//! checkout carrying the repository's real `scripts/install.sh`, whose stage
//! mode fetches and verifies and whose fresh path installs. Its Makefile's
//! `setup` stands in for the build: it writes a `bin/ways` that exits 0 or
//! refuses to project, or the build fails, so the install succeeds or stops on
//! purpose.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Sandbox {
    base: PathBuf,
}

const BUILDS: &str = "# agent-ways test checkout\nsetup:\n\tmkdir -p bin && printf '#!/bin/sh\\nexit 0\\n' > bin/ways && chmod +x bin/ways\n";
const FAILS: &str = "# agent-ways test checkout\nsetup:\n\t@echo build failed; exit 1\n";
/// A setup that downloads a release binary, and the source-build targets
/// `ways manage update --ref` runs, each leaving a mark of what it built.
const SOURCE: &str = "# agent-ways test checkout\nsetup:\n\tmkdir -p bin && printf '#!/bin/sh\\nexit 0\\n' > bin/ways && chmod +x bin/ways && echo release > bin/built-by\n\
%-rebuild:\n\tmkdir -p bin && echo $@ >> bin/rebuilt\nrelink:\n\t@true\n";
/// Builds a `bin/ways` whose repair refuses, as a real path at a projected
/// root makes it.
const REFUSES: &str = "# agent-ways test checkout\nsetup:\n\tmkdir -p bin && printf '#!/bin/sh\\necho refused: skills is a real directory >&2\\nexit 1\\n' > bin/ways && chmod +x bin/ways\n";

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

impl Sandbox {
    /// An origin repository, the app cloned from it, and the operator's
    /// config, state and cache beside it.
    fn new(name: &str, makefile: &str) -> Sandbox {
        let base = std::env::temp_dir().join(format!("ways-reinstall-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let origin = base.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        let installer = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/install.sh");
        write(&origin.join("scripts/install.sh"), &std::fs::read_to_string(installer).unwrap());
        write(&origin.join("hooks/check-config-updates.sh"), "#!/bin/sh\n");
        write(&origin.join("tools/suite-bins"), "ways\n");
        write(&origin.join("skills/x/SKILL.md"), "x\n");
        write(&origin.join("Makefile"), makefile);
        git(&origin, &["init", "-q"]);
        git(&origin, &["add", "."]);
        git(&origin, &["commit", "-qm", "one"]);
        let s = Sandbox { base };
        std::fs::create_dir_all(s.base.join("data")).unwrap();
        git(&s.base.join("data"), &["clone", "-q", origin.to_str().unwrap(), "agent-ways"]);
        write(&s.base.join("config/agent-ways/ways.yaml"), "# mine\n");
        write(&s.base.join("state/agent-ways/events.jsonl"), "{}\n");
        write(&s.base.join("cache/agent-ways/user/corpus.jsonl"), "{}\n");
        std::fs::create_dir_all(s.base.join("home")).unwrap();
        std::fs::create_dir_all(s.base.join("tmp")).unwrap();
        s
    }

    fn app(&self) -> PathBuf {
        self.base.join("data/agent-ways")
    }

    fn stage(&self) -> PathBuf {
        self.base.join("data/agent-ways.reinstall")
    }

    fn ways(&self, args: &[&str]) -> Output {
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
            .env("NO_COLOR", "1")
            .env_remove("XDG_BIN_HOME")
            .env_remove("XDG_RUNTIME_DIR")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CLAUDE_PROJECT_DIR")
            .output()
            .expect("run ways")
    }

    /// The operator's files and the app are all where they were.
    fn untouched(&self) {
        assert!(self.app().join("Makefile").is_file(), "the app was removed");
        assert!(self.base.join("cache/agent-ways/user/corpus.jsonl").is_file(), "the cache was removed");
        assert!(self.base.join("config/agent-ways/ways.yaml").is_file());
        assert!(!self.stage().exists(), "a failed fetch leaves no stage");
    }

    fn kept_what_the_operator_owns(&self) {
        assert_eq!(std::fs::read_to_string(self.base.join("config/agent-ways/ways.yaml")).unwrap(), "# mine\n");
        assert!(self.base.join("state/agent-ways/events.jsonl").is_file());
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn text(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

#[test]
fn without_yes_it_prints_the_plan_and_changes_nothing() {
    let s = Sandbox::new("plan", BUILDS);
    let out = s.ways(&["manage", "reinstall"]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    assert!(t.contains("Fetch and verify") && t.contains(s.stage().to_str().unwrap()), "{t}");
    assert!(t.contains("Uninstall, keeping your config and state") && t.contains("Delete:"), "{t}");
    assert!(t.contains("Install from the stage") && t.contains(" on main"), "{t}");
    assert!(t.contains("Nothing changed"), "{t}");
    assert!(!t.contains("local changes"), "a clean app says nothing: {t}");
    write(&s.app().join("mine.txt"), "x\n");
    let t = text(&s.ways(&["manage", "reinstall"]));
    assert!(t.contains("has local changes, which the uninstall deletes"), "{t}");
    s.untouched();
}

#[test]
fn purge_is_refused() {
    let s = Sandbox::new("purge", BUILDS);
    let out = s.ways(&["manage", "reinstall", "--yes", "--purge"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(text(&out).contains("does not take --purge"));
    s.untouched();
}

#[test]
fn a_failed_fetch_removes_nothing() {
    let s = Sandbox::new("nofetch", BUILDS);
    git(&s.app(), &["remote", "set-url", "origin", s.base.join("gone").to_str().unwrap()]);
    let out = s.ways(&["manage", "reinstall", "--yes"]);
    let t = text(&out);
    assert!(!out.status.success(), "{t}");
    assert!(t.contains("nothing was removed"), "{t}");
    s.untouched();
}

#[test]
fn a_commit_the_source_lacks_fails_verification_and_removes_nothing() {
    let s = Sandbox::new("norev", BUILDS);
    write(&s.app().join("local.txt"), "x\n");
    git(&s.app(), &["add", "."]);
    git(&s.app(), &["commit", "-qm", "local only"]);
    let out = s.ways(&["manage", "reinstall", "--yes"]);
    let t = text(&out);
    assert!(!out.status.success(), "{t}");
    assert!(t.contains("does not have") && t.contains("nothing was removed"), "{t}");
    s.untouched();
}

#[test]
fn a_failed_install_keeps_the_stage_and_prints_the_finishing_line() {
    let s = Sandbox::new("noinstall", FAILS);
    let out = s.ways(&["manage", "reinstall", "--yes"]);
    let t = text(&out);
    assert!(!out.status.success(), "{t}");
    assert!(s.stage().join("scripts/install.sh").is_file(), "the stage is kept: {t}");
    let finish = format!("cd '{}' && AGENT_WAYS_ORIGIN='{}' bash scripts/install.sh", s.stage().display(), s.base.join("origin").display());
    assert!(t.contains(&finish), "{t}");
    assert!(!s.app().exists(), "a half-made app would turn the finishing run into an update");
    s.kept_what_the_operator_owns();
}

#[test]
fn a_projection_stop_keeps_the_built_app_and_names_repair_as_the_finish() {
    let s = Sandbox::new("refused", REFUSES);
    let out = s.ways(&["manage", "reinstall", "--yes"]);
    let t = text(&out);
    assert!(!out.status.success(), "{t}");
    assert!(s.app().join("bin/ways").is_file(), "the built app is kept: {t}");
    assert!(!s.stage().exists(), "the stage is not needed once the app is built: {t}");
    assert!(t.contains("Projection stopped") && t.contains("ways manage repair"), "{t}");
    assert!(t.contains("Fix the cause above") && !t.contains("nothing in"), "a failed repair names no cause of its own: {t}");
    assert!(!t.contains("bash scripts/install.sh"), "the finish is a repair, not a reinstall from the stage: {t}");
    s.kept_what_the_operator_owns();
}

#[test]
fn an_installer_without_stage_mode_is_not_run() {
    let s = Sandbox::new("oldinstaller", BUILDS);
    // An installer from before --stage updates in place and exits 0.
    write(&s.app().join("scripts/install.sh"), "touch \"$(dirname \"$0\")/../updated\"\n");
    let out = s.ways(&["manage", "reinstall", "--yes"]);
    let t = text(&out);
    assert!(!out.status.success(), "{t}");
    assert!(t.contains("--stage") && t.contains("nothing was removed"), "{t}");
    assert!(!s.app().join("updated").exists(), "the old installer ran: {t}");
    s.untouched();
}

#[test]
fn a_detached_source_build_is_rebuilt_from_source() {
    let s = Sandbox::new("detached", SOURCE);
    let rev = git(&s.app(), &["rev-parse", "HEAD"]);
    git(&s.app(), &["checkout", "-q", "--detach"]);
    let out = s.ways(&["manage", "reinstall", "--yes"]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    assert_eq!(git(&s.app(), &["rev-parse", "HEAD"]), rev, "{t}");
    let rebuilt = std::fs::read_to_string(s.app().join("bin/rebuilt")).unwrap_or_default();
    for target in ["ways-rebuild", "ways-audit-rebuild", "attend-rebuild", "attend-chat-rebuild"] {
        assert!(rebuilt.lines().any(|l| l == target), "{target} did not run: {rebuilt}\n{t}");
    }
}

#[test]
fn a_branch_install_keeps_the_release_binaries() {
    let s = Sandbox::new("onbranch", SOURCE);
    let out = s.ways(&["manage", "reinstall", "--yes"]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    assert!(!s.app().join("bin/rebuilt").exists(), "a branch install is the release channel: {t}");
}

#[test]
fn an_app_without_git_beside_a_stage_names_the_finish() {
    let s = Sandbox::new("nogit", BUILDS);
    git(&s.base.join("data"), &["clone", "-q", s.base.join("origin").to_str().unwrap(), "agent-ways.reinstall"]);
    std::fs::remove_dir_all(s.app().join(".git")).unwrap();
    let out = s.ways(&["manage", "reinstall", "--yes"]);
    let t = text(&out);
    assert!(!out.status.success(), "{t}");
    let finish = format!("cd '{}' && AGENT_WAYS_ORIGIN='{}' bash scripts/install.sh", s.stage().display(), s.base.join("origin").display());
    assert!(t.contains(&finish), "{t}");
    assert!(s.stage().join("scripts/install.sh").is_file(), "the stage is kept: {t}");
}

#[test]
fn a_relative_origin_resolves_from_the_app_dir() {
    let s = Sandbox::new("relorigin", BUILDS);
    git(&s.app(), &["remote", "set-url", "origin", "../../origin"]);
    let out = s.ways(&["manage", "reinstall", "--yes"]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    assert!(s.app().join("bin/ways").is_file(), "{t}");
}

#[test]
fn a_stage_left_beside_a_changed_app_is_not_discarded() {
    let s = Sandbox::new("leftstage", BUILDS);
    write(&s.stage().join("scripts/install.sh"), "# an earlier run's stage\n");
    std::fs::remove_file(s.app().join("hooks/check-config-updates.sh")).unwrap();
    let out = s.ways(&["manage", "reinstall", "--yes"]);
    let t = text(&out);
    assert!(!out.status.success(), "{t}");
    assert!(s.stage().join("scripts/install.sh").is_file(), "the stage is kept: {t}");
    assert!(t.contains("bash scripts/install.sh"), "it names the finish from the stage: {t}");
    assert!(s.app().join("Makefile").is_file(), "nothing was removed: {t}");
}

#[test]
fn a_failed_uninstall_keeps_the_stage_and_prints_the_finishing_line() {
    let uid = Command::new("id").arg("-u").output().unwrap();
    if String::from_utf8_lossy(&uid.stdout).trim() == "0" {
        return; // root deletes a read-only directory, so the uninstall cannot fail here
    }
    let s = Sandbox::new("nouninstall", BUILDS);
    let locked = s.app().join("locked");
    write(&locked.join("file"), "x\n");
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
    let out = s.ways(&["manage", "reinstall", "--yes"]);
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    let t = text(&out);
    assert!(!out.status.success(), "{t}");
    assert!(s.stage().join("scripts/install.sh").is_file(), "the stage is kept: {t}");
    let finish = format!("cd '{}' && AGENT_WAYS_ORIGIN='{}' bash scripts/install.sh", s.stage().display(), s.base.join("origin").display());
    assert!(t.contains(&finish), "{t}");
    s.kept_what_the_operator_owns();
}

#[test]
fn a_reinstall_puts_back_the_same_commit_branch_and_origin() {
    let s = Sandbox::new("ok", BUILDS);
    let rev = git(&s.app(), &["rev-parse", "HEAD"]);
    // The origin moves on; the reinstall still puts back the installed commit.
    write(&s.base.join("origin/later.txt"), "x\n");
    git(&s.base.join("origin"), &["add", "."]);
    git(&s.base.join("origin"), &["commit", "-qm", "later"]);
    let out = s.ways(&["manage", "reinstall", "--yes"]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    assert_eq!(git(&s.app(), &["rev-parse", "HEAD"]), rev, "{t}");
    assert_eq!(git(&s.app(), &["symbolic-ref", "--short", "HEAD"]), "main");
    assert_eq!(git(&s.app(), &["remote", "get-url", "origin"]), s.base.join("origin").to_str().unwrap());
    assert!(s.app().join("bin/ways").is_file(), "the staged installer ran: {t}");
    assert!(!s.stage().exists(), "the stage is removed after a good install");
    s.kept_what_the_operator_owns();
}

#[test]
fn a_reinstall_projects_every_enabled_target_the_config_lists() {
    let s = Sandbox::new("targets", BUILDS);
    let (home, work) = (s.base.join("home/.claude"), s.base.join("home/work-claude"));
    write(
        &s.base.join("config/agent-ways/config.yaml"),
        &format!("targets:\n  - path: {}\n  - path: {}\n", home.display(), work.display()),
    );
    let out = s.ways(&["manage", "reinstall", "--yes"]);
    let t = text(&out);
    assert!(out.status.success(), "{t}");
    for dir in [&home, &work] {
        let link = std::fs::read_link(dir.join("skills")).unwrap_or_else(|e| panic!("{} is not projected: {e}\n{t}", dir.display()));
        assert_eq!(link, s.app().join("skills"), "{t}");
    }
}

/// A value flag without its value stops the installer with a usage message
/// and exit 2, before it fetches or installs anything.
#[test]
fn the_installer_refuses_a_value_flag_without_its_value() {
    let s = Sandbox::new("flags", BUILDS);
    let stage = s.base.join("staged");
    let stage = stage.to_str().unwrap();
    let origin = s.base.join("origin");
    let origin = origin.to_str().unwrap();
    for args in [
        vec!["--stage"],
        vec!["--branch"],
        vec!["--stage", stage, "--rev", "abc", "--from"],
        vec!["--stage", "--from", origin, "--rev", "abc"],
        vec!["--stage", "", "--from", origin, "--rev", "abc"],
    ] {
        let out = Command::new("bash")
            .arg("scripts/install.sh")
            .args(&args)
            .current_dir(s.app())
            .env("HOME", s.base.join("home"))
            .env("XDG_DATA_HOME", s.base.join("data"))
            .env("XDG_CACHE_HOME", s.base.join("cache"))
            .output()
            .expect("run install.sh");
        let t = text(&out);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {t}");
        assert!(t.contains("needs a value"), "{args:?}: {t}");
        assert!(!s.base.join("staged").exists() && !s.base.join("data/--from").exists(), "{args:?} staged something: {t}");
        assert!(!s.app().join("bin").exists(), "{args:?} installed: {t}");
    }
}
