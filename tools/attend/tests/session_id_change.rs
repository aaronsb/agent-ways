// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! A session id that changes under a listening session (#725 review,
//! finding 4).
//!
//! Claude Code's `/clear` gives the running process a new session id and
//! rewrites its session record. The enrollment record and the seen-set
//! follow the new id, so the drain under it keeps delivering and does not
//! deliver again what the old id saw.

mod common;
use common::Fixture;
use std::time::Duration;

/// #725 re-review, finding 5: a session enrolled only by a join keeps its
/// enrollment, seen-set and channels when `/clear` changes its id, with no
/// process of its own to move them.
#[test]
fn a_join_only_session_follows_an_id_change() {
    let f = Fixture::new("idjoin");
    f.ok(&["channel", "join", "side"]);
    f.put("_broadcast", "other-1-before", "before the clear", Duration::ZERO);
    assert!(f.drain("plain").contains("before the clear"));

    f.set_session_id("sess-join-new");
    f.put("_broadcast", "other-1-after", "open after", Duration::ZERO);
    f.put(&f.project_tray(), "other-1-to", "addressed after", Duration::ZERO);
    f.put("@side", "other-1-side", "channel after", Duration::ZERO);
    let out = f.drain("plain");
    for body in ["open after", "addressed after", "channel after"] {
        assert!(out.contains(body), "missing {body}: {out}");
    }
    assert!(!out.contains("before the clear"), "nothing seen before is delivered again: {out}");
    assert!(f.marker("sess-join-new").exists());
    assert!(!f.marker("sess-idjoin").exists());
    assert!(f.ok(&["channel", "--joined"]).contains("#side"));
}
