//! The settings screens (ADR-504 §8) through the real binary.
//!
//! Each test runs `ways` with HOME and every XDG directory in a fixture of
//! its own, and drives the screens headless: `--keys` feeds the real key
//! handler, `--snap WxH` prints the frame in the test kit's format. Paths
//! under the fixture home show as `~`, so frames do not depend on where the
//! fixture is.
//!
//! Golden frames live in `fixtures/settings-tui/`. `AGENT_TUI_BLESS=1`
//! records them for review, and that run fails by design; a run without it
//! checks them. `SETTINGS_TUI_BIN` runs the suite against another build,
//! such as the parent commit's, to see which tests fail before the change.

// Unix only: the fixture runs the binary with a minimal Unix environment
// and a shell-script stand-in, and the golden frames are Unix paths.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use agent_tui::testkit::Goldens;

static SEQ: AtomicUsize = AtomicUsize::new(0);

struct Fx {
    root: PathBuf,
}

/// A small corpus: three ways, one of them under another.
const WAYS: &[&str] = &["itops/incident", "softwaredev/code", "softwaredev/code/testing"];

impl Fx {
    fn new() -> Fx {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("ways-settings-tui-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["home/.config", "home/proj/.git", "home/.claude/projects", "home/.local/state", "home/.cache"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let fx = Fx { root };
        for id in WAYS {
            let name = id.rsplit('/').next().unwrap();
            fx.file(&format!(".local/share/agent-ways/hooks/ways/{id}/{name}.md"), &format!("---\ndescription: the {name} way\nvocabulary: {name}\n---\n# {name}\n"));
        }
        fx
    }

    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.home().join(rel)
    }

