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
    /// The root as the filesystem resolves it: on macOS `/tmp` is a link to
    /// `/private/tmp`, and the binary prints a `--project` it resolved in
    /// this form. Elsewhere it is `root`.
    real: PathBuf,
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
        // The project: a root `--project` and the walk up from a working
        // directory both find (`settings_root_of` in cmd/settings).
        std::fs::create_dir_all(root.join("proj/.claude")).unwrap();
        std::fs::write(root.join("proj/CLAUDE.md"), "").unwrap();
        // The root stays as written, the length above; the resolved form is
        // kept beside it for `norm`. Windows resolves to a `\\?\` path the
        // binary never prints, so there it is the root itself.
        #[cfg(not(windows))]
        let real = std::fs::canonicalize(&root).unwrap();
        #[cfg(windows)]
        let real = root.clone();
        Fx { root, real }
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

    /// Output with the fixture root as `<ROOT>`, in its resolved form and as
    /// written. On Windows the path that follows the root is written with
    /// `/`, so goldens and assertions read the same on every platform;
    /// nothing but those paths is changed.
    fn norm(&self, b: &[u8]) -> String {
        let text = String::from_utf8_lossy(b).to_string();
        let root = self.root.to_string_lossy().to_string();
        #[cfg(not(windows))]
        return text.replace(&*self.real.to_string_lossy(), "<ROOT>").replace(&root, "<ROOT>");
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

    /// `p` as the binary's fix commands print it, by the rule of
    /// `lifecycle::shell_word` (bare when every character is plain,
    /// single-quoted otherwise, as a Windows path with `\` is), then
    /// normalised as output is.
    fn word(&self, p: &Path) -> String {
        let s = p.display().to_string();
        let w = if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-~:@+=,".contains(c)) {
            s
        } else {
            format!("'{}'", s.replace('\'', "'\\''"))
        };
        self.norm(w.as_bytes())
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
    assert_eq!(f.run(&["settings", "set", "ways.project.itops/incident", "false"]), (String::new(), "project: <ROOT>/proj/.claude/ways.yaml\n".to_string(), 0));
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
    // 0: done: nothing on stdout, and stderr names the file written.
    assert_eq!(f.run(&["settings", "set", "matching.semantic_fire_probability", "0.4"]), (String::new(), "user: <ROOT>/xdg/config/agent-ways/config.yaml\n".to_string(), 0));
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
    assert_eq!(report["accepted"].as_array().unwrap().len(), 11);
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
    assert!(err.contains("expected one of anthropic, openrouter, claude-code, found 'mine'"), "{err}");
    assert!(!agent.exists());
    assert_eq!(f.run(&["settings", "set", "gate.engine", "openrouter"]).2, 0);
    // A profile of the user's own becomes a choice.
    f.write(&agent, "profiles:\n  mine:\n    provider: anthropic\n    model: claude-sonnet-5-5\n");
    assert_eq!(f.run(&["settings", "set", "gate.engine", "mine"]), (String::new(), "user: <ROOT>/xdg/config/agent-ways/agent.yaml\n".to_string(), 0));
    let (out, _, _) = f.run(&["settings", "help", "gate.engine"]);
    assert!(out.contains("type:    one of anthropic, openrouter, claude-code, mine"), "{out}");
    let v: serde_json::Value = serde_json::from_str(&f.run(&["settings", "get", "gate.engine", "--json"]).0).unwrap();
    assert_eq!(v["options"], serde_json::json!(["anthropic", "openrouter", "claude-code", "mine"]));
    assert_eq!(v["value"], "mine");
    // Every other key's object is as it was: no options.
    let v: serde_json::Value = serde_json::from_str(&f.run(&["settings", "get", "gate.mode", "--json"]).0).unwrap();
    assert!(v.get("options").is_none(), "{v}");
    let (out, _, _) = f.run(&["settings", "emit", "gate"]);
    assert!(out.starts_with("# gate.engine: one of anthropic, openrouter, claude-code, mine\n"), "{out}");
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
        out.contains("agent.yaml:1: [gate] engine: expected one of anthropic, openrouter, claude-code, found 'gone'; it loads as written, and `ways settings set gate.engine <choice>` repairs it"),
        "{out}"
    );
    assert!(err.contains("1 finding; each line names its repair") && !err.contains("fix"), "{err}");
    assert_eq!(f.run(&["settings", "get", "gate.engine"]).0, "gone\n");
}

// ── the model of a profile, from the provider's cached list (#795) ──

