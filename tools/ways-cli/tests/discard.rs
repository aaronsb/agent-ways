//! A switched-off way does not fire on any lane, whatever switched it off.
//!
//! Each test builds its own home (ways under `~/.claude/hooks/ways`, user
//! config, XDG state) and project, so the event log it reads is its own.
//! The helpers are copied from `session_sim.rs`, which has no shared module.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// ── Harness (copied from session_sim.rs) ──────────────────────

/// Mirror of `session::sessions_root()`; see the note in `session_sim.rs`.
fn sessions_root() -> String {
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
        return format!("{xdg}/claude-sessions");
    }
    #[cfg(windows)]
    {
        let base = std::env::var("LOCALAPPDATA")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned());
        format!("{base}/claude-ways/sessions")
    }
    #[cfg(not(windows))]
    {
        let uid = Command::new("id").arg("-u").output().ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "0".to_string());
        format!("/tmp/.claude-sessions-{uid}")
    }
}

fn ways_bin() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    path.pop();
    path.push("ways");
    if !path.exists() {
        path = PathBuf::from(env!("CARGO_BIN_EXE_ways"));
    }
    path
}

/// A `ways` command isolated from the operator's install and session, run in
/// `project` as Claude Code would run it there.
fn ways_cmd(home: &Path, project: &Path) -> Command {
    let mut cmd = Command::new(ways_bin());
    cmd.current_dir(project)
        .env("PWD", project)
        .env("CLAUDE_PROJECT_DIR", project)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_BIN_HOME", home.join(".local/bin"));
    for var in ["CLAUDE_AGENT_ID", "CLAUDE_SESSION_ID", "CLAUDE_CODE_SESSION_ID", "CLAUDE_CONFIG_DIR", "WAYS_AGENT_SOCK", "WAYS_CLAUDE_BIN"] {
        cmd.env_remove(var);
    }
    cmd
}

fn clean_markers(session_id: &str) {
    let _ = std::fs::remove_dir_all(format!("{}/{session_id}", sessions_root()));
}

/// Install a stub engine in `engine`: a `way-embed` that prints `rows` for
/// `match` and fails everything else, an English model file, a corpus naming
/// `ids`, and a manifest carrying the test calibration g(s) = σ(10·s − 2.5).
#[cfg(unix)]
fn install_stub_engine(engine: &Path, ids: &[&str], rows: &[(&str, f64)]) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(engine).unwrap();
    let printed: String = rows.iter().map(|(id, c)| format!("printf '%s\\t%s\\n' '{id}' '{c}'\n")).collect();
    let stub = engine.join("way-embed");
    std::fs::write(&stub, format!("#!/bin/sh\n[ \"$1\" = match ] || exit 2\n{printed}")).unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(engine.join("minilm-l6-v2.gguf"), b"stub").unwrap();
    let corpus: String = ids
        .iter()
        .map(|id| format!("{}\n", serde_json::json!({"id": id, "description": "d", "vocabulary": "v", "embed_model": "en"})))
        .collect();
    std::fs::write(engine.join("ways-corpus-en.jsonl"), corpus).unwrap();
    std::fs::write(
        engine.join("embed-manifest.json"),
        serde_json::json!({"calibration": {"en": {"a": 10.0, "b": -2.5, "auc": 1.0, "n": 0}}}).to_string(),
    )
    .unwrap();
    // Retry while the kernel reports the fresh stub busy (ETXTBSY).
    for attempt in 0.. {
        match Command::new(&stub).arg("match").output() {
            Ok(o) => {
                assert!(o.status.success(), "the stub runs: {o:?}");
                break;
            }
            Err(e) if e.raw_os_error() == Some(26) && attempt < 50 => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(e) => panic!("the stub engine does not run: {e}"),
        }
    }
}

// ── Fixture ────────────────────────────────────────────────────

/// What switches `lab/*` off.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Source {
    /// Nothing: every way is on.
    None,
    /// `ways: {lab/<way>: false}` in the project's `.claude/ways.yaml`.
    ProjectWayKey,
    /// `disabled_domains: [lab]` in the project's `.claude/ways.yaml`.
    ProjectDomain,
    /// `disabled_domains: [lab]` in the user config.
    UserDomain,
    /// `enabled: false` in the project's `.claude/ways.yaml`.
    ProjectOff,
}

