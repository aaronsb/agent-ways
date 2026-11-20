// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The relevance gate on the tool lanes, in every agent thread (#1287).
//!
//! Each scenario runs `ways hook <event>` the way Claude Code does, against
//! an isolated HOME and XDG tree, with `gate.engine: claude-code` and a
//! judge host played by this test through `ways judge take` and `ways judge
//! answer`, the verbs the ways mod runs. The host blocks the ways whose
//! route names `drop` and passes the rest, so a scenario can tell what the
//! judge saw, what the hook injected, which fires were recorded and what
//! the decision log says.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn ways_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ways"))
}

/// Mirror of `session::sessions_root()`, as session_sim.rs keeps it.
fn sessions_root() -> String {
    if let Ok(xdg) = std::env::var("XDG_RUNTIME_DIR") {
        return format!("{xdg}/claude-sessions");
    }
    let uid = Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "0".to_string());
    format!("/tmp/.claude-sessions-{uid}")
}

/// One scenario's tree: a home with its own ways, a project, a session.
struct Sim {
    base: PathBuf,
    home: PathBuf,
    state: PathBuf,
    project: PathBuf,
    session: String,
}

const MAIN_PROSE: &str = "lead narration about the release notes";
const SUB_BRIEF: &str = "subagent brief to fix the failing parser test";
const SUB_PROSE: &str = "subagent reasoning about the parser fix";

