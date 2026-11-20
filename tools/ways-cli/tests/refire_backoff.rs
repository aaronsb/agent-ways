// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! Refire back-off per agent thread.
//!
//! Each scenario runs `ways hook <event>` the way Claude Code does, against
//! an isolated HOME and XDG tree. The agents' transcripts are written by the
//! test, so it sets each thread's token position before each call. Every
//! transcript names `claude-haiku-4-5`, a 200k window, and the way `bo/w`
//! has `refire: 0.05`, so its half-life H is 10,000 tokens. A way refires
//! once its salience is strictly below one half, at H + 1 tokens after its
//! last fire; after n fires in a thread it waits H × g^(n-1) + 1.
//!
//! References:
//! - ADR-711: the back-off schedule, per-thread counts, the compaction
//!   grant, and what pulls and SubagentStart injections count.

use std::path::PathBuf;
use std::process::{Command, Stdio};

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

/// The half-life of `bo/w` in tokens: 0.05 of a 200k window.
const H: u64 = 10_000;
const MODEL: &str = "claude-haiku-4-5";

struct Sim {
    base: PathBuf,
    home: PathBuf,
    state: PathBuf,
    project: PathBuf,
    session: String,
}

impl Sim {
    /// A tree with `bo/w` on `^git commit` (`refire: 0.05`), `bo/once` on
    /// `^git tag` (`refire: once`), and `bo/sub` for a dispatch that says
    /// "deploy", which reaches subagents. `config` is written as the user's
    /// config.yaml when given.
    fn new(name: &str, config: Option<&str>) -> Sim {
        let base = std::env::temp_dir().join(format!("ways-backoff-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let home = base.join("home");
        let state = base.join("state");
        let project = base.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let ways = home.join(".claude/hooks/ways/bo");
        for (id, fields) in [
            ("w", "commands: ^git\\ commit\nscope: agent\nrefire: 0.05"),
            ("once", "commands: ^git\\ tag\nscope: agent\nrefire: once"),
            ("sub", "pattern: \\bdeploy\\b\npattern_strict: true\ncommands: ^make\\ deploy\nscope: agent, subagent\nrefire: 0.05"),
        ] {
            let dir = ways.join(id);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("{id}.md")), format!("---\ndescription: test way {id}\n{fields}\n---\n# Marker {id}\n\nbody of {id}\n")).unwrap();
        }
        if let Some(yaml) = config {
            let dir = home.join(".config/agent-ways");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("config.yaml"), yaml).unwrap();
        }
        let session = format!("backoff-{name}-{}", std::process::id());
        let _ = std::fs::remove_dir_all(format!("{}/{session}", sessions_root()));
        let sim = Sim { base, home, state, project, session };
        sim.at(None, 0);
        sim
    }

    fn transcripts(&self) -> PathBuf {
        let slug: String = self.project.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
        self.home.join(".claude/projects").join(slug)
    }

    fn transcript(&self) -> PathBuf {
        self.transcripts().join(format!("{}.jsonl", self.session))
    }

    /// Sets `agent`'s token position (main: `None`) to `tokens`, with one
    /// assistant turn of `model` that reports it.
    fn at_model(&self, agent: Option<&str>, tokens: u64, model: &str) {
        let path = match agent {
            None => self.transcript(),
            Some(a) => self.transcripts().join(&self.session).join(format!("subagents/agent-{a}.jsonl")),
        };
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let line = format!(
            r#"{{"type":"assistant","message":{{"role":"assistant","model":"{model}","content":[{{"type":"text","text":"working"}}],"usage":{{"input_tokens":{tokens},"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}}}}"#
        );
        std::fs::write(path, format!("{line}\n")).unwrap();
    }

    fn at(&self, agent: Option<&str>, tokens: u64) {
        self.at_model(agent, tokens, MODEL);
    }

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
        for var in [
            "CLAUDE_PROJECT_DIR",
            "CLAUDE_AGENT_ID",
            "CLAUDE_SESSION_ID",
            "CLAUDE_CODE_SESSION_ID",
            "CLAUDE_CONFIG_DIR",
            "CLAUDE_CONTEXT_WINDOW",
            "WAYS_AGENT_SOCK",
            "WAYS_CLAUDE_BIN",
        ] {
            cmd.env_remove(var);
        }
        cmd
    }

    /// Runs `ways hook <event>` on `payload`, with the session, cwd and
    /// transcript filled in, and `agent_id` when given; returns stdout.
    fn hook(&self, event: &str, mut payload: serde_json::Value, agent: Option<&str>) -> String {
        payload["session_id"] = self.session.clone().into();
        payload["cwd"] = self.project.to_string_lossy().into_owned().into();
        payload["transcript_path"] = self.transcript().to_string_lossy().into_owned().into();
        if let Some(a) = agent {
            payload["agent_id"] = a.into();
        }
        let mut child = self.ways().args(["hook", event]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        std::io::Write::write_all(&mut child.stdin.take().unwrap(), payload.to_string().as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "hook {event}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    fn bash(&self, agent: Option<&str>, command: &str) -> String {
        self.hook(
            "command",
            serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": {"command": command}}),
            agent,
        )
    }

    /// Whether a `git commit` from `agent` at `tokens` delivers `bo/w`.
    fn commit_at(&self, agent: Option<&str>, tokens: u64) -> bool {
        self.at(agent, tokens);
        self.bash(agent, "git commit -m x").contains("# Marker w")
    }

    /// Compaction: SessionStart with `source: compact`.
    fn compact(&self) {
        self.hook("session-start", serde_json::json!({"hook_event_name": "SessionStart", "source": "compact"}), None);
    }

    /// The fire rows of this session (`way_fired` and `way_redisclosed`)
    /// for `way`, oldest first.
    fn fires(&self, way: &str) -> Vec<serde_json::Value> {
        self.events(|v| (v["event"] == "way_fired" || v["event"] == "way_redisclosed") && v["way"] == way)
    }

    fn events(&self, pick: impl Fn(&serde_json::Value) -> bool) -> Vec<serde_json::Value> {
        std::fs::read_to_string(self.state.join("agent-ways/events.jsonl"))
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter(|v| v["session"] == self.session.as_str() && pick(v))
            .collect()
    }

    /// The thread's stored fire count for `way`, 0 when none is stored.
    fn count(&self, agent: &str, way: &str) -> u64 {
        let path = self.state.join("agent-ways/refire-counts").join(&self.session).join(format!("{agent}.json"));
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v[way].as_u64())
            .unwrap_or(0)
    }
}

