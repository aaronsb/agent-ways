//! `ways manage repair` installs, updates and removes the attend mod through
//! Claude Code's plugin CLI (ADR-406 item 8). A stub `claude` on PATH keeps the
//! marketplace and plugin lists in files and logs every call, so the test
//! reads exactly what reconcile asked Claude Code to do.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const STUB: &str = r#"#!/bin/sh
# A stand-in for `claude plugin ...`: state in $STUB_STATE, one log line a call.
echo "${CLAUDE_CONFIG_DIR:-unset}|$*" >> "$STUB_STATE/log"
m="$STUB_STATE/markets.json"; p="$STUB_STATE/plugins.json"
[ -f "$m" ] || echo '[]' > "$m"
[ -f "$p" ] || echo '[]' > "$p"
case "$*" in
  "plugin marketplace list --json") cat "$m" ;;
  "plugin list --json") cat "$p" ;;
  "plugin marketplace add "*) printf '[{"name":"agent-ways","source":"directory","path":"%s"}]\n' "$4" > "$m" ;;
  # Claude Code uninstalls a marketplace's plugins when it removes the marketplace.
  "plugin marketplace remove agent-ways") echo '[]' > "$m"; echo '[]' > "$p" ;;
  "plugin marketplace update agent-ways") ;;
  "plugin install attend@agent-ways --scope user"|"plugin update attend@agent-ways --scope user")
    printf '[{"id":"attend@agent-ways","version":"%s","scope":"user"}]\n' "$STUB_SHIPPED" > "$p" ;;
  "plugin uninstall attend@agent-ways --scope user") echo '[]' > "$p" ;;
  *) echo "stub claude: unexpected: $*" >&2; exit 2 ;;
esac
"#;

struct Fx {
    root: PathBuf,
}

