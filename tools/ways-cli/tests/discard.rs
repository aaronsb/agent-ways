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
/// the control that stays on (except under [`Source::ProjectOff`]), and
/// `lab/quiet` a way no surface here matches.
const KEYWORD_WAYS: [(&str, &str); 3] = [("lab/kw", "widget"), ("ctl/kw", "gadget"), ("lab/quiet", "sprocket")];

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

    /// The ways delivered to the session, sorted: fires without a
    /// `discarded` object.
    fn fired(&self) -> Vec<String> {
        let mut ways: Vec<String> = self
            .events("way_fired")
            .iter()
            .filter(|v| v.get("discarded").is_none())
            .map(|v| v["way"].as_str().unwrap_or("").to_string())
            .collect();
        ways.sort();
        ways
    }

    /// The discarded fires, as `(way, filter, toggle, scope, lane)`, sorted.
    fn discarded(&self) -> Vec<Discarded> {
        let s = |v: &serde_json::Value| v.as_str().map(str::to_string);
        let mut rows: Vec<Discarded> = self
            .events("way_fired")
            .iter()
            .filter(|v| v.get("discarded").is_some())
            .map(|v| {
                let d = &v["discarded"];
                (s(&v["way"]).unwrap_or_default(), s(&d["filter"]).unwrap_or_default(), s(&d["toggle"]), s(&d["scope"]), s(&v["lane"]).unwrap_or_default())
            })
            .collect();
        rows.sort();
        rows
    }

    /// The one discarded fire of `way`.
    fn discarded_event(&self, way: &str) -> serde_json::Value {
        let rows: Vec<serde_json::Value> = self.events("way_fired").into_iter().filter(|v| v["way"] == way && v.get("discarded").is_some()).collect();
        assert_eq!(rows.len(), 1, "one discarded fire of {way}: {rows:#?}");
        rows.into_iter().next().unwrap()
    }

    /// Run `ways <args>` in this fixture; return stdout.
    fn ways(&self, args: &[&str]) -> String {
        let out = ways_cmd(&self.home, &self.project).args(args).output().expect("Failed to run ways");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
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

/// A discarded fire as the tests compare it: way, filter, toggle, scope, lane.
type Discarded = (String, String, Option<String>, Option<String>, String);

/// The discarded fires `source` should log for `ways` on `lane`.
fn expected(source: Source, lane: &str, ways: &[&str]) -> Vec<Discarded> {
    let mut rows: Vec<Discarded> = ways
        .iter()
        .map(|way| {
            let (filter, toggle, scope) = match source {
                Source::ProjectWayKey => ("toggle", Some(way.to_string()), "project"),
                Source::ProjectDomain => ("domain", None, "project"),
                Source::UserDomain => ("domain", None, "user"),
                Source::ProjectOff => ("project_off", None, "project"),
                Source::None => unreachable!("nothing is switched off"),
            };
            (way.to_string(), filter.to_string(), toggle, Some(scope.to_string()), lane.to_string())
        })
        .collect();
    rows.sort();
    rows
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

// ── Step 2: a switched-off way that would have fired is logged ─────

/// The ways each source discards on a keyword lane: under `enabled: false`
/// the control is off too. `lab/quiet` matches nothing and never appears.
fn keyword_discards(source: Source, lane: &str) -> Vec<Discarded> {
    match source {
        Source::ProjectOff => expected(source, lane, &["ctl/kw", "lab/kw"]),
        _ => expected(source, lane, &["lab/kw"]),
    }
}

/// Run one keyword lane under every source and check its discarded fires.
fn each_source_discards(lane: &str, drive: impl Fn(&Fixture)) {
    let on = Fixture::new(&format!("{lane}-log-on"), Source::None);
    drive(&on);
    assert!(on.discarded().is_empty(), "{lane}: nothing is switched off, nothing is discarded");
    for source in SOURCES {
        let f = Fixture::new(&format!("{lane}-log-{source:?}"), source);
        drive(&f);
        assert_eq!(f.discarded(), keyword_discards(source, lane), "{lane} under {source:?}");
    }
}

#[test]
fn prompt_keyword_lane_logs_a_discarded_fire() {
    each_source_discards("prompt", |f| {
        f.scan(&["prompt", "--query", "please build the widget and the gadget"]);
    });
    let f = Fixture::new("prompt-span", Source::ProjectDomain);
    f.scan(&["prompt", "--query", "please build the widget and the gadget"]);
    let ev = f.discarded_event("lab/kw");
    assert_eq!((ev["trigger"].as_str(), ev["matched_span"].as_str()), (Some("keyword"), Some("widget")), "{ev}");
}

#[test]
fn command_lane_logs_a_discarded_fire() {
    each_source_discards("command", |f| {
        f.scan(&["command", "--command", "widgetctl run && gadgetctl run"]);
    });
}

#[test]
fn file_lane_logs_a_discarded_fire() {
    each_source_discards("file", |f| {
        f.scan(&["file", "--path", "/src/a.widget"]);
        f.scan(&["file", "--path", "/src/a.gadget"]);
    });
}

#[test]
fn tool_lane_logs_a_discarded_fire() {
    each_source_discards("tool", |f| {
        f.hook_tool("WidgetTool");
        f.hook_tool("GadgetTool");
    });
}

#[test]
fn task_lane_logs_a_discarded_fire() {
    each_source_discards("task", |f| {
        f.scan(&["task", "--query", "please build the widget and the gadget"]);
    });
}

/// The semantic lane logs `lab/sem` with the scores of the single-vector
/// candidate log had it competed: cosine 0.40 beside `ctl/sem` at 0.40, and
/// under the toggle `lab/cold` at 0.10 too. `lab/cold` would not have fired
/// and logs nothing. A project switched off embeds nothing, so its semantic
/// lane logs nothing.
#[cfg(unix)]
#[test]
fn prompt_semantic_lane_logs_a_discarded_fire_with_its_scores() {
    for source in SOURCES {
        let f = Fixture::semantic(&format!("sem-log-{source:?}"), source);
        f.scan(&["prompt", "--query", "tell me how this part of the system works"]);
        if source == Source::ProjectOff {
            assert!(f.discarded().is_empty(), "no semantic signal under enabled: false: {:?}", f.discarded());
            continue;
        }
        assert_eq!(f.discarded(), expected(source, "prompt", &["lab/sem"]), "semantic lane under {source:?}");
        let ev = f.discarded_event("lab/sem");
        assert_eq!(ev["trigger"], "semantic:embedding:en", "{ev}");
        assert_eq!(ev["cosine"].as_f64(), Some(0.4), "{ev}");
        let (share, margin) = (ev["share"].as_f64().unwrap(), ev["margin"].as_f64());
        if source == Source::ProjectWayKey {
            let want = (5.0f64).exp() / (2.0 * (5.0f64).exp() + (1.25f64).exp());
            assert!((share - want).abs() < 1e-3, "share among ctl/sem and lab/cold: {ev}");
            assert_eq!(margin, Some(0.3), "lab/cold is next below: {ev}");
        } else {
            assert_eq!(share, 0.5, "an even split with ctl/sem: {ev}");
            assert_eq!(margin, None, "nothing enabled below it: {ev}");
        }
    }
}

/// Add a check to `lab/kw` firing on its command, and a state way `lab/st`
/// triggered by a file the project holds.
fn add_check_and_state_ways(f: &Fixture) {
    let ways = f.home.join(".claude/hooks/ways");
    std::fs::write(
        ways.join("lab/kw/kw.check.md"),
        "---\ndescription: kw check\nvocabulary: kw\ncommands: ^widgetctl\\b\nscope: agent\n---\n## anchor\n\nAnchor.\n\n## check\n\n- [ ] Checked\n",
    )
    .unwrap();
    std::fs::create_dir_all(ways.join("lab/st")).unwrap();
    std::fs::write(
        ways.join("lab/st/st.md"),
        "---\ndescription: st guidance\ntrigger: file-exists\npath: marker.txt\nscope: agent\nrefire: 0.15\n---\n# Marker lab/st\n",
    )
    .unwrap();
    std::fs::write(f.project.join("marker.txt"), "").unwrap();
}

/// A check refused for its switched-off way logs on the check lane once its
/// score clears the threshold, and a state way logs once per session however
/// often its condition holds.
#[test]
fn check_and_state_lanes_log_a_discarded_fire() {
    let on = Fixture::new("check-state-on", Source::None);
    add_check_and_state_ways(&on);
    on.scan(&["command", "--command", "widgetctl run"]);
    on.scan(&["state", "--hook-event", "UserPromptSubmit"]);
    assert_eq!(on.fired(), ["lab/kw", "lab/st"], "both fire when on");
    assert!(on.discarded().is_empty());

    let f = Fixture::new("check-state", Source::ProjectDomain);
    add_check_and_state_ways(&f);
    f.scan(&["command", "--command", "widgetctl run"]);
    f.scan(&["state", "--hook-event", "UserPromptSubmit"]);
    f.scan(&["state", "--hook-event", "UserPromptSubmit"]);
    let mut want = expected(Source::ProjectDomain, "check", &["lab/kw"]);
    want.extend(expected(Source::ProjectDomain, "command", &["lab/kw"]));
    want.extend(expected(Source::ProjectDomain, "state", &["lab/st"]));
    assert_eq!(f.discarded(), want);
    assert!(f.fired().is_empty(), "{:?}", f.fired());
}

/// The enabled ways see the same scan whether a way is switched off or gone:
/// the same ways fire and the hook prints the same bytes.
#[test]
fn enabled_ways_fire_the_same_with_a_way_switched_off_or_deleted() {
    type Drive = fn(&Fixture) -> String;
    let lanes: [(&str, Drive); 5] = [
        ("prompt", |f| f.scan(&["prompt", "--query", "please build the widget and the gadget"])),
        ("command", |f| f.scan(&["command", "--command", "widgetctl run && gadgetctl run"])),
        ("file", |f| f.scan(&["file", "--path", "/src/a.widget"]) + &f.scan(&["file", "--path", "/src/a.gadget"])),
        ("tool", |f| f.hook_tool("WidgetTool") + &f.hook_tool("GadgetTool")),
        ("task", |f| {
            f.scan(&["task", "--query", "please build the widget and the gadget"]);
            f.stashed().join(",")
        }),
    ];
    for (lane, drive) in lanes {
        for source in [Source::ProjectWayKey, Source::ProjectDomain, Source::UserDomain] {
            let off = Fixture::new(&format!("pc-{lane}-{source:?}"), source);
            let gone = Fixture::build(&format!("pc-{lane}-{source:?}-gone"), Source::None, false, &["lab/kw", "lab/quiet"]);
            let (off_out, gone_out) = (drive(&off), drive(&gone));
            assert!(!gone_out.is_empty(), "{lane}: the control fired and printed something");
            assert_eq!(off_out, gone_out, "{lane} under {source:?}: hook output");
            assert_eq!(off.fired(), gone.fired(), "{lane} under {source:?}: fired ways");
        }
    }
}

#[cfg(unix)]
#[test]
fn semantic_lane_fires_the_same_with_a_way_switched_off_or_deleted() {
    for source in [Source::ProjectWayKey, Source::ProjectDomain, Source::UserDomain] {
        let off = Fixture::semantic(&format!("pc-sem-{source:?}"), source);
        let absent: &[&str] = if source == Source::ProjectWayKey { &["lab/sem"] } else { &["lab/sem", "lab/cold"] };
        let gone = Fixture::build(&format!("pc-sem-{source:?}-gone"), Source::None, true, absent);
        let query = ["prompt", "--query", "tell me how this part of the system works"];
        let (off_out, gone_out) = (off.scan(&query), gone.scan(&query));
        assert!(!gone_out.is_empty(), "the control fired");
        assert_eq!(off_out, gone_out, "semantic lane under {source:?}: hook output");
        assert_eq!(off.fired(), gone.fired(), "semantic lane under {source:?}: fired ways");
    }
}

// ── Readers of fires ─────────────────────────────────────────

/// `ways tune stats` counts a discarded fire under its way with the filter,
/// beside the delivered fires, which do not include it.
#[test]
fn stats_count_a_discarded_fire_under_its_way_and_filter() {
    let f = Fixture::new("stats", Source::ProjectDomain);
    f.scan(&["prompt", "--query", "please build the widget and the gadget"]);
    let project = f.project.to_str().unwrap();
    let v: serde_json::Value = serde_json::from_str(&f.ways(&["tune", "stats", "--json", "--project", project])).unwrap();
    assert_eq!(v["way_fires"], 1, "only ctl/kw reached the session: {v:#}");
    assert_eq!(v["by_way"], serde_json::json!({"ctl/kw": 1}), "{v:#}");
    assert_eq!(v["discarded_fires"]["total"], 1, "{v:#}");
    assert_eq!(v["discarded_fires"]["by_filter"], serde_json::json!({"domain": 1}), "{v:#}");
    assert_eq!(v["discarded_fires"]["by_way"], serde_json::json!({"lab/kw": {"domain": 1}}), "{v:#}");
    let human = f.ways(&["tune", "stats", "--project", project]);
    let line = human.lines().find(|l| l.trim_start().starts_with("lab/kw")).unwrap_or_else(|| panic!("lab/kw listed: {human}"));
    assert!(line.contains("domain 1"), "the filter is shown: {line}");
    assert!(human.contains("Way fires: 1  |  Discarded: 1"), "{human}");
}

/// The session dump and the replay count what reached the session in their
/// fires and turns, and the discarded fires apart, by filter.
#[test]
fn session_views_keep_a_discarded_fire_out_of_their_fires() {
    let f = Fixture::new("dump", Source::ProjectWayKey);
    f.scan(&["prompt", "--query", "please build the widget and the gadget"]);
    let project = f.project.to_str().unwrap();

    let dump: serde_json::Value = serde_json::from_str(&f.ways(&["session", "dump", "--session", &f.session, "--project", project])).unwrap();
    let summary = &dump["summary"];
    assert_eq!((summary["total_fires"].as_u64(), summary["distinct_ways"].as_u64()), (Some(1), Some(1)), "{summary:#}");
    assert_eq!(summary["discarded"], serde_json::json!({"toggle": 1}), "{summary:#}");
    let rows: Vec<&str> = dump["turns"].as_array().unwrap().iter().flat_map(|t| t["fired_ways"].as_array().unwrap()).map(|w| w["way_id"].as_str().unwrap()).collect();
    assert_eq!(rows, ["ctl/kw"], "no turn carries the discarded fire: {dump:#}");

    let replay: serde_json::Value = serde_json::from_str(&f.ways(&["session", "replay", "--json", "--session", &f.session, "--project", project])).unwrap();
    let summary = &replay["summary"];
    assert_eq!(summary["total_fires"], 1, "{summary:#}");
    assert_eq!(summary["top_ways"], serde_json::json!([{"way": "ctl/kw", "fires": 1}]), "{summary:#}");
    assert_eq!(summary["discarded"], serde_json::json!({"toggle": 1}), "{summary:#}");
    let active: Vec<&str> = replay["frames"].as_array().unwrap().iter().flat_map(|fr| fr["active_ways"].as_array().unwrap()).map(|w| w["id"].as_str().unwrap_or("")).collect();
    assert!(!active.is_empty() && active.iter().all(|w| *w == "ctl/kw"), "only ctl/kw is in context: {active:?}");
}
