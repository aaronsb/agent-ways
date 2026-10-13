//! The one-release aliases of ADR-413 item 5, removed by issue #982: each old
//! flat verb names its grouped form on stderr, then runs it with the same
//! arguments, stdout and exit code.

mod common;

use common::Fixture;

/// Each old invocation, its grouped form, and the form the notice names.
const CASES: &[(&[&str], &[&str], &str)] = &[
    (&["send", "--json", "hi"], &["message", "send", "--json", "hi"], "message send"),
    (&["reply", "hi"], &["message", "reply", "hi"], "message reply"),
    (&["inbox", "--json"], &["message", "inbox", "--json"], "message inbox"),
    (&["inbox", "--drain", "--format", "plain"], &["message", "inbox", "--drain", "--format", "plain"], "message inbox"),
    (&["unread", "--json"], &["message", "unread", "--json"], "message unread"),
    (&["cleanup", "-n"], &["message", "cleanup", "-n"], "message cleanup"),
    (&["join", "ops", "--json"], &["channel", "join", "ops", "--json"], "channel join"),
    (&["leave", "ops", "--json"], &["channel", "leave", "ops", "--json"], "channel leave"),
    (&["channels", "--json"], &["channel", "--json"], "channel"),
    (&["channels", "--joined"], &["channel", "list", "--joined"], "channel"),
    (&["channels", "create", "ops", "the", "ops", "room"], &["channel", "create", "ops", "the", "ops", "room"], "channel"),
    (&["dissolve", "ghost"], &["channel", "dissolve", "ghost"], "channel dissolve"),
    (&["scene", "nosuch"], &["channel", "scene", "nosuch"], "channel scene"),
    (&["scenes"], &["channel", "scenes"], "channel scenes"),
    (&["peers", "--json"], &["peer", "list", "--json"], "peer list"),
    (&["whoami", "--machine"], &["peer", "whoami", "--machine"], "peer whoami"),
    (&["status", "--json"], &["peer", "status", "--json"], "peer status"),
    (&["sensors"], &["sensor", "list"], "sensor list"),
    (&["permissions", "audit"], &["sensor", "permissions"], "sensor permissions"),
    (&["tune"], &["config", "tune"], "config tune"),
];

/// A signal id carries its write time (`user-<nanos>-<seq>`); two runs of a
/// send differ only there.
fn without_signal_ids(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut digits = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        out.push_str(if digits.len() >= 16 { "N" } else { &digits });
        digits.clear();
        out.push(c);
    }
    out.push_str(if digits.len() >= 16 { "N" } else { &digits });
    out
}

#[test]
fn each_alias_names_the_new_form_and_runs_it() {
    for (old, new, named) in CASES {
        // A fresh fixture under one tag for each form, so both run against
        // the same state at the same paths.
        let tag = format!("alias-{}", old.join("-"));
        let aliased = Fixture::new(&tag).attend(old);
        let grouped = Fixture::new(&tag).attend(new);

        let stderr = String::from_utf8_lossy(&aliased.stderr);
        let notice = format!("note: `attend {}` is now `attend {named}`", old[0]);
        assert_eq!(stderr.lines().next(), Some(notice.as_str()), "{old:?}");
        assert_eq!(aliased.status.code(), grouped.status.code(), "{old:?}: {stderr}");
        assert_eq!(
            without_signal_ids(&String::from_utf8_lossy(&aliased.stdout)),
            without_signal_ids(&String::from_utf8_lossy(&grouped.stdout)),
            "{old:?}"
        );
        let grouped_err = String::from_utf8_lossy(&grouped.stderr);
        assert!(!grouped_err.contains("note: `attend"), "{new:?} carries no notice: {grouped_err}");
        let rest: Vec<&str> = stderr.lines().skip(1).collect();
        assert_eq!(without_signal_ids(&rest.join("\n")), without_signal_ids(grouped_err.trim_end()), "{old:?}");
    }
}

/// The cases cover a failing run, so a forwarded exit code is not only 0.
#[test]
fn an_alias_forwards_a_failure() {
    let f = Fixture::new("alias-fail");
    let out = f.attend(&["scene", "nosuch"]);
    assert!(!out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn aliases_are_hidden_from_help() {
    let f = Fixture::new("alias-help");
    let help = String::from_utf8_lossy(&f.attend(&["--help"]).stdout).into_owned();
    for (old, _, _) in CASES {
        assert!(!help.lines().any(|l| l.trim_start().starts_with(&format!("{} ", old[0]))), "{} listed:\n{help}", old[0]);
    }
}