impl Fx {
    fn new(name: &str) -> Fx {
        let root = std::env::temp_dir().join(format!("ways-attend-mod-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["home/.config", "home/.local/share", "home/.local/state", "cache", "dest", "state", "stubbin", "project"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        // The app source: one projection root of each kind, and the marketplace.
        let src = root.join("src");
        write(&src.join("skills/wrap/SKILL.md"), "---\nx\n");
        write(&src.join("hooks/ways/meta/a.md"), "---\nx\n");
        write(&src.join("hooks/check-config-updates.sh"), "#!/bin/sh\n");
        write(&src.join("plugins/.claude-plugin/marketplace.json"), r#"{"name":"agent-ways","plugins":[{"name":"attend","source":"./attend"}]}"#);
        write(&src.join("plugins/attend/.claude-plugin/plugin.json"), r#"{"name":"attend","version":"0.17.0"}"#);
        copy_dir(&src, &root.join("moved"));
        let stub = root.join("stubbin/claude");
        write(&stub, STUB);
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
        Fx { root }
    }

    fn src(&self) -> PathBuf {
        self.root.join("src")
    }
    fn dest(&self) -> PathBuf {
        self.root.join("dest")
    }
    fn state(&self) -> PathBuf {
        self.root.join("state")
    }
    fn user_config(&self) -> PathBuf {
        self.root.join("home/.config/agent-ways/config.yaml")
    }

    fn ways(&self, path: &str) -> Command {
        let home = self.root.join("home");
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ways"));
        cmd.current_dir(self.root.join("project"))
            .env("PWD", self.root.join("project"))
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", self.root.join("cache"))
            .env("PATH", path)
            .env("STUB_STATE", self.state())
            .env("STUB_SHIPPED", "0.17.0");
        for var in ["CLAUDE_PROJECT_DIR", "CLAUDE_CONFIG_DIR", "CLAUDE_SESSION_ID", "WAYS_CLAUDE_BIN"] {
            cmd.env_remove(var);
        }
        cmd
    }

    fn stub_path(&self) -> String {
        format!("{}:/usr/bin:/bin", self.root.join("stubbin").display())
    }

    /// `ways manage repair` against the one destination.
    fn repair(&self) -> Output {
        self.repair_from(&self.src())
    }

    /// `ways manage repair` from the app source at `src`.
    fn repair_from(&self, src: &Path) -> Output {
        let out = self
            .ways(&self.stub_path())
            .args(["manage", "repair", "--source"])
            .arg(src)
            .arg("--dest")
            .arg(self.dest())
            .output()
            .unwrap();
        assert!(out.status.success(), "repair failed: {}", String::from_utf8_lossy(&out.stderr));
        out
    }

    /// The calls that changed something, without the list queries.
    fn changes(&self) -> Vec<String> {
        let log = std::fs::read_to_string(self.state().join("log")).unwrap_or_default();
        let _ = std::fs::remove_file(self.state().join("log"));
        log.lines().filter(|l| !l.ends_with("list --json")).map(str::to_string).collect()
    }

    fn installed(&self, version: &str) {
        write(&self.state().join("plugins.json"), &format!(r#"[{{"id":"attend@agent-ways","version":"{version}"}}]"#));
        write(
            &self.state().join("markets.json"),
            &format!(r#"[{{"name":"agent-ways","source":"directory","path":"{}"}}]"#, self.src().join("plugins").display()),
        );
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let dst = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dst);
        } else {
            std::fs::create_dir_all(to).unwrap();
            std::fs::copy(entry.path(), dst).unwrap();
        }
    }
}

fn write(p: &Path, text: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

#[test]
fn repair_adds_the_marketplace_and_installs_under_the_targets_config_dir() {
    let fx = Fx::new("fresh");
    let out = fx.repair();
    let dest = fx.dest().display().to_string();
    let plugins = fx.src().join("plugins").display().to_string();
    assert_eq!(
        fx.changes(),
        vec![
            format!("{dest}|plugin marketplace add {plugins} --scope user"),
            format!("{dest}|plugin install attend@agent-ways --scope user"),
        ]
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("installed attend@agent-ways"), "{err}");

    // Converged: a second repair asks and changes nothing.
    fx.repair();
    assert_eq!(fx.changes(), Vec::<String>::new());
}

#[test]
fn an_older_installed_version_is_updated() {
    let fx = Fx::new("older");
    fx.installed("0.16.0");
    fx.repair();
    let dest = fx.dest().display().to_string();
    assert_eq!(
        fx.changes(),
        vec![
            format!("{dest}|plugin marketplace update agent-ways"),
            format!("{dest}|plugin update attend@agent-ways --scope user"),
        ]
    );
}

#[test]
fn install_false_removes_the_mod_and_the_marketplace() {
    let fx = Fx::new("off");
    fx.installed("0.17.0");
    write(&fx.user_config(), "attend_mod:\n  install: false\n");
    fx.repair();
    let dest = fx.dest().display().to_string();
    assert_eq!(
        fx.changes(),
        vec![format!("{dest}|plugin uninstall attend@agent-ways --scope user"), format!("{dest}|plugin marketplace remove agent-ways")]
    );
}

#[test]
fn a_disabled_target_loses_the_mod_before_its_links() {
    let fx = Fx::new("disabled");
    fx.repair();
    fx.changes();
    assert!(std::fs::symlink_metadata(fx.dest().join("skills")).is_ok());
    write(&fx.user_config(), &format!("targets:\n  - path: {}\n    enabled: false\n", fx.dest().display()));
    let out = fx.ways(&fx.stub_path()).args(["manage", "repair", "--source"]).arg(fx.src()).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let dest = fx.dest().display().to_string();
    assert_eq!(
        fx.changes(),
        vec![format!("{dest}|plugin uninstall attend@agent-ways --scope user"), format!("{dest}|plugin marketplace remove agent-ways")]
    );
    assert!(std::fs::symlink_metadata(fx.dest().join("skills")).is_err(), "links withdrawn");
}

#[test]
fn an_attend_plugin_from_another_marketplace_is_reported_and_left_alone() {
    let fx = Fx::new("foreign");
    write(&fx.state().join("plugins.json"), r#"[{"id":"attend@mine","version":"9.9.9"}]"#);
    let out = fx.repair();
    assert_eq!(fx.changes(), Vec::<String>::new());
    assert!(String::from_utf8_lossy(&out.stderr).contains("attend@mine"));
}

#[test]
fn a_failed_call_or_a_missing_claude_never_fails_the_repair() {
    let fx = Fx::new("failing");
    // `claude` answers every list with an error.
    write(&fx.root.join("stubbin/claude"), "#!/bin/sh\necho boom >&2\nexit 1\n");
    let out = fx.repair();
    assert!(String::from_utf8_lossy(&out.stderr).contains("could not read Claude Code's plugins: boom"));

    if !Path::new("/usr/bin/claude").exists() && !Path::new("/bin/claude").exists() {
        let out = fx.ways("/usr/bin:/bin").args(["manage", "repair", "--source"]).arg(fx.src()).arg("--dest").arg(fx.dest()).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        assert!(String::from_utf8_lossy(&out.stderr).contains("`claude` is not on PATH"));
    }
}

#[test]
fn a_moved_app_reinstalls_the_mod_in_one_repair() {
    let fx = Fx::new("moved");
    fx.repair();
    fx.changes();
    let moved = fx.root.join("moved");
    fx.repair_from(&moved);
    let dest = fx.dest().display().to_string();
    let plugins = moved.join("plugins").display().to_string();
    assert_eq!(
        fx.changes(),
        vec![
            format!("{dest}|plugin marketplace remove agent-ways"),
            format!("{dest}|plugin marketplace add {plugins} --scope user"),
            format!("{dest}|plugin install attend@agent-ways --scope user"),
        ]
    );
    // One repair converged: the next changes nothing.
    fx.repair_from(&moved);
    assert_eq!(fx.changes(), Vec::<String>::new());
}

#[test]
fn an_agent_ways_marketplace_this_app_never_added_is_left_alone() {
    let fx = Fx::new("foreign-market");
    // A dev checkout's marketplace, added by hand: its folder still holds one.
    let dev = fx.root.join("dev/plugins");
    write(&dev.join(".claude-plugin/marketplace.json"), r#"{"name":"agent-ways","plugins":[]}"#);
    write(&fx.state().join("markets.json"), &format!(r#"[{{"name":"agent-ways","source":"directory","path":"{}"}}]"#, dev.display()));
    write(&fx.state().join("plugins.json"), r#"[{"id":"attend@agent-ways","version":"0.1.0"}]"#);
    let out = fx.repair();
    assert_eq!(fx.changes(), Vec::<String>::new());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains(&dev.display().to_string()), "{err}");
    let recover = format!("run `CLAUDE_CONFIG_DIR={} claude plugin marketplace remove agent-ways` and then `ways manage repair`", fx.dest().display());
    assert!(err.contains(&recover), "the note gives the way out: {err}");
    // Disabling the target leaves it alone too.
    write(&fx.user_config(), &format!("targets:\n  - path: {}\n    enabled: false\n", fx.dest().display()));
    let out = fx.ways(&fx.stub_path()).args(["manage", "repair", "--source"]).arg(fx.src()).output().unwrap();
    assert!(out.status.success());
    assert_eq!(fx.changes(), Vec::<String>::new());
}

/// The record of where the marketplace was added from is gone (the state
/// root was cleared, or the install predates it) and the app moved: the old
/// folder holds no marketplace any more and attend is installed from it, so
/// it is this app's, and one repair moves it.
fn lose_the_record_and_move(fx: &Fx) -> PathBuf {
    for entry in walk(&fx.root.join("home")) {
        if entry.file_name().is_some_and(|n| n == "attend-mod-marketplace") {
            std::fs::remove_file(&entry).unwrap();
        }
    }
    let moved = fx.root.join("moved");
    std::fs::remove_dir_all(fx.src()).unwrap();
    moved
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[test]
fn a_moved_app_with_no_record_reinstalls_the_mod_in_one_repair() {
    let fx = Fx::new("moved-no-record");
    fx.repair();
    fx.changes();
    let moved = lose_the_record_and_move(&fx);
    fx.repair_from(&moved);
    let dest = fx.dest().display().to_string();
    assert_eq!(
        fx.changes(),
        vec![
            format!("{dest}|plugin marketplace remove agent-ways"),
            format!("{dest}|plugin marketplace add {} --scope user", moved.join("plugins").display()),
            format!("{dest}|plugin install attend@agent-ways --scope user"),
        ]
    );
    fx.repair_from(&moved);
    assert_eq!(fx.changes(), Vec::<String>::new());
}

#[test]
fn a_disabled_target_with_no_record_loses_a_mod_whose_folder_is_gone() {
    let fx = Fx::new("disable-no-record");
    fx.repair();
    fx.changes();
    let moved = lose_the_record_and_move(&fx);
    write(&fx.user_config(), &format!("targets:\n  - path: {}\n    enabled: false\n", fx.dest().display()));
    let out = fx.ways(&fx.stub_path()).args(["manage", "repair", "--source"]).arg(&moved).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let dest = fx.dest().display().to_string();
    assert_eq!(
        fx.changes(),
        vec![format!("{dest}|plugin uninstall attend@agent-ways --scope user"), format!("{dest}|plugin marketplace remove agent-ways")]
    );
}

/// A dev checkout's marketplace whose plugins/ folder is missing for now (a
/// branch without it), with attend installed from it and no record: the
/// checkout is still there, so it is the operator's. The repair reports it
/// with the way out and changes nothing.
#[test]
fn a_dev_marketplace_missing_for_now_is_reported_not_claimed() {
    let fx = Fx::new("dev-missing");
    let checkout = fx.root.join("dev");
    std::fs::create_dir_all(&checkout).unwrap();
    let plugins = checkout.join("plugins");
    write(&fx.state().join("markets.json"), &format!(r#"[{{"name":"agent-ways","source":"directory","path":"{}"}}]"#, plugins.display()));
    write(&fx.state().join("plugins.json"), r#"[{"id":"attend@agent-ways","version":"0.17.0"}]"#);
    let out = fx.repair();
    assert_eq!(fx.changes(), Vec::<String>::new());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains(&plugins.display().to_string()) && err.contains("ways manage repair"), "{err}");
}