#[test]
fn a_profile_s_model_is_one_of_the_cached_list_of_its_provider() {
    let f = Fx::new();
    let cache = |p: &str| f.root.join(format!("xdg/cache/agent-ways/agent/models-{p}.json"));
    let list = |p: &str, at: u64, ids: &[&str]| {
        let models: Vec<_> = ids.iter().map(|i| serde_json::json!({"id": i, "name": i, "input_per_mtok": null, "output_per_mtok": null})).collect();
        f.write(&cache(p), &serde_json::json!({"provider": p, "fetched_at": at, "models": models}).to_string());
    };
    let key = "gate.profiles.anthropic.model";
    // No list yet: text, with the way to fetch one, and the id shape still checked.
    let v: serde_json::Value = serde_json::from_str(&f.run(&["settings", "get", key, "--json"]).0).unwrap();
    assert_eq!(v["options"], serde_json::Value::Null);
    let (out, _, _) = f.run(&["settings", "help", key]);
    assert!(out.contains("text (the choices could not be listed: model list not fetched; run `ways agent models --provider anthropic`)"), "{out}");
    assert_eq!(f.run(&["settings", "set", key, "claude-sonnet-5-5"]).2, 0);
    let (_, err, code) = f.run(&["settings", "set", key, "not a model"]);
    assert_eq!(code, 3, "{err}");
    // A cached list, however old, is what is offered; each provider has its own.
    list("anthropic", 1, &["claude-sonnet-5-5", "claude-haiku-4-5"]);
    list("openrouter", 1, &["anthropic/claude-haiku-4.5", "openai/gpt-5"]);
    let v: serde_json::Value = serde_json::from_str(&f.run(&["settings", "get", key, "--json"]).0).unwrap();
    assert_eq!(v["options"], serde_json::json!(["claude-haiku-4-5", "claude-sonnet-5-5"]));
    let v: serde_json::Value = serde_json::from_str(&f.run(&["settings", "get", "gate.profiles.openrouter.model", "--json"]).0).unwrap();
    assert_eq!(v["options"], serde_json::json!(["anthropic/claude-haiku-4.5", "openai/gpt-5"]));
    // A model the list lacks is refused with nothing written; one it has is set.
    let agent = f.root.join("xdg/config/agent-ways/agent.yaml");
    std::fs::remove_file(&agent).unwrap();
    let (_, err, code) = f.run(&["settings", "set", key, "claude-opus-9"]);
    assert_eq!(code, 3, "{err}");
    assert!(err.contains("expected one of claude-haiku-4-5, claude-sonnet-5-5, found 'claude-opus-9'"), "{err}");
    assert!(!agent.exists());
    assert_eq!(f.run(&["settings", "set", key, "claude-sonnet-5-5"]).2, 0);
    // A user profile follows its own provider, and emit notes the choices.
    f.write(&agent, "profiles:\n  mine:\n    provider: openrouter\n    model: openai/gpt-5\n");
    assert_eq!(f.run(&["settings", "set", "gate.profiles.mine.model", "openai/gpt-5"]).2, 0);
    assert_eq!(f.run(&["settings", "set", "gate.profiles.mine.model", "claude-sonnet-5-5"]).2, 3);
    let (out, _, _) = f.run(&["settings", "emit", "gate"]);
    assert!(out.contains("# gate.profiles.anthropic.model: one of claude-haiku-4-5, claude-sonnet-5-5\n"), "{out}");
    // The pattern as written is no instance: set and get refuse it, and no
    // profile named `*` is written.
    for args in [["settings", "set", "gate.profiles.*.model", "zzz-not-listed"], ["settings", "get", "gate.profiles.*.model", "--json"]] {
        let (_, err, code) = f.run(&args);
        assert_eq!(code, 2, "{args:?}: {err}");
        assert!(err.contains("name the instance, e.g. gate.profiles.anthropic.model"), "{err}");
    }
    assert!(!std::fs::read_to_string(&agent).unwrap_or_default().contains('*'));
    // The pattern key's help names no one provider.
    let (out, _, _) = f.run(&["settings", "help", "gate.profiles.*.model"]);
    assert!(out.contains("text (the choices could not be listed: the list depends on the profile's provider)"), "{out}");
    // The canonical body shows the shipped provider, and its note follows it
    // even when a stored patch moves that profile to another provider.
    f.write(&agent, "profiles:\n  anthropic:\n    provider: openrouter\n    model: openai/gpt-5\n");
    let (canon, _, _) = f.run(&["settings", "emit", "gate"]);
    assert!(canon.contains("# gate.profiles.anthropic.model: one of claude-haiku-4-5, claude-sonnet-5-5\n") && canon.contains("    provider: anthropic\n"), "{canon}");
    let (eff, _, _) = f.run(&["settings", "emit", "--effective", "gate"]);
    assert!(eff.contains("# gate.profiles.anthropic.model: one of anthropic/claude-haiku-4.5, openai/gpt-5\n") && eff.contains("    provider: openrouter\n"), "{eff}");
    f.write(&agent, "profiles:\n  mine:\n    provider: openrouter\n    model: openai/gpt-5\n");
    // What emit prints, apply takes back unchanged.
    let emitted = f.run(&["settings", "emit", "gate"]).0;
    let (out, err, code) = f.run_stdin(&["settings", "apply", "--dry-run"], Some(&emitted));
    assert_eq!(code, 0, "{out}{err}");
    // apply checks a model against the provider's list too.
    let (_, _, code) = f.run_stdin(&["settings", "apply", "--dry-run"], Some("profiles:\n  anthropic:\n    model: nope-1\n"));
    assert_eq!(code, 3);
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
        "settings-trace: load-sections ways:config [ways,ways.switch,ways.subagents,ways.domains,matching,install.targets,install.secret_path_deny,ways.project,ways.log]";
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
    assert!(err.contains("`ways manage target add|enable|disable|remove <dir> --commit` repairs it"), "{err}");
    assert_eq!(std::fs::read_to_string(f.user()).unwrap(), src);
    // The load diagnostic names that command too, not fix.
    let (_, err, _) = f.run(&["settings", "get", "install.targets"]);
    assert!(err.contains("ways manage target") && !err.contains("settings fix install"), "{err}");
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
        let (_, err, _) = f.run(&["manage", "target", "list"]);
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
fn the_language_is_en_auto_or_an_active_language_code() {
    let f = Fx::new();
    let v = json(&f, &["settings", "get", "ways.language", "--json"]);
    let options: Vec<&str> = v["options"].as_array().unwrap().iter().map(|o| o.as_str().unwrap()).collect();
    assert_eq!(&options[..4], ["en", "auto", "ar", "de"]);
    assert_eq!(options.iter().filter(|o| **o == "en").count(), 1, "{options:?}");
    assert!(options.contains(&"pt-br") && options.contains(&"ja"));
    assert_eq!(v["value"], "auto");
    // The same list from another directory: it never depends on the cwd.
    f.write(&f.root.join("proj/.claude/ways/p/p.md"), "---\ndescription: d\n---\n");
    assert_eq!(json(&f, &["settings", "get", "ways.language", "--json"])["options"], v["options"]);
    for ok in ["ja", "en", "auto", "pt-br"] {
        assert_eq!(f.run(&["settings", "set", "ways.language", ok]).2, 0, "{ok}");
    }
    // A name is no code: the key's doc says code, and the files hold codes.
    for bad in ["japanese", "日本語", "xx"] {
        let (_, err, code) = f.run(&["settings", "set", "ways.language", bad]);
        assert_eq!(code, 3, "{bad}: {err}");
        assert!(err.contains(&format!("found '{bad}'")), "{err}");
    }
    assert!(std::fs::read_to_string(f.user()).unwrap().contains("language: pt-br"));
    // A value written by hand that is no code loads as written and is a lint finding.
    f.write(&f.user(), "language: japanese\n");
    assert_eq!(f.run(&["settings", "get", "ways.language"]).0, "japanese\n");
    assert_eq!(f.run(&["settings", "lint"]).2, 3);
    assert!(json(&f, &["settings", "get", "ways.default_scope", "--json"]).get("options").is_none());
}

#[test]
fn the_disabled_domains_are_picked_from_the_corpus_domains() {
    let f = Fx::new();
    let way = "---\ndescription: d\n---\n";
    f.write(&f.root.join("xdg/data/agent-ways/hooks/ways/ea/x/x.md"), way);
    f.write(&f.root.join("xdg/data/agent-ways/hooks/ways/.hidden/h.md"), way);
    f.write(&f.root.join("xdg/data/agent-ways/hooks/ways/notes/note.md"), "no frontmatter\n");
    f.write(&f.root.join("xdg/config/agent-ways/ways/itops/y/y.md"), way);
    let v = json(&f, &["settings", "get", "ways.disabled_domains", "--json"]);
    // Hidden directories and directories with no way are not domains.
    assert_eq!(v["options"], serde_json::json!(["ea", "itops"]));
    assert_eq!(v["value"], serde_json::json!([]));
    let (out, _, _) = f.run(&["settings", "help", "ways.disabled_domains"]);
    assert!(out.contains("a list, each one of ea, itops"), "{out}");
    // A list, or the comma text, of domains in the list.
    assert_eq!(f.run(&["settings", "set", "ways.disabled_domains", "[ea, itops]"]).2, 0);
    assert_eq!(json(&f, &["settings", "get", "ways.disabled_domains", "--json"])["value"], serde_json::json!(["ea", "itops"]));
    // One outside the list refuses the whole write.
    let before = std::fs::read_to_string(f.user()).unwrap();
    let (_, err, code) = f.run(&["settings", "set", "ways.disabled_domains", "ea,nope"]);
    assert_eq!(code, 3, "{err}");
    assert!(err.contains("nope"), "{err}");
    assert_eq!(std::fs::read_to_string(f.user()).unwrap(), before);
}

#[test]
fn a_domain_of_description_less_ways_is_a_disabled_domains_choice() {
    let f = Fx::new();
    // A file-triggered way: a frontmatter fence, no `description:`.
    f.write(&f.root.join("xdg/data/agent-ways/hooks/ways/files/w/w.md"), "---\nfiles: \\.rs$\n---\n");
    let v = json(&f, &["settings", "get", "ways.disabled_domains", "--json"]);
    assert_eq!(v["options"], serde_json::json!(["files"]));
    assert_eq!(f.run(&["settings", "set", "ways.disabled_domains", "files"]).2, 0);
}

#[test]
fn a_project_s_own_domain_is_a_choice_through_its_layer_and_a_stored_one_stays_settable() {
    let f = Fx::new();
    let way = "---\ndescription: d\n---\n";
    f.write(&f.root.join("xdg/data/agent-ways/hooks/ways/ea/x/x.md"), way);
    f.write(&f.root.join("proj/.claude/ways/mine/m.md"), way);
    let p = f.root.join("proj");
    let p = p.to_str().unwrap();
    // The project layer lists its own domain, so `set --project` and a write
    // from inside the project take it, and the user file's write does too:
    // one rule, the layers' (no special case by key).
    let v = json(&f, &["settings", "get", "ways.disabled_domains", "--json"]);
    assert_eq!(v["options"], serde_json::json!(["ea", "mine"]));
    let (_, err, code) = f.run(&["settings", "set", "ways.disabled_domains", "mine", "--project", p]);
    assert_eq!(code, 0, "{err}");
    assert!(std::fs::read_to_string(f.overlay()).unwrap().contains("mine"));
    assert_eq!(f.run(&["settings", "set", "ways.disabled_domains", "nope", "--project", p]).2, 3);
    // From another directory the project's domain is not listed, but a
    // stored one stays settable, and adding a listed one succeeds.
    f.write(&f.user(), "disabled_domains: [mine]\n");
    let other = f.root.join("home");
    let from_other = |args: &[&str]| {
        let mut c = f.cmd(args);
        c.current_dir(&other).env("CLAUDE_PROJECT_DIR", &other);
        c.output().unwrap()
    };
    let out = from_other(&["settings", "get", "ways.disabled_domains", "--json"]);
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["options"], serde_json::json!(["ea"]));
    assert!(from_other(&["settings", "set", "ways.disabled_domains", "mine,ea"]).status.success());
    assert_eq!(from_other(&["settings", "set", "ways.disabled_domains", "mine,nope"]).status.code(), Some(3));
    f.write(&f.user(), "disabled_domains: [mine]\n");
    let out = from_other(&["settings", "lint"]);
    assert_eq!(out.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&out.stdout).contains("mine"), "{}", String::from_utf8_lossy(&out.stdout));
}

