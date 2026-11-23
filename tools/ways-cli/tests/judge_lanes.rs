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
        let uid = Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "0".to_string());
        format!("/tmp/.claude-sessions-{uid}")
    }
}

/// One scenario's tree: a home with its own ways, a project, a session.
struct Sim {
    base: PathBuf,
    home: PathBuf,
    state: PathBuf,
    project: PathBuf,
    session: String,
    /// Runs the hook on the shipped judge timing, without the seam.
    shipped_timing: bool,
}

/// The hook's test seam for judge timing (`WAYS_TEST_JUDGE_*` in gate.rs).
/// The product waits 500 ms for a host to claim a job and about 2 s for its
/// answer, and a runner under load can pause the host's `take` past either,
/// so the scenario fails on the runner and not on the code. These scenarios
/// run with both set to a minute, so neither the claim window nor the
/// judge's own budget ends a wait. The hook's own deadline still does: the
/// tool lanes allow the hook about 9 s, and the gate and `show` hold the
/// ways back as that deadline nears. A host stalled for roughly 9 s can
/// still fail a scenario. The shipped values are tested in the gate's unit
/// tests, on a clock the test advances and on the real clock, and by the
/// two scenarios that mark their `Sim` `shipped_timing`.
const SEAM: [(&str, &str); 2] = [("WAYS_TEST_JUDGE_CLAIM_WINDOW_MS", "60000"), ("WAYS_TEST_JUDGE_BUDGET_MS", "60000")];

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
        let sim = Sim { base, home, state, project, session, shipped_timing: false };
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
        for (var, ms) in SEAM {
            if self.shipped_timing {
                cmd.env_remove(var);
            } else {
                cmd.env(var, ms);
            }
        }
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

    /// Writes a way `judged/<rel>` with the frontmatter lines `extra`.
    fn way(&self, rel: &str, extra: &str) {
        let name = rel.rsplit('/').next().unwrap();
        let dir = self.home.join(".claude/hooks/ways/judged").join(rel);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(format!("{name}.md")),
            format!("---\ndescription: test way {name}\n{extra}\nrefire: 0.15\n---\n# Marker {name}\n\nbody of {name}\n"),
        )
        .unwrap();
    }

    /// Runs `ways hook <event>` on a raw payload, with this session's id.
    fn raw_hook(&self, event: &str, mut payload: serde_json::Value) -> String {
        payload["session_id"] = self.session.clone().into();
        payload["cwd"] = self.project.to_string_lossy().into_owned().into();
        payload["transcript_path"] = self.transcript().to_string_lossy().into_owned().into();
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
        // A failed scenario prints its events, a gate's fallback reason
        // among them, before the tree that holds them goes.
        if std::thread::panicking() {
            let events = std::fs::read_to_string(self.state.join("agent-ways/events.jsonl")).unwrap_or_default();
            for line in events.lines() {
                eprintln!("{}: {line}", self.session);
            }
        }
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

/// Takes one job as the mod does, waiting up to 10 s; the job, or `None`
/// when none came. A take that got none says how long it waited, what it
/// printed and how it exited, for a failed scenario's output.
fn take_job(env: &HostEnv) -> Option<serde_json::Value> {
    let begun = Instant::now();
    let out = env.ways().args(["judge", "take", "--session", &env.session, "--wait", "--timeout-ms", "10000"]).output().unwrap();
    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if line.is_empty() || line == "standby" {
        eprintln!(
            "judge host: no job after {:?}: stdout {line:?}, {}, stderr {:?}",
            begun.elapsed(),
            out.status,
            String::from_utf8_lossy(&out.stderr).trim()
        );
        return None;
    }
    Some(serde_json::from_str(&line).unwrap())
}

/// Takes one job and never answers it, as a host that hangs would; the job's
/// prompt, or `None` when no job came within 10 s.
fn take_silently(env: &HostEnv) -> Option<String> {
    take_job(env).map(|job| job["prompt"].as_str().unwrap().to_string())
}

/// Takes one job and answers it; `None` when no job came within 10 s.
fn answer_one(env: &HostEnv) -> Option<String> {
    let job = take_job(env)?;
    let prompt = job["prompt"].as_str().unwrap().to_string();
    let mut judgements = Vec::new();
    for (i, block) in prompt.split("<guidance id=\"g").skip(1).enumerate() {
        let route = block.lines().nth(1).unwrap_or("");
        let drop = route.rsplit(" › ").next().unwrap_or("").contains("drop");
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
    assert!(p.contains(SUB_BRIEF), "the subagent's own request, its brief: {p}");
    assert!(!p.contains(SUB_PROSE), "no narration (ADR-712, request + call): {p}");
    assert!(!p.contains(MAIN_PROSE) && !p.contains("write the release notes"), "never the lead's: {p}");
    assert!(out.contains("# Marker keep") && !out.contains("# Marker drop"), "{out}");
    assert!(s.fired("judged/keep", Some("asub1")));
    assert!(!s.fired("judged/drop", Some("asub1")), "a blocked way keeps its refire budget");
    let r = s.wait_decision(|r| r["surface"] == "command");
    assert_eq!(r["agent"], "asub1");
    assert_eq!(outcome(&r, "judged/drop")["result"], "judge_block");
    assert_eq!(outcome(&r, "judged/keep")["verdict"], "pass");
    assert_eq!(r["judge"]["status"], "judged");
    assert!(r["judge"]["judge_ms"].is_u64(), "{r:#}");
    assert_eq!(r["judge"]["request"], "found", "the record says the judge read the brief: {r:#}");
}

/// The main agent's Bash call is judged the same way, on its own context.
#[test]
fn the_main_agents_bash_candidates_reach_the_judge() {
    let s = Sim::new("main-bash", Some(ENFORCE));
    let host = s.host(1);
    let out = s.bash(None);
    let prompts = host.join().unwrap();
    assert_eq!(prompts.len(), 1);
    let p = &prompts[0];
    assert!(p.contains("write the release notes") && !p.contains(SUB_BRIEF), "the main agent's own request: {p}");
    assert!(!p.contains(MAIN_PROSE), "no narration: {p}");
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

/// Shadow mode injects every candidate at once. Under claude-code no host
/// is open once the hook returns, so no detached judge starts, no job is
/// written, and the session logs the skip once.
#[test]
fn shadow_under_claude_code_injects_everything_and_starts_no_detached_judge() {
    let s = Sim::new("shadow", Some("engine: claude-code\nmode: shadow\nheld_sample: 0\n"));
    let out = s.bash(Some("asub1"));
    assert!(out.contains("# Marker keep") && out.contains("# Marker drop"), "{out}");
    assert!(s.fired("judged/drop", Some("asub1")));
    let r = s.wait_decision(|r| r["surface"] == "command");
    assert_eq!(r["judge"]["status"], "detach_skipped", "no verdicts follow: {r:#}");
    s.raw_hook("file", serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "Edit", "tool_input": {"file_path": "/p/src/lib.rs"}}));
    s.assert_no_detached_judge();
}

/// Waits long enough for a detached judge to have written its job, then
/// checks that none did: no job to take, no detached record, and one
/// `judge_detach_skipped` line for the session.
impl Sim {
    fn assert_no_detached_judge(&self) {
        std::thread::sleep(Duration::from_millis(1500));
        let out = self.ways().args(["judge", "take", "--session", &self.session]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "", "no job was written");
        assert!(self.decisions().iter().all(|r| r.get("detached").is_none()), "{:#?}", self.decisions());
        let events = std::fs::read_to_string(self.state.join("agent-ways/events.jsonl")).unwrap_or_default();
        let skips: Vec<serde_json::Value> = events
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .filter(|v: &serde_json::Value| v["event"] == "judge_detach_skipped" && v["session"] == self.session.as_str())
            .collect();
        assert_eq!(skips.len(), 1, "logged once per session: {skips:#?}");
        assert_eq!(skips[0]["reason"], "claude-code engine has no host outside a hook");
    }
}

/// The files under a session's directory, relative to it: the fire markers,
/// the epoch and whatever else a hook leaves there.
fn session_files(s: &Sim) -> Vec<String> {
    fn walk(dir: &std::path::Path, base: &std::path::Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, base, out);
            } else {
                out.push(p.strip_prefix(base).unwrap().to_string_lossy().into_owned());
            }
        }
    }
    let base = PathBuf::from(format!("{}/{}", sessions_root(), s.session));
    let mut out = Vec::new();
    walk(&base, &base, &mut out);
    out.sort();
    out
}

