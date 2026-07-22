//! `attend whoami` — print this session's canonical bus identity.
//!
//! The CLI accessor for the `(sessionId ∩ origin_path)` derivation
//! (issue #378), so external consumers — hooks, scripts, the planned
//! drain checkpoint (ADR-171 research) — obtain the stable key by
//! shelling out to attend instead of re-implementing resolution or
//! reading attend-owned state. CLI is the contract.
//!
//! `--machine` emits `key=value` lines of ONLY the stable fields.
//! The display name (nickname + instance suffix) appears in the
//! human table as context, but is deliberately absent from machine
//! output: ordinals are presentation and must never become keys.

use agent_identity::{Identity, TermCaps};

pub(crate) fn cmd_whoami(machine: bool) {
    let ident = attend_session::identity();

    if machine {
        println!("session_id={}", ident.session_id);
        println!("origin_path={}", ident.origin_path);
        println!("resolved={}", ident.resolved);
        return;
    }

    let caps = TermCaps::detect();
    let display = Identity::for_cwd(&ident.origin_path, caps);
    let instance = attend_instances::Registry::new()
        .lookup(&ident.origin_path, &ident.session_id);
    let rendered = match &instance {
        Some(suffix) => format!("{}-{}", display.nickname, suffix),
        None => display.nickname.to_string(),
    };

    let mut t = agent_fmt::Table::new(&["", "Value"]);
    t.add(vec!["session", ident.session_id.as_str()]);
    t.add(vec!["origin", ident.origin_path.as_str()]);
    t.add(vec![
        "resolved",
        if ident.resolved {
            "yes (session record)"
        } else {
            "no (pid/cwd fallback)"
        },
    ]);
    t.add(vec!["display", rendered.as_str()]);
    t.print();

    if !ident.resolved {
        eprintln!(
            "\n[attend] no Claude session owns this process — identity is a fallback; \
             registry and groups will not treat it as a session"
        );
    }
}