// ── per-way toggles that name no way (ADR-131) ──────────────────

/// A way file at `root/<id>/<leaf>.md`.
fn way(f: &Fx, root: &Path, id: &str) {
    let leaf = id.rsplit('/').next().unwrap();
    f.write(&root.join(id).join(format!("{leaf}.md")), "---\ndescription: a way\n---\nbody\n");
}

/// Shipped `data`, `data/schema-docs` and `workstation/shell/shell-prompt`,
/// and a project way `local/mine`.
fn renamed_corpus(f: &Fx) {
    let shipped = f.root.join("home/.claude/hooks/ways");
    for id in ["data", "data/schema-docs", "workstation/shell/shell-prompt"] {
        way(f, &shipped, id);
    }
    way(f, &f.root.join("proj/.claude/ways"), "local/mine");
}

#[test]
fn a_toggle_naming_a_renamed_way_is_a_lint_warning_and_exits_clean() {
    let f = Fx::new();
    renamed_corpus(&f);
    f.write(&f.overlay(), "ways:\n  data/documentation: false\n  workstation/shell/prompt: false\n  data/schema-docs: false\n  workstation/**: false\n  local/mine: false\n");
    let (out, err, code) = f.run(&["settings", "lint"]);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(
        out,
        "<ROOT>/proj/.claude/ways.yaml:2: [ways.project] ways.data/documentation: warning: names no way in the project, user or shipped ways, so it switches nothing; nearest way: data/schema-docs\n\
         <ROOT>/proj/.claude/ways.yaml:3: [ways.project] ways.workstation/shell/prompt: warning: names no way in the project, user or shipped ways, so it switches nothing; nearest way: workstation/shell/shell-prompt\n"
    );
}

#[test]
fn a_pattern_matching_no_way_is_a_lint_warning_and_one_matching_a_way_is_not() {
    let f = Fx::new();
    renamed_corpus(&f);
    f.write(&f.overlay(), "ways:\n  itops/**: false\n  data/**: false\n  \"*/mine\": false\n  workstation/shell/**: true\n");
    let (out, _, code) = f.run(&["settings", "lint"]);
    assert_eq!(code, 0);
    assert_eq!(out, "<ROOT>/proj/.claude/ways.yaml:2: [ways.project] ways.itops/**: warning: names no way in the project, user or shipped ways, so it switches nothing\n");
}