const SOURCES: [Source; 4] = [Source::ProjectWayKey, Source::ProjectDomain, Source::UserDomain, Source::ProjectOff];

/// A way that fires on every lane: the prompt and task keyword, a command, a
/// file and a tool name, each spelled with `word`.
fn write_way(root: &Path, id: &str, word: &str) {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    let name = id.rsplit('/').next().unwrap();
    std::fs::write(
        dir.join(format!("{name}.md")),
        format!(
            "---\ndescription: {name} guidance\nvocabulary: {name} words\npattern: \\b{word}\\b\ncommands: ^{word}ctl\\b\nfiles: \\.{word}$\ntool_name: ^{word}tool$\nscope: agent, subagent\nrefire: 0.15\n---\n# Marker {id}\n"
        ),
    )
    .unwrap();
}

/// A way with no keyword hit, for the semantic lane.
fn write_semantic_way(root: &Path, id: &str) {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    let name = id.rsplit('/').next().unwrap();
    std::fs::write(
        dir.join(format!("{name}.md")),
        format!("---\ndescription: {name} guidance\nvocabulary: {name} words\npattern: \\bzzzneverzzz\\b\nscope: agent, subagent\nrefire: 0.15\n---\n# Marker {id}\n"),
    )
    .unwrap();
}

struct Fixture {
    base: PathBuf,
    home: PathBuf,
    project: PathBuf,
    session: String,
}

/// Ways in the keyword fixture: `lab/kw` is the one switched off, `ctl/kw`
/// the control that stays on (except under [`Source::ProjectOff`]).
const KEYWORD_WAYS: [(&str, &str); 2] = [("lab/kw", "widget"), ("ctl/kw", "gadget")];

/// Ways in the semantic fixture and the cosines the stub engine scores them
/// at: 0.40 maps to p ≈ 0.82 and fires; 0.10 maps to p ≈ 0.18 and does not.
const SEMANTIC_ROWS: [(&str, f64); 3] = [("lab/sem", 0.40), ("ctl/sem", 0.40), ("lab/cold", 0.10)];

impl Fixture {
    fn new(tag: &str, source: Source) -> Self {
        Self::build(tag, source, false, &[])
    }

    fn semantic(tag: &str, source: Source) -> Self {
        Self::build(tag, source, true, &[])
    }

