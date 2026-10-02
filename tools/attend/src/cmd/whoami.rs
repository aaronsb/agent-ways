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
//!
//! `--display` prints the display name alone, for status lines and
//! prompts that render it. It is the presentation accessor, kept apart
//! from `--machine` so the key contract stays free of ordinals.

use agent_identity::Identity;
use agent_theme::ColorDepth;

pub(crate) fn cmd_whoami(machine: bool, display: bool) {
    let ident = attend_presence::session::identity();

    if machine {
        for line in machine_lines(&ident) {
            println!("{line}");
        }
        return;
    }

    let rendered = display_name(&ident);
    if display {
        println!("{rendered}");
        return;
    }

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
    // Two processes with different XDG_CACHE_HOME share no mesh; the root
    // in use is how an operator sees that.
    let cache = attend_presence::cache::dir().to_string_lossy().into_owned();
    t.add(vec!["cache", cache.as_str()]);
    t.print();

    if !ident.resolved() {
        eprintln!(
            "\n[attend] identity is not fully resolved — the instance roster will \
             exclude this process; sends and group membership use the fallback id"
        );
    }
}

/// The rendered display name: the origin path's nickname, plus the
/// instance suffix when several sessions share that origin.
fn display_name(ident: &attend_presence::session::SessionIdentity) -> String {
    let nickname = Identity::for_cwd(&ident.origin_path, ColorDepth::detect()).nickname;
    let instance = attend_instances::Registry::new()
        .lookup(&ident.origin_path, &ident.session_id);
    join_display(nickname, instance.as_deref())
}

fn join_display(nickname: &str, instance: Option<&str>) -> String {
    match instance {
        Some(suffix) => format!("{nickname}-{suffix}"),
        None => nickname.to_string(),
    }
}

/// The `--machine` output contract: `key=value` lines of the stable
/// identity fields, nothing else. Downstream consumers (hooks, the
/// drain checkpoint) key on these; the display name is deliberately
/// absent — ordinals are presentation, never keys.
fn machine_lines(ident: &attend_presence::session::SessionIdentity) -> Vec<String> {
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
        let ident = attend_presence::session::SessionIdentity {
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

    #[test]
    fn display_name_carries_the_instance_suffix_when_present() {
        assert_eq!(join_display("Chaucer", None), "Chaucer");
        assert_eq!(join_display("Chaucer", Some("2")), "Chaucer-2");
    }
}