/// With the gate off, a tool call does what it did before the gate reached
/// the tool lanes. With no agent.yaml, a broken one, or `mode: off` with a
/// held sample asked for, the Bash, tool and file lanes inject the same ways
/// and leave the same fire markers, refire holds the second call as before,
/// and there is no judge job, no detached judge, no decision record and no
/// gate event.
#[test]
fn with_the_gate_off_nothing_changes() {
    let mut reference: Option<(Vec<String>, Vec<String>)> = None;
    for (name, yaml) in [
        ("off", None),
        ("broken", Some("engine: [unclosed\n")),
        ("mode-off", Some("engine: claude-code\nmode: off\nheld_sample: 1\n")),
    ] {
        let s = Sim::new(name, None);
        if let Some(y) = yaml {
            let dir = s.home.join(".config/agent-ways");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("agent.yaml"), y).unwrap();
        }
        s.way("toolkeep", "tool_name: ^ExitPlanMode$\nscope: agent");
        let outs = vec![
            s.bash(Some("asub1")),
            s.bash(Some("asub1")),
            s.hook("tool", "ExitPlanMode", serde_json::json!({}), None),
            s.hook("file", "Edit", serde_json::json!({"file_path": "/srv/p/src/lib.rs"}), None),
        ];
        assert!(outs[0].contains("# Marker keep") && outs[0].contains("# Marker drop"), "{name}: {}", outs[0]);
        assert!(!outs[1].contains("# Marker keep"), "{name}: refire holds as before: {}", outs[1]);
        assert!(outs[2].contains("# Marker toolkeep"), "{name}: {}", outs[2]);
        assert!(outs[3].contains("# Marker keepfile") && outs[3].contains("# Marker dropfile"), "{name}: {}", outs[3]);
        assert!(s.fired("judged/keep", Some("asub1")) && s.fired("judged/drop", Some("asub1")), "{name}");
        // A detached judge would write its record a moment after the hook.
        std::thread::sleep(Duration::from_millis(300));
        let jobs = PathBuf::from(format!("{}/{}/judge", sessions_root(), s.session));
        assert!(!jobs.exists() || std::fs::read_dir(&jobs).unwrap().next().is_none(), "{name}: no judge job");
        assert!(s.decisions().is_empty(), "{name}: no decision record: {:#?}", s.decisions());
        let events = std::fs::read_to_string(s.state.join("agent-ways/events.jsonl")).unwrap_or_default();
        for word in ["gate_fallback", "way_judged", "judge_detach", "\"detached\""] {
            assert!(!events.contains(word), "{name}: no {word} event: {events}");
        }
        let state = (outs, session_files(&s));
        assert!(state.1.iter().any(|f| f.replace('\\', "/").ends_with("keep/.marker.asub1")),"{name}: the snapshot sees the markers: {:?}", state.1);
        match &reference {
            None => reference = Some(state),
            Some(r) => assert_eq!(&state, r, "{name}: the same output and session state as with no agent.yaml"),
        }
    }
}

