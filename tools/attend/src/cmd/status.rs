//! `attend peer status` — show every running `attend run` with the session it
//! serves, signal counts, and focus.
//! The state is `verbs::status`; this renders it.

pub(crate) fn cmd_status(json: bool) {
    let s = crate::verbs::status::status();
    if json {
        crate::cmd::print_json(&s);
        return;
    }

    // Single table: Section | Detail | Info
    let mut t = agent_fmt::Table::new(&["", "Detail", "Info"]);
    t.align(0, agent_fmt::Align::Left);

    // ── Instances section. `ps` lists every `attend run` on the machine,
    // one per session that runs attend, so each row names the session it
    // serves. Two rows are normal when two sessions run attend.
    if s.instances.is_empty() {
        t.add(vec!["instances", "(none)", ""]);
    } else {
        for (i, inst) in s.instances.iter().enumerate() {
            let label = if i == 0 { "instances" } else { "" };
            let info = match &inst.name {
                Some(name) => instance_info(name, inst.this_session, inst.duplicate),
                None => unowned_info(inst.pid, inst.reparented),
            };
            t.add(vec![label, &format!("PID {}", inst.pid), &info]);
        }
    }

    // ── Separator
    t.add(vec!["", "", ""]);

    // ── Signals section
    t.add(vec!["signals", "project", &format!("{} pending", s.signals.project)]);
    t.add(vec!["", "#open", &format!("{} pending", s.signals.open)]);
    t.add(vec!["", "this session", &s.unread]);

    // ── Separator
    t.add(vec!["", "", ""]);

    // ── Keepwarm section (ADR-182)
    t.add(vec!["keepwarm", &s.keepwarm, ""]);

    // ── Separator
    t.add(vec!["", "", ""]);

    // ── Focus section
    if s.channels.is_empty() {
        t.add(vec!["channels", "project only", ""]);
    } else {
        for (i, c) in s.channels.iter().enumerate() {
            let label = if i == 0 { "channels" } else { "" };
            let pin = if c.pinned { " (pinned)" } else { "" };
            let info = format!("{}{pin}", c.name);
            t.add(vec![label, &info, ""]);
        }
    }

    t.print();
    // Printed whole, outside the table, which would cut a long path.
    println!("cache: {}", s.cache.display());
}

/// What a row says when no Claude Code session record names the run. That
/// alone proves nothing (a record can be missing or unreadable), so the
/// run is called leftover only when it was reparented to init or a
/// subreaper.
fn unowned_info(pid: u32, reparented: bool) -> String {
    let base = format!("no Claude Code session found for pid {pid}");
    if reparented {
        format!("{base}; a leftover run, safe to stop")
    } else {
        base
    }
}

/// What an `attend run` row says about its owner. `name` is the owning
/// session's display name. The session lock (ADR-129) allows one run per
/// session, so a second is worth flagging.
fn instance_info(name: &str, mine: bool, duplicate: bool) -> String {
    let whose = if mine { "this session" } else { "another session" };
    if duplicate {
        format!("{name} ({whose}, duplicate run)")
    } else {
        format!("{name} ({whose})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_run_names_the_session_it_serves() {
        assert_eq!(instance_info("Chaucer-alpha", true, false), "Chaucer-alpha (this session)");
        assert_eq!(instance_info("McKay", false, false), "McKay (another session)");
    }

    #[test]
    fn a_second_run_for_one_session_is_flagged() {
        assert_eq!(instance_info("Chaucer", true, true), "Chaucer (this session, duplicate run)");
    }

    #[test]
    fn a_run_without_a_session_is_leftover_only_when_reparented() {
        assert_eq!(unowned_info(42, false), "no Claude Code session found for pid 42");
        assert_eq!(unowned_info(42, true), "no Claude Code session found for pid 42; a leftover run, safe to stop");
    }
}