#[test]
fn a_trailing_star_key_is_a_lint_warning_naming_the_toggles_for_its_old_meaning() {
    let f = Fx::new();
    renamed_corpus(&f);
    f.write(&f.overlay(), "ways:\n  data/*: false\n  workstation/*: true\n");
    let (out, err, code) = f.run(&["settings", "lint"]);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(
        out,
        "<ROOT>/proj/.claude/ways.yaml:2: [ways.project] ways.data/*: warning: `/*` matches only the ways directly under data; for data and every way under it, write `data: false` plus `data/**: false`\n\
         <ROOT>/proj/.claude/ways.yaml:3: [ways.project] ways.workstation/*: warning: `/*` matches only the ways directly under workstation; for workstation and every way under it, write `workstation: true` plus `workstation/**: true`\n\
         <ROOT>/proj/.claude/ways.yaml:3: [ways.project] ways.workstation/*: warning: names no way in the project, user or shipped ways, so it switches nothing\n"
    );
}

#[test]
fn a_true_false_tie_between_patterns_is_a_lint_warning() {
    let f = Fx::new();
    renamed_corpus(&f);
    f.write(&f.overlay(), "ways:\n  workstation/*/shell-prompt: true\n  \"*/shell/shell-prompt\": false\n  data/**: false\n");
    let (out, err, code) = f.run(&["settings", "lint"]);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(
        out,
        "<ROOT>/proj/.claude/ways.yaml:3: [ways.project] ways.*/shell/shell-prompt: warning: ties with ways.workstation/*/shell-prompt on workstation/shell/shell-prompt at the same specificity with the opposite value, so the tie resolves to false\n"
    );
}

#[test]
fn a_segment_mixing_text_and_a_wildcard_is_a_lint_error() {
    let f = Fx::new();
    renamed_corpus(&f);
    f.write(&f.overlay(), "ways:\n  data/schema*: false\n  data/schema-docs: false\n");
    let (out, err, code) = f.run(&["settings", "lint"]);
    assert_eq!(code, 3, "{out}{err}");
    assert_eq!(
        out,
        "<ROOT>/proj/.claude/ways.yaml:2: [ways.project] ways.data/schema*: a segment mixes text and a wildcard, so the key is read literally and matches no way; write `*` or `**` as a whole segment\n"
    );
}

#[test]
fn a_mixed_segment_before_a_trailing_star_gets_only_the_error() {
    let f = Fx::new();
    renamed_corpus(&f);
    f.write(&f.overlay(), "ways:\n  data*/*: false\n");
    let (out, err, code) = f.run(&["settings", "lint"]);
    assert_eq!(code, 3, "{out}{err}");
    assert_eq!(
        out,
        "<ROOT>/proj/.claude/ways.yaml:2: [ways.project] ways.data*/*: a segment mixes text and a wildcard, so the key is read literally and matches no way; write `*` or `**` as a whole segment\n"
    );
}

#[test]
fn a_trailing_star_key_beside_its_directory_key_keeps_that_key() {
    let f = Fx::new();
    renamed_corpus(&f);
    let shipped = f.root.join("home/.claude/hooks/ways");
    for id in ["softwaredev/code", "softwaredev/code/quality"] {
        way(&f, &shipped, id);
    }
    f.write(&f.overlay(), "ways:\n  softwaredev/code: true\n  softwaredev/code/*: false\n");
    let (out, err, code) = f.run(&["settings", "lint"]);
    assert_eq!(code, 0, "{out}{err}");
    assert_eq!(
        out,
        "<ROOT>/proj/.claude/ways.yaml:3: [ways.project] ways.softwaredev/code/*: warning: `/*` matches only the ways directly under softwaredev/code; for every way under it, write `softwaredev/code/**: false`; the existing `softwaredev/code: true` stays, so softwaredev/code itself stays on\n"
    );
}

#[test]
fn a_bare_star_key_is_set_linted_and_listed_as_a_toggle() {
    let f = Fx::new();
    renamed_corpus(&f);
    let (out, err, code) = f.run(&["settings", "set", "ways.project.*", "false"]);
    assert_eq!((out.as_str(), code), ("", 0), "{err}");
    assert!(std::fs::read_to_string(f.overlay()).unwrap().contains("  \"*\": false\n"), "the key is quoted");
    assert_eq!(f.run(&["settings", "get", "ways.project.*"]).0, "false\n");
    let (out, err, code) = f.run(&["settings", "lint"]);
    assert_eq!((out.as_str(), code), ("", 0), "`*` matches the one-segment way data: {err}");
    let (out, _, _) = f.run(&["status"]);
    assert!(out.contains("Disabled ways:    * (project scope"), "{out}");
}

#[test]
fn setting_a_key_with_a_mixed_segment_is_refused_as_lint_reports_it() {
    let f = Fx::new();
    let (out, err, code) = f.run(&["settings", "set", "ways.project.doc*", "false"]);
    assert_eq!((out.as_str(), code), ("", 2));
    assert_eq!(
        err,
        "ways settings: ways.project.doc*: a segment mixes text and a wildcard, so the key is read literally and matches no way; write `*` or `**` as a whole segment\n"
    );
    assert!(!f.overlay().exists(), "nothing is written");
}