impl Sim {
    /// A tree whose ways are `judged/keep` and `judged/drop` on `^git commit`
    /// and `judged/keepfile` and `judged/dropfile` on `\.rs$`. `agent_yaml`
    /// is written as the user's agent.yaml when given, and a judge host is
    /// registered for the session.
    fn new(name: &str, agent_yaml: Option<&str>) -> Sim {
        let base = std::env::temp_dir().join(format!("ways-judge-lanes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let home = base.join("home");
        let state = base.join("state");
        let project = base.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let ways = home.join(".claude/hooks/ways/judged");
        for (id, trigger) in [
            ("keep", "commands: ^git\\ commit"),
            ("drop", "commands: ^git\\ commit"),
            ("keepfile", "files: \\.rs$"),
            ("dropfile", "files: \\.rs$"),
        ] {
            let dir = ways.join(id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join(format!("{id}.md")),
                format!("---\ndescription: test way {id}\n{trigger}\nscope: agent\nrefire: 0.15\n---\n# Marker {id}\n\nbody of {id}\n"),
            )
            .unwrap();
        }
        let session = format!("judge-{name}-{}", std::process::id());
        let _ = std::fs::remove_dir_all(format!("{}/{session}", sessions_root()));
        let sim = Sim { base, home, state, project, session };
        sim.write_transcripts();
        if let Some(yaml) = agent_yaml {
            let dir = sim.home.join(".config/agent-ways");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("agent.yaml"), yaml).unwrap();
            let out = sim.ways().args(["judge", "host", "--session", &sim.session]).output().unwrap();
            assert!(out.status.success(), "judge host: {}", String::from_utf8_lossy(&out.stderr));
        }
        sim
    }

    /// The main agent's transcript and a subagent's, each one human turn
    /// and the agent's prose after it.
    fn write_transcripts(&self) {
        let slug: String = self.project.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        let dir = self.home.join(".claude/projects").join(slug);
        std::fs::create_dir_all(dir.join(&self.session).join("subagents")).unwrap();
        let user = |t: &str| format!(r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"text","text":"{t}"}}]}}}}"#);
        let assistant = |t: &str| format!(r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{t}"}}]}}}}"#);
        std::fs::write(dir.join(format!("{}.jsonl", self.session)), format!("{}\n{}\n", user("write the release notes"), assistant(MAIN_PROSE))).unwrap();
        std::fs::write(
            dir.join(&self.session).join("subagents/agent-asub1.jsonl"),
            format!("{}\n{}\n", user(SUB_BRIEF), assistant(SUB_PROSE)),
        )
        .unwrap();
    }

    fn transcript(&self) -> PathBuf {
        let slug: String = self.project.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        self.home.join(".claude/projects").join(slug).join(format!("{}.jsonl", self.session))
    }

    /// A `ways` command isolated from the operator's install and session.
    fn ways(&self) -> Command {
        let mut cmd = Command::new(ways_bin());
        cmd.current_dir(&self.project)
            .env("PWD", &self.project)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            .env("XDG_STATE_HOME", &self.state)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_BIN_HOME", self.home.join(".local/bin"));
        for var in ["CLAUDE_PROJECT_DIR", "CLAUDE_AGENT_ID", "CLAUDE_SESSION_ID", "CLAUDE_CODE_SESSION_ID", "CLAUDE_CONFIG_DIR", "WAYS_AGENT_SOCK", "WAYS_CLAUDE_BIN"] {
            cmd.env_remove(var);
        }
        cmd
    }

    /// Runs `ways hook <event>` on `tool_input`, from `agent` when given,
    /// and returns its stdout.
    fn hook(&self, event: &str, tool: &str, tool_input: serde_json::Value, agent: Option<&str>) -> String {
        let mut payload = serde_json::json!({
            "session_id": self.session,
            "hook_event_name": "PreToolUse",
            "tool_name": tool,
            "tool_input": tool_input,
            "cwd": self.project,
            "transcript_path": self.transcript(),
        });
        if let Some(a) = agent {
            payload["agent_id"] = a.into();
        }
        let mut child = self.ways().args(["hook", event]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        std::io::Write::write_all(&mut child.stdin.take().unwrap(), payload.to_string().as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "hook {event}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn bash(&self, agent: Option<&str>) -> String {
        self.hook("command", "Bash", serde_json::json!({"command": "git commit -m fix", "description": "Commit the parser fix"}), agent)
    }

    /// A judge host on a thread: it answers up to `jobs` jobs, blocking every
    /// way whose route names `drop`, and returns each job's prompt.
    fn host(&self, jobs: usize) -> std::thread::JoinHandle<Vec<String>> {
        let ways = self.ways_args();
        std::thread::spawn(move || {
            let mut prompts = Vec::new();
            for _ in 0..jobs {
                let Some(prompt) = answer_one(&ways) else { break };
                prompts.push(prompt);
            }
            prompts
        })
    }

    fn ways_args(&self) -> HostEnv {
        HostEnv { home: self.home.clone(), state: self.state.clone(), project: self.project.clone(), session: self.session.clone() }
    }

    /// Whether `way` has a fire recorded for `agent` (main: `None`).
    fn fired(&self, way: &str, agent: Option<&str>) -> bool {
        let marker = format!("{}/{}/ways/{way}/.marker.{}", sessions_root(), self.session, agent.unwrap_or("main"));
        std::path::Path::new(&marker).exists()
    }

    /// The decision records of this session.
    fn decisions(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.state.join("agent-ways/decisions.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter(|v| v["session"] == self.session)
            .collect()
    }

    /// Waits up to 15 s for a decision record that `pick` accepts.
    fn wait_decision(&self, pick: impl Fn(&serde_json::Value) -> bool) -> serde_json::Value {
        let until = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(r) = self.decisions().into_iter().find(|r| pick(r)) {
                return r;
            }
            assert!(Instant::now() < until, "no matching decision record: {:#?}", self.decisions());
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Sim {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(format!("{}/{}", sessions_root(), self.session));
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

struct HostEnv {
    home: PathBuf,
    state: PathBuf,
    project: PathBuf,
    session: String,
}

impl HostEnv {
    fn ways(&self) -> Command {
        let mut cmd = Command::new(ways_bin());
        cmd.current_dir(&self.project)
            .env("HOME", &self.home)
            .env("XDG_STATE_HOME", &self.state)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_CACHE_HOME", self.home.join(".cache"));
        cmd
    }
}

/// Takes one job and answers it; `None` when no job came within 10 s.
fn answer_one(env: &HostEnv) -> Option<String> {
    let out = env.ways().args(["judge", "take", "--session", &env.session, "--wait", "--timeout-ms", "10000"]).output().unwrap();
    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if line.is_empty() || line == "standby" {
        return None;
    }
    let job: serde_json::Value = serde_json::from_str(&line).unwrap();
    let prompt = job["prompt"].as_str().unwrap().to_string();
    let mut judgements = Vec::new();
    for (i, block) in prompt.split("<guidance id=\"g").skip(1).enumerate() {
        let route = block.lines().nth(1).unwrap_or("");
        let drop = route.contains("drop");
        judgements.push(serde_json::json!({"id": format!("g{}", i + 1), "relevant": !drop, "confidence": 0.95}));
    }
    let answer = serde_json::json!({"text": serde_json::json!({"judgements": judgements}).to_string()});
    let mut child = env
        .ways()
        .args(["judge", "answer", "--session", &env.session, "--job", job["id"].as_str().unwrap()])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(&mut child.stdin.take().unwrap(), answer.to_string().as_bytes()).unwrap();
    assert!(child.wait().unwrap().success());
    Some(prompt)
}

fn outcome<'r>(record: &'r serde_json::Value, way: &str) -> &'r serde_json::Value {
    record["outcomes"].as_array().unwrap().iter().find(|o| o["way"] == way).unwrap_or_else(|| panic!("no outcome for {way}: {record:#}"))
}

const ENFORCE: &str = "engine: claude-code\nmode: enforce\nheld_sample: 0\n";

/// A subagent's Bash call: both ways reach the judge with the subagent's own
/// context, the blocked one is not injected and has no fire recorded, and
/// the decision record names the lane, the agent and the verdicts.
#[test]
fn a_subagents_bash_candidates_reach_the_judge_and_a_blocked_one_stays_out() {
    let s = Sim::new("sub-bash", Some(ENFORCE));
    let host = s.host(1);
    let out = s.bash(Some("asub1"));
    let prompts = host.join().unwrap();
    assert_eq!(prompts.len(), 1, "one judge request for the call");
    let p = &prompts[0];
    assert!(p.contains("judged › keep") && p.contains("judged › drop"), "both candidates judged: {p}");
    assert!(p.contains("$ git commit -m fix") && p.contains("About to run: commit the parser fix"), "the call is the judge's input: {p}");
    assert!(p.contains(SUB_PROSE), "the subagent's own context: {p}");
    assert!(!p.contains(MAIN_PROSE), "never the lead's: {p}");
    assert!(out.contains("# Marker keep") && !out.contains("# Marker drop"), "{out}");
    assert!(s.fired("judged/keep", Some("asub1")));
    assert!(!s.fired("judged/drop", Some("asub1")), "a blocked way keeps its refire budget");
    let r = s.wait_decision(|r| r["surface"] == "command");
    assert_eq!(r["agent"], "asub1");
    assert_eq!(outcome(&r, "judged/drop")["result"], "judge_block");
    assert_eq!(outcome(&r, "judged/keep")["verdict"], "pass");
    assert_eq!(r["judge"]["status"], "judged");
    assert!(r["judge"]["judge_ms"].is_u64(), "{r:#}");
}

/// The main agent's Bash call is judged the same way, on its own context.
#[test]
fn the_main_agents_bash_candidates_reach_the_judge() {
    let s = Sim::new("main-bash", Some(ENFORCE));
    let host = s.host(1);
    let out = s.bash(None);
    let prompts = host.join().unwrap();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains(MAIN_PROSE) && !prompts[0].contains(SUB_PROSE), "{}", prompts[0]);
    assert!(out.contains("# Marker keep") && !out.contains("# Marker drop"), "{out}");
    assert!(s.fired("judged/keep", None) && !s.fired("judged/drop", None));
    let r = s.wait_decision(|r| r["surface"] == "command");
    assert_eq!(r["agent"], "main");
}

/// The file lane: an edit's candidates are judged against the path.
#[test]
fn the_file_lanes_candidates_reach_the_judge() {
    let s = Sim::new("file", Some(ENFORCE));
    let host = s.host(1);
    let out = s.hook("file", "Edit", serde_json::json!({"file_path": "/srv/p/src/lib.rs"}), Some("asub1"));
    let prompts = host.join().unwrap();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains("/srv/p/src/lib.rs"), "{}", prompts[0]);
    assert!(out.contains("# Marker keepfile") && !out.contains("# Marker dropfile"), "{out}");
    assert!(!s.fired("judged/dropfile", Some("asub1")));
    let r = s.wait_decision(|r| r["surface"] == "file");
    assert_eq!(outcome(&r, "judged/dropfile")["result"], "judge_block");
}

/// Shadow mode injects every candidate at once and judges them off the hot
/// path; the verdicts reach the decision log.
#[test]
fn shadow_injects_everything_and_logs_the_verdicts() {
    let s = Sim::new("shadow", Some("engine: claude-code\nmode: shadow\nheld_sample: 0\n"));
    let host = s.host(1);
    let out = s.bash(Some("asub1"));
    assert!(out.contains("# Marker keep") && out.contains("# Marker drop"), "{out}");
    assert!(s.fired("judged/drop", Some("asub1")));
    let r = s.wait_decision(|r| r["surface"] == "command" && r["judge"]["status"] == "judged");
    assert_eq!(r["agent"], "asub1");
    assert_eq!(outcome(&r, "judged/drop")["verdict"], "would_block");
    assert_eq!(outcome(&r, "judged/drop")["result"], "fired");
    assert!(r["judge"]["judge_ms"].is_u64(), "{r:#}");
    assert_eq!(host.join().unwrap().len(), 1, "one detached request");
}

/// With no engine and no key the gate is off: every candidate is injected,
/// as before, and no judge job is written.
#[test]
fn with_the_gate_off_nothing_changes() {
    let s = Sim::new("off", None);
    let out = s.bash(Some("asub1"));
    assert!(out.contains("# Marker keep") && out.contains("# Marker drop"), "{out}");
    assert!(s.fired("judged/keep", Some("asub1")) && s.fired("judged/drop", Some("asub1")));
    let jobs = PathBuf::from(format!("{}/{}/judge", sessions_root(), s.session));
    assert!(!jobs.exists() || std::fs::read_dir(&jobs).unwrap().next().is_none(), "no judge job");
}

/// A candidate the refire curve holds is never injected and never holds the
/// call; with `held_sample: 1` it is judged off the hot path and logged as
/// `held_refire` with its verdict. The way the judge blocked on the first
/// call kept its refire budget, so the second call judges it again on the
/// hot path.
#[test]
fn a_held_candidate_is_judged_off_the_hot_path_for_the_record() {
    let s = Sim::new("held", Some("engine: claude-code
mode: enforce
held_sample: 1
"));
    let first = s.host(1);
    s.bash(None);
    assert_eq!(first.join().unwrap().len(), 1);
    // A host waits for the next job before it comes, as the mod does.
    let host = s.host(2);
    std::thread::sleep(Duration::from_millis(200));
    let again = s.bash(None);
    assert!(!again.contains("# Marker keep"), "held by refire: {again}");
    let r = s.wait_decision(|r| r["surface"] == "command" && r.get("detached").is_some());
    let prompts = host.join().unwrap();
    assert_eq!(prompts.len(), 2, "the blocked way again on the hot path, the held one detached");
    assert!(prompts[0].contains("judged › drop") && !prompts[0].contains("judged › keep"), "{}", prompts[0]);
    assert!(prompts[1].contains("judged › keep"), "{}", prompts[1]);
    assert_eq!(outcome(&r, "judged/keep")["result"], "held_refire", "{r:#}");
    assert_eq!(outcome(&r, "judged/keep")["verdict"], "pass", "{r:#}");
    assert!(r["judge"]["judge_ms"].is_u64(), "{r:#}");
    assert!(!s.fired("judged/drop", None), "blocked twice, never fired");
}