/// A judge that takes the job and never answers, and two ways whose macros
/// each run out their 3 s, still leave the Bash hook done inside its 10 s
/// bound, with both ways shown: the hook keeps one deadline, the judge
/// fails open within it, and each macro gets only what is left.
#[test]
fn a_hung_judge_and_slow_macros_fail_open_inside_the_hook_bound() {
    let mut s = Sim::new("hung", Some(ENFORCE));
    s.shipped_timing = true;
    for id in ["keep", "drop"] {
        std::fs::write(s.home.join(format!(".claude/hooks/ways/judged/{id}/macro.sh")), "sleep 5\necho late\n").unwrap();
        let md = s.home.join(format!(".claude/hooks/ways/judged/{id}/{id}.md"));
        let text = std::fs::read_to_string(&md).unwrap().replacen("refire: 0.15\n", "refire: 0.15\nmacro: append\n", 1);
        std::fs::write(&md, text).unwrap();
    }
    let env = s.ways_args();
    let host = std::thread::spawn(move || take_silently(&env));
    let begun = Instant::now();
    let out = s.bash(None);
    let took = begun.elapsed();
    assert!(host.join().unwrap().is_some(), "the judge was asked");
    assert!(took < Duration::from_millis(9_500), "the hook took {took:?}, past its 9 s deadline");
    assert!(out.contains("# Marker keep") && out.contains("# Marker drop"), "fails open: {out}");
}

