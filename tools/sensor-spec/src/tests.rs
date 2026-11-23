// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

use super::*;

fn y(s: &str) -> Value {
    serde_yaml::from_str(s).unwrap()
}

fn invalid(p: &Parsed) -> Vec<String> {
    p.problems.iter().filter(|p| p.severity == Severity::Invalid).map(|p| p.to_string()).collect()
}

const SLACK: &str = r#"
name: slack-unread
description: Unread messages in Slack
kind: mcp
lane: message
server: slack
tool: list_unread
args: { channel: general }
state:
  unread:  { type: number, from: "$.unread_count" }
  last_ts: { type: string, max: 32, from: "$.messages[0].ts" }
  user:    { type: string, max: 64, from: "$.messages[0].user.name" }
  text:    { type: string, max: 512, from: "$.messages[0].text" }
triggers:
  - name: new-message
    state: { unread: rises, last_ts: changed }
    match: any
    author: user
    magnitude: 1
    description: "the last message in slack mcp from {state.user} at {state.last_ts} says {state.text}"
"#;

#[test]
fn the_adr_slack_example_parses_on_the_message_lane() {
    let p = parse("slack-unread", &y(SLACK));
    assert!(p.problems.is_empty(), "{:?}", p.problems);
    let s = p.spec.unwrap();
    assert_eq!((s.kind, s.lane(), s.source()), (Kind::Mcp, Lane::Message, "slack list_unread".to_string()));
    assert_eq!(s.state["unread"].ty, StateType::Number);
    assert_eq!(s.state["text"].max, 512);
    let Test::State(t) = &s.triggers[0].test else { panic!() };
    assert_eq!(t["unread"], Change::Rises);
    assert_eq!(s.triggers[0].match_, Match::Any);
    assert_eq!(s.cadence, DEFAULTS);
}

#[test]
fn a_message_lane_with_no_author_runs_on_the_event_lane_and_lints_an_error() {
    let p = parse("w", &y("kind: command\nargv: [curl, x]\noutput: json\nlane: message\ntriggers:\n  - {name: a, path: \"$.items[*]\", key: id, on: added}\n"));
    let s = p.spec.unwrap();
    assert_eq!(s.lane(), Lane::Event);
    assert!(p.problems.iter().any(|p| p.severity == Severity::Error && p.at == "lane" && p.message.contains("trigger a has none")), "{:?}", p.problems);
    let p = parse("c", &y("kind: crate\ncrate: peers\nlane: message\n"));
    assert_eq!(p.spec.unwrap().lane(), Lane::Event);
    let p = parse("c", &y("kind: crate\ncrate: peers\nlane: message\nauthor: sender\n"));
    assert!(p.problems.is_empty(), "{:?}", p.problems);
    assert_eq!(p.spec.unwrap().lane(), Lane::Message);
}

#[test]
fn a_file_carrying_lane_exception_fails_lint_with_a_message_naming_its_removal() {
    let p = parse("k", &y("kind: crate\ncrate: keepwarm\nlane: message\nlane_exception: ADR-182\n"));
    let e: Vec<&Problem> = p.problems.iter().filter(|p| p.at == "lane_exception").collect();
    assert_eq!(e.len(), 1, "{:?}", p.problems);
    assert_eq!(e[0].severity, Severity::Error);
    assert_eq!(
        e[0].to_string(),
        "lane_exception: error: lane_exception is removed from the sensor spec and is ignored; delete it. peers and keepwarm keep the message lane without it"
    );
    let s = p.spec.expect("the sensor still loads, without the field");
    assert_eq!(s.lane(), Lane::Event, "no file field keeps an authorless sensor on the message lane");
    let p = parse("c", &y("kind: command\nargv: [x]\noutput: json\nlane_exception: mine\n"));
    assert!(p.spec.is_some());
    assert_eq!(p.problems.iter().map(|p| (p.at.as_str(), p.severity)).collect::<Vec<_>>(), [("lane_exception", Severity::Error)]);
}

