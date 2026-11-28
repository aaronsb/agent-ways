// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `ways judge continuance` end to end: the built binary, an isolated HOME
//! and XDG tree, and on a key engine a stub agent on the socket in place of
//! the ways agent.
//!
//! References:
//! - ADR-418 §10: the continuance request through the judge interface.
#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const CONDITIONS: &str = include_str!("../../ways-agent-core/tests/fixtures/conditions.json");
const VERDICT: &str = r#"{"verdict":"stop","handoff":"sufficient","reason":"the card landed and nothing is pending","turns_left":0,"next_step":"start the brief card"}"#;

struct Fx {
    base: PathBuf,
    session: String,
}

impl Fx {
    fn new(name: &str, agent_yaml: &str) -> Fx {
        let base = std::env::temp_dir().join(format!("ways-cont-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        for d in ["home", "config/agent-ways/keys", "state", "runtime", "run", "proj"] {
            std::fs::create_dir_all(base.join(d)).unwrap();
        }
        for d in ["runtime", "run"] {
            std::fs::set_permissions(base.join(d), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        std::fs::write(base.join("config/agent-ways/agent.yaml"), agent_yaml).unwrap();
        std::fs::write(base.join("config/agent-ways/keys/anthropic"), "sk-ant-test-not-a-key\n").unwrap();
        std::fs::write(base.join("proj/handoff.wrap.md"), "<!-- wrap=w1 head=abc -->\nContinue work on the brief.\nDO NEXT: run the tests\n").unwrap();
        Fx { base, session: format!("cont-{name}-{}", std::process::id()) }
    }

    fn sock(&self) -> PathBuf {
        self.base.join("run/agent.sock")
    }

    fn ways(&self, args: &[&str], stdin: &str) -> Output {
        let mut c = Command::new(env!("CARGO_BIN_EXE_ways"));
        c.args(args)
            .current_dir(self.base.join("proj"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self.base.join("home"))
            .env("XDG_CONFIG_HOME", self.base.join("config"))
            .env("XDG_STATE_HOME", self.base.join("state"))
            .env("XDG_RUNTIME_DIR", self.base.join("runtime"))
            .env("XDG_CACHE_HOME", self.base.join("cache"))
            .env("XDG_DATA_HOME", self.base.join("data"))
            .env("WAYS_AGENT_SOCK", self.sock())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c.spawn().unwrap();
        child.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
        child.wait_with_output().unwrap()
    }

    fn continuance(&self) -> Output {
        self.ways(&["judge", "continuance", "--session", &self.session, "--handoff", "handoff.wrap.md"], CONDITIONS)
    }

    fn events(&self) -> String {
        std::fs::read_to_string(self.base.join("state/agent-ways/events.jsonl")).unwrap_or_default()
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn line(out: &Output) -> serde_json::Value {
    assert!(out.status.success(), "exit {:?}: {}", out.status, String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(text.lines().count(), 1, "one line: {text:?}");
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text:?}"))
}

#[test]
fn on_a_key_engine_a_stub_agent_answers_and_the_verb_prints_the_verdict() {
    let fx = Fx::new("key", "engine: anthropic\n");
    let listener = UnixListener::bind(fx.sock()).unwrap();
    let agent = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut request = String::new();
        BufReader::new(&stream).read_line(&mut request).unwrap();
        let reply = serde_json::json!({
            // The current core, so the client does not retire the stub.
            "agent": {"version": "0", "core": ways_agent_core::protocol::CORE_VERSION, "exe": "/stub"},
            "kind": "continued",
            "engine": "anthropic",
            "provider": "anthropic",
            "model": "claude-haiku-5-5",
            "verdict": serde_json::from_str::<serde_json::Value>(VERDICT).unwrap(),
            "latency_ms": 800,
            "call": {"engine": "anthropic", "provider": "anthropic", "model": "claude-haiku-5-5", "candidates": 0,
                     "usage": {"input_tokens": 3000, "output_tokens": 90}, "cost_usd": 0.000345, "cost_source": "price_table"}
        });
        (&stream).write_all(format!("{reply}\n").as_bytes()).unwrap();
        serde_json::from_str::<serde_json::Value>(&request).unwrap()
    });
    let out = fx.continuance();
    let request = agent.join().unwrap();
    assert_eq!((request["op"].as_str(), request["engine"].as_str()), (Some("continuance"), Some("anthropic")));
    assert_eq!(request["deadline_ms"], 30_000);
    assert!(request["handoff"].as_str().unwrap().contains("DO NEXT: run the tests"));
    assert_eq!(line(&out), serde_json::from_str::<serde_json::Value>(VERDICT).unwrap());
    let events = fx.events();
    let call = events.lines().find(|l| l.contains("\"judge_call\"")).unwrap_or_else(|| panic!("no judge_call: {events}"));
    assert!(call.contains("\"purpose\":\"continuance\"") && call.contains("\"model\":\"claude-haiku-5-5\""), "{call}");
}

#[test]
fn with_the_agent_unreachable_it_prints_a_fallback_naming_why_and_exits_0() {
    let fx = Fx::new("absent", "engine: anthropic\n");
    // A start failed moments ago: the client does not start another, so
    // nothing outside the fixture is spawned.
    let marker = fx.base.join("state/agent-ways/agent/start-failed");
    std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
    std::fs::write(&marker, "").unwrap();
    let v = line(&fx.continuance());
    assert_eq!(v, serde_json::json!({"fallback": "agent_start_backoff"}));
}

#[test]
fn on_claude_code_it_prints_the_job_and_parse_prints_the_verdict() {
    let fx = Fx::new("cc", "engine: claude-code\nmode: off\n");
    let host = fx.ways(&["judge", "host", "--session", &fx.session], "");
    assert!(host.status.success(), "{}", String::from_utf8_lossy(&host.stderr));
    let v = line(&fx.continuance());
    let job = &v["job"];
    assert_eq!((job["model"].as_str(), job["max_tokens"].as_u64(), job["timeout_ms"].as_u64()), (Some("claude-haiku-5-5"), Some(4096), Some(30_000)));
    assert!(job["prompt"].as_str().unwrap().contains("```handoff\n<!-- wrap=w1 head=abc -->\nContinue work on the brief."));
    let answer = serde_json::json!({"text": VERDICT, "usage": {"input_tokens": 3000, "output_tokens": 90, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 0}});
    let parsed = fx.ways(&["judge", "continuance", "--session", &fx.session, "--parse"], &answer.to_string());
    assert_eq!(line(&parsed), serde_json::from_str::<serde_json::Value>(VERDICT).unwrap());
    assert!(fx.events().contains("\"cost_source\":\"account\""), "{}", fx.events());
    // Prose besides the verdict falls back, and still exits 0.
    let prose = serde_json::json!({"text": format!("I say stop.\n{VERDICT}")});
    let v = line(&fx.ways(&["judge", "continuance", "--session", &fx.session, "--parse"], &prose.to_string()));
    assert!(v["fallback"].as_str().unwrap().starts_with("invalid_reply: "), "{v}");
    // Stdin that is not the conditions is a usage error.
    let bad = fx.ways(&["judge", "continuance", "--session", &fx.session, "--handoff", "handoff.wrap.md"], "{}");
    assert_eq!(bad.status.code(), Some(2));
}