/// A tool lane whose judge takes the job and never answers fails open
/// inside the hook bound: both ways shown, and the record names the
/// fallback.
#[test]
fn a_tool_lane_judge_that_times_out_fails_open_inside_the_bound() {
    let mut s = Sim::new("tool-hung", Some(ENFORCE));
    s.shipped_timing = true;
    s.way("toolkeep", "tool_name: ^ExitPlanMode$\nscope: agent");
    s.way("tooldrop", "tool_name: ^ExitPlanMode$\nscope: agent");
    let env = s.ways_args();
    let host = std::thread::spawn(move || take_silently(&env));
    let begun = Instant::now();
    let out = s.hook("tool", "ExitPlanMode", serde_json::json!({}), None);
    let took = begun.elapsed();
    assert!(host.join().unwrap().is_some(), "the judge was asked");
    assert!(took < Duration::from_millis(9_500), "the hook took {took:?}, past its 9 s deadline");
    assert!(out.contains("# Marker toolkeep") && out.contains("# Marker tooldrop"), "fails open: {out}");
    let r = s.wait_decision(|r| r["surface"] == "tool");
    assert_eq!(r["judge"]["status"], "fallback", "{r:#}");
    assert_eq!(outcome(&r, "judged/tooldrop")["result"], "fired", "{r:#}");
}

/// A gate switched on with no judge host registered for the session fails
/// open at once on a tool lane: nothing waits on a host that is not there.
#[test]
fn a_tool_lane_with_no_judge_host_fails_open_at_once() {
    let s = Sim::new("no-host", None);
    let dir = s.home.join(".config/agent-ways");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("agent.yaml"), ENFORCE).unwrap();
    let begun = Instant::now();
    let out = s.bash(None);
    let took = begun.elapsed();
    assert!(took < Duration::from_secs(3), "the hook took {took:?} with no host to wait on");
    assert!(out.contains("# Marker keep") && out.contains("# Marker drop"), "fails open: {out}");
    assert!(s.fired("judged/drop", None), "a way shown on fail-open has its fire recorded");
    let r = s.wait_decision(|r| r["surface"] == "command");
    assert_eq!(r["judge"]["status"], "fallback", "{r:#}");
}

/// The dispatch lane judges a subagent's ways against its brief. A dispatch
/// whose every way the judge blocks still leaves its (empty) stash, as one
/// that matched nothing does, so the next SubagentStart claims that one and
/// not a later dispatch's.
#[test]
fn the_dispatch_lane_judges_and_keeps_each_stash_to_its_dispatch() {
    let s = Sim::new("task", Some(ENFORCE));
    s.way("taskdrop", "pattern: droptopic\nscope: subagent");
    s.way("taskkeep", "pattern: keeptopic\nscope: subagent");
    let host = s.host(2);
    s.raw_hook("task", serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "Agent", "tool_input": {"prompt": "work on the droptopic"}}));
    s.raw_hook("task", serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "Agent", "tool_input": {"prompt": "work on the keeptopic"}}));
    let prompts = host.join().unwrap();
    assert_eq!(prompts.len(), 2, "both dispatches judged");
    assert!(prompts[0].contains("work on the droptopic"), "the brief is the judge's input: {}", prompts[0]);
    let first = s.raw_hook("subagent-start", serde_json::json!({"hook_event_name": "SubagentStart", "agent_id": "afirst"}));
    let second = s.raw_hook("subagent-start", serde_json::json!({"hook_event_name": "SubagentStart", "agent_id": "asecond"}));
    assert!(!first.contains("# Marker taskdrop") && !first.contains("# Marker taskkeep"), "the first dispatch's stash is empty: {first}");
    assert!(second.contains("# Marker taskkeep"), "the second gets its own: {second}");
    let r = s.wait_decision(|r| r["surface"] == "task" && r["outcomes"][0]["way"] == "judged/taskdrop");
    assert_eq!(outcome(&r, "judged/taskdrop")["result"], "judge_block", "{r:#}");
    assert_eq!(r["judge"]["request"], "found", "a dispatch is judged against its brief: {r:#}");
}

