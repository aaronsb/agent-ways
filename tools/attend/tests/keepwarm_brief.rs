// SPDX-FileCopyrightText: 2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `attend keepwarm brief` through the CLI: the conditions the continuance
//! judge reads, measured in a temp repository. A stand-in `ways` serves the
//! transcript reading and the settings, and a PATH built for each test
//! holds `gh` absent, failing, hanging or answering, so no test touches the
//! network.

mod common;
use common::Fixture;

/// A build without the keepwarm sensor refuses the brief, as it refuses
/// the other verbs that are not `check`.
#[cfg(not(feature = "sensor-keepwarm"))]
#[test]
fn without_the_sensor_the_brief_refuses() {
    let f = Fixture::new("kw-brief-no-sensor");
    assert_eq!(f.attend(&["keepwarm", "brief"]).status.code(), Some(1));
}

#[cfg(feature = "sensor-keepwarm")]
mod measured {
    use super::*;
    use serde_json::Value;
    use std::path::{Path, PathBuf};
    use std::process::Output;
    use std::time::{Duration, Instant};
    use ways_agent_core::continuance::{Conditions, Measured, Relays};

    /// The key set of #1393's fixture, which the output must match exactly.
    const FIXTURE: &str = include_str!("../../ways-agent-core/tests/fixtures/conditions.json");

    const WRAP_ID: &str = "20261127T000000Z-abcdef12";

