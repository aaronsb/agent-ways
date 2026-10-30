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

/// Install a late-interaction stub engine in `engine` (adapted from the one in
/// `session_sim.rs`): `match --batch` scores each chunk by the keyword it
/// names, from `chunks`; a single-query `match` scores every way at 0;
/// `similarity --batch` corroborates every pair at 0.9. With `sidecar`, a
/// body sidecar holds a record of no sections for every way in `ids`, so a
/// survivor confirms against its peak, and `match --vectors` returns a
/// vector per chunk.
#[cfg(unix)]
fn install_late_interaction_stub(engine: &Path, ids: &[&str], chunks: &[(&str, Vec<(&str, f64)>)], sidecar: bool) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(engine).unwrap();
    let arms: String = chunks
        .iter()
        .map(|(word, rows)| {
            let printed: String = rows.iter().map(|(id, c)| format!("printf '%s\\t%s\\t%s\\n' $i {id} {c}\n")).collect();
            format!("*{word}*)\n{printed};;\n")
        })
        .collect();
    let script = format!(
        "#!/bin/sh\n\
         case \"$1\" in\n\
         match)\n\
           case \" $* \" in *\" --batch \"*)\n\
             vec=0; case \" $* \" in *\" --vectors \"*) vec=1 ;; esac\n\
             i=0\n\
             while IFS= read -r line || [ -n \"$line\" ]; do\n\
               [ $vec = 1 ] && printf 'v\\t%s\\t1,0\\n' $i\n\
               case \"$line\" in\n{arms}esac\n\
               i=$((i+1))\n\
             done ;;\n\
           *) for id in {ids}; do printf '%s\\t%s\\n' $id 0.0; done ;;\n\
           esac ;;\n\
         similarity) while IFS= read -r line; do echo 0.9; done ;;\n\
         *) exit 2 ;;\n\
         esac\n",
        ids = ids.join(" "),
    );
    let stub = engine.join("way-embed");
    std::fs::write(&stub, script).unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(engine.join("minilm-l6-v2.gguf"), b"stub").unwrap();
    let corpus: String = ids
        .iter()
        .map(|id| format!("{}\n", serde_json::json!({"id": id, "description": "d", "vocabulary": "v", "embed_model": "en"})))
        .collect();
    std::fs::write(engine.join("ways-corpus-en.jsonl"), corpus).unwrap();
    let mut manifest = serde_json::json!({"calibration": {"en": {"a": 10.0, "b": -2.5, "auc": 1.0, "n": 0}}});
    if sidecar {
        manifest["way_hashes"] = ids.iter().map(|id| (id.to_string(), serde_json::json!("0000000000000001"))).collect::<serde_json::Map<_, _>>().into();
        manifest["body_sidecar"] = serde_json::json!({"vectors": true});
        // The sidecar's model id names the model file and this stub by size
        // and modification time (`sidecar::model_id`).
        let bin = std::fs::metadata(&stub).unwrap();
        let mtime = bin.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let model = format!("minilm-l6-v2.gguf:4|way-embed:{}:{mtime}|sections:1", bin.len());
        let put = |out: &mut Vec<u8>, s: &str| {
            out.extend_from_slice(&(s.len() as u16).to_le_bytes());
            out.extend_from_slice(s.as_bytes());
        };
        let mut bytes = b"WAYSBODY".to_vec();
        for n in [1u32, 2, ids.len() as u32, 0] {
            bytes.extend_from_slice(&n.to_le_bytes());
        }
        put(&mut bytes, &model);
        for id in ids {
            put(&mut bytes, id);
            bytes.extend_from_slice(&1u64.to_le_bytes());
            bytes.extend_from_slice(&0u32.to_le_bytes());
        }
        std::fs::write(engine.join("ways-body-en.bin"), bytes).unwrap();
    }
    std::fs::write(engine.join("embed-manifest.json"), manifest.to_string()).unwrap();
    for attempt in 0.. {
        match Command::new(&stub).arg("similarity").stdin(Stdio::null()).output() {
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
    write_way_with(root, id, word, "agent, subagent", "0.15");
}

/// [`write_way`] with its `scope:` and `refire:`.
fn write_way_with(root: &Path, id: &str, word: &str, scope: &str, refire: &str) {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    let name = id.rsplit('/').next().unwrap();
    std::fs::write(
        dir.join(format!("{name}.md")),
        format!(
            "---\ndescription: {name} guidance\nvocabulary: {name} words\npattern: \\b{word}\\b\ncommands: ^{word}ctl\\b\nfiles: \\.{word}$\ntool_name: ^{word}tool$\nscope: {scope}\nrefire: {refire}\n---\n# Marker {id}\n"
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
        let f = Self::base(tag, source, if semantic { "lab/sem" } else { "lab/kw" });
        let ways = f.ways_root();
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
                install_stub_engine(&f.engine(), &ids, &rows);
            }
        } else {
            for (id, word) in KEYWORD_WAYS {
                if !absent.contains(&id) {
                    write_way(&ways, id, word);
                }
            }
        }
        f
    }

    /// A fixture with no ways, whose `source` switches `off_way` off (or the
    /// `lab` domain).
    fn base(tag: &str, source: Source, off_way: &str) -> Self {
        let base = std::env::temp_dir().join(format!("ways-discard-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let home = base.join("home");
        let project = base.join("project");
        std::fs::create_dir_all(project.join(".claude")).unwrap();
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

    /// The late-interaction fixture: `ctl/alpha` and `ctl/beta` each win the
    /// chunk naming them, and `lab/off`, switched off, outscores `ctl/beta`
    /// in its chunk. With `gone`, `lab/off` is deleted from the corpus and
    /// the engine. With `sidecar`, a body sidecar covers every way.
    #[cfg(unix)]
    fn late(tag: &str, source: Source, gone: bool, sidecar: bool) -> Self {
        let f = Self::base(tag, source, "lab/off");
        let mut ids = vec!["ctl/alpha", "ctl/beta"];
        if !gone {
            ids.push("lab/off");
        }
        for id in &ids {
            write_semantic_way(&f.ways_root(), id);
        }
        let beta: Vec<(&str, f64)> = if gone { vec![("ctl/beta", 0.45)] } else { vec![("lab/off", 0.60), ("ctl/beta", 0.45)] };
        install_late_interaction_stub(&f.engine(), &ids, &[("Alpha", vec![("ctl/alpha", 0.45)]), ("Beta", beta)], sidecar);
        f
    }

    fn ways_root(&self) -> PathBuf {
        self.home.join(".claude/hooks/ways")
    }

    /// Where the embedding engine and corpus live.
    fn engine(&self) -> PathBuf {
        self.home.join(".cache/agent-ways/user")
    }

    /// Run `ways hook <event>` with `payload` on stdin; return stdout.
    fn hook(&self, event: &str, payload: &str) -> String {
        use std::io::Write;
        let mut child = ways_cmd(&self.home, &self.project)
            .args(["hook", event])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("Failed to run ways hook");
        child.stdin.take().unwrap().write_all(payload.as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    }

    /// The outcomes of this session's prompt decision records, by way.
    fn outcomes(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.home.join(".local/state/agent-ways/decisions.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter(|v| v["session"] == self.session.as_str())
            .flat_map(|v| v["outcomes"].as_array().cloned().unwrap_or_default())
            .collect()
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
        self.hook("tool", &format!(r#"{{"session_id":"{}","hook_event_name":"PreToolUse","tool_name":"{tool}"}}"#, self.session))
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

    /// The `check_fired` events as `(check, filter or "")`: a delivered check
    /// fire has no filter.
    fn checks(&self) -> Vec<(String, String)> {
        self.events("check_fired")
            .iter()
            .map(|v| (v["check"].as_str().unwrap_or("").to_string(), v["discarded"]["filter"].as_str().unwrap_or("").to_string()))
            .collect()
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
    assert_eq!(ev["scope"], "agent", "the session's scope, as on a delivered fire: {ev}");
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
    let f = Fixture::new("task-scope", Source::ProjectDomain);
    f.scan(&["task", "--query", "please build the widget and the gadget"]);
    assert_eq!(f.discarded_event("lab/kw")["scope"], "subagent");
}

/// The late-interaction lane, two topics: the enabled ways fire the same with
/// `lab/off` switched off or deleted, so the switched-off way takes no share
/// in its chunk. Without a body sidecar the shadow pass would have to embed
/// to confirm, so it logs nothing.
#[cfg(unix)]
#[test]
fn late_interaction_fires_the_same_with_a_way_switched_off_or_deleted() {
    let query = ["prompt", "--query", "Please look at the Alpha topic now. Please look at the Beta topic too."];
    for sidecar in [false, true] {
        let off = Fixture::late(&format!("li-off-{sidecar}"), Source::ProjectDomain, false, sidecar);
        let gone = Fixture::late(&format!("li-gone-{sidecar}"), Source::None, true, sidecar);
        let (off_out, gone_out) = (off.scan(&query), gone.scan(&query));
        assert!(gone_out.contains("Marker ctl/beta") && gone_out.contains("Marker ctl/alpha"), "both fire: {gone_out}");
        assert_eq!(off_out, gone_out, "sidecar {sidecar}: hook output");
        assert_eq!(off.fired(), gone.fired(), "sidecar {sidecar}: fired ways");
        assert_eq!(off.outcomes(), gone.outcomes(), "sidecar {sidecar}: decision outcomes");
        if !sidecar {
            assert!(off.discarded().is_empty(), "no confirmation without the sidecar: {:?}", off.discarded());
        }
    }
}

/// With a body sidecar the shadow pass confirms `lab/off` without embedding
/// and logs it with the scores of its own pass beside the enabled ways:
/// it peaks at 0.60 over `ctl/beta` at 0.45 in the Beta chunk, so its share
/// is that chunk's softmax mass over two chunks.
#[cfg(unix)]
#[test]
fn late_interaction_shadow_pass_logs_its_scores() {
    let f = Fixture::late("li-shadow", Source::ProjectDomain, false, true);
    f.scan(&["prompt", "--query", "Please look at the Alpha topic now. Please look at the Beta topic too."]);
    assert_eq!(f.discarded(), expected(Source::ProjectDomain, "prompt", &["lab/off"]));
    let ev = f.discarded_event("lab/off");
    assert_eq!(ev["trigger"], "semantic:late-interaction:en", "{ev}");
    let mass = 1.0 / (1.0 + (-(0.60f64 - 0.45) / 0.08).exp());
    assert_eq!(ev["cosine"].as_f64(), Some(0.6), "{ev}");
    assert!((ev["share"].as_f64().unwrap() - mass / 2.0).abs() < 1e-3, "{ev}");
    assert!((ev["margin"].as_f64().unwrap() - 0.15).abs() < 1e-3, "{ev}");
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

/// A check refused for its switched-off way logs a discarded check fire once
/// its score clears the threshold, apart from the way's own discarded fire;
/// a state way logs once per refire window however often its condition holds.
#[test]
fn check_and_state_lanes_log_a_discarded_fire() {
    let on = Fixture::new("check-state-on", Source::None);
    add_check_and_state_ways(&on);
    on.scan(&["command", "--command", "widgetctl run"]);
    on.scan(&["state", "--hook-event", "UserPromptSubmit"]);
    assert_eq!(on.fired(), ["lab/kw", "lab/st"], "both fire when on");
    assert_eq!(on.checks(), [("lab/kw".to_string(), String::new())], "the check fires when on");
    assert!(on.discarded().is_empty());

    let f = Fixture::new("check-state", Source::ProjectDomain);
    add_check_and_state_ways(&f);
    f.scan(&["command", "--command", "widgetctl run"]);
    f.scan(&["state", "--hook-event", "UserPromptSubmit"]);
    f.scan(&["state", "--hook-event", "UserPromptSubmit"]);
    let mut want = expected(Source::ProjectDomain, "command", &["lab/kw"]);
    want.extend(expected(Source::ProjectDomain, "state", &["lab/st"]));
    assert_eq!(f.discarded(), want, "the check adds no discarded fire of its way");
    assert_eq!(f.checks(), [("lab/kw".to_string(), "domain".to_string())], "one discarded check fire");
    let check = &f.events("check_fired")[0];
    assert_eq!((check["lane"].as_str(), check["fire_count"].as_str()), (Some("check"), Some("1")), "{check}");
    assert!(f.fired().is_empty(), "{:?}", f.fired());

    // The discarded check decays on its own count, which the real one never sees.
    f.scan(&["command", "--command", "widgetctl run"]);
    let counts: Vec<String> = f.events("check_fired").iter().map(|v| v["fire_count"].as_str().unwrap_or("").to_string()).collect();
    assert_eq!(counts, ["1", "2"]);
    assert_eq!(f.discarded().len(), 2, "the way's own discard is held by its refire window: {:?}", f.discarded());
    assert!(!PathBuf::from(format!("{}/{}/check-fires/lab/kw/.value", sessions_root(), f.session)).exists(), "the real check count is untouched");

    let project = f.project.to_str().unwrap();
    let v: serde_json::Value = serde_json::from_str(&f.ways(&["tune", "stats", "--json", "--project", project])).unwrap();
    assert_eq!(v["check_fires"], 0, "{v:#}");
    assert_eq!(v["discarded_check_fires"]["by_check"], serde_json::json!({"lab/kw": {"domain": 2}}), "{v:#}");
    assert_eq!(v["discarded_fires"]["by_way"], serde_json::json!({"lab/kw": {"domain": 1}, "lab/st": {"domain": 1}}), "{v:#}");
}

/// The reviewer's repro: two `refire: once` ways, one on and one switched
/// off, met three times on the prompt lane and twice per round on the file
/// lane. The enabled way fires once and is held once; the switched-off way
/// logs one discarded fire, as the refire window allows one fire.
#[test]
fn a_discarded_fire_meets_the_refire_window() {
    let f = Fixture::base("refire", Source::ProjectDomain, "lab/once");
    write_way_with(&f.ways_root(), "lab/once", "doohickey", "agent", "once");
    write_way_with(&f.ways_root(), "ctl/once", "thingamajig", "agent", "once");
    for _ in 0..3 {
        f.scan(&["prompt", "--query", "fix the doohickey and the thingamajig"]);
        f.scan(&["file", "--path", "/src/a.doohickey"]);
        f.scan(&["file", "--path", "/src/a.thingamajig"]);
    }
    assert_eq!(f.fired(), ["ctl/once"], "the enabled way fires once");
    let held: Vec<String> = f.events("way_suppressed").iter().map(|v| v["way"].as_str().unwrap_or("").to_string()).collect();
    assert_eq!(held, ["ctl/once"], "and is held once in its window");
    assert_eq!(f.discarded(), expected(Source::ProjectDomain, "prompt", &["lab/once"]), "one discarded fire in the window");
    assert!(
        !PathBuf::from(format!("{}/{}/way-engagement/lab__once.json", sessions_root(), f.session)).exists(),
        "the switched-off way's own refire state is never written"
    );
}

/// The same prompt twice discards once, like the enabled way it would be.
#[test]
fn the_same_prompt_twice_discards_once() {
    let f = Fixture::new("twice", Source::ProjectWayKey);
    for _ in 0..2 {
        f.scan(&["prompt", "--query", "please build the widget and the gadget"]);
    }
    assert_eq!(f.fired(), ["ctl/kw"]);
    assert_eq!(f.discarded(), expected(Source::ProjectWayKey, "prompt", &["lab/kw"]));
}

/// A switched-off way scoped to teammates only is not eligible on the agent's
/// prompt lane or a plain subagent's task lane, so it logs nothing.
#[test]
fn a_switched_off_way_out_of_scope_logs_nothing() {
    let f = Fixture::new("teammate", Source::ProjectDomain);
    write_way_with(&f.ways_root(), "lab/team", "widget", "teammate", "0.15");
    f.scan(&["prompt", "--query", "please build the widget and the gadget"]);
    f.scan(&["task", "--query", "please build the widget and the gadget"]);
    let ways: Vec<String> = f.discarded().into_iter().map(|d| d.0).collect();
    assert_eq!(ways, ["lab/kw", "lab/kw"], "lab/kw on the prompt and task lanes; lab/team on neither");
}

/// A pattern hit below the keyword floor does not fire an enabled way, and
/// does not discard a switched-off one.
#[cfg(unix)]
#[test]
fn a_keyword_hit_under_the_floor_logs_nothing() {
    let f = Fixture::new("floor", Source::ProjectDomain);
    // Cosine 0.07 maps to p ≈ 0.142, under the floor of 0.15.
    install_stub_engine(&f.engine(), &["lab/kw", "ctl/kw"], &[("lab/kw", 0.07), ("ctl/kw", 0.07)]);
    f.scan(&["prompt", "--query", "please build the widget and the gadget"]);
    let gated: Vec<String> = f.events("way_keyword_gated").iter().map(|v| v["way"].as_str().unwrap_or("").to_string()).collect();
    assert_eq!(gated, ["ctl/kw"], "the enabled way's hit is gated");
    assert!(f.discarded().is_empty(), "{:?}", f.discarded());
}

/// A pull refused for a switched-off way logs one discarded fire on the
/// pull lane, and the hook that follows records no delivery and no refire
/// state. With nothing switched off the pull is served and stamped.
#[test]
fn the_pull_lane_logs_a_refused_read() {
    let pull = |f: &Fixture| {
        let out = ways_cmd(&f.home, &f.project).args(["lookup", "read", "--session", &f.session, "--", "lab/kw"]).output().expect("Failed to run ways lookup read");
        f.hook("pull", &format!(r#"{{"session_id":"{}","tool_name":"ways_read","tool_input":{{"id":"lab/kw"}},"tool_response":{{"content":"x"}}}}"#, f.session));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    let engagement = |f: &Fixture| PathBuf::from(format!("{}/{}/way-engagement/lab__kw.json", sessions_root(), f.session)).exists();

    let on = Fixture::new("pull-on", Source::None);
    assert!(pull(&on).contains("Marker lab/kw"), "the read serves the body");
    assert!(on.discarded().is_empty());
    assert!(engagement(&on), "the hook stamps the pull");

    let f = Fixture::new("pull", Source::ProjectWayKey);
    assert!(!pull(&f).contains("Marker lab/kw"), "the read is refused");
    assert_eq!(f.discarded(), expected(Source::ProjectWayKey, "pull", &["lab/kw"]));
    assert!(f.fired().is_empty() && !engagement(&f), "no delivery and no refire state");

    // A read with no session belongs to no session's fires.
    ways_cmd(&f.home, &f.project).args(["lookup", "read", "--", "lab/kw"]).output().expect("Failed to run ways lookup read");
    let all = std::fs::read_to_string(f.home.join(".local/state/agent-ways/events.jsonl")).unwrap_or_default();
    assert_eq!(all.lines().filter(|l| l.contains("\"discarded\"")).count(), 1, "{all}");
}

/// On the Bash lane a switched-off child keeps the boost of an enabled parent
/// fired in the same scan, as an enabled child would: `ctl/kw/child` scores
/// p ≈ 0.45 on the command's description, under the bar of 0.5 and over the
/// boosted bar of 0.4.
#[cfg(unix)]
#[test]
fn a_switched_off_child_keeps_an_enabled_parents_boost_on_the_command_lane() {
    let run = |tag: &str, source: Source| {
        let f = Fixture::base(tag, source, "ctl/kw/child");
        write_way(&f.ways_root(), "ctl/kw", "gadget");
        write_semantic_way(&f.ways_root(), "ctl/kw/child");
        install_stub_engine(&f.engine(), &["ctl/kw", "ctl/kw/child"], &[("ctl/kw", 0.0), ("ctl/kw/child", 0.23)]);
        f.scan(&["command", "--command", "gadgetctl run", "--description", "run the gadget"]);
        f
    };
    let on = run("boost-on", Source::None);
    assert_eq!(on.fired(), ["ctl/kw", "ctl/kw/child"], "the child fires on its parent's boost when on");
    let f = run("boost", Source::ProjectWayKey);
    assert_eq!(f.fired(), ["ctl/kw"]);
    assert_eq!(f.discarded(), expected(Source::ProjectWayKey, "command", &["ctl/kw/child"]));
}

/// A stash written while a way was on, claimed after it was switched off,
/// injects only the enabled way and logs the other as a discarded fire on the
/// subagent lane, in the subagent's scope.
#[test]
fn the_subagent_lane_logs_a_way_switched_off_after_dispatch() {
    let f = Fixture::new("subagent", Source::None);
    f.scan(&["task", "--query", "please build the widget and the gadget"]);
    assert_eq!(f.stashed(), ["ctl/kw", "lab/kw"]);
    std::fs::write(f.project.join(".claude/ways.yaml"), "ways:\n  lab/kw: false\n").unwrap();
    let out = f.hook("subagent-start", &format!(r#"{{"session_id":"{}","agent_id":"a1","agent_type":"general-purpose","hook_event_name":"SubagentStart"}}"#, f.session));
    assert!(out.contains("Marker ctl/kw") && !out.contains("Marker lab/kw"), "{out}");
    assert_eq!(f.discarded(), expected(Source::ProjectWayKey, "subagent", &["lab/kw"]));
    assert_eq!(f.discarded_event("lab/kw")["scope"], "subagent");
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

/// A boost counts only from a parent that was shown. `big/first` matches more
/// specifically, is admitted first and fills the hook's context budget, so
/// the enabled parent `ctl/kw` matches and is withheld; an enabled child that
/// needed its boost is withheld with it, and the switched-off child logs
/// nothing. On the prompt and command lanes.
#[cfg(unix)]
#[test]
fn an_unshown_parent_boosts_no_discard() {
    let run = |tag: &str, source: Source, lane: &[&str]| {
        let f = Fixture::base(tag, source, "ctl/kw/child");
        write_way(&f.ways_root(), "ctl/kw", "gadget");
        let big = f.ways_root().join("big/first");
        std::fs::create_dir_all(&big).unwrap();
        std::fs::write(
            big.join("first.md"),
            format!("---\ndescription: first guidance\nvocabulary: first words\npattern: fix the gadget\ncommands: ^gadgetctl run\nscope: agent\nrefire: 0.15\n---\n# Marker big/first\n\n{}\n", "lorem ipsum ".repeat(840)),
        )
        .unwrap();
        write_semantic_way(&f.ways_root(), "ctl/kw/child");
        install_stub_engine(&f.engine(), &["big/first", "ctl/kw", "ctl/kw/child"], &[("big/first", 0.30), ("ctl/kw", 0.30), ("ctl/kw/child", 0.23)]);
        f.scan(lane);
        f
    };
    let lanes: [(&str, &[&str]); 2] = [
        ("prompt", &["prompt", "--query", "please fix the gadget for me"]),
        ("command", &["command", "--command", "gadgetctl run", "--description", "run the gadget"]),
    ];
    for (name, lane) in lanes {
        let on = run(&format!("unshown-on-{name}"), Source::None, lane);
        assert_eq!(on.fired(), ["big/first"], "{name}: the parent is withheld and its child with it");
        let f = run(&format!("unshown-{name}"), Source::ProjectWayKey, lane);
        assert!(f.discarded().is_empty(), "{name}: {:?}", f.discarded());
    }
}

/// A session-start way is delivered once per session, so its discard logs
/// once per session as the ticks advance past its refire window; a
/// file-exists way keeps the window on both sides.
#[test]
fn a_session_start_way_discards_once_per_session() {
    let f = Fixture::base("session-start", Source::ProjectDomain, "lab/ss");
    let way = |id: &str, trigger: &str| {
        let dir = f.ways_root().join(id);
        std::fs::create_dir_all(&dir).unwrap();
        let name = id.rsplit('/').next().unwrap();
        std::fs::write(dir.join(format!("{name}.md")), format!("---\ndescription: {name} guidance\n{trigger}\nscope: agent\nrefire: 0.15\n---\n# Marker {id}\n")).unwrap();
    };
    way("lab/ss", "trigger: session-start");
    way("ctl/ss", "trigger: session-start");
    way("lab/fe", "trigger: file-exists\npath: marker.txt");
    way("ctl/fe", "trigger: file-exists\npath: marker.txt");
    std::fs::write(f.project.join("marker.txt"), "").unwrap();
    let transcript = f.base.join(format!("{}.jsonl", f.session));
    for tick in [1_000u64, 2_000, 80_000, 90_000, 200_000] {
        let line = format!(r#"{{"type":"assistant","message":{{"model":"claude-opus-5-5","usage":{{"input_tokens":{tick},"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#);
        std::fs::write(&transcript, format!("{line}\n")).unwrap();
        f.scan(&["state", "--hook-event", "UserPromptSubmit", "--transcript", transcript.to_str().unwrap()]);
    }
    // A delivered refire logs `way_redisclosed`.
    let shown = |id: &str| {
        let fires = f.events("way_fired").into_iter().filter(|v| v.get("discarded").is_none());
        fires.chain(f.events("way_redisclosed")).filter(|v| v["way"] == id).count()
    };
    let discarded = |id: &str| f.discarded().iter().filter(|d| d.0 == id).count();
    assert_eq!((shown("ctl/ss"), discarded("lab/ss")), (1, 1), "once per session on both sides");
    assert_eq!((shown("ctl/fe"), discarded("lab/fe")), (2, 2), "a file-exists way refires past its window on both sides");
}

/// Parallel hooks claim a discarded fire once, as they deliver a fire once:
/// each round, a fresh session runs four `scan file` calls on a switched-off
/// way's file and four on an enabled way's, all at once. The rounds default
/// to 12; `WAYS_DISCARD_RACE_ROUNDS` raises them.
#[test]
fn parallel_scans_discard_once_per_window() {
    let rounds: usize = std::env::var("WAYS_DISCARD_RACE_ROUNDS").ok().and_then(|s| s.parse().ok()).unwrap_or(12);
    let f = Fixture::new("race", Source::ProjectDomain);
    for round in 0..rounds {
        let session = format!("{}-r{round}", f.session);
        clean_markers(&session);
        let children: Vec<std::process::Child> = ["/src/a.widget", "/src/a.gadget"]
            .iter()
            .cycle()
            .take(8)
            .map(|path| {
                ways_cmd(&f.home, &f.project)
                    .args(["scan", "file", "--path", path, "--session", &session, "--project"])
                    .arg(&f.project)
                    .stdout(Stdio::null())
                    .spawn()
                    .expect("Failed to run ways scan file")
            })
            .collect();
        for mut child in children {
            assert!(child.wait().unwrap().success());
        }
        let events: Vec<serde_json::Value> = std::fs::read_to_string(f.home.join(".local/state/agent-ways/events.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter(|v| v["event"] == "way_fired" && v["session"] == session.as_str())
            .collect();
        let count = |way: &str, discarded: bool| events.iter().filter(|v| v["way"] == way && v.get("discarded").is_some() == discarded).count();
        assert_eq!((count("ctl/kw", false), count("lab/kw", true)), (1, 1), "round {round}: one delivered fire and one discarded fire");
        clean_markers(&session);
    }
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