/// A dispatch whose brief matches no way still leaves an (empty) stash.
/// SubagentStart claims the oldest stash, so without it the first subagent
/// would take the second dispatch's way and scan id, and the second would get
/// nothing.
#[test]
fn a_dispatch_that_matches_nothing_still_keeps_its_stash() {
    let s = Sim::new("task-empty", Some(ENFORCE));
    s.way("taskonly", "pattern: onlytopic\nscope: subagent");
    let host = s.host(1);
    s.raw_hook("task", serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "Agent", "tool_input": {"prompt": "tidy the changelog"}}));
    s.raw_hook("task", serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "Agent", "tool_input": {"prompt": "work on the onlytopic"}}));
    host.join().unwrap();
    let first = s.raw_hook("subagent-start", serde_json::json!({"hook_event_name": "SubagentStart", "agent_id": "afirst"}));
    let second = s.raw_hook("subagent-start", serde_json::json!({"hook_event_name": "SubagentStart", "agent_id": "asecond"}));
    assert!(!first.contains("# Marker taskonly"), "the first dispatch matched nothing: {first}");
    assert!(second.contains("# Marker taskonly"), "the second gets its own way: {second}");
}

/// The tool lane judges a `tool_name:` way like any other candidate.
#[test]
fn the_tool_lane_judges_its_candidates() {
    let s = Sim::new("tool", Some(ENFORCE));
    s.way("toolkeep", "tool_name: ^ExitPlanMode$\nscope: agent");
    s.way("tooldrop", "tool_name: ^ExitPlanMode$\nscope: agent");
    let host = s.host(1);
    let out = s.raw_hook("tool", serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "ExitPlanMode", "tool_input": {}}));
    let prompts = host.join().unwrap();
    assert!(prompts[0].contains("About to use the ExitPlanMode tool."), "{}", prompts[0]);
    assert!(out.contains("# Marker toolkeep") && !out.contains("# Marker tooldrop"), "{out}");
    assert!(!s.fired("judged/tooldrop", None));
    s.wait_decision(|r| r["surface"] == "tool");
}

/// The state lane judges its triggered ways against the prompt in hand.
#[test]
fn the_state_lane_judges_its_triggered_ways() {
    let s = Sim::new("state", Some(ENFORCE));
    s.way("statekeep", "trigger: session-start\nscope: agent");
    s.way("statedrop", "trigger: session-start\nscope: agent");
    let host = s.host(1);
    let out = s.raw_hook("state", serde_json::json!({"hook_event_name": "UserPromptSubmit", "prompt": "tidy the release notes"}));
    let prompts = host.join().unwrap();
    assert!(prompts[0].contains("tidy the release notes"), "{}", prompts[0]);
    assert!(out.contains("# Marker statekeep") && !out.contains("# Marker statedrop"), "{out}");
    let r = s.wait_decision(|r| r["surface"] == "state");
    assert_eq!(r["judge"]["request"], "found", "{r:#}");
}