#[test]
fn status_lists_a_toggle_naming_no_way_apart_from_the_disabled_ways() {
    let f = Fx::new();
    renamed_corpus(&f);
    f.write(&f.overlay(), "ways:\n  data/documentation: false\n  data/schema-docs: false\n  local/mine: false\n  data/**: false\n  bogus/thing:\n    threshold: 0.5\n");
    let (out, err, _) = f.run(&["status"]);
    let line = |p: &str| out.lines().find(|l| l.starts_with(p)).unwrap_or_else(|| panic!("no {p} line in\n{out}{err}")).to_string();
    assert_eq!(line("Disabled ways:"), "Disabled ways:    data/schema-docs, local/mine, data/** (project scope, ADR-131; `dir/**` covers every way under `dir`)");
    // An entry that sets no `enabled` is still a key status checks, as lint does.
    assert_eq!(
        line("Toggles naming no way:"),
        "Toggles naming no way: data/documentation, nearest data/schema-docs; bogus/thing in <ROOT>/proj/.claude/ways.yaml switch nothing"
    );
    let (lint, _, _) = f.run(&["settings", "lint"]);
    let warned: Vec<&str> = lint.lines().filter(|l| l.contains("warning: names no way")).collect();
    assert_eq!(warned.len(), 2, "{lint}");
    assert!(warned[0].contains("ways.data/documentation:") && warned[1].contains("ways.bogus/thing:"), "{lint}");
    let (out, _, _) = f.run(&["status", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["disabled_ways"], serde_json::json!(["data/schema-docs", "local/mine", "data/**"]));
    assert_eq!(v["toggles_checked"], true);
    assert_eq!(v["unmatched_toggles"][0]["key"], "data/documentation");
    assert_eq!(v["unmatched_toggles"][0]["nearest"], "data/schema-docs");
    assert_eq!(v["unmatched_toggles"][1]["key"], "bogus/thing");
    assert_eq!(v["unmatched_toggles"].as_array().unwrap().len(), 2);
    // The way counts by origin (ADR-516 §5), with no other names.
    let ways = v["ways"].as_object().unwrap();
    assert_eq!(ways.keys().collect::<Vec<_>>(), ["builtin", "user"]);
    assert!(v["ways"]["builtin"]["total"].as_u64().is_some_and(|n| n > 0) && v["ways"]["user"]["semantic"].is_u64(), "{}", v["ways"]);
    let (text, _, _) = f.run(&["status"]);
    assert!(text.contains("\nBuiltin ways: ") && !text.contains("Core ways"), "{text}");
}

#[test]
fn without_the_shipped_ways_no_toggle_is_judged() {
    let f = Fx::new();
    // A project way and a user way, and no shipped root.
    way(&f, &f.root.join("proj/.claude/ways"), "local/mine");
    way(&f, &f.root.join("xdg/config/agent-ways/ways"), "mine/own");
    assert!(!f.root.join("home/.claude/hooks/ways").exists() && !f.root.join("xdg/data/agent-ways/hooks/ways").exists());
    f.write(&f.overlay(), "ways:\n  data/schema-docs: false\n  local/mine: false\n  gone/way: false\n");
    let (out, err, code) = f.run(&["settings", "lint"]);
    assert_eq!((out.as_str(), code), ("", 0), "{err}");
    let (out, _, _) = f.run(&["status"]);
    assert!(out.contains("\nWay toggles: not checked, because the shipped ways were not found\n"), "{out}");
    assert!(!out.contains("naming no way"), "{out}");
    assert!(out.contains("Disabled ways:    data/schema-docs, local/mine, gone/way "), "{out}");
    let (out, _, _) = f.run(&["status", "--json"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["toggles_checked"], false);
    assert_eq!(v["unmatched_toggles"], serde_json::json!([]));
}

#[test]
fn a_toggle_naming_a_way_in_a_disabled_domain_is_not_flagged() {
    let f = Fx::new();
    renamed_corpus(&f);
    f.write(&f.user(), "disabled_domains: [data]\n");
    f.write(&f.overlay(), "ways:\n  data/schema-docs: false\n  data/**: false\n");
    let (out, err, code) = f.run(&["settings", "lint"]);
    assert_eq!((out.as_str(), code), ("", 0), "{err}");
    let (out, _, _) = f.run(&["status"]);
    assert!(!out.contains("naming no way") && !out.contains("not checked"), "{out}");
}

#[test]
fn status_says_nothing_of_toggles_when_each_names_a_way() {
    let f = Fx::new();
    renamed_corpus(&f);
    f.write(&f.overlay(), "ways:\n  data/schema-docs: false\n  local/mine: false\n");
    let (out, _, _) = f.run(&["status"]);
    assert!(!out.contains("naming no way"), "{out}");
    assert!(out.contains("Disabled ways:    data/schema-docs, local/mine "), "{out}");
}

// ── --project names the project enclosing a path ──────────────

/// stdout, stderr and exit code of `args` run from `dir` under the fixture
/// root, as an operator's shell runs it: no `CLAUDE_PROJECT_DIR`.
fn run_from(f: &Fx, dir: &str, args: &[&str], input: Option<&str>) -> (String, String, i32) {
    let cwd = f.root.join(dir);
    std::fs::create_dir_all(&cwd).unwrap();
    let mut c = f.cmd(args);
    c.current_dir(&cwd).env_remove("CLAUDE_PROJECT_DIR");
    c.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = c.spawn().unwrap();
    if let Some(i) = input {
        use std::io::Write;
        child.stdin.take().unwrap().write_all(i.as_bytes()).unwrap();
    }
    let out = child.wait_with_output().unwrap();
    (f.norm(&out.stdout), f.norm(&out.stderr), out.status.code().unwrap_or(-1))
}

#[test]
fn project_dot_from_a_subdirectory_is_the_project_root() {
    let f = Fx::new();
    let obj = r#"{"mods":{"attend":{"keepwarm":"off"}}}"#;
    let (out, err, code) = run_from(&f, "proj/tools", &["settings", "apply", "--dry-run", "--project", "."], Some(obj));
    assert_eq!(code, 0, "{err}");
    let report: serde_json::Value = serde_json::from_str(&out).unwrap();
    let written: Vec<&String> = report["written"].as_object().unwrap().keys().collect();
    assert_eq!(written, ["<ROOT>/proj/.claude/ways.yaml"], "the root's file, by its absolute path");
    assert_eq!(err, "project: <ROOT>/proj/.claude/ways.yaml (dry run, nothing written)\n");
    let (_, err, code) = run_from(&f, "proj/tools", &["settings", "set", "mods.attend.keepwarm", "off", "--project", "."], None);
    assert_eq!((err.as_str(), code), ("project: <ROOT>/proj/.claude/ways.yaml\n", 0));
    assert!(std::fs::read_to_string(f.overlay()).unwrap().contains("keepwarm: off"));
    assert!(!f.root.join("proj/tools/.claude").exists(), "nothing is written under the subdirectory");
}

#[test]
fn a_relative_project_path_resolves_against_the_working_directory() {
    let f = Fx::new();
    std::fs::create_dir_all(f.root.join("proj/tools/deep")).unwrap();
    let (_, err, code) = run_from(&f, "home", &["settings", "set", "mods.attend.keepwarm", "off", "--project", "../proj/tools/deep"], None);
    assert_eq!((err.as_str(), code), ("project: <ROOT>/proj/.claude/ways.yaml\n", 0));
    let (_, err, code) = run_from(&f, "proj/tools", &["settings", "unset", "mods.attend.keepwarm", "--project", ".."], None);
    assert_eq!((err.as_str(), code), ("project: <ROOT>/proj/.claude/ways.yaml\n", 0));
}

/// Whether no directory above the fixture root is a project root as the
/// settings resolver sees one, so a directory under it with no marker of
/// its own is in no project. A temp directory inside a checkout (some CI and
/// nix setups) encloses everything; tests that need no project skip there.
fn outside_any_project(f: &Fx) -> bool {
    use ways_core::util::{is_project_root, SETTINGS_OVERLAYS};
    let marked = |d: &Path| is_project_root(d, SETTINGS_OVERLAYS) || d.join(".git").exists();
    let clear = !f.real.ancestors().any(marked);
    if !clear {
        eprintln!("skipped: a directory above {} is a project root", f.root.display());
    }
    clear
}

#[test]
fn a_write_to_a_project_path_that_is_no_project_is_a_usage_error_and_writes_nothing() {
    let f = Fx::new();
    // A path that does not exist.
    let (_, err, code) = run_from(&f, "proj", &["settings", "set", "mods.attend.keepwarm", "off", "--project", "nope"], None);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--project nope: no such directory"), "{err}");
    if !outside_any_project(&f) {
        return;
    }
    // A directory no project encloses, such as a scratch directory.
    let scratch = f.root.join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let at = scratch.to_str().unwrap();
    for args in [
        vec!["settings", "set", "mods.attend.keepwarm", "off", "--project", at],
        vec!["settings", "unset", "mods.attend.keepwarm", "--project", at],
        vec!["settings", "fix", "ways", "--project", at],
    ] {
        let (_, err, code) = run_from(&f, "home", &args, None);
        assert_eq!(code, 2, "{args:?}: {err}");
        assert!(err.contains("no project encloses <ROOT>/scratch; a project root holds .claude/settings.json, .claude/settings.local.json, .claude/ways.yaml, .claude/attend.yaml, or CLAUDE.md beside .claude/; failing those, a .git"), "{args:?}: {err}");
    }
    let (out, err, code) = run_from(&f, "home", &["settings", "apply", "--project", at], Some("mods:\n  attend:\n    keepwarm: off\n"));
    assert_eq!((out.as_str(), code), ("", 2), "{err}");
    // With no --project, a project-scope key outside any project has no file.
    let (_, err, code) = run_from(&f, "scratch", &["settings", "set", "ways.project.itops/incident", "false"], None);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("no project encloses <ROOT>/scratch; pass --project <dir>"), "{err}");
    assert!(!scratch.join(".claude").exists(), "no .claude/ is made in a directory that is not a project's");
}

// The attend mod and keepwarm read `list mods.attend --effective --project
// <session dir>`, and a session may start in the home directory or a
// scratch one: a read there has no project layer and keeps the user's.
#[test]
fn a_read_of_a_project_path_that_is_no_project_keeps_the_user_layer() {
    let f = Fx::new();
    f.write(&f.user(), "mods:\n  attend:\n    listening: active\n");
    let scratch = f.root.join("scratch");
    std::fs::create_dir_all(&scratch).unwrap();
    let home = f.root.join("home");
    for at in [scratch.to_str().unwrap(), home.to_str().unwrap(), "nope"] {
        let args = ["settings", "list", "mods.attend", "--json", "--effective", "--project", at];
        let (out, err, code) = run_from(&f, "home", &args, None);
        assert_eq!(code, 0, "{at}: {err}");
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        let k = &v["keys"]["mods.attend.listening"];
        assert_eq!((k["value"].as_str(), k["layer"].as_str()), (Some("active"), Some("user")), "{at}: {k}");
        assert_eq!(run_from(&f, "home", &["settings", "get", "mods.attend.listening", "--project", at], None).0, "active\n", "{at}");
    }
    if !outside_any_project(&f) {
        return;
    }
    let at = scratch.to_str().unwrap();
    for args in [vec!["settings", "lint", "--project", at], vec!["settings", "emit", "mods.attend", "--effective", "--project", at]] {
        let (_, err, code) = run_from(&f, "home", &args, None);
        assert_eq!(code, 0, "{args:?}: {err}");
    }
    // The screens open, with a project's keys not applicable and how a
    // session starts left where it is.
    let (out, err, code) = run_from(&f, "home", &["settings", "mod", "--snap", "100x30", "--project", at], None);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("writes user — <ROOT>/scratch"), "{out}");
    let (out, err, code) = run_from(&f, "home", &["settings", "mod", "--snap", "160x30", "--keys", "down", "--project", at], None);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("not applicable"), "{out}");
    assert!(!scratch.join(".claude").exists());
}

