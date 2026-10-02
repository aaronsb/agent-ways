//! `ways settings` (ADR-503) and the commands that became its aliases.
//!
//! Each test runs the built binary with HOME and every XDG directory in a
//! fixture of its own. The alias goldens under `fixtures/settings-aliases`
//! were captured from the commit before `ways settings` (cda1042b) by running
//! the same sequence; the fixture root here has the same length as the one
//! that capture used, so `config show`'s padded table compares byte for byte.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static SEQ: AtomicUsize = AtomicUsize::new(0);

struct Fx {
    root: PathBuf,
}

impl Fx {
    /// A fixture root of exactly 19 characters, like `/tmp/tmp.XXXXXXXXXX`.
    fn new() -> Fx {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let root = PathBuf::from(format!("/tmp/w{n:02}-{:010}", std::process::id()));
        assert_eq!(root.as_os_str().len(), 19);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("home/.claude")).unwrap();
        std::fs::create_dir_all(root.join("xdg/config")).unwrap();
        std::fs::create_dir_all(root.join("proj")).unwrap();
        Fx { root }
    }

    fn user(&self) -> PathBuf {
        self.root.join("xdg/config/agent-ways/config.yaml")
    }

    fn overlay(&self) -> PathBuf {
        self.root.join("proj/.claude/ways.yaml")
    }

    fn cmd(&self, args: &[&str]) -> Command {
        // SETTINGS_CLI_BIN runs the suite against another build, such as the
        // parent commit's, to see which tests fail before the change.
        let bin = std::env::var_os("SETTINGS_CLI_BIN").map(PathBuf::from).unwrap_or_else(|| env!("CARGO_BIN_EXE_ways").into());
        let mut c = Command::new(bin);
        c.args(args)
            .current_dir(self.root.join("proj"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.root.join("home"))
            .env("XDG_CONFIG_HOME", self.root.join("xdg/config"))
            .env("XDG_STATE_HOME", self.root.join("xdg/state"))
            .env("XDG_CACHE_HOME", self.root.join("xdg/cache"))
            .env("XDG_DATA_HOME", self.root.join("xdg/data"))
            .env("CLAUDE_PROJECT_DIR", self.root.join("proj"))
            .env("NO_COLOR", "1");
        c
    }

    /// stdout, stderr and exit code, with the fixture root as `<ROOT>`.
    fn run(&self, args: &[&str]) -> (String, String, i32) {
        self.run_stdin(args, None)
    }

    fn run_stdin(&self, args: &[&str], input: Option<&str>) -> (String, String, i32) {
        let mut c = self.cmd(args);
        c.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = c.spawn().unwrap();
        if let Some(i) = input {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(i.as_bytes()).unwrap();
        }
        let out = child.wait_with_output().unwrap();
        let root = self.root.to_string_lossy().to_string();
        let norm = |b: &[u8]| String::from_utf8_lossy(b).replace(&root, "<ROOT>");
        (norm(&out.stdout), norm(&out.stderr), out.status.code().unwrap_or(-1))
    }

    fn write(&self, path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn golden(name: &str) -> (String, String, i32) {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/settings-aliases");
    let read = |ext: &str| std::fs::read_to_string(dir.join(format!("{name}.{ext}"))).unwrap();
    (read("out"), read("err"), read("code").trim().parse().unwrap())
}

fn golden_file(name: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/settings-aliases");
    std::fs::read_to_string(dir.join(format!("{name}.file"))).unwrap()
}

fn parsed(text: &str) -> serde_yaml::Value {
    match serde_yaml::from_str(text).unwrap() {
        serde_yaml::Value::Null => serde_yaml::Value::Mapping(Default::default()),
        v => v,
    }
}

/// The file an alias leaves holds the same settings as before, and every
/// comment the earlier writer kept.
fn same_file(path: &Path, name: &str) {
    let want = golden_file(name);
    let got = std::fs::read_to_string(path).unwrap_or_else(|_| "<absent>\n".into());
    if want == "<absent>\n" {
        assert_eq!(got, want, "{name}");
        return;
    }
    assert_eq!(parsed(&got), parsed(&want), "{name}: {got}");
    for line in want.lines().filter(|l| l.trim_start().starts_with('#')) {
        assert!(got.contains(line), "{name}: lost comment {line:?} in {got}");
    }
}

// ── aliases ────────────────────────────────────────────────────

#[test]
fn config_show_prints_and_exits_as_before() {
    let f = Fx::new();
    assert_eq!(f.run(&["config", "show"]), golden("show-empty"));
    assert_eq!(f.run(&["config", "show", "--json"]), golden("show-empty-json"));
    f.write(&f.user(), "# my config\nlanguage: es\nsemantic_fire_probability: 0.4  # tuned\nrefire_presets:\n  normal: 0.2\n");
    assert_eq!(f.run(&["config", "show"]), golden("show-user"));
    assert_eq!(f.run(&["config", "show", "--json"]), golden("show-user-json"));
    // refire_presets is a HashMap, so its key order varies run to run, before
    // and after; the document is compared parsed.
    let (out, err, code) = f.run(&["config", "show", "--json", "--effective"]);
    let (g_out, g_err, g_code) = golden("show-user-eff");
    let j = |s: &str| serde_json::from_str::<serde_json::Value>(s).unwrap();
    assert_eq!((j(&out), err, code), (j(&g_out), g_err, g_code));
}

#[test]
fn disable_and_enable_print_exit_and_write_as_before() {
    let f = Fx::new();
    let ov = f.overlay();
    assert_eq!(f.run(&["disable", "itops/incident"]), golden("disable-new"));
    same_file(&ov, "disable-new");
    f.write(&ov, "# overlay\nlanguage: en\n\nways:\n  # keep me\n  meta/introspection: false\n\nparent_boost_floor: 0.40\n");
    assert_eq!(f.run(&["disable", "itops/incident"]), golden("disable-existing"));
    same_file(&ov, "disable-existing");
    assert_eq!(f.run(&["enable", "itops/incident"]), golden("enable-one"));
    same_file(&ov, "enable-one");
    assert_eq!(f.run(&["enable", "itops/incident"]), golden("enable-again"));
    assert_eq!(f.run(&["enable", "meta/introspection"]), golden("enable-last"));
    same_file(&ov, "enable-last");
    std::fs::remove_file(&ov).unwrap();
    assert_eq!(f.run(&["enable", "itops/incident"]), golden("enable-nofile"));
    assert_eq!(f.run(&["disable", "Bad Name"]), golden("disable-bad"));
    assert_eq!(f.run(&["disable", "--list"]), golden("disable-list"));
}

#[test]
fn disable_and_settings_set_write_the_same_key() {
    let f = Fx::new();
    assert_eq!(f.run(&["settings", "set", "ways.project.itops/incident", "false"]).2, 0);
    let by_settings = std::fs::read_to_string(f.overlay()).unwrap();
    std::fs::remove_file(f.overlay()).unwrap();
    assert_eq!(f.run(&["disable", "itops/incident"]).2, 0);
    assert_eq!(std::fs::read_to_string(f.overlay()).unwrap(), by_settings);
    assert_eq!(f.run(&["settings", "get", "ways.project.itops/incident"]).0, "false\n");
    assert_eq!(f.run(&["disable", "--list", "--names-only"]).0, "itops/incident\n");
}

// ── property mode: exit codes ──────────────────────────────────

#[test]
fn set_exit_codes() {
    let f = Fx::new();
    // 0: done, and nothing printed.
    assert_eq!(f.run(&["settings", "set", "matching.semantic_fire_probability", "0.4"]), (String::new(), String::new(), 0));
    // 2: unknown key, usage error.
    let (_, err, code) = f.run(&["settings", "set", "matching.nope", "1"]);
    assert_eq!(code, 2);
    assert_eq!(err.lines().count(), 1, "{err}");
    assert!(err.contains("ways settings list"), "{err}");
    assert_eq!(f.run(&["settings", "set", "matching.semantic_fire_probability"]).2, 2);
    // 3: rejected by the schema, nothing written.
    let before = std::fs::read_to_string(f.user()).unwrap();
    let (_, err, code) = f.run(&["settings", "set", "matching.semantic_fire_probability", "1.4"]);
    assert_eq!(code, 3);
    assert!(err.contains("ways settings help matching.semantic_fire_probability"), "{err}");
    assert_eq!(f.run(&["settings", "set", "gate.mode", "loud"]).2, 3);
    assert_eq!(f.run(&["settings", "set", "gate.keys.anthropic", "sk-ant-xyz"]).2, 3);
    assert_eq!(f.run(&["settings", "set", "install.targets", "[]"]).2, 3);
    assert_eq!(std::fs::read_to_string(f.user()).unwrap(), before);
    // 4: written, but the project layer overrides it; stderr names the file.
    assert_eq!(f.run(&["settings", "set", "ways.language", "ja", "--project", f.root.join("proj").to_str().unwrap()]).2, 0);
    let (_, err, code) = f.run(&["settings", "set", "ways.language", "es"]);
    assert_eq!(code, 4);
    assert!(err.contains("<ROOT>/proj/.claude/ways.yaml"), "{err}");
    assert!(std::fs::read_to_string(f.user()).unwrap().contains("language: es"));
    // 5: the write failed (the lock cannot be taken); the file is untouched.
    let before = std::fs::read_to_string(f.user()).unwrap();
    std::fs::create_dir_all(f.root.join("xdg/config/agent-ways/config.yaml.lock/x")).unwrap();
    let (_, err, code) = f.run(&["settings", "set", "matching.near_miss_margin", "0.1"]);
    assert_eq!(code, 5, "{err}");
    assert!(err.contains("nothing written"), "{err}");
    assert_eq!(std::fs::read_to_string(f.user()).unwrap(), before);
}

#[test]
fn set_keeps_every_unrelated_line_of_the_file() {
    let f = Fx::new();
    let src = "# ways configuration\n# language: en\n\nlanguage: es   # spanish\ndisabled_domains: [ea]\n\n# matching\nsemantic_fire_probability: 0.5  # tuned\nrefire_presets:\n    normal: 0.2  # slower\n# end of file\n";
    f.write(&f.user(), src);
    assert_eq!(f.run(&["settings", "set", "matching.semantic_fire_probability", "0.4"]).2, 0);
    assert_eq!(std::fs::read_to_string(f.user()).unwrap(), src.replace("probability: 0.5", "probability: 0.4"));
    assert_eq!(f.run(&["settings", "set", "matching.refire_presets.rare", "0.3"]).2, 0);
    assert_eq!(
        std::fs::read_to_string(f.user()).unwrap(),
        src.replace("probability: 0.5", "probability: 0.4").replace("  # slower\n", "  # slower\n    rare: 0.3\n")
    );
    assert_eq!(f.run(&["settings", "unset", "matching.refire_presets.rare"]).2, 0);
    assert_eq!(std::fs::read_to_string(f.user()).unwrap(), src.replace("probability: 0.5", "probability: 0.4"));
    let names: Vec<_> = std::fs::read_dir(f.user().parent().unwrap()).unwrap().flatten().map(|e| e.file_name()).collect();
    assert_eq!(names, vec![std::ffi::OsString::from("config.yaml")], "no lock or temp file left");
}

// ── provenance and fallback ────────────────────────────────────

#[test]
fn get_json_names_the_layer_default_and_file() {
    let f = Fx::new();
    f.write(&f.user(), "semantic_fire_probability: 0.4\nlanguage: es\n");
    f.write(&f.overlay(), "language: ja\n");
    let j = |args: &[&str]| -> serde_json::Value { serde_json::from_str(&f.run(args).0).unwrap() };
    let v = j(&["settings", "get", "matching.semantic_fire_probability", "--json"]);
    assert_eq!((v["value"].as_f64(), v["default"].as_f64(), v["layer"].as_str()), (Some(0.4), Some(0.5), Some("user")));
    assert_eq!(v["file"], "<ROOT>/xdg/config/agent-ways/config.yaml");
    let v = j(&["settings", "get", "ways.language", "--json"]);
    assert_eq!((v["value"].as_str(), v["layer"].as_str()), (Some("ja"), Some("project")));
    assert_eq!(v["file"], "<ROOT>/proj/.claude/ways.yaml");
    let v = j(&["settings", "get", "matching.near_miss_margin", "--json"]);
    assert_eq!((v["layer"].as_str(), v["file"].is_null()), (Some("default"), true));
    let v = j(&["settings", "list", "--json"]);
    assert_eq!(v["view"], "stored");
    assert_eq!(v["fragment"]["<ROOT>/proj/.claude/ways.yaml"]["language"], "ja");
    assert_eq!(v["fragment"]["<ROOT>/xdg/config/agent-ways/config.yaml"]["semantic_fire_probability"], 0.4);
    assert!(v["keys"].get("matching.near_miss_margin").is_none(), "stored view holds only what files set");
}

#[test]
fn a_malformed_section_falls_back_to_canonical_and_the_rest_load() {
    let f = Fx::new();
    f.write(&f.user(), "language: es\n# tuned\nsemantic_fire_probability: 0.4\nparent_boost_floor: 9\n");
    assert_eq!(f.run(&["settings", "get", "ways.language"]).0, "es\n");
    assert_eq!(f.run(&["settings", "get", "matching.semantic_fire_probability"]).0, "0.5\n", "the whole section is canonical");
    let (out, _, code) = f.run(&["settings", "lint"]);
    assert_eq!(code, 3);
    assert_eq!(out, "<ROOT>/xdg/config/agent-ways/config.yaml:4: [matching] parent_boost_floor: 9 is outside 0..1\n");
    // The command that loads it says so on stderr, and names the repair.
    let (_, err, _) = f.run(&["config", "show"]);
    assert!(err.contains("config.yaml:4: [matching]") && err.contains("ways settings fix matching"), "{err}");
    // Loading never rewrote the file; fix does, on request, for that section only.
    assert!(std::fs::read_to_string(f.user()).unwrap().contains("parent_boost_floor: 9"));
    assert_eq!(f.run(&["settings", "fix", "matching"]).2, 0);
    let text = std::fs::read_to_string(f.user()).unwrap();
    assert!(text.starts_with("language: es\n# tuned\n"), "{text}");
    assert_eq!(f.run(&["settings", "lint"]).2, 0);
    // A typo in agent.yaml falls only its section back; the mode still loads.
    f.write(&f.root.join("xdg/config/agent-ways/agent.yaml"), "mode: shadow\nprofiles:\n  anthropic:\n    treshold: 0.4\n");
    assert_eq!(f.run(&["settings", "get", "gate.mode"]).0, "shadow\n");
    let (out, _, code) = f.run(&["settings", "lint"]);
    assert_eq!(code, 3);
    assert!(out.contains("agent.yaml:4: [gate.profiles] profiles.anthropic.treshold: unknown key"), "{out}");
}

#[test]
fn file_reads_a_copy_on_its_own() {
    let f = Fx::new();
    let copy = f.root.join("copy.yaml");
    f.write(&copy, "language: de\nnear_miss_margin: 0.2\n");
    f.write(&f.user(), "language: es\n");
    let c = copy.to_str().unwrap();
    assert_eq!(f.run(&["settings", "get", "ways.language", "--file", c]).0, "de\n");
    assert!(f.run(&["settings", "list", "matching", "--file", c]).0.contains("matching.near_miss_margin=0.2\n"));
    assert_eq!(f.run(&["settings", "lint", "--file", c]).2, 0);
    assert_eq!(f.run(&["settings", "get", "ways.language", "--file", "/nonexistent"]).2, 2);
}

// ── object mode ────────────────────────────────────────────────

#[test]
fn an_emitted_section_applies_unchanged() {
    let f = Fx::new();
    let (frag, _, code) = f.run(&["settings", "emit", "matching"]);
    assert_eq!(code, 0);
    let path = f.root.join("frag.yaml");
    std::fs::write(&path, &frag).unwrap();
    let (out, err, code) = f.run(&["settings", "apply", "--file", path.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}{err}");
    let report: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(report["rejected"].as_array().unwrap().is_empty());
    assert_eq!(report["accepted"].as_array().unwrap().len(), 9);
    let written = &report["written"]["<ROOT>/xdg/config/agent-ways/config.yaml"];
    assert_eq!(parsed(&serde_yaml::to_string(written).unwrap()), parsed(&frag), "the fragment written fits back into the file");
    // A multi-file emit applies too.
    let (all, _, _) = f.run(&["settings", "emit"]);
    assert!(all.contains("---\n# agent.yaml\n"), "{all}");
    let (_, _, code) = f.run_stdin(&["settings", "apply", "--dry-run"], Some(&all));
    assert_eq!(code, 0);
}

#[test]
fn apply_reports_each_key_and_exits_by_the_worst() {
    let f = Fx::new();
    let (out, _, code) = f.run_stdin(&["settings", "apply"], Some("semantic_fire_probability: 2\nmode: shadow\nbogus: 1\n"));
    assert_eq!(code, 3);
    let r: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(r["accepted"], serde_json::json!(["gate.mode"]));
    let rejected: Vec<&str> = r["rejected"].as_array().unwrap().iter().map(|x| x["key"].as_str().unwrap()).collect();
    assert_eq!(rejected, vec!["matching.semantic_fire_probability", "bogus"]);
    assert_eq!(r["written"]["<ROOT>/xdg/config/agent-ways/agent.yaml"], serde_json::json!({ "mode": "shadow" }));
    // Dry run writes nothing.
    let (_, _, code) = f.run_stdin(&["settings", "apply", "--dry-run"], Some("{\"language\": \"es\"}"));
    assert_eq!(code, 0);
    assert!(!f.user().exists());
    // 4 when an accepted key is overridden by a higher layer.
    f.write(&f.overlay(), "language: ja\n");
    let (out, _, code) = f.run_stdin(&["settings", "apply"], Some("language: es\n"));
    assert_eq!(code, 4, "{out}");
    // 2 when the object is not one.
    assert_eq!(f.run_stdin(&["settings", "apply"], Some("- a\n")).2, 2);
}

// ── help ───────────────────────────────────────────────────────

#[test]
fn help_prints_the_schema_text() {
    let f = Fx::new();
    let (out, _, code) = f.run(&["settings", "help", "matching.semantic_fire_probability"]);
    assert_eq!(code, 0);
    assert!(out.contains("type:    float, 0..1") && out.contains("default: 0.5") && out.contains("ADR-156"), "{out}");
    assert!(f.run(&["settings", "help", "gate"]).0.contains("gate.mode"));
    assert_eq!(f.run(&["settings", "help", "nope"]).2, 2);
}

// ── hook paths load only their sections (ADR-503 §5) ───────────

#[test]
fn hook_commands_load_only_their_sections() {
    let f = Fx::new();
    // Files for every section the hooks read, and a broken attend config that
    // would show on stderr if the hook path touched it.
    f.write(&f.user(), "language: en\n");
    f.write(&f.overlay(), "ways: {}\n");
    f.write(&f.root.join("xdg/config/agent-ways/agent.yaml"), "mode: shadow\n");
    f.write(&f.root.join("xdg/config/attend/config.yaml"), "engagement: [\n");
    for args in [
        vec!["scan", "prompt", "--query=write a unit test", "--session=s1"],
        vec!["scan", "command", "--command=git status", "--session=s1"],
        vec!["show", "way", "softwaredev/code/testing", "--session=s1"],
        vec!["scan", "state", "--session=s1", "--hook-event=SessionStart"],
    ] {
        let mut c = f.cmd(&args);
        c.env("WAYS_SETTINGS_TRACE", "1");
        let out = c.output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        let trace: Vec<&str> = err.lines().filter(|l| l.starts_with("settings-trace:")).collect();
        assert!(trace.iter().any(|l| l.contains("load ways:config")), "{args:?} loaded no settings: {err}");
        if args[1] == "prompt" {
            assert!(trace.iter().any(|l| l.contains("load ways-agent:agent")), "the gate reads agent.yaml: {err}");
        }
        for line in &trace {
            assert!(
                *line == "settings-trace: load ways:config [ways,matching,install,ways.project]"
                    || *line == "settings-trace: load ways-agent:agent [gate,gate.profiles]",
                "{args:?} loaded more than its sections: {line}"
            );
        }
        assert!(!err.contains("attend"), "{args:?} read attend's settings: {err}");
    }
    // The full tree is built only by the settings verbs.
    let mut c = f.cmd(&["settings", "list"]);
    c.env("WAYS_SETTINGS_TRACE", "1");
    assert!(String::from_utf8_lossy(&c.output().unwrap().stderr).contains("settings-trace: tree"));
}
