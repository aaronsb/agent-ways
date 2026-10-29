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
    assert_eq!((s.kind, s.lane(), s.source()), (Kind::Mcp, Lane::Message, "slack/list_unread".to_string()));
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
    let p = parse("k", &y("kind: crate\ncrate: keepwarm\nlane: message\nlane_exception: ADR-182\n"));
    assert_eq!(p.spec.unwrap().lane(), Lane::Message);
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
    let base = "kind: mcp\nserver: slack\ntool: list\nargs: {c: g}\nprobe: {server: slack, tool: list, args: {c: g}, calls_match: true, confirmed: true}\n";
    let Block::Mcp(m) = parse("s", &y(base)).spec.unwrap().block else { panic!() };
    assert!(m.probe_holds());
    let Block::Mcp(m) = parse("s", &y(&base.replace("args: {c: g}\nprobe", "args: {c: h}\nprobe"))).spec.unwrap().block else { panic!() };
    assert!(!m.probe_holds(), "a change to args voids the record");
}
