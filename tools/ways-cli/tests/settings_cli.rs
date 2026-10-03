//! `ways settings` (ADR-503).
//!
//! Each test runs the built binary with HOME and every XDG directory in a
//! fixture of its own.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static SEQ: AtomicUsize = AtomicUsize::new(0);

struct Fx {
    root: PathBuf,
}

impl Fx {
    /// A fixture root of exactly 19 characters, like `/tmp/tmp.XXXXXXXXXX`.
    /// It must be absolute on the platform: the binary ignores a relative
    /// `XDG_*` value by design (`paths::xdg_base`), and `/tmp/...` is not
    /// absolute on Windows. There it sits on the temp directory's drive,
    /// `C:\w00-000000001234`.
    fn new() -> Fx {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        #[cfg(not(windows))]
        let root = PathBuf::from(format!("/tmp/w{n:02}-{:010}", std::process::id()));
        #[cfg(windows)]
        let root = {
            let tmp = std::env::temp_dir();
            let drive = tmp.components().next().expect("temp dir has a drive").as_os_str().to_string_lossy().to_string();
            PathBuf::from(format!("{drive}\\w{n:02}-{:012}", std::process::id()))
        };
        assert!(root.is_absolute(), "{}", root.display());
        assert_eq!(root.as_os_str().len(), 19);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("home/.claude")).unwrap();
        std::fs::create_dir_all(root.join("xdg/config")).unwrap();
        std::fs::create_dir_all(root.join("proj")).unwrap();
        // The locales `ways.language` may name (de, es, ja), as override files.
        let loc = root.join("xdg/data/agent-ways/hooks/ways/loc");
        std::fs::create_dir_all(&loc).unwrap();
        for lang in ["de", "es", "ja"] {
            std::fs::write(loc.join(format!("loc.{lang}.md")), "x\n").unwrap();
        }
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
        c.args(args).current_dir(self.root.join("proj")).env_clear();
        #[cfg(not(windows))]
        c.env("PATH", "/usr/bin:/bin");
        // Windows: the process needs its system variables, and home_dir()
        // reads USERPROFILE before HOME, so both name the fixture home.
        #[cfg(windows)]
        {
            for var in ["PATH", "SystemRoot", "SystemDrive", "TEMP", "TMP", "windir"] {
                if let Some(v) = std::env::var_os(var) {
                    c.env(var, v);
                }
            }
            c.env("USERPROFILE", self.root.join("home"));
        }
        c.env("HOME", self.root.join("home"))
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
        (self.norm(&out.stdout), self.norm(&out.stderr), out.status.code().unwrap_or(-1))
    }

    /// Output with the fixture root as `<ROOT>`. On Windows the path that
    /// follows the root is written with `/`, so goldens and assertions read
    /// the same on every platform; nothing but those paths is changed.
    fn norm(&self, b: &[u8]) -> String {
        let text = String::from_utf8_lossy(b).to_string();
        let root = self.root.to_string_lossy().to_string();
        #[cfg(not(windows))]
        return text.replace(&root, "<ROOT>");
        #[cfg(windows)]
        {
            // JSON escapes each backslash, so the root appears in three forms.
            let mut t = text.replace(&root.replace('\\', "\\\\"), "<ROOT>");
            t = t.replace(&root, "<ROOT>").replace(&root.replace('\\', "/"), "<ROOT>");
            let mut out = String::with_capacity(t.len());
            let mut rest = t.as_str();
            while let Some(i) = rest.find("<ROOT>") {
                out.push_str(&rest[..i + "<ROOT>".len()]);
                rest = &rest[i + "<ROOT>".len()..];
                let end = rest.find(|c: char| c.is_whitespace() || matches!(c, '"' | ')' | ',' | '\'')).unwrap_or(rest.len());
                out.push_str(&rest[..end].replace("\\\\", "/").replace('\\', "/"));
                rest = &rest[end..];
            }
            out.push_str(rest);
            out
        }
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

fn parsed(text: &str) -> serde_yaml::Value {
    match serde_yaml::from_str(text).unwrap() {
        serde_yaml::Value::Null => serde_yaml::Value::Mapping(Default::default()),
        v => v,
    }
}

// ── per-project way switches ───────────────────────────────

#[test]
fn a_project_switch_round_trips_through_settings_and_keeps_comments() {
    let f = Fx::new();
    let ov = f.overlay();
    f.write(&ov, "# overlay\nlanguage: en\n\nways:\n  # keep me\n  meta/introspection: false\n");
    assert_eq!(f.run(&["settings", "set", "ways.project.itops/incident", "false"]), (String::new(), String::new(), 0));
    let text = std::fs::read_to_string(&ov).unwrap();
    assert!(text.starts_with("# overlay\n") && text.contains("  # keep me\n"), "{text}");
    assert_eq!(f.run(&["settings", "get", "ways.project.itops/incident"]).0, "false\n");
    assert_eq!(
        f.run(&["settings", "list", "ways.project"]).0,
        "ways.project.meta/introspection=false\nways.project.itops/incident=false\n"
    );
    assert_eq!(f.run(&["settings", "unset", "ways.project.itops/incident"]).2, 0);
    assert_eq!(f.run(&["settings", "unset", "ways.project.meta/introspection"]).2, 0);
    assert_eq!(f.run(&["settings", "list", "ways.project"]).0, "");
    assert!(std::fs::read_to_string(&ov).unwrap().starts_with("# overlay\nlanguage: en\n"));
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
    assert_eq!(err.matches("nothing written").count(), 1, "{err}");
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
    let (_, err, _) = f.run(&["settings", "list"]);
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

// ── a choice computed from the files (#777) ────────────────────

#[test]
fn the_engine_is_one_of_the_profiles_in_effect() {
    let f = Fx::new();
    let agent = f.root.join("xdg/config/agent-ways/agent.yaml");
    // The shipped profiles only: anything else is refused and nothing written.
    let (_, err, code) = f.run(&["settings", "set", "gate.engine", "mine"]);
    assert_eq!(code, 3, "{err}");
    assert!(err.contains("expected one of anthropic, openrouter, found 'mine'"), "{err}");
    assert!(!agent.exists());
    assert_eq!(f.run(&["settings", "set", "gate.engine", "openrouter"]).2, 0);
    // A profile of the user's own becomes a choice.
    f.write(&agent, "profiles:\n  mine:\n    provider: anthropic\n    model: claude-sonnet-5-5\n");
    assert_eq!(f.run(&["settings", "set", "gate.engine", "mine"]), (String::new(), String::new(), 0));
    let (out, _, _) = f.run(&["settings", "help", "gate.engine"]);
    assert!(out.contains("type:    one of anthropic, openrouter, mine"), "{out}");
    let v: serde_json::Value = serde_json::from_str(&f.run(&["settings", "get", "gate.engine", "--json"]).0).unwrap();
    assert_eq!(v["options"], serde_json::json!(["anthropic", "openrouter", "mine"]));
    assert_eq!(v["value"], "mine");
    // Every other key's object is as it was: no options.
    let v: serde_json::Value = serde_json::from_str(&f.run(&["settings", "get", "gate.mode", "--json"]).0).unwrap();
    assert!(v.get("options").is_none(), "{v}");
    let (out, _, _) = f.run(&["settings", "emit", "gate"]);
    assert!(out.starts_with("# gate.engine: one of anthropic, openrouter, mine\n"), "{out}");
    // apply checks against the files and the object together.
    let (out, _, code) = f.run_stdin(&["settings", "apply", "--dry-run"], Some("engine: theirs\nprofiles:\n  theirs:\n    provider: openrouter\n    model: x/y\n"));
    assert_eq!(code, 0, "{out}");
    let (out, _, code) = f.run_stdin(&["settings", "apply", "--dry-run"], Some("engine: nobody\n"));
    assert_eq!(code, 3, "{out}");
    // A value the list lacks, written by hand, loads and is a lint finding.
    f.write(&agent, "engine: gone\n");
    let (out, err, code) = f.run(&["settings", "lint"]);
    assert_eq!(code, 3);
    assert!(
        out.contains("agent.yaml:1: [gate] engine: expected one of anthropic, openrouter, found 'gone'; it loads as written, and `ways settings set gate.engine <choice>` repairs it"),
        "{out}"
    );
    assert!(err.contains("1 finding; each line names its repair") && !err.contains("fix"), "{err}");
    assert_eq!(f.run(&["settings", "get", "gate.engine"]).0, "gone\n");
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
    // The hooks share `config::global()` with reconcile, so they read every
    // ways section; what they must not do is take a file whole
    // (`load-all`), build the settings tree, lint, or probe the key store.
    // Each load names its sections (`load-sections`), and the names are
    // exactly the lists the hook code passes.
    let f = Fx::new();
    f.write(&f.user(), "language: en\n");
    f.write(&f.overlay(), "ways: {}\n");
    f.write(&f.root.join("xdg/config/agent-ways/agent.yaml"), "mode: shadow\n");
    // attend's schema is in ways' registry (#698), and hooks must still not
    // read attend's files: a broken one would print its finding here.
    f.write(&f.root.join("xdg/config/attend/config.yaml"), "engagement: [\n");
    let ways_line =
        "settings-trace: load-sections ways:config [ways,ways.switch,ways.subagents,ways.domains,matching,install.targets,install.secret_path_deny,ways.project]";
    let agent_line = "settings-trace: load-sections ways-agent:agent [gate,gate.mode,gate.profiles]";
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
        assert!(trace.contains(&ways_line), "{args:?} loaded no ways settings by section: {err}");
        if args[1] == "prompt" {
            assert!(trace.contains(&agent_line), "the gate reads agent.yaml by section: {err}");
        }
        for line in &trace {
            assert!(*line == ways_line || *line == agent_line, "{args:?} loaded more than its sections: {line}");
        }
        assert!(!err.contains("attend"), "{args:?} read attend's settings: {err}");
    }
    // The settings verbs take files whole and build the tree; the trace shows it.
    let mut c = f.cmd(&["settings", "list"]);
    c.env("WAYS_SETTINGS_TRACE", "1");
    let err = String::from_utf8_lossy(&c.output().unwrap().stderr).to_string();
    assert!(err.contains("settings-trace: tree") && err.contains("settings-trace: load-all ways:config"), "{err}");
}

// ── review findings (#713) ─────────────────────────────────────

#[test]
fn fix_never_drops_the_targets_list() {
    let f = Fx::new();
    f.write(&f.user(), "secret_path_deny: \"false\"\ntargets:\n  - path: /a\n    enabled: false\n  - path: /b\n");
    let (_, err, code) = f.run(&["settings", "fix", "install"]);
    assert_eq!(code, 0, "{err}");
    let text = std::fs::read_to_string(f.user()).unwrap();
    assert!(text.contains("targets:\n  - path: /a\n    enabled: false\n  - path: /b\n"), "{text}");
    assert!(text.contains("secret_path_deny: true"), "{text}");
    // A bad target entry is refused by fix, which names the command that
    // owns the list; the list is untouched.
    let src = "targets:\n  - path: /a\n    enabled: false\n  - path: /b\n    enabled: maybe\n";
    f.write(&f.user(), src);
    let (_, err, code) = f.run(&["settings", "fix", "install"]);
    assert_eq!(code, 3, "{err}");
    assert!(err.contains("`ways target add|enable|disable|remove <dir>` repairs it"), "{err}");
    assert_eq!(std::fs::read_to_string(f.user()).unwrap(), src);
    // The load diagnostic names that command too, not fix.
    let (_, err, _) = f.run(&["settings", "get", "install.targets"]);
    assert!(err.contains("ways target") && !err.contains("settings fix install"), "{err}");
}

#[test]
fn one_bad_target_entry_keeps_the_others_and_stays_withdrawn() {
    // S1: a bad field in one entry no longer drops the list into the
    // implicit ~/.claude, which the operator disabled here.
    let f = Fx::new();
    f.write(&f.user(), "targets:\n  - path: ~/.claude\n    enabled: false\n  - path: ~/.claude-work\n    enabled: maybe\n");
    let (out, _, _) = f.run(&["settings", "get", "install.targets"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v, serde_json::json!([{"path": "~/.claude", "enabled": false}, {"path": "~/.claude-work", "enabled": false}]));
}

#[test]
fn fix_gate_keeps_the_gate_off() {
    // B1: `fix gate` touches the gate section alone and repairs engine.
    let f = Fx::new();
    let agent = f.root.join("xdg/config/agent-ways/agent.yaml");
    f.write(&agent, "engine: 5\nmode: off   # gate switched off on purpose\n");
    let (_, err, code) = f.run(&["settings", "fix", "gate"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(std::fs::read_to_string(&agent).unwrap(), "mode: off   # gate switched off on purpose\n");
    assert_eq!(f.run(&["settings", "lint"]).2, 0);
}

#[test]
fn fix_ways_in_a_project_keeps_it_switched_off() {
    // B1: in a project file fix removes the bad key, never writes canonical.
    let f = Fx::new();
    f.write(&f.overlay(), "language: 7\nenabled: false\ndisabled_domains: [ea, itops]\n");
    let (_, err, code) = f.run(&["settings", "fix", "ways", "--project", f.root.join("proj").to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(std::fs::read_to_string(f.overlay()).unwrap(), "enabled: false\ndisabled_domains: [ea, itops]\n");
    assert_eq!(f.run(&["settings", "get", "ways.enabled"]).0, "false\n");
}

#[test]
fn fix_repairs_a_per_entry_section_until_lint_is_clean() {
    // S3: a section that is not a mapping, and an integer entry key.
    let f = Fx::new();
    let proj = f.root.join("proj");
    let p = proj.to_str().unwrap();
    f.write(&f.overlay(), "ways: 5\n");
    assert_eq!(f.run(&["settings", "fix", "ways.project", "--project", p]).2, 0);
    assert_eq!(f.run(&["settings", "lint", "--project", p]).2, 0);
    f.write(&f.overlay(), "ways:\n  123: false\n  a/b: false\n");
    assert_eq!(f.run(&["settings", "fix", "ways.project", "--project", p]).2, 0);
    assert_eq!(std::fs::read_to_string(f.overlay()).unwrap(), "ways:\n  a/b: false\n");
    assert_eq!(f.run(&["settings", "lint", "--project", p]).2, 0);
}

#[test]
fn an_unparseable_project_file_keeps_ways_switched_off() {
    // Before this PR the whole file was ignored, `enabled: false` with it.
    // Now the whole file fails closed: ways are off in the project.
    let f = Fx::new();
    f.write(&f.overlay(), "enabled: false\nlanguage: [\nways:\n  itops/incident: false\n");
    assert_eq!(f.run(&["settings", "get", "ways.enabled"]).0, "false\n");
}

#[test]
fn a_finding_reads_with_a_colon_and_prints_once_per_hook() {
    let f = Fx::new();
    f.write(&f.user(), "mdoe: 1\n");
    let (_, err, _) = f.run(&["settings", "lint"]);
    assert!(err.is_empty() || !err.contains("mdoe: 1"), "{err}");
    let (out, _, _) = f.run(&["settings", "lint"]);
    assert_eq!(out, "<ROOT>/xdg/config/agent-ways/config.yaml:1: mdoe: unknown key; it is ignored\n");
    // L1: a hook command that loads the config twice prints the finding once.
    let mut c = f.cmd(&["scan", "prompt", "--query=write a unit test", "--session=s1"]);
    let err = String::from_utf8_lossy(&c.output().unwrap().stderr).to_string();
    assert_eq!(err.matches("mdoe").count(), 1, "{err}");
}

#[test]
fn a_bad_value_never_switches_back_on_what_was_turned_off() {
    let f = Fx::new();
    // A bad domain list keeps the project switched off.
    f.write(&f.overlay(), "disabled_domains: ea,itops\nenabled: false\n");
    assert_eq!(f.run(&["settings", "get", "ways.enabled"]).0, "false\n");
    // One bad toggle keeps the other disabled ways disabled.
    f.write(&f.overlay(), "ways:\n  itops/incident: false\n  meta/introspection: no\n  ea/x: false\n");
    // A bad toggle fails closed: it reads as disabled too.
    assert_eq!(
        f.run(&["settings", "list", "ways.project"]).0,
        "ways.project.itops/incident=false\nways.project.meta/introspection=false\nways.project.ea/x=false\n"
    );
    // fix writes the bad toggle's closed reading, so it stays off.
    assert_eq!(f.run(&["settings", "fix", "ways.project", "--project", f.root.join("proj").to_str().unwrap()]).2, 0);
    assert_eq!(
        std::fs::read_to_string(f.overlay()).unwrap(),
        "ways:\n  itops/incident: false\n  meta/introspection: false\n  ea/x: false\n"
    );
    // A bad secret_path_deny keeps the recorded targets.
    f.write(&f.user(), "secret_path_deny: \"false\"\ntargets:\n  - path: /srv/work/.claude\n");
    assert!(f.run(&["settings", "get", "install.targets"]).0.contains("/srv/work/.claude"));
    // A bad engine keeps the gate off.
    f.write(&f.root.join("xdg/config/agent-ways/agent.yaml"), "engine: 5\nmode: off\n");
    assert_eq!(f.run(&["settings", "get", "gate.mode"]).0, "off\n");
}

#[cfg(unix)]
#[test]
fn set_through_a_symlinked_0600_config_keeps_the_link_and_mode() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fx::new();
    let real = f.root.join("dotfiles/config.yaml");
    f.write(&real, "# mine\nlanguage: es\n");
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::create_dir_all(f.user().parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&real, f.user()).unwrap();
    assert_eq!(f.run(&["settings", "set", "matching.semantic_fire_probability", "0.4"]).2, 0);
    assert!(std::fs::symlink_metadata(f.user()).unwrap().file_type().is_symlink());
    assert_eq!(std::fs::read_to_string(&real).unwrap(), "# mine\nlanguage: es\nsemantic_fire_probability: 0.4\n");
    assert_eq!(std::fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o600);
}

#[test]
fn get_reports_a_fallback_on_stderr_and_keeps_stdout_the_value() {
    let f = Fx::new();
    f.write(&f.user(), "semantic_fire_probability: 0.35\nparent_boost_floor: 9\n");
    let (out, err, code) = f.run(&["settings", "get", "matching.semantic_fire_probability"]);
    assert_eq!((out.as_str(), code), ("0.5\n", 0));
    assert!(err.contains("config.yaml:2: [matching] parent_boost_floor") && err.contains("resolve from the layers beneath"), "{err}");
}

#[test]
fn an_unknown_top_level_key_is_reported_at_load() {
    let f = Fx::new();
    f.write(&f.user(), "langauge: es\n");
    let (_, err, _) = f.run(&["settings", "list"]);
    assert_eq!(err.lines().filter(|l| l.contains("langauge")).count(), 1, "{err}");
    assert!(err.contains("unknown key"), "{err}");
}

#[test]
fn a_refire_preset_above_one_keeps_the_matching_section() {
    let f = Fx::new();
    f.write(&f.user(), "semantic_fire_probability: 0.35\nrefire_presets:\n  never: 5\n");
    let (out, err, _) = f.run(&["settings", "get", "matching.semantic_fire_probability"]);
    assert_eq!(out, "0.35\n", "{err}");
    assert_eq!(f.run(&["settings", "lint"]).2, 0);
}

// ── a file that does not parse fails closed (#713) ─────────────
//
// The operator's decision, "whole file fails closed": a settings file that
// does not parse sets nothing, and every switch in its scope is off. Hooks
// never fail over it, and each says so on stderr in one line.

/// Broken shapes from the review rounds. Each fails to parse; the second
/// half (BOM, indented lines before the first entry, tab indent, quoted
/// brackets) are the N3 shapes that line-by-line salvage missed.
const BROKEN: &[&str] = &[
    "  language: en\n#ña\nenabled: false\n",
    "enabled: \"false\n",
    "enabled: false\nenabled: true\n",
    "{enabled: false, x: [}\n",
    "\u{feff}enabled: false\nx: [\n",
    "  enabled: false\nlanguage: [\n",
    "\tenabled: false\n",
    "ways:\n\ta/b: false\n  c/d: false\n",
    "{note: \"{\", enabled: false, x: [}\n",
    // A file that turns nothing off, broken: closed all the same.
    "enabled: true\nlanguage: [\n",
];

const HOOKS: &[&[&str]] = &[
    &["show", "core", "--session=s1"],
    &["scan", "prompt", "--query=write a unit test", "--session=s1"],
    &["scan", "command", "--command=git status", "--session=s1"],
    &["scan", "state", "--session=s1", "--hook-event=SessionStart"],
];

/// Every hook exits 0. Each hook that loads the settings names the file,
/// its line and its closed scope; `show core` reads no settings, so it has
/// nothing to report.
fn hooks_say_the_file_is_closed(f: &Fx, file: &str, text: &str) {
    for args in HOOKS {
        let (_, err, code) = f.run(args);
        assert_eq!(code, 0, "{text:?} {args:?}: {err}");
        if args[0] == "show" {
            continue;
        }
        let line = err.lines().find(|l| l.contains(file) && l.contains("does not parse"));
        let line = line.unwrap_or_else(|| panic!("{text:?} {args:?}: no parse line for {file}: {err}"));
        assert!(line.contains("whole file fails closed"), "{line}");
        assert!(line.contains(&format!("{file}:")), "the line names the file and line: {line}");
    }
}

#[test]
fn an_unparseable_project_file_switches_ways_off_for_the_project() {
    for text in BROKEN {
        let f = Fx::new();
        assert!(serde_yaml::from_str::<serde_yaml::Value>(text).is_err(), "{text:?} parses");
        f.write(&f.overlay(), text);
        assert_eq!(f.run(&["settings", "get", "ways.enabled"]).0, "false\n", "{text:?}");
        hooks_say_the_file_is_closed(&f, "<ROOT>/proj/.claude/ways.yaml", text);
        // lint and status show the same line.
        let (out, _, code) = f.run(&["settings", "lint"]);
        assert_eq!(code, 3);
        assert!(out.contains("<ROOT>/proj/.claude/ways.yaml") && out.contains("whole file fails closed"), "{out}");
    }
}

#[test]
fn an_unparseable_user_file_projects_nowhere() {
    // The N3 user rows: a broken user file that withdraws ~/.claude must not
    // fall to the implicit ~/.claude.
    for text in [
        "\u{feff}targets:\n  - path: ~/.claude\n    enabled: false\nx: [\n",
        "  targets:\n    - path: ~/.claude\n      enabled: false\nx: [\n",
        "{note: \"{\", targets: [{path: ~/.claude, enabled: false}], x: [}\n",
    ] {
        let f = Fx::new();
        f.write(&f.user(), text);
        assert_eq!(f.run(&["settings", "get", "ways.enabled"]).0, "false\n", "{text:?}");
        assert_eq!(f.run(&["settings", "get", "install.targets"]).0, "[]\n", "{text:?}");
        assert_eq!(f.run(&["settings", "get", "install.secret_path_deny"]).0, "true\n", "{text:?}");
        let (_, err, _) = f.run(&["target", "list"]);
        assert!(!err.contains("implicit:"), "{text:?}: {err}");
        hooks_say_the_file_is_closed(&f, "<ROOT>/xdg/config/agent-ways/config.yaml", text);
        let (out, _, _) = f.run(&["status"]);
        assert!(out.contains("whole file fails closed"), "status shows it: {out}");
    }
}

#[test]
fn an_unparseable_target_file_switches_ways_off_for_that_target() {
    let f = Fx::new();
    let target_cfg = f.root.join("target.yaml");
    let home = f.root.join("home").join(".claude");
    f.write(
        &f.user(),
        &format!("targets:\n  - path: '{}'\n    config: '{}'\n", home.display(), target_cfg.display()),
    );
    f.write(&target_cfg, "enabled: true\nsecret_path_deny: false\nx: [\n");
    assert_eq!(f.run(&["settings", "get", "ways.enabled"]).0, "false\n");
    assert_eq!(f.run(&["settings", "get", "install.secret_path_deny"]).0, "true\n");
    hooks_say_the_file_is_closed(&f, "<ROOT>/target.yaml", "target file");
}

#[test]
fn an_unparseable_agent_yaml_turns_the_gate_off() {
    // With a key file present, so the gate would run: the hook stays exit 0,
    // names the file, and the gate is off (no provider call: see
    // gate::a_broken_agent_yaml_or_bad_mode_never_calls_the_provider).
    let f = Fx::new();
    let keys = f.root.join("xdg/config/agent-ways/keys");
    std::fs::create_dir_all(&keys).unwrap();
    std::fs::write(keys.join("anthropic"), "sk-ant-test-not-a-real-key-0000").unwrap();
    f.write(&f.root.join("xdg/config/agent-ways/agent.yaml"), "mode: shadow\nprofiles:\n  anthropic: [\n");
    assert_eq!(f.run(&["settings", "get", "gate.mode"]).0, "off\n");
    let (_, err, code) = f.run(&["scan", "prompt", "--query=write a unit test", "--session=s1"]);
    assert_eq!(code, 0, "{err}");
    let line = err.lines().find(|l| l.contains("<ROOT>/xdg/config/agent-ways/agent.yaml") && l.contains("does not parse"));
    let line = line.unwrap_or_else(|| panic!("{err}"));
    assert!(line.contains("gate is off until it is fixed"), "{line}");
}

#[test]
fn an_agent_yaml_with_a_latin1_comment_reads_the_same_everywhere() {
    // U1: the gate read agent.yaml strictly as UTF-8, the other loaders
    // lossily, so a Latin-1 comment made the hook's gate fail while
    // `settings get` read the file.
    let f = Fx::new();
    let keys = f.root.join("xdg/config/agent-ways/keys");
    std::fs::create_dir_all(&keys).unwrap();
    std::fs::write(keys.join("anthropic"), "sk-ant-test-not-a-real-key-0000").unwrap();
    let agent = f.root.join("xdg/config/agent-ways/agent.yaml");
    std::fs::write(&agent, b"mode: shadow\n# caf\xe9\n").unwrap();
    assert_eq!(f.run(&["settings", "get", "gate.mode"]), ("shadow\n".to_string(), String::new(), 0));
    let (_, err, code) = f.run(&["scan", "prompt", "--query=write a unit test", "--session=s1"]);
    assert_eq!(code, 0, "{err}");
    assert!(!err.contains("agent.yaml"), "the hook's gate reads the file too: {err}");
}

#[test]
fn fix_refuses_an_unparseable_file_and_says_to_fix_it_by_hand() {
    let f = Fx::new();
    let text = "language: es\nx: [\n";
    f.write(&f.user(), text);
    let (_, err, code) = f.run(&["settings", "fix", "ways"]);
    assert_eq!(code, 5, "{err}");
    assert!(err.contains("Fix the file's syntax by hand"), "{err}");
    assert_eq!(std::fs::read_to_string(f.user()).unwrap(), text);
}

#[test]
fn fix_removes_a_key_the_file_may_not_hold() {
    // S1: project-only toggles in the user file; fix used to loop on them.
    let f = Fx::new();
    f.write(&f.user(), "language: es\nways:\n  a/b: false\n  c/d: maybe\n");
    let (_, err, code) = f.run(&["settings", "fix", "ways.project"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(std::fs::read_to_string(f.user()).unwrap(), "language: es\n");
    assert_eq!(f.run(&["settings", "lint"]).2, 0);
    // L2: targets in a project file is removed by fix, which the diagnostic names.
    f.write(&f.overlay(), "targets: 5\nenabled: false\n");
    let p = f.root.join("proj");
    let (_, err, _) = f.run(&["settings", "get", "ways.enabled"]);
    assert!(err.contains("ways settings fix install.targets"), "{err}");
    assert_eq!(f.run(&["settings", "fix", "install", "--project", p.to_str().unwrap()]).2, 0);
    assert_eq!(std::fs::read_to_string(f.overlay()).unwrap(), "enabled: false\n");
}

#[test]
fn fix_all_reports_a_key_no_section_owns() {
    // L1: `fix ""` exited 0 while lint still exited 3.
    let f = Fx::new();
    f.write(&f.user(), "mdoe: 1\nlanguage: es\n");
    let (_, err, code) = f.run(&["settings", "fix", ""]);
    assert_eq!(code, 3, "{err}");
    assert!(err.contains("mdoe"), "{err}");
    assert_eq!(f.run(&["settings", "lint"]).2, 3);
}

fn json(f: &Fx, args: &[&str]) -> serde_json::Value {
    serde_json::from_str(&f.run(args).0).unwrap()
}

#[test]
fn the_active_theme_is_one_of_the_installed_themes() {
    let f = Fx::new();
    let (_, err, code) = f.run(&["settings", "set", "theme.active", "mine"]);
    assert_eq!(code, 3, "{err}");
    assert!(err.contains("expected one of terminal, agent-ways, nord,") && err.contains("found 'mine'"), "{err}");
    assert!(!f.user().exists());
    assert_eq!(f.run(&["settings", "set", "theme.active", "nord"]).2, 0);
    // A theme file of the user's own becomes a choice.
    let theme = include_str!("../../agent-theme/themes/nord.theme").replace("THEME_NAME=\"nord\"", "THEME_NAME=\"mine\"");
    f.write(&f.root.join("xdg/config/agent-ways/themes/mine.theme"), &theme);
    assert_eq!(f.run(&["settings", "set", "theme.active", "mine"]).2, 0);
    let v = json(&f, &["settings", "get", "theme.active", "--json"]);
    let options = v["options"].as_array().unwrap();
    assert_eq!(options[0], "terminal");
    assert_eq!(options.last().unwrap(), "mine");
    assert_eq!(v["value"], "mine");
    // The other theme key is unchanged: no options.
    assert!(json(&f, &["settings", "get", "theme.shape", "--json"]).get("options").is_none());
}

#[test]
fn the_language_is_en_auto_or_a_locale_a_way_carries() {
    let f = Fx::new();
    let v = json(&f, &["settings", "get", "ways.language", "--json"]);
    assert_eq!(v["options"], serde_json::json!(["en", "auto", "de", "es", "ja"]));
    assert_eq!(v["value"], "auto");
    let (_, err, code) = f.run(&["settings", "set", "ways.language", "fr"]);
    assert_eq!(code, 3, "{err}");
    assert!(err.contains("expected one of en, auto, de, es, ja, found 'fr'"), "{err}");
    assert!(!f.user().exists());
    // A packed stub in the user's own root, and a way of the project's, add locales.
    f.write(&f.root.join("xdg/config/agent-ways/ways/mine/mine.locales.jsonl"), "{\"lang\":\"fr\",\"description\":\"d\"}\n");
    f.write(&f.root.join("proj/.claude/ways/p/p.ko.md"), "x\n");
    f.write(&f.overlay(), "enabled: true\n");
    let v = json(&f, &["settings", "get", "ways.language", "--json"]);
    assert_eq!(v["options"], serde_json::json!(["en", "auto", "de", "es", "fr", "ja", "ko"]));
    assert_eq!(f.run(&["settings", "set", "ways.language", "fr"]).2, 0);
    assert_eq!(f.run(&["settings", "set", "ways.language", "en"]).2, 0);
    assert_eq!(f.run(&["settings", "set", "ways.language", "auto"]).2, 0);
    // A value written by hand that no way carries loads as written and is a lint finding.
    f.write(&f.user(), "language: xx\n");
    assert_eq!(f.run(&["settings", "get", "ways.language"]).0, "xx\n");
    assert_eq!(f.run(&["settings", "lint"]).2, 3);
    // Another key of the section has no options.
    assert!(json(&f, &["settings", "get", "ways.default_scope", "--json"]).get("options").is_none());
}

#[test]
fn a_profile_s_model_stays_text_because_its_list_needs_the_network() {
    let f = Fx::new();
    let agent = f.root.join("xdg/config/agent-ways/agent.yaml");
    let (out, _, _) = f.run(&["settings", "help", "gate.profiles.anthropic.model"]);
    assert!(out.contains("text (the choices could not be listed: model list needs the network; run `ways agent models`)"), "{out}");
    // Any model id is taken; one that is no id is refused.
    assert_eq!(f.run(&["settings", "set", "gate.profiles.anthropic.model", "claude-sonnet-5-5"]), (String::new(), String::new(), 0));
    assert!(std::fs::read_to_string(&agent).unwrap().contains("claude-sonnet-5-5"));
    assert_eq!(f.run(&["settings", "set", "gate.profiles.anthropic.model", "not a model"]).2, 3);
    // `options` is null: a computed choice whose source could not answer, so no picker.
    let v = json(&f, &["settings", "get", "gate.profiles.anthropic.model", "--json"]);
    assert_eq!(v["options"], serde_json::Value::Null, "{v}");
    assert!(v.as_object().unwrap().contains_key("options"));
    assert_eq!(v["value"], "claude-sonnet-5-5");
}

#[test]
fn the_disabled_domains_are_picked_from_the_corpus_domains() {
    let f = Fx::new();
    f.write(&f.root.join("xdg/data/agent-ways/hooks/ways/ea/x/x.md"), "---\ndescription: d\n---\n");
    f.write(&f.root.join("xdg/data/agent-ways/hooks/ways/.hidden/h.md"), "x\n");
    f.write(&f.root.join("xdg/data/agent-ways/hooks/ways/empty/note.txt"), "x\n");
    f.write(&f.root.join("xdg/config/agent-ways/ways/itops/y/y.md"), "---\ndescription: d\n---\n");
    f.write(&f.root.join("proj/.claude/ways/mine/m.md"), "---\ndescription: d\n---\n");
    f.write(&f.overlay(), "enabled: true\n");
    let v = json(&f, &["settings", "get", "ways.disabled_domains", "--json"]);
    assert_eq!(v["options"], serde_json::json!(["ea", "itops", "loc", "mine"]));
    assert_eq!(v["value"], serde_json::json!([]));
    let (out, _, _) = f.run(&["settings", "help", "ways.disabled_domains"]);
    assert!(out.contains("a list, each one of ea, itops, loc, mine"), "{out}");
    // A list, or the comma text, of domains in the list.
    assert_eq!(f.run(&["settings", "set", "ways.disabled_domains", "[ea, itops]"]).2, 0);
    assert_eq!(f.run(&["settings", "set", "ways.disabled_domains", "ea,mine"]).2, 0);
    assert_eq!(json(&f, &["settings", "get", "ways.disabled_domains", "--json"])["value"], serde_json::json!(["ea", "mine"]));
    // One outside the list refuses the whole write.
    let before = std::fs::read_to_string(f.user()).unwrap();
    let (_, err, code) = f.run(&["settings", "set", "ways.disabled_domains", "ea,nope"]);
    assert_eq!(code, 3, "{err}");
    assert!(err.contains("nope"), "{err}");
    assert_eq!(std::fs::read_to_string(f.user()).unwrap(), before);
    // A domain written by hand that no root holds loads and still silences it.
    f.write(&f.user(), "disabled_domains: [gone]\n");
    assert_eq!(json(&f, &["settings", "get", "ways.disabled_domains", "--json"])["value"], serde_json::json!(["gone"]));
}
