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
        for line in machine_lines(&ident) {
            println!("{line}");
        }
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
        if ident.resolved() {
            "yes (session record)"
        } else if ident.session_resolved {
            "partial (session record has no cwd; origin is process cwd)"
        } else {
            "no (pid/cwd fallback)"
        },
    ]);
    t.add(vec!["display", rendered.as_str()]);
    t.print();

    if !ident.resolved() {
        eprintln!(
            "\n[attend] identity is not fully resolved — the instance roster will \
             exclude this process; sends and group membership use the fallback id"
        );
    }
}

/// The `--machine` output contract: `key=value` lines of the stable
/// identity fields, nothing else. Downstream consumers (hooks, the
/// drain checkpoint) key on these; the display name is deliberately
/// absent — ordinals are presentation, never keys.
fn machine_lines(ident: &attend_session::SessionIdentity) -> Vec<String> {
    vec![
        format!("session_id={}", ident.session_id),
        format!("origin_path={}", ident.origin_path),
        format!("resolved={}", ident.resolved()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_output_is_stable_fields_only() {
        let ident = attend_session::SessionIdentity {
            session_id: "sess-x".into(),
            origin_path: "/proj".into(),
            session_resolved: true,
            origin_resolved: true,
        };
        let lines = machine_lines(&ident);
        assert_eq!(
            lines,
            vec![
                "session_id=sess-x".to_string(),
                "origin_path=/proj".to_string(),
                "resolved=true".to_string(),
            ]
        );
        // Contract guard: no display/ordinal fields may creep in.
        assert!(lines.iter().all(|l| !l.contains("display") && !l.contains("instance")));
    }
}