    fn file(&self, rel: &str, text: &str) {
        let p = self.path(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    /// A project at `rel` with `ways`, which Claude Code knows: its
    /// transcripts directory records the path it was named from.
    fn known(&self, rel: &str, ways: &[&str]) {
        let path = self.path(rel).display().to_string();
        let slug: String = path.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        self.file(&format!(".claude/projects/{slug}/sessions-index.json"), &format!("{{\"originalPath\": \"{path}\"}}"));
        for id in ways {
            let name = id.rsplit('/').next().unwrap();
            self.file(&format!("{rel}/.claude/ways/{id}/{name}.md"), &format!("---\ndescription: the {name} way of {rel}\nvocabulary: {name}\n---\n"));
        }
    }

    fn read(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.path(rel)).ok()
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let bin = std::env::var_os("SETTINGS_TUI_BIN").map(PathBuf::from).unwrap_or_else(|| env!("CARGO_BIN_EXE_ways").into());
        let mut c = Command::new(bin);
        let home = self.home();
        c.args(args).current_dir(home.join("proj")).env_clear();
        c.env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("CLAUDE_PROJECT_DIR", home.join("proj"))
            .env("NO_COLOR", "1");
        c
    }

    /// Run `ways` and return stdout, stderr and the exit code.
    fn run(&self, args: &[&str]) -> (String, String, i32) {
        self.run_with(args, &[])
    }

    /// [`Fx::run`] with extra environment.
    fn run_with(&self, args: &[&str], env: &[(&str, &Path)]) -> (String, String, i32) {
        let mut c = self.cmd(args);
        for (k, v) in env {
            c.env(k, v);
        }
        let o = c.output().unwrap();
        (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned(), o.status.code().unwrap_or(-1))
    }

    /// The screens on `tab`, the keys fed, then a frame at `size`.
    fn snap(&self, tab: &str, keys: &str, size: &str, depth: &str) -> String {
        let mut args = vec!["settings", tab, "--depth", depth, "--snap", size];
        if !keys.is_empty() {
            args.extend(["--keys", keys]);
        }
        let (out, err, code) = self.run(&args);
        assert_eq!(code, 0, "ways settings {tab} --keys {keys}: {err}");
        assert!(out.starts_with("agent-tui frame "), "not a frame: {out}{err}");
        // A reading action starts a real command and its frame depends on
        // when that ends: no shot may pick one.
        assert!(!out.contains("running…"), "ways settings {tab} --keys {keys} started a reading action:\n{out}");
        out
    }

    /// [`Fx::snap`] with extra environment.
    fn snap_with(&self, tab: &str, keys: &str, size: &str, depth: &str, env: &[(&str, &Path)]) -> String {
        let args = ["settings", tab, "--depth", depth, "--snap", size, "--keys", keys];
        let (out, err, code) = self.run_with(&args, env);
        assert_eq!(code, 0, "ways settings {tab} --keys {keys}: {err}");
        assert!(out.starts_with("agent-tui frame "), "not a frame: {out}{err}");
        out
    }

    /// The screens on `tab`, the keys fed, then what is left pending.
    fn drive(&self, tab: &str, keys: &str) -> String {
        let (out, err, code) = self.run(&["settings", tab, "--keys", keys]);
        assert_eq!(code, 0, "ways settings {tab} --keys {keys}: {err}");
        out
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// Make a stand-in script runnable.
fn executable(p: &Path) {
    std::fs::set_permissions(p, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
}

fn glyphs(frame: &str) -> String {
    frame.split("\nstyles\n").next().unwrap_or(frame).to_string()
}

fn goldens() -> Goldens {
    Goldens::new(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/settings-tui"))
}

// ── the apply path writes what `set` writes ────────────────────

/// A user file with comments and a hand-set value, which both writers must
/// keep as they are.
const USER: &str = "# my settings, by hand\nsemantic_fire_probability: 0.5  # tuned\n\nlanguage: auto\n";

#[test]
fn a_change_applied_in_the_screens_writes_the_bytes_set_writes() {
    let (cli, tui) = (Fx::new(), Fx::new());
    for fx in [&cli, &tui] {
        fx.file(".config/agent-ways/config.yaml", USER);
    }
    // In the order review applies them: the tree's, which is the schema's.
    for (key, value) in [
        ("matching.parent_boost_floor", "0.25"),
        ("matching.near_miss_margin", "0.1"),
        ("gate.mode", "shadow"),
        ("ways.project.itops/incident", "false"),
    ] {
        let (_, err, code) = cli.run(&["settings", "set", key, value]);
        assert_eq!(code, 0, "set {key}: {err}");
    }
    // The same four changes in the screens: two on matching, applied as one
    // write, one on gate, one on ways, each tab reviewed and applied.
    let left = tui.drive(
        "matching",
        "/ text:near_miss enter down down enter e ctrl-u text:0.1 enter \
         / text:parent_boost enter down down enter e ctrl-u text:0.25 enter w a \
         3 down down enter down enter w a \
         / text:incident enter end enter enter w a",
    );
    assert_eq!(left, "nothing pending\n", "every change applied");
    for rel in [".config/agent-ways/config.yaml", ".config/agent-ways/agent.yaml", "proj/.claude/ways.yaml"] {
        let (a, b) = (cli.read(rel), tui.read(rel));
        assert!(a.is_some(), "{rel} was written by set");
        assert_eq!(b, a, "{rel}: the screens wrote other bytes than `ways settings set`");
    }
    assert!(tui.read(".config/agent-ways/config.yaml").unwrap().starts_with("# my settings, by hand\nsemantic_fire_probability: 0.5  # tuned\n"));
}

/// An attend user file with comments, which both writers must keep.
const ATTEND: &str = "# my attend settings\nengagement:\n  decay_per_minute: 0.1  # tuned\n";

#[test]
fn an_attend_change_applied_in_the_screens_writes_the_bytes_set_writes() {
    let (cli, tui) = (Fx::new(), Fx::new());
    for fx in [&cli, &tui] {
        fx.file(".config/attend/config.yaml", ATTEND);
    }
    for (key, value) in [
        ("attend.governor.base_cooldown", "30"),
        ("attend.engagement.decay_per_minute", "0.07"),
        ("attend.sensors.processes.enabled", "false"),
    ] {
        let (_, err, code) = cli.run(&["settings", "set", key, value]);
        assert_eq!(code, 0, "set {key}: {err}");
    }
    // The same three on attend's two tabs, each reviewed and applied.
    let left = tui.drive(
        "attend",
        "/ text:base_cool enter down down enter e ctrl-u text:30 enter \
         / text:decay_per enter down down enter e ctrl-u text:0.07 enter w a \
         6 down down down right down enter w a",
    );
    assert_eq!(left, "nothing pending\n", "every change applied");
    let rel = ".config/attend/config.yaml";
    assert_eq!(tui.read(rel), cli.read(rel), "the screens wrote other bytes than `ways settings set`");
    assert!(tui.read(rel).unwrap().starts_with("# my attend settings\nengagement:\n  decay_per_minute: 0.07  # tuned\n"));
    // attend runs on what the screens wrote.
    let (out, _, _) = tui.run(&["settings", "get", "attend.sensors.processes.enabled"]);
    assert_eq!(out, "false\n");
}

#[test]
fn an_unset_watch_list_shows_the_built_in_one_is_in_effect() {
    let fx = Fx::new();
    // processes, opened: its watch list is unset, so the sensor's own applies.
    let f = glyphs(&fx.snap("sensors", "down down down right", "100x30", "16"));
    let row = f.lines().find(|l| l.contains(" watch ")).unwrap_or_default();
    assert!(row.contains("(built-in)") && !row.contains("[]"), "{f}");
    // An explicit empty list replaces it, and shows as one.
    fx.file(".config/attend/config.yaml", "sensors:\n  processes:\n    watch: []\n");
    let f = glyphs(&fx.snap("sensors", "down down down right", "100x30", "16"));
    assert!(f.lines().any(|l| l.contains(" watch ") && l.contains("[]")), "{f}");
}

#[test]
fn a_finding_in_an_attend_file_marks_its_row_on_attend_s_tab() {
    let fx = Fx::new();
    fx.file(".config/attend/config.yaml", "engagement:\n  burst_window: 900\n");
    fx.file("proj/.claude/attend.yaml", "sensors:\n  -processes:\n");
    let f = glyphs(&fx.snap("attend", "down", "120x30", "16"));
    assert!(f.contains("▾ findings") && f.contains("[attend.engagement] engagement.burst_window: unknown"), "{f}");
    assert!(f.contains("ways settings fix attend.engagement"), "the finding row queues fix:\n{f}");
    assert!(!f.contains("-processes"), "a sensor's finding is on the sensors tab:\n{f}");
    let f = glyphs(&fx.snap("sensors", "down", "160x30", "16"));
    assert!(f.contains("'-processes' is not a sensor name"), "{f}");
    // fix, queued from the finding row, repairs the user file.
    let left = fx.drive("attend", "down a enter y w a");
    assert_eq!(left, "nothing pending\n");
    assert_eq!(fx.read(".config/attend/config.yaml").as_deref(), Some(""));
}

#[test]
fn the_theme_choice_is_a_settings_key_written_as_set_writes_it() {
    let (cli, tui) = (Fx::new(), Fx::new());
    let (_, err, code) = cli.run(&["settings", "set", "theme.active", "nord"]);
    assert_eq!(code, 0, "{err}");
    // terminal, agent-ways, nord: the cursor's third row, made active.
    tui.drive("theme", "down down enter");
    assert_eq!(tui.read(".config/agent-ways/config.yaml"), cli.read(".config/agent-ways/config.yaml"));
    assert!(!tui.path(".config/agent-ways/themes/active").exists(), "no provisional choice file");
    let (out, _, _) = tui.run(&["settings", "get", "theme.active"]);
    assert_eq!(out, "nord\n");
    // The screens open on the choice the key holds.
    let f = glyphs(&tui.snap("theme", "", "100x12", "16"));
    assert!(f.lines().any(|l| l.contains("● nord")), "{f}");
}

#[test]
fn the_old_active_theme_file_is_not_read() {
    let fx = Fx::new();
    fx.file(".config/agent-ways/themes/active", "dracula\n");
    let f = glyphs(&fx.snap("theme", "", "100x12", "16"));
    assert!(f.lines().any(|l| l.contains("● terminal")) && !f.lines().any(|l| l.contains("● dracula")), "{f}");
}

// ── a broken file fails closed and takes no write ──────────────

const BROKEN: &str = "near_miss_margin: 0.2\nmatching: [unclosed\n";

#[test]
fn a_file_that_does_not_parse_shows_its_finding_and_takes_no_write() {
    let fx = Fx::new();
    fx.file(".config/agent-ways/config.yaml", BROKEN);
    let f = glyphs(&fx.snap("matching", "down", "100x30", "16"));
    for want in ["▾ findings", "does not parse", "the whole", "~/.config/agent-ways/config.yaml:3"] {
        assert!(f.contains(want), "{want} missing:\n{f}");
    }
    // Editing a value that file holds is refused, and says why.
    let f = glyphs(&fx.snap("matching", "/ text:near_miss enter end enter e", "200x30", "16"));
    assert!(f.contains("locked: ~/.config/agent-ways/config.yaml does not parse"), "{f}");
    let f = glyphs(&fx.snap("matching", "/ text:near_miss enter end enter enter", "200x30", "16"));
    assert!(f.contains("locked:"), "Enter on a locked number refuses too:\n{f}");
    // A bool in the same file: Enter would toggle it; it is locked instead.
    let f = glyphs(&fx.snap("install", "/ text:secret_path enter end enter enter", "200x30", "16"));
    assert!(f.contains("locked:"), "{f}");
    // Review and apply have nothing to write; the theme choice, which lives
    // in the same file, is refused with the writer's reason.
    let left = fx.drive("matching", "/ text:near_miss enter down enter e text:9 enter w a");
    assert_eq!(left, "nothing pending\n");
    let f = glyphs(&fx.snap("theme", "down down enter", "200x30", "16"));
    assert!(f.contains("rejected: ~/.config/agent-ways/config.yaml does not parse, so it fails closed and takes no write"), "{f}");
    assert_eq!(fx.read(".config/agent-ways/config.yaml").as_deref(), Some(BROKEN), "the broken file is untouched");
}

#[test]
fn a_finding_in_one_section_marks_its_row_and_offers_fix() {
    let fx = Fx::new();
    fx.file(".config/agent-ways/config.yaml", "near_miss_margin: 7\n");
    let f = glyphs(&fx.snap("matching", "down", "100x30", "16"));
    assert!(f.contains("▾ findings") && f.contains("[matching] near_miss_margin: 7 is outside"), "{f}");
    assert!(f.lines().any(|l| l.contains("near_miss_margin") && l.contains(" !")), "the row is marked:\n{f}");
    // The finding row's action queues `ways settings fix matching`, which
    // review applies; the file then lints clean.
    let left = fx.drive("matching", "down a enter y w a");
    assert_eq!(left, "nothing pending\n");
    let (_, err, code) = fx.run(&["settings", "lint"]);
    assert_eq!(code, 0, "{err}");
}

// ── the CLI around the screens ─────────────────────────────────

#[test]
fn bare_settings_in_a_pipe_prints_list_and_a_tab_needs_a_terminal() {
    let fx = Fx::new();
    let (bare, _, code) = fx.run(&["settings"]);
    let (list, _, _) = fx.run(&["settings", "list"]);
    assert_eq!((code, &bare), (0, &list));
    let (_, err, code) = fx.run(&["settings", "gate"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("need a terminal"), "{err}");
    let (_, err, code) = fx.run(&["settings", "nope", "--snap", "80x25"]);
    assert_eq!(code, 2);
    assert!(err.contains("no tab nope; the tabs are ways, matching, gate, install, attend, sensors, theme"), "{err}");
}

#[test]
fn the_help_overlay_shows_the_text_settings_help_prints() {
    let fx = Fx::new();
    for tab in ["ways", "matching", "gate", "install", "attend", "sensors", "theme"] {
        let (help, _, code) = fx.run(&["settings", "help", tab]);
        assert_eq!(code, 0);
        let f = glyphs(&fx.snap(tab, "?", "160x60", "16"));
        assert!(f.contains(&format!("help: {tab}")), "{f}");
        for line in help.lines().filter(|l| !l.trim().is_empty()) {
            assert!(f.contains(line.trim_end()), "{tab}: `{line}` is not in the overlay:\n{f}");
        }
    }
}

// ── rows keep their place; commands report what they printed ──

/// The tree pane of a frame: the left half of each row, values included.
fn tree_pane(frame: &str) -> Vec<String> {
    glyphs(frame).lines().skip(2).map(|l| l.chars().take(56).collect()).collect()
}

#[test]
fn setting_a_way_toggle_keeps_every_row_in_place() {
    let fx = Fx::new();
    // Open project, itops and softwaredev; the cursor on softwaredev. Under
    // project, the shipped ways follow this project's section.
    let keys = "end right down down down down right down down right";
    let before = fx.snap("ways", keys, "100x30", "16");
    let (_, err, code) = fx.run(&["settings", "set", "ways.project.softwaredev/code/testing", "false"]);
    assert_eq!(code, 0, "{err}");
    let after = fx.snap("ways", keys, "100x30", "16");
    assert_eq!(tree_pane(&after), tree_pane(&before), "a toggle a file names takes the same place as one it does not");
    assert!(tree_pane(&after).iter().any(|l| l.contains('▌') && l.contains("softwaredev")), "{}", glyphs(&after));
}

#[test]
fn after_an_apply_the_cursor_is_on_the_same_key() {
    let fx = Fx::new();
    // Toggle softwaredev/code/testing, apply; the reload puts the toggle in
    // the project file, and the cursor stays on it.
    let f = glyphs(&fx.snap("ways", "end right down down down down right down down right down right down enter w a", "100x30", "16"));
    let cursor = tree_pane(&f).into_iter().find(|l| l.contains('▌')).unwrap_or_default();
    assert!(cursor.contains("testing") && cursor.contains("false"), "the cursor moved off its key:
{f}");
    let pane = tree_pane(&f).join("
");
    assert!(pane.find("itops").unwrap() < pane.find("softwaredev").unwrap(), "{pane}");
    assert_eq!(fx.read("proj/.claude/ways.yaml").as_deref().map(|t| t.contains("softwaredev/code/testing: false")), Some(true));
}

#[test]
fn a_failed_command_reports_its_stdout_when_stderr_is_empty() {
    let fx = Fx::new();
    // A stand-in for the binary that prints its refusal on stdout and fails,
    // as `ways agent key add` does when a check is not confirmed.
    let runner = fx.path("runner.sh");
    std::fs::write(&runner, "#!/bin/sh
echo 'check: the key was not confirmed'
exit 1
").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::PermissionsExt::set_mode(&mut std::fs::metadata(&runner).unwrap().permissions(), 0o755);
    #[cfg(unix)]
    std::fs::set_permissions(&runner, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    // install: the targets row's menu, add, a directory, confirmed, review,
    // apply. (Plan only reads: it runs at once and is never queued.)
    let (out, err, code) = fx.run_with(
        &["settings", "install", "--depth", "16", "--snap", "240x30", "--keys", "a down enter text:/tmp/x enter y w a"],
        &[("WAYS_SETTINGS_RUNNER", &runner)],
    );
    assert_eq!(code, 0, "{err}");
    let f = glyphs(&out);
    assert!(f.contains("exit 1: check: the key was not confirmed"), "{f}");
    assert!(f.contains("The command may have done part of its work and the tree is read again"), "{f}");
}

#[test]
fn keys_may_come_before_the_other_flags() {
    let fx = Fx::new();
    let (out, err, code) = fx.run(&["settings", "matching", "--keys", "down", "--snap", "80x12", "--depth", "16"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.starts_with("agent-tui frame 80x12"), "{out}");
}

#[test]
fn a_key_script_cannot_type_a_secret() {
    let fx = Fx::new();
    let (_, err, code) = fx.run(&["settings", "gate", "--keys", "/ text:keys.anthropic enter end enter enter text:sk-123", "--snap", "80x12"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--keys cannot type into a masked entry"), "{err}");
}

// ── golden frames ──────────────────────────────────────────────

/// Each settings tab in browse, edit and review, at 100x30 in the 16-colour
/// default, attend's two tabs among them; the theme tab's edit is its slot editor and its review is the
/// exit guard listing its unsaved edits. Then the minimum size, a chosen
/// theme in truecolor, and a broken file.
#[test]
fn golden_frames() {
    let fx = Fx::new();
    fx.file(".config/agent-ways/config.yaml", "# by hand\nnear_miss_margin: 0.1\n");
    let mut g = goldens();
    let shots: &[(&str, &str, &str, &str)] = &[
        ("ways-browse", "ways", "down down down down down down right", "100x30"),
        ("ways-edit", "ways", "down down down e", "100x30"),
        ("ways-review", "ways", "down enter / text:incident enter end enter enter w", "100x30"),
        ("matching-browse", "matching", "", "100x30"),
        ("matching-edit", "matching", "down down e ctrl-u text:0.2", "100x30"),
        ("matching-review", "matching", "down e ctrl-u text:0.4 enter down down down down e ctrl-u text:0.2 enter w", "100x30"),
        ("gate-browse", "gate", "down down down right down right", "100x30"),
        ("gate-edit", "gate", "down e text:anthropic", "100x30"),
        ("install-browse", "install", "", "100x30"),
        ("install-edit", "install", "down down down e", "100x30"),
        ("install-review", "install", "down down down enter a down enter y w", "100x30"),
        ("theme-browse", "theme", "down down", "100x30"),
        ("theme-edit", "theme", "down down e text:mine enter down down down down down down down down enter", "100x30"),
        ("theme-review", "theme", "down down e text:mine enter q", "100x30"),
        ("matching-80x25", "matching", "down down", "80x25"),
        ("attend-browse", "attend", "down down right", "100x30"),
        ("attend-edit", "attend", "down down right down down down down e ctrl-u text:900", "100x30"),
        ("attend-review", "attend", "right down e ctrl-u text:30 enter w", "100x30"),
        ("sensors-browse", "sensors", "down down down right", "100x30"),
        ("sensors-edit", "sensors", "down right down down e ctrl-u text:45", "100x30"),
        ("sensors-review", "sensors", "down down down right down enter w", "100x30"),
    ];
    for (name, tab, keys, size) in shots {
        g.check_text(name, &fx.snap(tab, keys, size, "16"));
    }
    // A stored anthropic key, so its menu has rotate and remove: remove is
    // queued and confirmed. Its check only reads and runs at once; the shots
    // that pick it below run a stand-in, never the real check.
    let keyed = Fx::new();
    keyed.file(".config/agent-ways/config.yaml", "# by hand\nnear_miss_margin: 0.1\n");
    keyed.file(".config/agent-ways/keys/anthropic", "sk-ant-golden-frame-fixture");
    g.check_text("gate-review", &keyed.snap("gate", "down down enter down enter / text:keys.anthropic enter end enter a down enter y w", "100x30", "16"));
    // The picker over a fixed choice, the mode, and over the engine, whose
    // choices are the shipped profiles and one of the user's own.
    g.check_text("gate-pick-mode", &fx.snap("gate", "down down enter down", "100x30", "16"));
    let mine = Fx::new();
    mine.file(".config/agent-ways/agent.yaml", "engine: mine\nprofiles:\n  mine:\n    provider: anthropic\n    model: claude-sonnet-5-5\n");
    g.check_text("gate-pick-engine", &mine.snap("gate", "down enter", "100x30", "16"));
    // A chosen theme at truecolor: its roles, and the editor's swatches.
    g.check_text("theme-nord-truecolor", &fx.snap("theme", "down down enter 1", "100x30", "truecolor"));
    // A project way with a macro: its switch above, what it is below.
    let way = Fx::new();
    way.file("proj/.claude/ways/api/dual/dual.md", "---\ndescription: Two modes for every endpoint, read and write.\nvocabulary: endpoint read write mode\npattern: \\bapi\\b\nmacro: prepend\nrefire: normal\n---\nbody\n");
    way.file("proj/.claude/ways/api/dual/macro.sh", "#!/bin/sh\n# lists the endpoints\nls src/routes\n");
    g.check_text("ways-project-way", &way.snap("ways", "end right down down right down", "100x30", "16"));
    // Other projects Claude Code knows: hidden behind a row by default, a
    // group each in the all projects view (picked from the menu).
    let projects = Fx::new();
    projects.known("proj", &["api/dual"]);
    projects.known("Projects/gateway", &["api/auth", "api/rate-limit", "deploy/canary"]);
    projects.known("Projects/notes", &["writing/tone"]);
    projects.file("Projects/gateway/.claude/ways.yaml", "ways:\n  api/rate-limit: false\n");
    g.check_text("ways-other-projects-hidden", &projects.snap("ways", "end right end", "100x30", "16"));
    let all = "end right a down enter down down down down down down down right down right down down";
    g.check_text("ways-all-projects", &projects.snap("ways", all, "100x30", "16"));
    let broken = Fx::new();
    broken.file(".config/agent-ways/config.yaml", BROKEN);
    g.check_text("matching-broken", &broken.snap("matching", "", "100x30", "16"));
    // The response modal (#778): a reading action's outcome with what its
    // command printed. The key check, through a stand-in for the binary
    // that answers as a check does, passing and then failing.
    let check = "/ text:keys.anthropic enter end enter a down down enter";
    keyed.file("runner.sh", "#!/bin/sh\necho 'provider: anthropic'\necho 'key:      sk-ant-…ture'\necho 'model:    claude-haiku-4-5'\necho 'result:   accepted'\n");
    executable(&keyed.path("runner.sh"));
    g.check_text("gate-check-pass", &keyed.snap_with("gate", check, "100x30", "16", &[("WAYS_SETTINGS_RUNNER", &keyed.path("runner.sh"))]));
    keyed.file("runner.sh", "#!/bin/sh\necho 'provider: anthropic'\necho 'check: the key was refused (401)' >&2\nexit 1\n");
    g.check_text("gate-check-fail", &keyed.snap_with("gate", check, "100x30", "16", &[("WAYS_SETTINGS_RUNNER", &keyed.path("runner.sh"))]));
    // Lint on the findings of a broken file, the real command: a fail with
    // its findings. On the install tab over sound files: a pass.
    g.check_text("matching-lint-fail", &broken.snap("matching", "a enter", "100x30", "16"));
    g.check_text("install-lint-pass", &fx.snap("install", "down down down a down down enter", "100x30", "16"));
    // A report: the targets row's plan, the real command, for a Claude
    // config directory under the fixture home. One that is not there is an
    // error: the exit code and what the command printed on stderr.
    let planned = Fx::new();
    planned.file("work-claude/settings.json", "{}\n");
    g.check_text("install-plan-report", &planned.snap("install", "p text:~/work-claude enter", "100x30", "16"));
    g.check_text("install-plan-error", &fx.snap("install", "p text:~/elsewhere enter", "100x30", "16"));
    g.finish();
}