/// The post-tool lane judges the ways their postchecks asked for.
///
/// Unix only, like the other postcheck scenarios in session_sim.rs: the
/// postcheck is a shebang script, which Windows runs through bash.
#[cfg(unix)]
#[test]
fn the_post_tool_lane_judges_what_the_postchecks_ask_for() {
    let s = Sim::new("post", Some(ENFORCE));
    for id in ["postkeep", "postdrop"] {
        s.way(id, "scope: agent");
        let script = s.home.join(format!(".claude/hooks/ways/judged/{id}/postcheck.sh"));
        std::fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::PermissionsExt::set_mode(&mut std::fs::metadata(&script).unwrap().permissions(), 0o755);
        #[cfg(unix)]
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    }
    let host = s.host(1);
    let out = s.raw_hook("post-tool", serde_json::json!({"hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_input": {"command": "ls"}}));
    let prompts = host.join().unwrap();
    assert!(prompts[0].contains("Used the Bash tool."), "{}", prompts[0]);
    assert!(out.contains("# Marker postkeep") && !out.contains("# Marker postdrop"), "{out}");
    s.wait_decision(|r| r["surface"] == "post_tool");
}

/// A parent the judge blocks keeps its rank before its child in the record,
/// and a child the judge passed is still shown: the judge saw it.
#[test]
fn a_blocked_parent_keeps_its_place_before_its_child() {
    let s = Sim::new("tree", Some(ENFORCE));
    s.way("dropper", "commands: ^make\\b\nscope: agent");
    s.way("dropper/kid", "commands: ^make\\b\nscope: agent");
    let host = s.host(1);
    let out = s.hook("command", "Bash", serde_json::json!({"command": "make test", "description": "Run the tests"}), None);
    host.join().unwrap();
    assert!(!out.contains("# Marker dropper") && out.contains("# Marker kid"), "{out}");
    let r = s.wait_decision(|r| r["surface"] == "command");
    let order: Vec<(&str, &str)> = r["outcomes"].as_array().unwrap().iter().map(|o| (o["way"].as_str().unwrap(), o["result"].as_str().unwrap())).collect();
    assert_eq!(order, [("judged/dropper", "judge_block"), ("judged/dropper/kid", "fired")], "{r:#}");
}

/// A candidate the refire curve holds is never injected and never holds the
/// call. Under claude-code `held_sample: 1` starts no detached judge for it:
/// the way the judge blocked on the first call is judged again on the hot
/// path, and the held one is only logged as skipped, once.
#[test]
fn a_held_candidate_under_claude_code_starts_no_detached_judge() {
    let s = Sim::new("held", Some("engine: claude-code\nmode: enforce\nheld_sample: 1\n"));
    let first = s.host(1);
    s.bash(None);
    assert_eq!(first.join().unwrap().len(), 1);
    // A host waits for the next job before it comes, as the mod does.
    let host = s.host(1);
    std::thread::sleep(Duration::from_millis(200));
    let again = s.bash(None);
    assert!(!again.contains("# Marker keep"), "held by refire: {again}");
    let prompts = host.join().unwrap();
    assert_eq!(prompts.len(), 1, "the blocked way again on the hot path");
    assert!(prompts[0].contains("judged › drop") && !prompts[0].contains("judged › keep"), "{}", prompts[0]);
    assert!(!s.fired("judged/drop", None), "blocked twice, never fired");
    s.assert_no_detached_judge();
}

/// The fires `agent`'s thread has counted for `way` for the refire
/// back-off, 0 when none.
fn refire_count(s: &Sim, agent: &str, way: &str) -> u64 {
    let path = s.state.join("agent-ways/refire-counts").join(&s.session).join(format!("{agent}.json"));
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v[way].as_u64())
        .unwrap_or(0)
}

/// A candidate the judge blocks leaves the refire back-off as it was: no
/// fire counted toward its n and no clock started. The way it passed is
/// counted once.
#[test]
fn a_blocked_candidate_leaves_the_refire_count_and_clock_alone() {
    let s = Sim::new("backoff", Some(ENFORCE));
    let host = s.host(1);
    s.bash(Some("asub1"));
    assert_eq!(host.join().unwrap().len(), 1);
    assert_eq!(refire_count(&s, "asub1", "judged/drop"), 0, "a blocked way is not counted");
    assert_eq!(refire_count(&s, "asub1", "judged/keep"), 1);
    let clock = PathBuf::from(format!("{}/{}/agents/asub1/way-engagement/judged__drop.json", sessions_root(), s.session));
    assert!(!clock.exists(), "a blocked way starts no clock");
}