    fn now() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
    }

    fn on_path(name: &str) -> PathBuf {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|d| d.join(name))
            .find(|p| p.is_file())
            .unwrap_or_else(|| panic!("no {name} on PATH"))
    }

    fn script(path: &Path, body: &str) {
        std::fs::write(path, format!("#!/bin/sh\n{body}")).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A fixture whose project is a git repository, and a PATH of the
    /// tools the brief may run and nothing else: no `gh` unless a test
    /// writes one.
    struct Repo {
        f: Fixture,
        shims: PathBuf,
        path: std::ffi::OsString,
    }

    impl Repo {
        fn new(tag: &str) -> Self {
            let f = Fixture::new(tag);
            let shims = f.home.join("shims");
            let tools = f.home.join("tools");
            std::fs::create_dir_all(&shims).unwrap();
            std::fs::create_dir_all(&tools).unwrap();
            for t in ["git", "sh", "cat", "sleep"] {
                std::os::unix::fs::symlink(on_path(t), tools.join(t)).unwrap();
            }
            let reading = f.home.join("brief-reading.json");
            let settings = f.home.join("brief-settings.json");
            std::fs::write(
                &reading,
                r#"{"tokens_used":250000,"tokens_total":1000000,"window_source":"model_table","model":"claude-opus-5-5","usage_tail":[]}"#,
            )
            .unwrap();
            std::fs::write(&settings, r#"{"view":"effective","keys":{"mods.attend.continuity.max_relays":{"value":4}}}"#).unwrap();
            script(
                &shims.join("ways"),
                &format!("if [ \"$1\" = settings ]; then exec cat '{}'; fi\nexec cat '{}'\n", settings.display(), reading.display()),
            );
            let path = std::env::join_paths([&shims, &tools]).unwrap();
            let r = Repo { f, shims, path };
            r.git(&["init", "-q", "-b", "main"]);
            r
        }

        fn origin(&self) -> &Path {
            Path::new(&self.f.origin)
        }

        fn git(&self, args: &[&str]) -> String {
            let out = std::process::Command::new(on_path("git"))
                .args(["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", "-c", "commit.gpgsign=false"])
                .args(args)
                .current_dir(self.origin())
                .env("HOME", &self.f.home)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }

        fn commit(&self, file: &str, msg: &str) -> String {
            std::fs::write(self.origin().join(file), msg).unwrap();
            self.git(&["add", file]);
            self.git(&["commit", "-q", "-m", msg]);
            self.git(&["rev-parse", "--short", "HEAD"])
        }

        /// The handoff, whose first line names `head`.
        fn handoff(&self, head: &str) -> PathBuf {
            let path = self.origin().join(".claude").join("handoff.wrap.md");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, format!("2026-11-27 {head} wrap={WRAP_ID}\nContinue work on the brief.\n")).unwrap();
            path
        }

        fn gh(&self, body: &str) {
            script(&self.shims.join("gh"), body);
        }

        fn brief_with(&self, args: &[&str], stdin: Option<&str>) -> (Output, Duration) {
            let mut cmd = self.f.command();
            // git looks for no repository above the fixture's home.
            cmd.env("PATH", &self.path).env("GIT_CEILING_DIRECTORIES", &self.f.home).args(["keepwarm", "brief"]).args(args);
            let started = Instant::now();
            let out = match stdin {
                None => cmd.output().unwrap(),
                Some(text) => {
                    use std::io::Write;
                    let mut child = cmd.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().unwrap();
                    child.stdin.take().unwrap().write_all(text.as_bytes()).unwrap();
                    child.wait_with_output().unwrap()
                }
            };
            (out, started.elapsed())
        }

        /// The brief's JSON, which must exit 0, parse as the judge's
        /// `Conditions` and hold exactly the fixture's keys.
        fn brief(&self, args: &[&str]) -> (Value, Conditions, Duration) {
            let (out, took) = self.brief_with(args, None);
            parsed(out, took)
        }
    }

    fn parsed(out: Output, took: Duration) -> (Value, Conditions, Duration) {
        assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stderr));
        let text = String::from_utf8(out.stdout).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"));
        let c: Conditions = serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("not Conditions: {e}: {text}"));
        let want: Value = serde_json::from_str(FIXTURE).unwrap();
        let keys = |v: &Value| v.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
        assert_eq!(keys(&v), keys(&want), "{text}");
        (v, c, took)
    }

    fn lines(v: &Value, key: &str) -> Vec<String> {
        v[key].as_array().unwrap_or_else(|| panic!("{key}: {v}")).iter().map(|l| l.as_str().unwrap().to_string()).collect()
    }

    /// Acceptance: in a repo whose handoff names an older head, the commits
    /// and merges since it are listed, and the older head is not.
    #[test]
    fn the_commits_since_the_handoffs_head_are_listed() {
        let r = Repo::new("kw-brief-commits");
        let wrapped = r.commit("a.txt", "before the wrap");
        r.handoff(&wrapped);
        r.commit("b.txt", "after the wrap");
        r.git(&["switch", "-q", "-c", "side"]);
        r.commit("c.txt", "on the side branch");
        r.git(&["switch", "-q", "main"]);
        r.git(&["merge", "-q", "--no-ff", "-m", "Merge branch side", "side"]);
        std::fs::write(r.origin().join("b.txt"), "changed").unwrap();

        let (v, _, _) = r.brief(&[]);
        let commits = lines(&v, "commits_since_wrap");
        assert_eq!(commits.len(), 3, "{commits:?}");
        assert!(commits[0].ends_with(" Merge branch side"), "{commits:?}");
        assert!(commits.iter().any(|c| c.ends_with(" after the wrap")), "{commits:?}");
        assert!(commits.iter().any(|c| c.ends_with(" on the side branch")), "{commits:?}");
        assert!(!commits.iter().any(|c| c.contains("before the wrap")), "{commits:?}");
        let merges = lines(&v, "merges_since_wrap");
        assert_eq!(merges.len(), 1, "{merges:?}");
        assert!(merges[0].ends_with(" Merge branch side"), "{merges:?}");
        let status = v["git_status"].as_str().unwrap();
        assert!(status.contains(" M b.txt"), "{status:?}");
        assert!(status.contains("?? .claude/"), "{status:?}");

        // The handoff passed by path gives the same; a head the repo does
        // not hold, or a handoff that is not there, gives unknown.
        let other = r.f.home.join("elsewhere.md");
        std::fs::write(&other, format!("2026-11-27 {wrapped} wrap={WRAP_ID}\n")).unwrap();
        let (v, _, _) = r.brief(&["--handoff", other.to_str().unwrap()]);
        assert_eq!(lines(&v, "commits_since_wrap"), commits);
        std::fs::write(&other, format!("2026-11-27 deadbeef wrap={WRAP_ID}\n")).unwrap();
        let (v, _, _) = r.brief(&["--handoff", other.to_str().unwrap()]);
        assert_eq!((&v["commits_since_wrap"], &v["merges_since_wrap"]), (&"unknown".into(), &"unknown".into()));
        let (v, _, _) = r.brief(&["--handoff", r.f.home.join("missing.md").to_str().unwrap()]);
        assert_eq!(v["commits_since_wrap"], "unknown");
        // At the head itself nothing has landed since.
        r.handoff(&r.git(&["rev-parse", "--short", "HEAD"]));
        let (v, _, _) = r.brief(&[]);
        assert_eq!(v["commits_since_wrap"], serde_json::json!([]));
    }

    /// Acceptance: with `gh` absent, failing as it does offline, or
    /// hanging, `open_prs` is unknown and the verb exits 0 within 5 s.
    #[test]
    fn without_gh_open_prs_is_unknown_within_five_seconds() {
        let r = Repo::new("kw-brief-nogh");
        r.commit("a.txt", "first");
        for case in ["absent", "offline", "hanging"] {
            match case {
                "absent" => {}
                "offline" => r.gh("echo 'error connecting to api.github.com' >&2\nexit 1\n"),
                _ => r.gh("exec sleep 30\n"),
            }
            let (v, _, took) = r.brief(&[]);
            assert_eq!(v["open_prs"], "unknown", "{case}: {v}");
            assert!(took < Duration::from_secs(5), "{case}: took {took:?}");
            // The other probes still measured.
            assert!(v["git_status"].is_string() && v["git_status"] != "unknown", "{case}: {v}");
        }
    }

    /// `gh` answering: the branch's open PRs, each with its checks rolled
    /// up to one state.
    #[test]
    fn open_prs_come_from_gh_for_the_current_branch() {
        let r = Repo::new("kw-brief-gh");
        r.commit("a.txt", "first");
        r.git(&["switch", "-q", "-c", "card/1396"]);
        let args = r.f.home.join("gh-args");
        r.gh(&format!(
            "echo \"$@\" > '{}'\ncat <<'EOF'\n{}\nEOF\n",
            args.display(),
            r#"[{"number":7,"title":"one","statusCheckRollup":[{"__typename":"CheckRun","status":"COMPLETED","conclusion":"SUCCESS"},{"__typename":"CheckRun","status":"COMPLETED","conclusion":"FAILURE"}]},
{"number":8,"title":"two","statusCheckRollup":[{"__typename":"CheckRun","status":"IN_PROGRESS","conclusion":""},{"__typename":"StatusContext","state":"SUCCESS"}]},
{"number":9,"title":"three","statusCheckRollup":[{"__typename":"CheckRun","status":"COMPLETED","conclusion":"SKIPPED"},{"__typename":"StatusContext","state":"SUCCESS"}]},
{"number":10,"title":"four","statusCheckRollup":[]}]"#
        ));
        let (v, _, _) = r.brief(&[]);
        assert_eq!(
            v["open_prs"],
            serde_json::json!([
                {"number": 7, "title": "one", "checks": "fail"},
                {"number": 8, "title": "two", "checks": "pending"},
                {"number": 9, "title": "three", "checks": "pass"},
                {"number": 10, "title": "four", "checks": "none"},
            ])
        );
        let called = std::fs::read_to_string(&args).unwrap();
        assert!(called.contains("pr list --head card/1396 --state open"), "{called}");
    }

    /// The keepwarm reading, the ledger and the settings: the fill, the
    /// price of a ping, the minutes since the operator's prompt, the relays.
    #[test]
    fn the_keepwarm_measures_come_from_the_reading_and_the_ledger() {
        let r = Repo::new("kw-brief-keepwarm");
        let ledger = r.f.cache().join("keepwarm").join(format!("{}.ledger.json", r.f.sid));
        std::fs::create_dir_all(ledger.parent().unwrap()).unwrap();
        let at = now() - 42 * 60 - 10;
        std::fs::write(
            &ledger,
            format!(r#"{{"misses":[],"last_ping":null,"stopped":null,"pinged_at":null,"updated_at":0,"policy":{{"operator_at":{at},"relays":2}}}}"#),
        )
        .unwrap();
        let (v, c, _) = r.brief(&[]);
        assert_eq!(v["fill_tokens"], 250_000);
        assert_eq!(v["fill_pct"], 25.0);
        assert_eq!(v["price_per_ping_usd"], 0.05);
        assert_eq!(v["minutes_since_operator_prompt"], 42);
        assert_eq!(c.relays, Measured::Known(Relays { n: 2, max: 4 }));

        // The window the mod passes wins over the one ways resolved.
        let (out, took) = r.brief_with(&[], Some(r#"{"window": 400000}"#));
        let (v, _, _) = parsed(out, took);
        assert_eq!(v["fill_pct"], 62.5);
    }

    /// Without a ledger the policy's measures are unknown, and without a
    /// repository so are git's; the verb still exits 0.
    #[test]
    fn what_cannot_be_read_is_unknown() {
        let r = Repo::new("kw-brief-unknown");
        std::fs::remove_dir_all(r.origin().join(".git")).unwrap();
        let (v, _, _) = r.brief(&[]);
        for key in ["minutes_since_operator_prompt", "relays", "git_status", "commits_since_wrap", "merges_since_wrap", "open_prs"] {
            assert_eq!(v[key], "unknown", "{key}: {v}");
        }
        // The mod's own measures are unknown when it passes none.
        for key in ["agents_in_flight", "shells_in_flight", "watchers_in_flight", "tasks_open", "tasks_in_progress", "tasks_closed_since_wrap"] {
            assert_eq!(v[key], "unknown", "{key}: {v}");
        }
        assert_eq!(v["fill_tokens"], 250_000);
    }

    /// What only the mod sees comes in on stdin and is merged: the agents
    /// and the task list. A key attend measures itself is not taken from
    /// stdin, and a value of the wrong shape is unknown.
    #[test]
    fn the_mods_measures_are_merged_from_stdin() {
        let r = Repo::new("kw-brief-stdin");
        r.commit("a.txt", "first");
        let stdin = r#"{"agents_in_flight":["card-1395 (running)"],"tasks_open":["[gh#1396] the brief"],"tasks_in_progress":[],
            "tasks_closed_since_wrap":"unknown","git_status":"from the mod","fill_tokens":1,"threads_awaiting_reply":9,"shells_in_flight":7}"#;
        let (out, took) = r.brief_with(&[], Some(stdin));
        let (v, _, _) = parsed(out, took);
        assert_eq!(v["agents_in_flight"], serde_json::json!(["card-1395 (running)"]));
        assert_eq!(v["tasks_open"], serde_json::json!(["[gh#1396] the brief"]));
        assert_eq!(v["tasks_in_progress"], serde_json::json!([]));
        assert_eq!(v["tasks_closed_since_wrap"], "unknown");
        assert_eq!(v["shells_in_flight"], "unknown");
        assert_ne!(v["git_status"], "from the mod");
        assert_eq!(v["fill_tokens"], 250_000);
        assert_eq!(v["threads_awaiting_reply"], 0);
        // Input that is not JSON leaves the mod's measures unknown.
        let (out, took) = r.brief_with(&[], Some("not json"));
        let (v, _, _) = parsed(out, took);
        assert_eq!(v["agents_in_flight"], "unknown");
    }

    /// The two counts come from one read of `attend message awaiting`: a
    /// question to the operator counts there, and not as a peer thread.
    #[test]
    fn an_operator_question_is_not_a_peer_thread() {
        let r = Repo::new("kw-brief-awaiting");
        let (v, _, _) = r.brief(&[]);
        assert_eq!((&v["threads_awaiting_reply"], &v["operator_questions"]), (&0.into(), &0.into()));
        r.f.ok(&["message", "send", "--to", "fixture-human", "--", "which way do we lean?"]);
        let awaiting: Value = serde_json::from_str(&r.f.ok(&["message", "awaiting", "--json"])).unwrap();
        assert_eq!((&awaiting["count"], &awaiting["operator"]), (&1.into(), &1.into()), "{awaiting}");
        let (v, _, _) = r.brief(&[]);
        assert_eq!((&v["threads_awaiting_reply"], &v["operator_questions"]), (&0.into(), &1.into()), "{v}");
    }
}
