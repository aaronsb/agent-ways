//! `ways manage repair` and `ways manage mod` install, update and remove the
//! mods through Claude Code's plugin CLI, each to its wanted state
//! `mods.<name>.install` (ADR-512): attend wanted by default, ways-judge
//! wanted with `gate.engine: claude-code`. A stub `claude` on PATH keeps the
//! marketplace and plugin lists in files, one set per config directory,
//! writes `enabledPlugins` into the target's settings as Claude Code does,
//! answers `--version` from a file, and logs every call, so the test reads
//! exactly what was asked of Claude Code.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const STUB: &str = r#"#!/bin/sh
# A stand-in for `claude plugin ...`: state in $STUB_STATE, one log line a call.
echo "${CLAUDE_CONFIG_DIR:-unset}|$*" >> "$STUB_STATE/log"
# The fixture's `dest` keeps its lists at the top; any other config dir
# keeps its own beside them.
s="$STUB_STATE"
if [ -n "${CLAUDE_CONFIG_DIR:-}" ] && [ "$(basename "$CLAUDE_CONFIG_DIR")" != dest ]; then
  s="$STUB_STATE/$(basename "$CLAUDE_CONFIG_DIR")"; mkdir -p "$s"
fi
m="$s/markets.json"; p="$s/plugins.json"
[ -f "$m" ] || echo '[]' > "$m"
[ -f "$p" ] || echo '[]' > "$p"
# A call whose arguments are a line of $STUB_STATE/fail fails.
if [ -f "$STUB_STATE/fail" ] && grep -qxF -- "$*" "$STUB_STATE/fail"; then
  echo "stub claude: failing $*" >&2; exit 1
fi
# The installed plugins, one JSON object a line.
entries() { tr -d '[]\n' < "$p" | sed 's/},{/}\
{/g' | grep -v '^$'; }
# Objects on stdin become plugins.json, and the target's enabledPlugins.
save() {
  objs=$(cat)
  printf '[%s]\n' "$(printf '%s\n' "$objs" | grep -v '^$' | paste -sd, -)" > "$p"
  if [ -n "${CLAUDE_CONFIG_DIR:-}" ]; then
    ids=$(printf '%s\n' "$objs" | sed -n 's/.*"id":"\([^"]*\)".*/"\1":true/p' | paste -sd, -)
    printf '{"enabledPlugins":{%s}}\n' "$ids" > "$CLAUDE_CONFIG_DIR/settings.json"
  fi
}
shipped() { case "$1" in attend@*) echo "$STUB_SHIPPED" ;; *) echo "$STUB_JUDGE_SHIPPED" ;; esac; }
case "$*" in
  "--version") if [ -f "$STUB_STATE/version" ]; then echo "$(cat "$STUB_STATE/version") (Claude Code)"; else echo "2.1.309 (Claude Code)"; fi ;;
  "plugin marketplace list --json") cat "$m" ;;
  # A file $STUB_STATE/preamble prints before the plugin list, as a notice would.
  "plugin list --json") [ -f "$STUB_STATE/preamble" ] && cat "$STUB_STATE/preamble"; cat "$p" ;;
  "plugin marketplace add "*) printf '[{"name":"agent-ways","source":"directory","path":"%s"}]\n' "$4" > "$m" ;;
  # Claude Code uninstalls a marketplace's plugins when it removes the marketplace.
  "plugin marketplace remove agent-ways") echo '[]' > "$m"; entries | grep -v '"id":"[^"]*@agent-ways"' | save ;;
  "plugin marketplace update agent-ways") ;;
  "plugin install "*"@agent-ways --scope user"|"plugin update "*"@agent-ways --scope user")
    { entries | grep -vF "\"id\":\"$3\""; printf '{"id":"%s","version":"%s","scope":"user"}\n' "$3" "$(shipped "$3")"; } | save ;;
  "plugin uninstall "*"@agent-ways --scope user") entries | grep -vF "\"id\":\"$3\"" | save ;;
  *) echo "stub claude: unexpected: $*" >&2; exit 2 ;;
esac
"#;