// The settings resolver walks to the nearest marker root before it takes a
// git checkout: a vendored checkout inside a project is the project's.
#[test]
fn a_nested_checkout_under_a_project_resolves_to_the_project() {
    let f = Fx::new();
    std::fs::create_dir_all(f.root.join("proj/vendor/lib/.git")).unwrap();
    std::fs::create_dir_all(f.root.join("proj/vendor/lib/x")).unwrap();
    let (_, err, code) = run_from(&f, "proj/vendor/lib/x", &["settings", "set", "mods.attend.keepwarm", "off", "--project", "."], None);
    assert_eq!((err.as_str(), code), ("project: <ROOT>/proj/.claude/ways.yaml\n", 0));
    assert!(!f.root.join("proj/vendor/lib/.claude").exists());
}

// A fresh repository with no .claude/ markers takes its first project write
// at its top level.
#[test]
fn a_repository_with_no_markers_resolves_to_its_top_level() {
    let f = Fx::new();
    if !outside_any_project(&f) {
        return;
    }
    std::fs::create_dir_all(f.root.join("repo/.git")).unwrap();
    std::fs::create_dir_all(f.root.join("repo/src")).unwrap();
    let (_, err, code) = run_from(&f, "repo/src", &["settings", "set", "mods.attend.keepwarm", "off", "--project", "."], None);
    assert_eq!((err.as_str(), code), ("project: <ROOT>/repo/.claude/ways.yaml\n", 0));
}

