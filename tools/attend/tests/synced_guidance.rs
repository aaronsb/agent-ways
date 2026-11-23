// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The synchronized copies of attend's messaging guidance say alike what
//! each copy must carry: the `/attend` primer (`skills/attend/SKILL.md`)
//! and the attend way (`hooks/ways/softwaredev/environment/attend/attend.md`).
//! Both are read at test time from the repository; neither is part of
//! attend's binary.

use std::path::Path;

/// The two synchronized sources, by name, with their text.
fn sources() -> [(&'static str, String); 2] {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let read = |rel: &str| std::fs::read_to_string(repo.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"));
    [("SKILL.md", read("skills/attend/SKILL.md")), ("attend.md", read("hooks/ways/softwaredev/environment/attend/attend.md"))]
}

/// The synchronized sources say alike that the operator sets
/// `mods.attend.listening: active`, since auto mode refuses an agent's write
/// to its own project config (the clean-slate test's finding 7).
#[test]
fn the_synced_sources_say_the_operator_sets_listening_active() {
    const MARK: &str = "**Starting active is the operator's setting.**";
    let sources = sources();
    let paragraphs: Vec<(&str, &str)> = sources
        .iter()
        .map(|(name, text)| {
            let para = text.lines().find(|l| l.starts_with(MARK)).unwrap_or_else(|| panic!("{name} lacks the paragraph"));
            (*name, para)
        })
        .collect();
    for (name, para) in &paragraphs {
        assert!(para.contains("mods.attend.listening: active") && para.contains("auto mode"), "{name}: {para}");
        assert_eq!(*para, paragraphs[0].1, "{name} differs from {}", paragraphs[0].0);
    }
}

/// The synchronized sources say alike that `#open` reaches every listening
/// session, other projects included, and send project chatter to a named
/// channel or a direct message.
#[test]
fn the_synced_sources_say_open_reaches_every_project() {
    const LINE: &str = "`#open` reaches every listening session, in other projects too, so keep chatter about this project to a named channel or a direct message.";
    for (name, text) in sources() {
        assert_eq!(text.matches(LINE).count(), 1, "{name} lacks the #open line");
    }
}