const ATTEND: &str = "attend@agent-ways";
const JUDGE: &str = "ways-judge@agent-ways";

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
        write(
            &src.join("plugins/.claude-plugin/marketplace.json"),
            r#"{"name":"agent-ways","plugins":[{"name":"attend","source":"./attend"},{"name":"ways-judge","source":"./ways-judge"}]}"#,
        );
        write(&src.join("plugins/attend/.claude-plugin/plugin.json"), r#"{"name":"attend","version":"0.17.0"}"#);
        write(&src.join("plugins/ways-judge/.claude-plugin/plugin.json"), r#"{"name":"ways-judge","version":"1.42.1"}"#);
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
            .env("STUB_SHIPPED", "0.17.0")
            .env("STUB_JUDGE_SHIPPED", "1.42.1");
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
            .args(["manage", "repair", "--commit", "--source"])
            .arg(src)
            .arg("--dest")
            .arg(self.dest())
            .output()
            .unwrap();
        assert!(out.status.success(), "repair failed: {}", String::from_utf8_lossy(&out.stderr));
        out
    }

    /// The calls that changed something, without the list and version
    /// queries.
    fn changes(&self) -> Vec<String> {
        let log = std::fs::read_to_string(self.state().join("log")).unwrap_or_default();
        let _ = std::fs::remove_file(self.state().join("log"));
        log.lines().filter(|l| !l.ends_with("list --json") && !l.ends_with("|--version")).map(str::to_string).collect()
    }

    /// The user config: the one target, enabled or not, whether attend is
    /// wanted, and whether ways-judge is.
    fn configure_mods(&self, enabled: bool, attend: bool, judge: bool) {
        write(
            &self.user_config(),
            &format!(
                "mods:\n  attend:\n    install: {attend}\n  ways-judge:\n    install: {judge}\n{}",
                self.targets(&[(&self.dest(), enabled)])
            ),
        );
    }

    /// The targets list for `dirs`, each enabled or not.
    fn targets(&self, dirs: &[(&Path, bool)]) -> String {
        let mut text = "targets:\n".to_string();
        for (d, on) in dirs {
            text.push_str(&format!("  - path: {}\n    enabled: {on}\n", d.display()));
        }
        text
    }

    fn judge_wanted(&self) -> bool {
        std::fs::read_to_string(self.user_config()).unwrap_or_default().contains("ways-judge:\n    install: true")
    }

    /// Both mods wanted and installed: attend by default, ways-judge as
    /// `gate.engine: claude-code` wants it.
    fn both(&self) {
        self.configure_mods(true, true, true);
        self.repair_targets(&self.src());
        assert!(self.has(JUDGE) && self.has(ATTEND), "{}", self.plugins());
        self.changes();
    }

    /// `repair --commit` against the target once its config disables it.
    fn repair_disabled(&self) {
        let text = std::fs::read_to_string(self.user_config()).unwrap_or_default();
        let text = text.replace("enabled: true", "enabled: false");
        let text = if text.contains("targets:") { text } else { format!("{text}{}", self.targets(&[(&self.dest(), false)])) };
        write(&self.user_config(), &text);
        self.repair_targets(&self.src());
    }

    /// `repair --commit` over the configured targets.
    fn repair_targets(&self, src: &Path) -> Output {
        let out = self.ways(&self.stub_path()).args(["manage", "repair", "--commit", "--source"]).arg(src).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        out
    }

    /// Both mods installed, attend kept or not, then the operator uninstalls
    /// ways-judge by hand.
    fn judge_removed_by_hand(&self, attend: bool) {
        self.both();
        self.configure_mods(true, attend, true);
        self.repair_targets(&self.src());
        self.by_hand(&["plugin", "uninstall", JUDGE, "--scope", "user"]);
        assert!(!self.has(JUDGE) && self.has(ATTEND) == attend, "{}", self.plugins());
        self.changes();
    }

    /// The config of the target at `dir`, where repair writes a removal by
    /// hand.
    fn target_config(&self, dir: &Path) -> PathBuf {
        let real = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        let key = real.display().to_string().replace(['/', ':'], "-");
        self.root.join("home/.config/agent-ways/targets").join(key).join("config.yaml")
    }

    fn target_text(&self, dir: &Path) -> String {
        std::fs::read_to_string(self.target_config(dir)).unwrap_or_default()
    }

    /// Claude Code's version as the stub's `--version` reports it.
    fn claude_code(&self, version: &str) {
        write(&self.state().join("version"), version);
    }

    /// `ways <args>` with the stub on PATH.
    fn run(&self, args: &[&str]) -> Output {
        self.ways(&self.stub_path()).args(args).env("NO_COLOR", "1").env("XDG_DATA_HOME", self.app_home()).output().unwrap()
    }

    /// `ways <args>` with `input` on stdin.
    fn run_stdin(&self, args: &[&str], input: &str) -> Output {
        use std::io::Write as _;
        let mut child = self
            .ways(&self.stub_path())
            .args(args)
            .env("NO_COLOR", "1")
            .env("XDG_DATA_HOME", self.app_home())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        child.wait_with_output().unwrap()
    }

    /// The directory `ways manage mod` reads the app from: `src` under it.
    fn app_home(&self) -> PathBuf {
        let data = self.root.join("data");
        if !data.join("agent-ways").exists() {
            std::fs::create_dir_all(&data).unwrap();
            std::os::unix::fs::symlink(self.src(), data.join("agent-ways")).unwrap();
        }
        data
    }

    /// The agent file, where `gate.engine` is written.
    fn agent_yaml(&self) -> PathBuf {
        self.root.join("home/.config/agent-ways/agent.yaml")
    }

    fn plugins(&self) -> String {
        std::fs::read_to_string(self.state().join("plugins.json")).unwrap_or_default()
    }

    fn has(&self, id: &str) -> bool {
        self.plugins().contains(&format!("\"id\":\"{id}\""))
    }

    /// The target's settings: the `enabledPlugins` the stub keeps.
    fn enabled(&self) -> String {
        std::fs::read_to_string(self.dest().join("settings.json")).unwrap_or_default()
    }

    /// The operator's own `claude plugin <args>` against the target.
    fn by_hand(&self, args: &[&str]) {
        let out = Command::new(self.root.join("stubbin/claude"))
            .args(args)
            .env("STUB_STATE", self.state())
            .env("CLAUDE_CONFIG_DIR", self.dest())
            .env("STUB_SHIPPED", "0.17.0")
            .env("STUB_JUDGE_SHIPPED", "1.42.1")
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }

    /// Calls to fail, by their exact arguments; empty clears them.
    fn fail(&self, calls: &[&str]) {
        write(&self.state().join("fail"), &calls.iter().map(|c| format!("{c}\n")).collect::<String>());
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
    write(&fx.user_config(), "mods:\n  attend:\n    install: false\n");
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
    let out = fx.ways(&fx.stub_path()).args(["manage", "repair", "--commit", "--source"]).arg(fx.src()).output().unwrap();
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
        let out = fx.ways("/usr/bin:/bin").args(["manage", "repair", "--commit", "--source"]).arg(fx.src()).arg("--dest").arg(fx.dest()).output().unwrap();
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
    let recover = format!("run `CLAUDE_CONFIG_DIR={} claude plugin marketplace remove agent-ways` and then `ways manage repair --commit`", fx.dest().display());
    assert!(err.contains(&recover), "the note gives the way out: {err}");
    // Disabling the target leaves it alone too.
    write(&fx.user_config(), &format!("targets:\n  - path: {}\n    enabled: false\n", fx.dest().display()));
    let out = fx.ways(&fx.stub_path()).args(["manage", "repair", "--commit", "--source"]).arg(fx.src()).output().unwrap();
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
    let out = fx.ways(&fx.stub_path()).args(["manage", "repair", "--commit", "--source"]).arg(&moved).output().unwrap();
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

#[test]
fn repair_leaves_an_unwanted_judge_mod_alone_and_installs_a_wanted_one() {
    let fx = Fx::new("judge-wanted");
    fx.configure_mods(true, true, false);
    fx.repair_targets(&fx.src());
    let dest = fx.dest().display().to_string();
    assert!(!fx.changes().iter().any(|c| c.contains(JUDGE)), "not wanted, no ways-judge call");
    fx.configure_mods(true, true, true);
    fx.repair_targets(&fx.src());
    assert_eq!(fx.changes(), vec![format!("{dest}|plugin install {JUDGE} --scope user")]);
    assert!(fx.enabled().contains(&format!("\"{JUDGE}\":true")), "{}", fx.enabled());
    fx.repair_targets(&fx.src());
    assert_eq!(fx.changes(), Vec::<String>::new(), "converged");
}

/// ADR-512's `observable`: the old record file is not read; only the wanted
/// state installs ways-judge.
#[test]
fn the_old_judge_record_file_is_not_read() {
    let fx = Fx::new("judge-old-record");
    fx.configure_mods(true, true, false);
    fx.repair_targets(&fx.src());
    let market = walk(&fx.root.join("home"))
        .into_iter()
        .find(|p| p.file_name().is_some_and(|n| n == "attend-mod-marketplace"))
        .expect("a repair has kept its record");
    write(&market.with_file_name("ways-judge-installed"), &format!("{JUDGE}\npending\n"));
    fx.changes();
    fx.repair_targets(&fx.src());
    assert!(!fx.changes().iter().any(|c| c.contains(JUDGE)), "the record file installs nothing");
    assert!(!fx.has(JUDGE));
    assert!(!market.with_file_name("ways-judge-installed").exists(), "repair deletes the old record");
}

/// `ways settings apply` writes `gate.engine` through the engine choice, as
/// `set` does (ADR-512 item 4): claude-code wants the mod first, a key engine
/// stops wanting it, and the next repair converges the mod.
#[test]
fn apply_writes_the_engine_and_the_judge_mods_wanted_state_together() {
    let fx = Fx::new("apply-engine");
    fx.configure_mods(true, true, false);
    fx.repair_targets(&fx.src());
    fx.changes();
    let out = fx.run_stdin(&["settings", "apply"], "engine: claude-code\n");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{}\n{err}", String::from_utf8_lossy(&out.stdout));
    assert!(fx.judge_wanted(), "{}", std::fs::read_to_string(fx.user_config()).unwrap());
    assert!(std::fs::read_to_string(fx.agent_yaml()).unwrap().contains("engine: claude-code"));
    assert!(err.contains("ways manage mod repair ways-judge --commit"), "apply names the repair: {err}");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(report["accepted"].as_array().unwrap().iter().any(|k| k == "gate.engine"), "{report}");
    fx.repair_targets(&fx.src());
    assert_eq!(fx.changes(), vec![format!("{}|plugin install {JUDGE} --scope user", fx.dest().display())]);

    let out = fx.run_stdin(&["settings", "apply"], "engine: anthropic\n");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(!fx.judge_wanted(), "a key engine stops wanting the mod");
    fx.repair_targets(&fx.src());
    assert_eq!(fx.changes(), vec![format!("{}|plugin uninstall {JUDGE} --scope user", fx.dest().display())]);

    // An object that clears the engine clears it the same way.
    write(&fx.agent_yaml(), "engine: claude-code\n");
    let out = fx.run_stdin(&["settings", "apply"], "engine: null\n");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(!std::fs::read_to_string(fx.agent_yaml()).unwrap().contains("engine:"), "{}", std::fs::read_to_string(fx.agent_yaml()).unwrap());
}

/// `ways settings apply` refuses the user config's `mods.ways-judge.install`
/// as `set` does, and writes nothing.
#[test]
fn apply_refuses_the_users_judge_install() {
    let fx = Fx::new("apply-refused");
    let out = fx.run_stdin(&["settings", "apply"], "mods:\n  ways-judge:\n    install: true\n");
    assert_eq!(out.status.code(), Some(3), "{}", String::from_utf8_lossy(&out.stdout));
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let rejected = report["rejected"].to_string();
    assert!(rejected.contains("mods.ways-judge.install") && rejected.contains("follows gate.engine"), "{rejected}");
    assert!(!fx.user_config().exists(), "nothing written");
    // The value in effect, as `ways settings emit` prints it, is passed over.
    let out = fx.run_stdin(&["settings", "apply"], "mods:\n  ways-judge:\n    install: false\n");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    assert!(!fx.user_config().exists(), "an unchanged value writes nothing");
}

#[test]
fn install_false_removes_attend_and_keeps_the_marketplace_the_judge_runs_from() {
    let fx = Fx::new("judge-install-false");
    fx.both();
    fx.configure_mods(true, false, true);
    fx.repair_targets(&fx.src());
    let dest = fx.dest().display().to_string();
    assert_eq!(fx.changes(), vec![format!("{dest}|plugin uninstall {ATTEND} --scope user")]);
    assert!(fx.has(JUDGE) && !fx.has(ATTEND), "{}", fx.plugins());
    fx.repair_targets(&fx.src());
    assert_eq!(fx.changes(), Vec::<String>::new(), "converged");
}

#[test]
fn a_disabled_target_holding_both_mods_loses_both_then_the_marketplace() {
    let fx = Fx::new("judge-disabled");
    fx.both();
    fx.repair_disabled();
    let dest = fx.dest().display().to_string();
    assert_eq!(
        fx.changes(),
        vec![
            format!("{dest}|plugin uninstall {JUDGE} --scope user"),
            format!("{dest}|plugin uninstall {ATTEND} --scope user"),
            format!("{dest}|plugin marketplace remove agent-ways"),
        ]
    );
    assert_eq!(fx.plugins().trim(), "[]");
    // The settings stay: enabling the target again installs what is wanted,
    // and the emptied record reads no mod as removed by hand.
    fx.configure_mods(true, true, true);
    fx.repair_targets(&fx.src());
    assert!(fx.has(JUDGE) && fx.has(ATTEND), "{}", fx.plugins());
    assert_eq!(fx.target_text(&fx.dest()), "", "no install false written");
}

#[test]
fn a_disabled_target_loses_the_judge_mod_from_a_source_that_no_longer_ships_it() {
    let fx = Fx::new("judge-unshipped");
    fx.both();
    std::fs::remove_dir_all(fx.src().join("plugins/ways-judge")).unwrap();
    fx.repair_disabled();
    let dest = fx.dest().display().to_string();
    assert_eq!(fx.changes()[0], format!("{dest}|plugin uninstall {JUDGE} --scope user"));
    assert!(!fx.has(JUDGE), "{}", fx.plugins());
}

#[test]
fn a_moved_app_holding_both_mods_reinstalls_both_in_one_repair() {
    let fx = Fx::new("judge-moved");
    fx.both();
    let moved = fx.root.join("moved");
    fx.repair_targets(&moved);
    let dest = fx.dest().display().to_string();
    assert_eq!(
        fx.changes(),
        vec![
            format!("{dest}|plugin marketplace remove agent-ways"),
            format!("{dest}|plugin marketplace add {} --scope user", moved.join("plugins").display()),
            format!("{dest}|plugin install {ATTEND} --scope user"),
            format!("{dest}|plugin install {JUDGE} --scope user"),
        ]
    );
    fx.repair_targets(&moved);
    assert_eq!(fx.changes(), Vec::<String>::new(), "converged");
}

/// Review of #1142, finding 1: the marketplace removal takes ways-judge with
/// it, and a later call fails. A failed plugin install no longer stops the
/// plan, and a failed install is never read as a removal by hand.
#[test]
fn a_failed_install_after_a_move_never_loses_the_judge_mod() {
    let fx = Fx::new("judge-moved-fails");
    fx.both();
    let moved = fx.root.join("moved");
    let dest = fx.dest().display().to_string();
    fx.fail(&[&format!("plugin install {ATTEND} --scope user")]);
    let out = fx.repair_targets(&moved);
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains(&format!("`claude plugin install {ATTEND} --scope user` failed")), "the failure is reported: {err}");
    assert!(err.contains(&format!("attend mod: installed {JUDGE} again")), "{err}");
    assert!(fx.has(JUDGE) && !fx.has(ATTEND), "{}", fx.plugins());
    fx.changes();
    fx.fail(&[]);
    fx.repair_targets(&moved);
    assert_eq!(fx.changes(), vec![format!("{dest}|plugin install {ATTEND} --scope user")]);

    let away = fx.root.join("away");
    copy_dir(&moved, &away);
    fx.fail(&[&format!("plugin install {JUDGE} --scope user")]);
    fx.repair_targets(&away);
    assert!(!fx.has(JUDGE) && !fx.enabled().contains(JUDGE), "lost: {}", fx.plugins());
    fx.changes();
    fx.fail(&[]);
    fx.repair_targets(&away);
    assert_eq!(fx.changes(), vec![format!("{dest}|plugin install {JUDGE} --scope user")]);
    assert_eq!(fx.target_text(&fx.dest()), "", "no install false written for a failed install");
}

/// ADR-512's `observable`: a fresh target where the mod was never installed,
/// or where its install failed, gets it at the next repair; absence alone is
/// never a removal by hand.
#[test]
fn a_never_installed_or_failed_mod_is_installed_not_read_as_removed() {
    let fx = Fx::new("never-installed");
    fx.configure_mods(true, true, false);
    fx.fail(&[&format!("plugin install {ATTEND} --scope user")]);
    fx.repair_targets(&fx.src());
    assert!(!fx.has(ATTEND));
    fx.changes();
    fx.fail(&[]);
    fx.repair_targets(&fx.src());
    assert_eq!(fx.changes(), vec![format!("{}|plugin install {ATTEND} --scope user", fx.dest().display())]);
    assert_eq!(fx.target_text(&fx.dest()), "");
}

/// ADR-512's `observable`: the operator uninstalls ways-judge by hand. Repair
/// does not reinstall it, writes `mods.ways-judge.install: false` in that
/// target's config, says so once, and leaves `gate.engine` as it is.
#[test]
fn a_judge_mod_uninstalled_by_hand_stays_gone_and_is_written_back_as_install_false() {
    let fx = Fx::new("judge-by-hand");
    fx.both();
    write(&fx.agent_yaml(), "engine: claude-code\n");
    fx.by_hand(&["plugin", "uninstall", JUDGE, "--scope", "user"]);
    fx.changes();
    let out = fx.repair_targets(&fx.src());
    assert_eq!(fx.changes(), Vec::<String>::new(), "nothing reinstalled");
    let err = String::from_utf8_lossy(&out.stderr);
    let said: Vec<&str> = err.lines().filter(|l| l.contains("removed by hand")).collect();
    assert_eq!(said.len(), 1, "one line says so: {err}");
    assert!(said[0].contains("mods.ways-judge.install: false") && said[0].contains("ways manage mod install ways-judge"), "{}", said[0]);
    assert!(fx.target_text(&fx.dest()).contains("ways-judge:\n    install: false"), "{}", fx.target_text(&fx.dest()));
    assert!(fx.judge_wanted(), "the user config is untouched");
    assert_eq!(std::fs::read_to_string(fx.agent_yaml()).unwrap(), "engine: claude-code\n", "gate.engine stays");
    assert!(!fx.has(JUDGE) && fx.has(ATTEND), "{}", fx.plugins());
    fx.repair_targets(&fx.src());
    assert_eq!(fx.changes(), Vec::<String>::new(), "and stays gone");

    // `mod check` reports the disagreement, names both fixes, and exits 3.
    let out = fx.run(&["manage", "mod", "check", "ways-judge"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(3), "{text}");
    assert!(text.contains("gate.engine is claude-code") && text.contains("ways manage mod install ways-judge") && text.contains("ways judge-setup"), "{text}");
}

/// With mods.attend.install false the judge's pass is the one that moves the
/// marketplace, and its failure is recovered the same way.
#[test]
fn install_false_and_a_moved_app_move_the_judge_mod_alone() {
    let fx = Fx::new("judge-moved-alone");
    fx.both();
    fx.configure_mods(true, false, true);
    fx.repair_targets(&fx.src());
    fx.changes();
    let moved = fx.root.join("moved");
    let dest = fx.dest().display().to_string();
    fx.fail(&[&format!("plugin install {JUDGE} --scope user")]);
    fx.repair_targets(&moved);
    assert_eq!(
        fx.changes(),
        vec![
            format!("{dest}|plugin marketplace remove agent-ways"),
            format!("{dest}|plugin marketplace add {} --scope user", moved.join("plugins").display()),
            format!("{dest}|plugin install {JUDGE} --scope user"),
        ]
    );
    assert!(!fx.has(JUDGE));
    fx.fail(&[]);
    fx.repair_targets(&moved);
    assert_eq!(fx.changes(), vec![format!("{dest}|plugin install {JUDGE} --scope user")]);
    assert!(fx.has(JUDGE) && !fx.has(ATTEND), "{}", fx.plugins());
}

/// The operator uninstalls ways-judge by hand and a repair writes the
/// removal back; the target is then disabled and enabled again. The setting
/// outlives the disable, so the mod is not reinstalled.
#[test]
fn a_judge_mod_uninstalled_by_hand_and_seen_by_repair_stays_gone_across_a_disable() {
    for attend in [true, false] {
        let fx = Fx::new(&format!("judge-by-hand-disable-{attend}"));
        fx.judge_removed_by_hand(attend);
        fx.repair_targets(&fx.src());
        assert!(fx.target_text(&fx.dest()).contains("install: false"), "attend {attend}");
        fx.configure_mods(false, attend, true);
        fx.repair_targets(&fx.src());
        fx.changes();
        fx.configure_mods(true, attend, true);
        fx.repair_targets(&fx.src());
        let changes = fx.changes();
        assert!(!changes.iter().any(|c| c.contains(JUDGE)), "attend {attend}: not reinstalled: {changes:?}");
        assert!(!fx.has(JUDGE), "attend {attend}: {}", fx.plugins());
    }
}

/// The operator uninstalls ways-judge by hand, then the app moves before any
/// repair. The repair that moves the marketplace installs again only what was
/// installed from it, and records the removal.
#[test]
fn a_judge_mod_uninstalled_by_hand_stays_gone_after_the_app_moves() {
    for attend in [true, false] {
        let fx = Fx::new(&format!("judge-by-hand-moved-{attend}"));
        fx.judge_removed_by_hand(attend);
        let moved = fx.root.join("moved");
        fx.repair_targets(&moved);
        let changes = fx.changes();
        assert!(!changes.iter().any(|c| c.contains(JUDGE)), "attend {attend}: not reinstalled: {changes:?}");
        assert_eq!(fx.has(ATTEND), attend, "attend {attend}: {}", fx.plugins());
        assert!(!fx.has(JUDGE), "attend {attend}: {}", fx.plugins());
        assert!(fx.target_text(&fx.dest()).contains("ways-judge:\n    install: false"), "attend {attend}: {}", fx.target_text(&fx.dest()));
        fx.repair_targets(&moved);
        fx.repair_targets(&moved);
        assert!(!fx.changes().iter().any(|c| c.contains(JUDGE)), "attend {attend}: and stays gone");
        fx.repair_targets(&moved);
        assert_eq!(fx.changes(), Vec::<String>::new(), "attend {attend}: converged");
    }
}

/// ADR-512's `observable`: with Claude Code below the ways-judge floor,
/// `ways settings set gate.engine claude-code` writes the wanted state and
/// the engine; repair reports the mod pending with the floor, at every run;
/// once Claude Code is upgraded the next repair installs it with no other
/// step.
#[test]
fn a_wanted_judge_mod_below_its_floor_is_pending_until_claude_code_is_upgraded() {
    let fx = Fx::new("judge-floor");
    fx.configure_mods(true, true, false);
    fx.claude_code("2.1.300");
    let out = fx.run(&["settings", "set", "gate.engine", "claude-code"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(fx.judge_wanted(), "{}", std::fs::read_to_string(fx.user_config()).unwrap());
    assert!(std::fs::read_to_string(fx.agent_yaml()).unwrap().contains("engine: claude-code"));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("ways manage mod repair ways-judge --commit"), "set names the repair: {err}");
    for _ in 0..2 {
        let out = fx.repair_targets(&fx.src());
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!fx.changes().iter().any(|c| c.contains(JUDGE)), "nothing for ways-judge");
        let line = err.lines().find(|l| l.contains("ways-judge mod")).unwrap_or_else(|| panic!("{err}"));
        assert!(line.contains("pending") && line.contains("2.1.300") && line.contains("2.1.304") && line.contains("ways manage update --commit"), "{line}");
    }
    assert_eq!(fx.target_text(&fx.dest()), "", "pending is never a removal");

    // `mod check`: one ⚠ line with the installed version, the floor and the
    // fix, and exit 3.
    let out = fx.run(&["manage", "mod", "check"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(3), "{text}");
    let warns: Vec<&str> = text.lines().filter(|l| l.contains('⚠')).collect();
    assert_eq!(warns.len(), 1, "{text}");
    assert!(warns[0].contains("2.1.300") && warns[0].contains("2.1.304") && warns[0].contains("ways manage update --commit"), "{}", warns[0]);

    fx.claude_code("2.1.304");
    fx.repair_targets(&fx.src());
    assert_eq!(fx.changes(), vec![format!("{}|plugin install {JUDGE} --scope user", fx.dest().display())]);
    let out = fx.run(&["manage", "mod", "check"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{text}");
    assert!(!text.contains('⚠') && text.contains("every check passes"), "{text}");
}

/// ADR-512's `observable`: a key engine chosen while the engine is
/// claude-code writes the engine, then `install: false`, and the next repair
/// uninstalls the mod from every enabled target.
#[test]
fn leaving_claude_code_writes_the_engine_then_install_false_and_repair_uninstalls() {
    let fx = Fx::new("judge-leave");
    fx.both();
    write(&fx.agent_yaml(), "engine: claude-code\n");
    let out = fx.run(&["settings", "set", "gate.engine", "anthropic"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let err = String::from_utf8_lossy(&out.stderr);
    let agent = err.find("agent.yaml").expect("the agent file is written");
    let user = err.rfind("config.yaml").expect("then the user config");
    assert!(agent < user, "engine first, then the wanted state: {err}");
    assert!(std::fs::read_to_string(fx.agent_yaml()).unwrap().contains("engine: anthropic"));
    assert!(!fx.judge_wanted());
    fx.repair_targets(&fx.src());
    assert_eq!(fx.changes(), vec![format!("{}|plugin uninstall {JUDGE} --scope user", fx.dest().display())]);
}

/// A direct write of the user config's `mods.ways-judge.install` is refused:
/// it follows the engine (ADR-512 item 4).
#[test]
fn the_user_configs_judge_install_is_set_by_the_engine_only() {
    let fx = Fx::new("judge-refused");
    for args in [&["settings", "set", "mods.ways-judge.install", "true"][..], &["settings", "unset", "mods.ways-judge.install"][..]] {
        let out = fx.run(args);
        assert_eq!(out.status.code(), Some(3), "{args:?}");
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(err.contains("follows gate.engine") && err.contains("ways judge-setup"), "{err}");
    }
    assert!(!fx.user_config().exists(), "nothing written");
}

/// ADR-512's `observable`: attend removed by hand in one target is written
/// back as `install: false` in that target's config only; every other target
/// keeps it; `ways manage mod install attend --commit` removes the override
/// and installs it again.
#[test]
fn attend_removed_by_hand_in_one_target_is_kept_out_of_that_target_only() {
    let fx = Fx::new("attend-by-hand");
    let other = fx.root.join("other");
    std::fs::create_dir_all(&other).unwrap();
    write(&fx.user_config(), &fx.targets(&[(&fx.dest(), true), (&other, true)]));
    fx.repair_targets(&fx.src());
    assert!(fx.has(ATTEND));
    let other_plugins = || std::fs::read_to_string(fx.state().join("other/plugins.json")).unwrap_or_default();
    assert!(other_plugins().contains(ATTEND), "{}", other_plugins());
    fx.by_hand(&["plugin", "uninstall", ATTEND, "--scope", "user"]);
    fx.changes();
    let out = fx.repair_targets(&fx.src());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(fx.changes().iter().all(|c| !c.contains("install")), "nothing reinstalled");
    assert!(err.contains("removed by hand") && err.contains("ways manage mod install attend"), "{err}");
    assert!(fx.target_text(&fx.dest()).contains("attend:\n    install: false"), "{}", fx.target_text(&fx.dest()));
    assert_eq!(fx.target_text(&other), "", "the other target's config is untouched");
    assert!(other_plugins().contains(ATTEND), "the other target keeps attend");
    assert!(!std::fs::read_to_string(fx.user_config()).unwrap().contains("install"), "the user config is untouched");

    // The way back.
    let out = fx.run(&["manage", "mod", "install", "attend", "--commit"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(!fx.target_text(&fx.dest()).contains("install: false"), "{}", fx.target_text(&fx.dest()));
    assert!(fx.has(ATTEND), "{}", fx.plugins());
}

/// ADR-512's `observable`: `mod uninstall attend` without --commit prints
/// the calls per target and changes nothing; with --commit it writes
/// `install: false` in the user config and uninstalls the plugin.
#[test]
fn mod_uninstall_attend_reports_until_commit() {
    let fx = Fx::new("attend-uninstall");
    fx.configure_mods(true, true, false);
    fx.repair_targets(&fx.src());
    fx.changes();
    let before = std::fs::read_to_string(fx.user_config()).unwrap();
    let out = fx.run(&["manage", "mod", "uninstall", "attend"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(text.contains(&format!("would run `claude plugin uninstall {ATTEND} --scope user`")) && text.contains("--commit"), "{text}");
    assert_eq!(fx.changes(), Vec::<String>::new(), "no call made");
    assert_eq!(std::fs::read_to_string(fx.user_config()).unwrap(), before, "nothing written");
    let out = fx.run(&["manage", "mod", "uninstall", "attend", "--commit"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(std::fs::read_to_string(fx.user_config()).unwrap().contains("attend:\n    install: false"));
    assert!(!fx.has(ATTEND), "{}", fx.plugins());
    assert!(fx.changes().contains(&format!("{}|plugin uninstall {ATTEND} --scope user", fx.dest().display())));
}

/// ADR-512's `observable`: `mod uninstall ways-judge` while the engine is
/// claude-code refuses, names `ways judge-setup` and changes nothing.
#[test]
fn mod_uninstall_ways_judge_is_refused_under_claude_code() {
    let fx = Fx::new("judge-uninstall-refused");
    fx.both();
    write(&fx.agent_yaml(), "engine: claude-code\n");
    let before = std::fs::read_to_string(fx.user_config()).unwrap();
    let out = fx.run(&["manage", "mod", "uninstall", "ways-judge", "--commit"]);
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stderr).contains("ways judge-setup"));
    assert_eq!(std::fs::read_to_string(fx.user_config()).unwrap(), before);
    assert!(fx.has(JUDGE) && fx.changes().is_empty());
}

/// ADR-512's `observable`: `mod install ways-judge` without --commit shows
/// the engine change and changes nothing; with --commit it is the engine
/// choice and installs the mod.
#[test]
fn mod_install_ways_judge_is_the_engine_choice_and_reports_until_commit() {
    let fx = Fx::new("judge-install");
    fx.configure_mods(true, true, false);
    fx.repair_targets(&fx.src());
    fx.changes();
    write(&fx.agent_yaml(), "engine: anthropic\n");
    let config = std::fs::read_to_string(fx.user_config()).unwrap();
    let out = fx.run(&["manage", "mod", "install", "ways-judge"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(text.contains("gate.engine: anthropic → claude-code") && text.contains("--commit"), "{text}");
    assert!(text.contains(&format!("would run `claude plugin install {JUDGE} --scope user`")), "{text}");
    assert_eq!(std::fs::read_to_string(fx.user_config()).unwrap(), config, "nothing written");
    assert_eq!(std::fs::read_to_string(fx.agent_yaml()).unwrap(), "engine: anthropic\n");
    assert!(fx.changes().is_empty() && !fx.has(JUDGE));
    let out = fx.run(&["manage", "mod", "install", "ways-judge", "--commit"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(fx.judge_wanted() && std::fs::read_to_string(fx.agent_yaml()).unwrap().contains("engine: claude-code"));
    assert!(fx.has(JUDGE), "{}", fx.plugins());
}

/// ADR-512's `observable`: a mod disabled in Claude Code stays installed and
/// disabled, and `mod check` names the enable command.
#[test]
fn a_disabled_mod_is_left_disabled_and_check_names_the_enable_command() {
    let fx = Fx::new("attend-disabled");
    fx.configure_mods(true, true, false);
    fx.repair_targets(&fx.src());
    fx.changes();
    write(&fx.dest().join("settings.json"), &format!("{{\"enabledPlugins\":{{\"{ATTEND}\":false}}}}"));
    fx.repair_targets(&fx.src());
    assert_eq!(fx.changes(), Vec::<String>::new(), "left as it is");
    let out = fx.run(&["manage", "mod", "check", "attend"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(3), "{text}");
    assert!(text.contains(&format!("claude plugin enable {ATTEND}")), "{text}");
}

/// A removal of the whole marketplace by hand takes every listed mod with
/// it, and each is read as removed by hand.
#[test]
fn a_marketplace_removed_by_hand_is_a_removal_of_every_mod_on_the_list() {
    let fx = Fx::new("market-by-hand");
    fx.both();
    fx.by_hand(&["plugin", "marketplace", "remove", "agent-ways"]);
    fx.changes();
    fx.repair_targets(&fx.src());
    assert_eq!(fx.changes(), Vec::<String>::new(), "nothing reinstalled");
    let text = fx.target_text(&fx.dest());
    assert!(text.contains("attend:\n    install: false") && text.contains("ways-judge:\n    install: false"), "{text}");
}

/// `mod list` names each mod, its version, its word and each target's state.
#[test]
fn mod_list_names_every_shipped_mod() {
    let fx = Fx::new("mod-list");
    fx.configure_mods(true, true, false);
    fx.repair_targets(&fx.src());
    let out = fx.run(&["manage", "mod", "list"]);
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("attend  0.17.0  converged") && text.contains("ways-judge  1.42.1  off"), "{text}");
    assert!(text.contains("installed 0.17.0, enabled"), "{text}");
}

// ── convergence review of the wanted state (follow-up to ADR-512 S2) ──

/// An explicit want after a removal by hand that no repair has seen yet
/// installs the mod: `mod install` forgets the list entry first, so the
/// removal is not written back as `install: false`.
#[test]
fn mod_install_after_an_unseen_removal_by_hand_installs_it() {
    let fx = Fx::new("install-unseen");
    fx.configure_mods(true, true, false);
    fx.repair_targets(&fx.src());
    fx.by_hand(&["plugin", "uninstall", ATTEND, "--scope", "user"]);
    fx.changes();
    let out = fx.run(&["manage", "mod", "install", "attend", "--commit"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(fx.has(ATTEND), "installed: {}\n{}", fx.plugins(), String::from_utf8_lossy(&out.stdout));
    assert!(!fx.target_text(&fx.dest()).contains("install: false"), "{}", fx.target_text(&fx.dest()));
    fx.repair_targets(&fx.src());
    assert!(fx.has(ATTEND), "and stays: {}", fx.plugins());
}

/// The same for ways-judge, through `mod install` and through the engine
/// choice (`ways settings set gate.engine claude-code`) before any repair.
#[test]
fn choosing_claude_code_after_an_unseen_removal_by_hand_installs_the_judge_mod() {
    for via_engine in [false, true] {
        let fx = Fx::new(&format!("judge-unseen-{via_engine}"));
        fx.both();
        write(&fx.agent_yaml(), "engine: claude-code\n");
        fx.by_hand(&["plugin", "uninstall", JUDGE, "--scope", "user"]);
        fx.changes();
        let out = if via_engine {
            // The engine changes away and back: an explicit choice.
            assert!(fx.run(&["settings", "set", "gate.engine", "anthropic"]).status.success());
            fx.run(&["settings", "set", "gate.engine", "claude-code"])
        } else {
            fx.run(&["manage", "mod", "install", "ways-judge", "--commit"])
        };
        assert!(out.status.success(), "via engine {via_engine}: {}", String::from_utf8_lossy(&out.stderr));
        fx.repair_targets(&fx.src());
        assert!(fx.has(JUDGE), "via engine {via_engine}: {}", fx.plugins());
        assert!(!fx.target_text(&fx.dest()).contains("install: false"), "via engine {via_engine}: {}", fx.target_text(&fx.dest()));
    }
}

/// A mod set to `install: false`, then uninstalled by hand before repair:
/// the repair that finds it gone drops it from the list, so wanting it again
/// later installs it instead of reading a removal by hand.
#[test]
fn a_stale_list_entry_never_turns_a_later_want_into_a_removal() {
    let fx = Fx::new("stale-entry");
    fx.configure_mods(true, true, false);
    fx.repair_targets(&fx.src());
    fx.configure_mods(true, false, false);
    fx.by_hand(&["plugin", "uninstall", ATTEND, "--scope", "user"]);
    fx.repair_targets(&fx.src());
    fx.changes();
    fx.configure_mods(true, true, false);
    fx.repair_targets(&fx.src());
    assert!(fx.has(ATTEND), "installed again: {}", fx.plugins());
    assert!(!fx.target_text(&fx.dest()).contains("install: false"), "{}", fx.target_text(&fx.dest()));
}

/// A plugin list Claude Code prints with a notice before the JSON is not
/// read as every mod removed by hand: the target is reported and left.
#[test]
fn an_unreadable_plugin_list_changes_nothing() {
    let fx = Fx::new("preamble");
    fx.both();
    write(&fx.state().join("preamble"), "A new version of Claude Code is available.\n");
    let out = fx.repair_targets(&fx.src());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("could not read Claude Code's plugins"), "{err}");
    assert_eq!(fx.changes(), Vec::<String>::new());
    assert_eq!(fx.target_text(&fx.dest()), "", "nothing written");
    assert!(fx.has(ATTEND) && fx.has(JUDGE));
}

/// An install from before ADR-512: gate.engine is claude-code, the user
/// config has no `mods.ways-judge.install`, and the mod is installed. Every
/// read and every what-if treats it as wanted.
#[test]
fn a_pre_adr_judge_mod_reads_as_wanted_before_any_write() {
    let fx = Fx::new("pre-adr");
    write(&fx.user_config(), &fx.targets(&[(&fx.dest(), true)]));
    fx.repair_targets(&fx.src());
    fx.by_hand(&["plugin", "install", JUDGE, "--scope", "user"]);
    write(&fx.agent_yaml(), "engine: claude-code\n");
    let before = std::fs::read_to_string(fx.user_config()).unwrap();
    fx.changes();
    for args in [&["manage", "mod", "repair"][..], &["manage", "repair"][..], &["manage", "mod", "check", "ways-judge"][..], &["manage", "mod", "list"][..]] {
        let out = fx.run(args);
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(!text.contains(&format!("uninstall {JUDGE}")), "{args:?}: {text}");
        assert!(!text.contains("not wanted"), "{args:?}: {text}");
    }
    let out = fx.run(&["manage", "mod", "check", "ways-judge"]);
    assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stdout));
    assert_eq!(std::fs::read_to_string(fx.user_config()).unwrap(), before, "what-if and reads write nothing");
    assert_eq!(fx.changes(), Vec::<String>::new());
}

/// With no `claude` at all, a wanted mod is pending (the host cannot run it
/// yet) and an unwanted one is off; neither reads as drift.
#[test]
fn no_claude_reads_as_pending_not_drift() {
    let fx = Fx::new("no-claude");
    fx.configure_mods(true, true, false);
    let out = fx.ways(&fx.stub_path()).args(["manage", "mod", "list"]).env("NO_COLOR", "1").env("XDG_DATA_HOME", fx.app_home()).env("WAYS_CLAUDE_BIN", fx.root.join("no-such-claude")).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("attend  0.17.0  pending") && text.contains("ways-judge  1.42.1  off"), "{text}");
    assert!(text.contains("`claude` is not on PATH") && !text.contains("drift"), "{text}");
}

/// Convergence re-review R1: the engine the files already hold, set again
/// (an emit then apply round trip, or a same-value `set`), is no new choice:
/// a removal by hand that no repair has seen yet is still respected.
#[test]
fn reasserting_claude_code_respects_an_unseen_removal_by_hand() {
    for via_apply in [true, false] {
        let fx = Fx::new(&format!("reassert-{via_apply}"));
        fx.both();
        write(&fx.agent_yaml(), "engine: claude-code\n");
        fx.by_hand(&["plugin", "uninstall", JUDGE, "--scope", "user"]);
        fx.changes();
        let out = if via_apply {
            fx.run_stdin(&["settings", "apply"], "engine: claude-code\n")
        } else {
            fx.run(&["settings", "set", "gate.engine", "claude-code"])
        };
        assert!(out.status.success(), "apply {via_apply}: {}", String::from_utf8_lossy(&out.stderr));
        fx.repair_targets(&fx.src());
        assert!(!fx.has(JUDGE), "apply {via_apply}: not reinstalled: {}", fx.plugins());
        assert!(fx.target_text(&fx.dest()).contains("ways-judge:\n    install: false"), "apply {via_apply}: {}", fx.target_text(&fx.dest()));
    }
}