#[test]
fn a_project_write_reads_back_from_the_project_layer() {
    let f = Fx::new();
    let set = run_from(&f, "proj/tools", &["settings", "set", "mods.attend.keepwarm", "off", "--project", "."], None);
    assert_eq!(set.2, 0, "{}", set.1);
    let (out, err, code) = run_from(&f, "proj/tools", &["settings", "list", "mods.attend", "--effective", "--json", "--project", "."], None);
    assert_eq!(code, 0, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    let k = &v["keys"]["mods.attend.keepwarm"];
    assert_eq!((k["value"].as_str(), k["layer"].as_str()), (Some("off"), Some("project")), "{k}");
    assert_eq!(k["file"].as_str(), Some("<ROOT>/proj/.claude/ways.yaml"), "{k}");
    // Without --project, the working directory's project is read the same way.
    let (out, _, _) = run_from(&f, "proj/tools", &["settings", "get", "mods.attend.keepwarm"], None);
    assert_eq!(out, "off\n");
    // And a project-scope key written from the subdirectory lands at the root.
    let (_, err, code) = run_from(&f, "proj/tools", &["settings", "set", "ways.project.itops/incident", "false"], None);
    assert_eq!((err.as_str(), code), ("project: <ROOT>/proj/.claude/ways.yaml\n", 0));
}

// ── the mods section and the retired attend_mod (ADR-512 items 1 and 2) ──

#[test]
fn mods_lists_each_mod_and_attend_mod_names_no_section() {
    let f = Fx::new();
    let (out, err, code) = f.run(&["settings", "list", "mods.attend", "--json", "--effective"]);
    assert_eq!(code, 0, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    for k in ["mods.attend.listening", "mods.attend.keepwarm", "mods.attend.install", "mods.attend.keepalive.enabled"] {
        assert!(v["keys"][k].is_object(), "{k}: {out}");
    }
    assert!(v["keys"]["mods.ways-judge.install"].is_null(), "mods.attend is attend's alone: {out}");
    let (out, _, code) = f.run(&["settings", "list", "mods"]);
    assert_eq!(code, 0);
    assert!(out.contains("mods.attend.listening=dormant\n") && out.contains("mods.ways-judge.install=false\n"), "{out}");
    assert_eq!(f.run(&["settings", "get", "mods.ways-judge.install"]).0, "false\n");
    assert_eq!(f.run(&["settings", "list", "attend_mod"]).2, 2);
    let (out, _, code) = f.run(&["settings", "help", "mods.attend"]);
    assert_eq!(code, 0);
    assert!(out.contains("mods.attend.listening") && !out.contains("mods.ways-judge.install"), "{out}");
}

#[test]
fn the_theme_mod_s_render_settings_are_set_like_every_other_key() {
    let f = Fx::new();
    let cases = [
        ("rail", "true", "false"),
        ("tables", "on", "text"),
        ("shimmer", "true", "false"),
        ("charts", "true", "false"),
        ("markdown", "true", "false"),
        ("icons", "unicode", "ascii"),
        ("frames", "true", "false"),
        ("prompt_box", "gradient", "off"),
        ("copy", "true", "false"),
        ("paths", "wrap", "truncate"),
        ("spinner_words", "[]", "Sailing, Docking"),
        ("done_words", "[]", "Docked"),
    ];
    for (k, default, to) in cases {
        let key = format!("mods.ways-theme.{k}");
        assert_eq!(f.run(&["settings", "get", &key]).0.trim(), default, "{key}");
        let (_, err, code) = f.run(&["settings", "set", &key, to]);
        assert_eq!(code, 0, "{key}: {err}");
    }
    let (out, _, _) = f.run(&["settings", "get", "mods.ways-theme.spinner_words", "--json"]);
    assert!(out.contains("Sailing") && out.contains("Docking"), "{out}");
    assert_eq!(f.run(&["settings", "get", "mods.ways-theme.prompt_box"]).0, "off\n");
    // Refused, nothing written: a value off the list, a word too long, thirteen words.
    let before = std::fs::read_to_string(f.user()).unwrap();
    assert_eq!(f.run(&["settings", "set", "mods.ways-theme.prompt_box", "neon"]).2, 3);
    assert_eq!(f.run(&["settings", "set", "mods.ways-theme.done_words", &"x".repeat(25)]).2, 3);
    assert_eq!(f.run(&["settings", "set", "mods.ways-theme.spinner_words", &vec!["a"; 13].join(",")]).2, 3);
    assert_eq!(std::fs::read_to_string(f.user()).unwrap(), before);
}

#[test]
fn a_file_holding_attend_mod_is_linted_with_its_fix_and_moved_by_fix() {
    let f = Fx::new();
    f.write(&f.user(), "# mine\nattend_mod:\n  listening: active\n");
    // Nothing reads it: the session starts dormant until it is moved.
    assert_eq!(f.run(&["settings", "get", "mods.attend.listening"]).0, "dormant\n");
    let (out, err, code) = f.run(&["settings", "lint"]);
    assert_eq!(code, 3, "{out}{err}");
    assert_eq!(
        out,
        "<ROOT>/xdg/config/agent-ways/config.yaml:2: attend_mod: `attend_mod` was retired by ADR-512 and is ignored; its keys are now under `mods.attend`, and `ways settings fix mods.attend` moves them\n"
    );
    assert_eq!(std::fs::read_to_string(f.user()).unwrap(), "# mine\nattend_mod:\n  listening: active\n", "lint writes nothing");
    let (_, err, code) = f.run(&["settings", "fix", "mods.attend"]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(std::fs::read_to_string(f.user()).unwrap(), "# mine\nmods:\n  attend:\n    listening: active\n");
    assert_eq!(f.run(&["settings", "get", "mods.attend.listening"]).0, "active\n");
    assert_eq!(f.run(&["settings", "lint"]).2, 0);
}

#[test]
fn fix_keeps_a_value_already_under_mods_attend_and_names_the_one_it_dropped() {
    let f = Fx::new();
    let p = f.root.join("proj");
    f.write(&f.overlay(), "mods:\n  attend:\n    listening: dormant\nattend_mod:\n  listening: active\n  keepwarm: off\n");
    let (out, _, code) = f.run(&["settings", "lint", "--project", p.to_str().unwrap()]);
    assert_eq!(code, 3);
    let fix = format!("`ways settings fix mods.attend --project {}` moves them", f.word(&p));
    assert!(out.contains("<ROOT>/proj/.claude/ways.yaml:4: attend_mod: ") && out.contains(&fix), "{out}");
    let (_, err, code) = f.run(&["settings", "fix", "mods.attend", "--project", p.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("dropped attend_mod.listening: mods.attend.listening already holds a value in <ROOT>/proj/.claude/ways.yaml, which stays"), "{err}");
    assert_eq!(parsed(&std::fs::read_to_string(f.overlay()).unwrap()), parsed("mods: {attend: {listening: dormant, keepwarm: 'off'}}"));
}

#[test]
fn lint_names_every_target_and_known_project_holding_attend_mod() {
    let f = Fx::new();
    let target = f.root.join("home/.claude");
    let tcfg = f.root.join("tcfg/config.yaml");
    f.write(&f.user(), &format!("targets:\n  - path: {}\n    config: {}\n", target.display(), tcfg.display()));
    f.write(&tcfg, "attend_mod:\n  install: false\n");
    // A project Claude Code knows, other than the one the command runs in.
    let other = f.root.join("other");
    f.write(&other.join(".claude/ways.yaml"), "attend_mod:\n  listening: active\n");
    f.write(&other.join("CLAUDE.md"), "");
    // Named as Claude Code names a project's transcripts directory, with
    // the cwd JSON-escaped (a Windows path holds `\`).
    let slug = claude_sessions::project_slug(&other.to_string_lossy());
    let record = serde_json::json!({ "cwd": other.to_string_lossy() });
    f.write(&target.join("projects").join(&slug).join("s.jsonl"), &format!("{record}\n"));
    let (out, err, code) = f.run(&["settings", "lint"]);
    assert_eq!(code, 3, "{out}{err}");
    let fix = format!("`ways settings fix mods.attend --target {}` moves them", f.word(&target));
    assert!(out.contains("<ROOT>/tcfg/config.yaml:1: attend_mod: ") && out.contains(&fix), "{out}");
    let fix = format!("`ways settings fix mods.attend --project {}` moves them", f.word(&other));
    assert!(out.contains("<ROOT>/other/.claude/ways.yaml:1: attend_mod: ") && out.contains(&fix), "{out}");
    let (_, err, code) = f.run(&["settings", "fix", "mods.attend", "--target", target.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(err, "target: <ROOT>/tcfg/config.yaml\n");
    assert_eq!(std::fs::read_to_string(&tcfg).unwrap(), "mods:\n  attend:\n    install: false\n");
    let (_, err, code) = f.run(&["settings", "fix", "mods.attend", "--project", other.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(f.run(&["settings", "lint"]).2, 0);
    assert_eq!(f.run(&["settings", "fix", "mods.attend", "--target", f.root.join("nope").to_str().unwrap()]).2, 2);
}

// ── review of the settings rename (3b5dff7b) ───────────────────

/// Finding 5: `lint --file` on a project's `ways.yaml` names that project's
/// fix command.
#[test]
fn lint_of_one_project_file_names_its_project_fix() {
    let f = Fx::new();
    f.write(&f.overlay(), "attend_mod:\n  listening: active\n");
    let (out, _, code) = f.run(&["settings", "lint", "--file", f.overlay().to_str().unwrap()]);
    assert_eq!(code, 3, "{out}");
    let fix = format!("`ways settings fix mods.attend --project {}` moves them", f.word(&f.root.join("proj")));
    assert!(out.contains(&fix), "{out}");
    assert_eq!(out.matches("attend_mod").count(), 2, "one finding, not two: {out}");
}

/// Finding 4: a section moved where `mods` is absent keeps every comment in
/// place, inline ones included.
#[test]
fn fix_moves_the_section_with_its_comments() {
    let f = Fx::new();
    f.write(&f.user(), "# top\nattend_mod:\n  # why active\n  listening: active # inline\n  keepalive:\n    # weekdays only\n    windows: [\"Mon-Fri 08:00-19:00\"]\nother: 1\n");
    let (_, err, _) = f.run(&["settings", "fix", "mods.attend"]);
    assert_eq!(
        std::fs::read_to_string(f.user()).unwrap(),
        "# top\nmods:\n  attend:\n    # why active\n    listening: active # inline\n    keepalive:\n      # weekdays only\n      windows: [\"Mon-Fri 08:00-19:00\"]\nother: 1\n",
        "{err}"
    );
}

/// Finding 4, the other case: where `mods` exists the values move one by
/// one, the comments inside the old section go with it, and fix says so.
#[test]
fn fix_into_an_existing_mods_section_drops_the_comments_and_says_so() {
    let f = Fx::new();
    f.write(&f.user(), "mods:\n  ways-judge:\n    install: false\nattend_mod:\n  # why active\n  listening: active\nother: 1\n");
    let (_, err, code) = f.run(&["settings", "fix", "mods.attend"]);
    assert_eq!(code, 0, "{err}");
    let text = std::fs::read_to_string(f.user()).unwrap();
    assert!(!text.contains("# why active"), "{text}");
    assert_eq!(parsed(&text), parsed("mods: {ways-judge: {install: false}, attend: {listening: active}}\nother: 1"));
    assert!(err.contains("the comments inside attend_mod"), "{err}");
}

/// Findings 6 and 7: each dropped value says why.
#[test]
fn fix_says_why_each_value_was_dropped() {
    let f = Fx::new();
    f.write(&f.user(), "attend_mod: true\n");
    let (_, err, code) = f.run(&["settings", "fix", "mods.attend"]);
    assert_eq!(code, 0, "{err}");
    assert!(err.contains("dropped attend_mod: it is `true`, not a section of keys") && !err.contains("already holds"), "{err}");
    f.write(&f.user(), "mods:\n  attend:\n    keepalive: off\nattend_mod:\n  keepalive:\n    enabled: true\n");
    let (_, err, _) = f.run(&["settings", "fix", "mods.attend"]);
    assert!(err.contains("dropped attend_mod.keepalive.enabled: mods.attend.keepalive is `off`"), "{err}");
}

/// Finding 9: a mod the app does not ship is no section to fix.
#[test]
fn fix_of_an_unknown_mod_is_a_usage_error() {
    let f = Fx::new();
    let (_, err, code) = f.run(&["settings", "fix", "mods.nosuch"]);
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("attend") && err.contains("ways-judge"), "{err}");
}

/// Finding 1: the hidden fixed-name report an updater runs on the newly
/// installed binary: one line per file holding the retired section.
#[test]
fn the_retired_settings_report_names_each_file_and_its_fix() {
    let f = Fx::new();
    let (out, _, code) = f.run(&["retired-settings"]);
    assert_eq!((out.as_str(), code), ("", 0), "nothing held, nothing said");
    f.write(&f.user(), "attend_mod:\n  listening: active\n");
    let (out, _, code) = f.run(&["retired-settings"]);
    assert_eq!(code, 0);
    assert_eq!(out, "<ROOT>/xdg/config/agent-ways/config.yaml:1 holds attend_mod, now mods.attend: `ways settings fix mods.attend` moves it\n");
}