impl Drop for Sim {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(format!("{}/{}", sessions_root(), self.session));
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// The field `key` of each row, as a number.
fn field(rows: &[serde_json::Value], key: &str) -> Vec<u64> {
    rows.iter().map(|r| r[key].as_str().and_then(|s| s.parse().ok()).or_else(|| r[key].as_u64()).unwrap_or(u64::MAX)).collect()
}

/// After the first fire the way waits H, then 2H, then 4H; each fire row
/// carries its n and the threshold the next refire waits.
#[test]
fn the_nth_refire_waits_h_times_two_to_the_n_minus_one() {
    let s = Sim::new("schedule", None);
    let mut t = 1_000;
    assert!(s.commit_at(None, t), "first fire");
    for (n, wait) in [(2, H), (3, 2 * H), (4, 4 * H)] {
        assert!(!s.commit_at(None, t + wait), "fire {n} held at exactly {wait} tokens");
        t += wait + 1;
        assert!(s.commit_at(None, t), "fire {n} after {wait} + 1 tokens");
    }
    let fires = s.fires("bo/w");
    assert_eq!(field(&fires, "refire_n"), vec![1, 2, 3, 4], "{fires:#?}");
    assert_eq!(field(&fires, "token_position"), vec![1_000, 11_001, 31_002, 71_003]);
    // The threshold the next refire waits: H × 2^(n-1), plus the one token
    // past the half-life.
    assert_eq!(field(&fires, "refire_threshold"), vec![H + 1, 2 * H + 1, 4 * H + 1, 8 * H + 1]);
    assert_eq!(s.count("main", "bo/w"), 4);
    // Each held call logged one suppression row per fire window, with the
    // tick, n and the threshold in force.
    let held = s.events(|v| v["event"] == "way_suppressed" && v["way"] == "bo/w");
    assert_eq!(field(&held, "token_position"), vec![11_000, 31_001, 71_002], "{held:#?}");
    assert_eq!(field(&held, "refire_n"), vec![1, 2, 3]);
    assert_eq!(field(&held, "refire_threshold"), vec![H + 1, 2 * H + 1, 4 * H + 1]);
}

/// `refire_backoff: 1` restores the schedule before back-off: every
/// refire waits H.
#[test]
fn a_backoff_factor_of_one_keeps_every_wait_at_h() {
    let s = Sim::new("g1", Some("refire_backoff: 1\n"));
    let mut t = 1_000;
    assert!(s.commit_at(None, t));
    for _ in 0..3 {
        assert!(!s.commit_at(None, t + H));
        t += H + 1;
        assert!(s.commit_at(None, t));
    }
    assert_eq!(field(&s.fires("bo/w"), "refire_n"), vec![1, 2, 3, 4]);
}

/// Main and two subagents count apart: main's fires do not lengthen a
/// subagent's wait, and neither subagent moves the other's count.
#[test]
fn each_agent_thread_keeps_its_own_count() {
    let s = Sim::new("threads", None);
    // Main fires three times: its next refire waits 4H.
    assert!(s.commit_at(None, 1_000));
    assert!(s.commit_at(None, 11_001));
    assert!(s.commit_at(None, 31_002));
    // Each subagent's first fire is its own, and its first refire waits H.
    for sub in ["asub1", "asub2"] {
        assert!(s.commit_at(Some(sub), 1_000), "{sub} first fire");
        assert!(!s.commit_at(Some(sub), 1_000 + H), "{sub} held at H");
    }
    assert!(s.commit_at(Some("asub1"), 11_001), "asub1 refires after H");
    assert!(!s.commit_at(Some("asub1"), 11_001 + H), "asub1's second refire waits 2H");
    assert!(s.commit_at(Some("asub2"), 11_001), "asub2's wait is unmoved by asub1");
    assert_eq!((s.count("main", "bo/w"), s.count("asub1", "bo/w"), s.count("asub2", "bo/w")), (3, 2, 2));
    let by_agent = |a: &str| field(&s.fires("bo/w").into_iter().filter(|r| r["agent_id"] == a).collect::<Vec<_>>(), "refire_n");
    assert_eq!(by_agent("main"), vec![1, 2, 3]);
    assert_eq!(by_agent("asub1"), vec![1, 2]);
    assert_eq!(by_agent("asub2"), vec![1, 2]);
}

/// Compaction clears the clock but not the count: the first match in the
/// new window fires (the grant), and the refire after it waits
/// H × g^(n-1) with n counted across both windows. Fire and held rows
/// carry the compaction epoch.
#[test]
fn compaction_grants_one_fire_and_keeps_the_count() {
    let s = Sim::new("compact", None);
    assert!(s.commit_at(None, 1_000));
    assert!(s.commit_at(None, 11_001));
    assert!(s.commit_at(None, 31_002));
    s.compact();
    assert!(s.commit_at(None, 5_000), "the grant after compaction");
    assert_eq!(s.count("main", "bo/w"), 4, "the count survives compaction");
    assert!(!s.commit_at(None, 5_000 + 8 * H), "n = 4: the refire waits 8H");
    assert!(s.commit_at(None, 5_001 + 8 * H));
    let fires = s.fires("bo/w");
    assert_eq!(field(&fires, "refire_n"), vec![1, 2, 3, 4, 5]);
    assert_eq!(field(&fires, "compaction_epoch"), vec![0, 0, 0, 1, 1], "{fires:#?}");
    // A held row carries the epoch too.
    let held = s.events(|v| v["event"] == "way_suppressed" && v["way"] == "bo/w");
    assert_eq!(field(&held, "compaction_epoch"), vec![1], "{held:#?}");
}

/// `once` fires once per context window: never again within the window,
/// and once more after a compaction.
#[test]
fn once_means_once_per_context_window() {
    let s = Sim::new("once", None);
    let tag = |t: u64| {
        s.at(None, t);
        s.bash(None, "git tag v1").contains("# Marker once")
    };
    assert!(tag(1_000));
    assert!(!tag(150_000));
    assert!(!tag(199_000), "never refires inside one 200k window");
    s.compact();
    assert!(tag(2_000), "the grant in the new window");
    assert!(!tag(199_000));
    assert_eq!(field(&s.fires("bo/once"), "refire_n"), vec![1, 2]);
}

/// A `ways_read` pull resets the clock as it always has, but does not count
/// toward n: the next refire waits H from the pull, not 2H.
#[test]
fn a_pull_leaves_the_count_alone() {
    let s = Sim::new("pull", None);
    assert!(s.commit_at(None, 1_000));
    s.at(None, 5_000);
    s.hook(
        "pull",
        serde_json::json!({"hook_event_name": "PostToolUse", "tool_name": "mcp__agent-ways__ways_read", "tool_input": {"id": "bo/w"}}),
        None,
    );
    assert_eq!(s.count("main", "bo/w"), 1, "a pull is not counted");
    assert!(!s.commit_at(None, 5_000 + H), "the pull moved the clock");
    assert!(s.commit_at(None, 5_001 + H), "one fire counted: the refire waits H");
    assert_eq!(field(&s.fires("bo/w"), "refire_n"), vec![1, 2]);
}

/// A way injected at SubagentStart is the subagent's first fire: n = 1, and
/// its first refire waits H.
#[test]
fn a_subagent_start_injection_is_the_first_fire() {
    let s = Sim::new("inject", None);
    s.hook(
        "task",
        serde_json::json!({"hook_event_name": "PreToolUse", "tool_name": "Task", "tool_input": {"prompt": "deploy the service"}}),
        None,
    );
    s.at(Some("ainj"), 1_000);
    let start = s.hook("subagent-start", serde_json::json!({"hook_event_name": "SubagentStart", "agent_type": "general-purpose"}), Some("ainj"));
    assert!(start.contains("# Marker sub"), "{start}");
    assert_eq!(s.count("ainj", "bo/sub"), 1);
    assert_eq!(s.count("main", "bo/sub"), 0, "main's count is untouched");
    let deploy = |t: u64| {
        s.at(Some("ainj"), t);
        s.bash(Some("ainj"), "make deploy").contains("# Marker sub")
    };
    assert!(!deploy(1_000 + H));
    assert!(deploy(1_001 + H), "the first refire waits H");
    assert_eq!(s.count("ainj", "bo/sub"), 2);
}

/// `ways session ways` shows each (agent, way) row's backed-off threshold,
/// from that agent's own window and n.
#[test]
fn ways_list_shows_each_threads_threshold() {
    let s = Sim::new("list", None);
    assert!(s.commit_at(None, 1_000));
    assert!(s.commit_at(None, 11_001));
    assert!(s.commit_at(None, 31_002));
    assert!(s.commit_at(Some("alist"), 1_000));
    // The subagent runs a 1M model: its half-life is 50,000 tokens.
    s.at_model(Some("alist"), 2_000, "claude-opus-5");
    let out = s.ways().args(["session", "ways", "--session", &s.session, "--json"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let threshold = |agent: Option<&str>| {
        json["ways"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == "bo/w" && r["agent_id"].as_str() == agent)
            .unwrap_or_else(|| panic!("no row for {agent:?}: {json:#}"))["refire_threshold_k"]
            .as_u64()
            .unwrap()
    };
    assert_eq!(threshold(None), 40, "main: n = 3 in a 200k window waits 4H");
    assert_eq!(threshold(Some("alist")), 50, "the subagent: n = 1 in its own 1M window");
}

/// `ways session reset --confirm` is the explicit "ways stopped firing"
/// action, so it clears the thread counts and the compaction epoch with
/// the clocks: the next match fires as the session's first, at n = 1.
#[test]
fn session_reset_clears_the_counts_and_the_epoch() {
    let s = Sim::new("reset", None);
    assert!(s.commit_at(None, 1_000));
    assert!(s.commit_at(None, 11_001));
    s.compact();
    assert!(s.commit_at(None, 2_000));
    assert_eq!(s.count("main", "bo/w"), 3);
    let out = s.ways().args(["session", "reset", "--session", &s.session, "--confirm"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(!s.state.join("agent-ways/refire-counts").join(&s.session).exists(), "the session's counts are gone");
    assert!(s.commit_at(None, 2_500), "the clock is cleared too");
    let fires = s.fires("bo/w");
    assert_eq!(field(&fires, "refire_n"), vec![1, 2, 3, 1], "{fires:#?}");
    assert_eq!(field(&fires, "compaction_epoch"), vec![0, 0, 1, 0]);
}