#[test]
fn memory_and_expires_are_read_on_any_trigger() {
    let p = parse(
        "u",
        &y(r#"
kind: command
argv: [ways, manage, update, --what-if, --json]
output: json
state:
  n: { type: number, from: "$.n" }
triggers:
  - { name: behind, path: "$", on: enters, where: { behind: true }, memory: session, expires: session }
  - { name: count, state: { n: rises }, memory: store, expires: 900 }
  - { name: plain, path: "$.items[*]", key: id, on: added }
"#),
    );
    assert!(p.problems.is_empty(), "{:?}", p.problems);
    let t = p.spec.unwrap().triggers;
    let got: Vec<(&str, Memory, Option<Expires>)> = t.iter().map(|t| (t.name.as_str(), t.memory, t.expires)).collect();
    assert_eq!(
        got,
        [("behind", Memory::Session, Some(Expires::Session)), ("count", Memory::Store, Some(Expires::Seconds(900))), ("plain", Memory::Store, None)]
    );
    assert_eq!(t[2].expiry(), Expires::Seconds(3600), "with no expires, a trigger's signal lapses after the hour of the decay window");
    assert_eq!(t[0].expiry(), Expires::Session);
}

#[test]
fn session_memory_on_a_state_trigger_is_refused() {
    let p = parse("s", &y("kind: command\nargv: [x]\noutput: json\nstate: {n: {type: number}}\ntriggers: [{name: a, state: {n: rises}, memory: session}]\n"));
    assert!(p.spec.is_none());
    assert_eq!(invalid(&p), ["triggers.a.memory: memory: session does not apply to a state trigger, which compares against the last state its store kept; use store, or leave memory unset"]);
}

/// A `matches` pattern compiles when the file loads; one that does not
/// compile makes the sensor invalid, at its path.
#[test]
fn a_matches_pattern_compiles_when_the_file_loads() {
    let doc = |re: &str| y(&format!("kind: command\nargv: [x]\noutput: json\ntriggers: [{{name: t, path: \"$\", on: added, where: {{s: {{matches: \"{re}\"}}}}}}]\n"));
    let p = parse("s", &doc("^a+$"));
    let Test::Items { where_, .. } = &p.spec.unwrap().triggers[0].test else { panic!() };
    let Condition::Ops(ops) = &where_["s"] else { panic!() };
    let [Op::Matches(re)] = ops.as_slice() else { panic!("{ops:?}") };
    assert_eq!((re.0.as_str(), re.0.is_match("aaa"), re.0.is_match("ab")), ("^a+$", true, false));
    let bad = parse("s", &doc("(unclosed"));
    assert!(bad.spec.is_none());
    assert_eq!(invalid(&bad), ["triggers.t.where.s.matches: not a regular expression: error: unclosed group"]);
}

/// The fingerprint follows what a trigger tests, not its name, magnitude
/// or wording.
#[test]
fn a_trigger_s_fingerprint_changes_with_what_it_tests() {
    let fp = |t: &str| {
        let doc = format!("kind: command\nargv: [x]\noutput: json\nstate: {{n: {{type: number}}}}\ntriggers: [{t}]\n");
        let p = parse("s", &y(&doc));
        assert!(p.problems.is_empty(), "{t}: {:?}", p.problems);
        p.spec.unwrap().triggers[0].fingerprint()
    };
    let base = fp("{name: a, path: \"$\", on: enters, where: {behind: true}}");
    assert_eq!(base.len(), 16, "{base}");
    assert!(base.chars().all(|c| c.is_ascii_hexdigit()), "{base}");
    assert_eq!(base, fp("{name: a, path: \"$\", on: enters, where: {behind: true}}"), "stable for the same definition");
    assert_eq!(base, fp("{name: b, path: \"$\", on: enters, where: {behind: true}, magnitude: 3, description: other, memory: session, expires: 60}"));
    for changed in [
        "{name: a, path: \"$\", on: enters, where: {behind: false}}",
        "{name: a, path: \"$.x\", on: enters, where: {behind: true}}",
        "{name: a, path: \"$\", on: leaves, where: {behind: true}}",
        "{name: a, path: \"$\", key: id, on: enters, where: {behind: true}}",
        "{name: a, path: \"$\", on: enters, where: {behind: true}, match: any}",
        "{name: a, state: {n: rises}}",
    ] {
        assert_ne!(base, fp(changed), "{changed}");
    }
    assert_eq!(
        fp("{name: a, path: \"$\", on: enters, where: {b: 2, a: {above: 1.5, below: 9}}}"),
        fp("{where: {a: {below: 9, above: 1.5}, b: 2}, on: enters, path: \"$\", name: a}"),
        "the order the file writes its keys in does not matter"
    );
}

/// The fingerprint hashes the trigger's source fields as the file states
/// them, as JSON with sorted keys, so no change to the crate's own types
/// moves it. Both literals were computed outside the crate; a change here
/// re-baselines every stored trigger memory.
#[test]
fn a_trigger_s_fingerprint_is_pinned_to_its_source_fields() {
    let doc = "kind: command\nargv: [x]\noutput: json\ntriggers:\n  - {name: behind, path: \"$\", on: enters, where: {behind: true, \"wind_*\": {above: 2.5, in: [a, \"b\\\"c\"]}}, match: any, magnitude: 1}\n";
    let t = parse("s", &y(doc)).spec.unwrap().triggers.remove(0);
    assert_eq!(t.definition(), r#"{"match":"any","on":"enters","path":"$","where":{"behind":true,"wind_*":{"above":2.5,"in":["a","b\"c"]}}}"#);
    assert_eq!(t.fingerprint(), "e6c80f2ecd8d3c48");
}

#[test]
fn an_invalid_memory_or_expires_fails_with_a_message_saying_what_it_takes() {
    let base = "kind: command\nargv: [x]\noutput: json\ntriggers: [{name: a, path: \"$\", on: added, ";
    let expires = "triggers.a.expires: expires is a whole number of seconds from 1 to 2592000, or session";
    let memory = "triggers.a.memory: memory is store or session";
    for (field, want) in [
        ("expires: 1h", expires),
        ("expires: 0", expires),
        ("expires: -5", expires),
        ("expires: 2592001", expires),
        ("expires: 1.5", expires),
        ("expires: true", expires),
        ("expires: Session", expires),
        ("memory: forever", memory),
        ("memory: 1", memory),
    ] {
        let p = parse("s", &y(&format!("{base}{field}}}]\n")));
        assert!(p.spec.is_none(), "{field} should not load");
        assert_eq!(invalid(&p), [want], "{field}");
    }
    let p = parse("s", &y(&format!("{base}expires: 2592000}}]\n")));
    assert_eq!(p.spec.unwrap().triggers[0].expires, Some(Expires::Seconds(2_592_000)), "the bound itself is allowed");
}

#[test]
fn a_crate_file_keeps_its_own_fields_for_the_host() {
    let p = parse("processes", &y("kind: crate\ncrate: processes\ninterval: 30\nmin_interval: 5\nwatch: [cargo]\n"));
    let s = p.spec.unwrap();
    let Block::Crate(c) = &s.block else { panic!() };
    assert_eq!(c.fields.get("watch"), Some(&y("[cargo]")));
    assert_eq!((s.cadence.interval, s.cadence.min_interval), (30, 5));
}

#[test]
fn problems_name_their_field() {
    let cases = [
        ("kind: sql\n", "kind"),
        ("description: no kind\n", "kind"),
        ("kind: crate\n", "crate"),
        ("kind: command\noutput: json\n", "argv"),
        ("kind: command\nargv: [x]\noutput: csv\n", "output"),
        ("kind: command\nargv: [x]\noutput: json\nshell: bash\n", "shell"),
        ("kind: crate\ncrate: git\ninterval: 0\n", "interval"),
        ("kind: crate\ncrate: git\ninterval: 10\nmin_interval: 20\n", "min_interval"),
        ("kind: crate\ncrate: git\nlane: chat\n", "lane"),
        ("kind: crate\ncrate: git\ntriggers: [{name: a, path: \"$\", on: added}]\n", "triggers"),
        ("kind: mcp\ntool: t\n", "server"),
        ("kind: mcp\nserver: s\n", "tool"),
        ("kind: command\nargv: [x]\noutput: lines\nstate: {n: {type: number}}\n", "state"),
        ("kind: command\nargv: [x]\noutput: json\nstate: {Bad: {type: number}}\n", "state.Bad"),
        ("kind: command\nargv: [x]\noutput: json\nstate: {n: {type: object}}\n", "state.n.type"),
        ("kind: command\nargv: [x]\noutput: json\nstate: {s: {type: string, max: 2000}}\n", "state.s.max"),
        ("kind: command\nargv: [x]\noutput: json\nstate: {n: {type: number, max: 3}}\n", "state.n.max"),
        ("kind: command\nargv: [x]\noutput: json\nstate: {n: {type: number, from: \"$.a[*]\"}}\n", "state.n.from"),
        ("kind: command\nargv: [x]\noutput: json\ntriggers: [{name: a, path: \"$\", on: enters}]\n", "triggers.a.where"),
        ("kind: command\nargv: [x]\noutput: json\ntriggers: [{name: a, path: \"$\", on: added, where: {x: {near: 1}}}]\n", "triggers.a.where.x.near"),
        ("kind: command\nargv: [x]\noutput: json\ntriggers: [{name: a, state: {n: rises}}]\n", "triggers.a.state.n"),
        ("kind: command\nargv: [x]\noutput: json\nstate: {s: {type: string}}\ntriggers: [{name: a, state: {s: rises}}]\n", "triggers.a.state.s"),
        ("kind: command\nargv: [x]\noutput: json\nstate: {s: {type: string}}\ntriggers: [{name: a, state: {s: changed}, description: \"{state.t}\"}]\n", "triggers.a.description"),
        ("kind: command\nargv: [x]\noutput: json\nstate: {s: {type: string}}\ntriggers: [{name: a, state: {s: changed}, author: who}]\n", "triggers.a.author"),
        ("kind: command\nargv: [x]\noutput: json\ntriggers: [{name: a, path: \"$\", on: added}, {name: a, path: \"$\", on: removed}]\n", "triggers.a"),
        ("name: other\nkind: crate\ncrate: git\n", "name"),
        ("spec: 9\nkind: crate\ncrate: git\n", "spec"),
    ];
    for (text, at) in cases {
        let p = parse("s", &y(text));
        assert!(p.spec.is_none(), "{text} should not load");
        assert!(p.problems.iter().any(|p| p.at == at && p.severity == Severity::Invalid), "{text}: want a problem at {at}, got {:?}", invalid(&p));
    }
}

#[test]
fn seventeen_fields_are_past_the_bound_and_large_maximums_warn() {
    let many: String = (0..17).map(|i| format!("  f{i}: {{type: boolean}}\n")).collect();
    let p = parse("s", &y(&format!("kind: command\nargv: [x]\noutput: json\nstate:\n{many}")));
    assert!(invalid(&p).iter().any(|m| m.contains("17 fields")), "{:?}", p.problems);
    let p = parse("s", &y("kind: command\nargv: [x]\noutput: json\nstate:\n  a: {type: string, max: 1024}\n  b: {type: string, max: 1024}\n"));
    assert!(p.spec.is_some());
    assert!(p.problems.iter().any(|p| p.severity == Severity::Warning && p.at == "state"), "{:?}", p.problems);
}

#[test]
fn a_ways_file_carries_its_rules_as_written() {
    let p = parse("w", &y("kind: ways\nrules: {r: {}}\n"));
    assert!(matches!(p.spec.unwrap().block, Block::Ways(_)));
}

#[test]
fn a_probe_record_holds_only_for_what_it_was_taken_with() {
    let base = "kind: mcp\nserver: slack\ntool: list\nargs: {c: g}\nprobe: {server: slack, binding: user:0123, tool: list, args: {c: g}, calls_match: true, confirmed: true}\n";
    let holds = |text: &str| {
        let Block::Mcp(m) = parse("s", &y(text)).spec.unwrap().block else { panic!() };
        m.probe.as_ref().is_some_and(|p| m.holds(p))
    };
    assert!(holds(base));
    assert!(!holds(&base.replace("binding: user:0123, ", "")), "a record bound to no server definition does not hold");
    let keyed = "kind: mcp\nserver: slack\ntool: list\nprobes:\n  /p/a: {server: slack, binding: user:1, tool: list, calls_match: true, confirmed: true}\n  /p/b: {server: slack, binding: user:2, tool: list, calls_match: true, confirmed: false}\n";
    let Block::Mcp(m) = parse("s", &y(keyed)).spec.unwrap().block else { panic!() };
    assert_eq!(m.probes.keys().collect::<Vec<_>>(), ["/p/a", "/p/b"], "each project keeps its own record");
    assert!(m.holds(&m.probes["/p/a"]) && !m.holds(&m.probes["/p/b"]));
    assert!(!holds(&base.replace("args: {c: g}\nprobe", "args: {c: h}\nprobe")), "a change to args voids the record");
}