    /// A fixture with the ways in `absent` deleted from the corpus.
    fn build(tag: &str, source: Source, semantic: bool, absent: &[&str]) -> Self {
        let base = std::env::temp_dir().join(format!("ways-discard-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let home = base.join("home");
        let project = base.join("project");
        std::fs::create_dir_all(project.join(".claude")).unwrap();
        let ways = home.join(".claude/hooks/ways");
        if semantic {
            for (id, _) in SEMANTIC_ROWS {
                if !absent.contains(&id) {
                    write_semantic_way(&ways, id);
                }
            }
            #[cfg(unix)]
            {
                let rows: Vec<(&str, f64)> = SEMANTIC_ROWS.iter().copied().filter(|(id, _)| !absent.contains(id)).collect();
                let ids: Vec<&str> = rows.iter().map(|(id, _)| *id).collect();
                install_stub_engine(&home.join(".cache/agent-ways/user"), &ids, &rows);
            }
        } else {
            for (id, word) in KEYWORD_WAYS {
                if !absent.contains(&id) {
                    write_way(&ways, id, word);
                }
            }
        }
        let off_way = if semantic { "lab/sem" } else { "lab/kw" };
        let project_yaml = match source {
            Source::ProjectWayKey => format!("ways:\n  {off_way}: false\n"),
            Source::ProjectDomain => "disabled_domains: [lab]\n".to_string(),
            Source::ProjectOff => "enabled: false\n".to_string(),
            Source::None | Source::UserDomain => String::new(),
        };
        std::fs::write(project.join(".claude/ways.yaml"), project_yaml).unwrap();
        if source == Source::UserDomain {
            let cfg = home.join(".config/agent-ways");
            std::fs::create_dir_all(&cfg).unwrap();
            std::fs::write(cfg.join("config.yaml"), "disabled_domains: [lab]\n").unwrap();
        }
        let session = format!("discard-{tag}-{}", std::process::id());
        clean_markers(&session);
        Fixture { base, home, project, session }
    }

    /// Run `ways scan <args>` for this session and project; return stdout.
    fn scan(&self, args: &[&str]) -> String {
        let out = ways_cmd(&self.home, &self.project)
            .arg("scan")
            .args(args)
            .args(["--session", &self.session, "--project"])
            .arg(&self.project)
            .output()
            .expect("Failed to run ways scan");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    /// Run `ways hook tool` for `tool` at PreToolUse; return stdout.
    fn hook_tool(&self, tool: &str) -> String {
        use std::io::Write;
        let mut child = ways_cmd(&self.home, &self.project)
            .args(["hook", "tool"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to run ways hook tool");
        let payload = format!(r#"{{"session_id":"{}","hook_event_name":"PreToolUse","tool_name":"{tool}"}}"#, self.session);
        child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    /// Every event named `event` this session logged, in order.
    fn events(&self, event: &str) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.home.join(".local/state/agent-ways/events.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter(|v| v["event"] == event && v["session"] == self.session.as_str())
            .collect()
    }

    /// The ways that fired, sorted.
    fn fired(&self) -> Vec<String> {
        let mut ways: Vec<String> = self.events("way_fired").iter().map(|v| v["way"].as_str().unwrap_or("").to_string()).collect();
        ways.sort();
        ways
    }

    /// The ways the task lane stashed for the next subagent, sorted.
    fn stashed(&self) -> Vec<String> {
        let dir = PathBuf::from(format!("{}/{}/subagent-stash", sessions_root(), self.session));
        let mut ways: Vec<String> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .filter_map(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
            .flat_map(|v| v["ways"].as_array().cloned().unwrap_or_default())
            .filter_map(|w| w.as_str().map(str::to_string))
            .collect();
        ways.sort();
        ways
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        clean_markers(&self.session);
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// The control way's expected fires: none when the whole project is off.
fn control(source: Source, id: &str) -> Vec<String> {
    if source == Source::ProjectOff { Vec::new() } else { vec![id.to_string()] }
}

/// Run one lane against every source: `drive` runs the lane in the fixture
/// and returns the ways it fired, which must be the control alone.
fn each_source(lane: &str, drive: impl Fn(&Fixture) -> Vec<String>) {
    let all = drive(&Fixture::new(&format!("{lane}-on"), Source::None));
    assert_eq!(all, ["ctl/kw", "lab/kw"], "{lane}: with nothing switched off, both ways fire");
    for source in SOURCES {
        let f = Fixture::new(&format!("{lane}-{source:?}"), source);
        assert_eq!(drive(&f), control(source, "ctl/kw"), "{lane} under {source:?}");
    }
}

// ── Step 1: a switched-off way does not fire ──────────────────

#[test]
fn prompt_keyword_lane_does_not_fire_a_switched_off_way() {
    each_source("prompt", |f| {
        f.scan(&["prompt", "--query", "please build the widget and the gadget"]);
        f.fired()
    });
}

#[cfg(unix)]
#[test]
fn prompt_semantic_lane_does_not_fire_a_switched_off_way() {
    let all = Fixture::semantic("sem-on", Source::None);
    all.scan(&["prompt", "--query", "tell me how this part of the system works"]);
    assert_eq!(all.fired(), ["ctl/sem", "lab/sem"], "with nothing switched off, both ways fire semantically");
    for source in SOURCES {
        let f = Fixture::semantic(&format!("sem-{source:?}"), source);
        f.scan(&["prompt", "--query", "tell me how this part of the system works"]);
        assert_eq!(f.fired(), control(source, "ctl/sem"), "semantic lane under {source:?}");
    }
}

#[test]
fn command_lane_does_not_fire_a_switched_off_way() {
    each_source("command", |f| {
        f.scan(&["command", "--command", "widgetctl run && gadgetctl run"]);
        f.fired()
    });
}

#[test]
fn file_lane_does_not_fire_a_switched_off_way() {
    each_source("file", |f| {
        f.scan(&["file", "--path", "/src/a.widget"]);
        f.scan(&["file", "--path", "/src/a.gadget"]);
        f.fired()
    });
}

#[test]
fn tool_lane_does_not_fire_a_switched_off_way() {
    each_source("tool", |f| {
        f.hook_tool("WidgetTool");
        f.hook_tool("GadgetTool");
        f.fired()
    });
}

#[test]
fn task_lane_does_not_stash_a_switched_off_way() {
    each_source("task", |f| {
        f.scan(&["task", "--query", "please build the widget and the gadget"]);
        f.stashed()
    });
}
