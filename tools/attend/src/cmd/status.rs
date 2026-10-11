//! `attend status` — show running instances, signal counts, and focus.
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

    // ── Instances section
    if s.instances.is_empty() {
        t.add(vec!["instances", "(none)", ""]);
    } else {
        for (i, inst) in s.instances.iter().enumerate() {
            let label = if i == 0 { "instances" } else { "" };
            t.add(vec![label, &format!("PID {}", inst.pid), &inst.command]);
        }
    }

    // ── Separator
    t.add(vec!["", "", ""]);

    // ── Signals section
    t.add(vec!["signals", "project", &format!("{} pending", s.signals.project)]);
    t.add(vec!["", "#open", &format!("{} pending", s.signals.open)]);

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
