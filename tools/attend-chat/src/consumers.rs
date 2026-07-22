//! Live-consumer consult for `/purge` — the ADR-170 seen-set
//! tightening, co-shipped with the ADR-172 drain (Decision 5).
//!
//! A purge may not shred a message that a live, resolved agent session
//! which *receives* the channel has not yet consumed. This module
//! answers "who are the live consumers and what have they seen":
//! heartbeat-fresh session ids (ADR-129), filtered to the channel's
//! recipients, mapped to the seen-sets they have checkpointed
//! (`attend-state`). Humans (sanitized-username members, ADR-170) keep
//! no seen-set and so never hold a purge open; neither does a session
//! that has never checkpointed — consumption records are the only
//! thing consulted, exactly what ADR-172 makes trustworthy.

use std::collections::HashSet;
use std::path::Path;

/// Seen-sets of every live agent session that receives `channel`.
/// `None` = the base `#open` channel (every live agent receives it);
/// `Some(g)` = only live members of `@g`.
pub fn live_consumer_seen_sets(base: &Path, channel: Option<&str>) -> Vec<HashSet<String>> {
    let members: Option<Vec<String>> = channel.map(|g| {
        attend_groups::load_groups(base)
            .get(g)
            .map(|e| e.members.clone())
            .unwrap_or_default()
    });

    let mut out = Vec::new();
    let entries = match std::fs::read_dir(attend_heartbeat::heartbeat_dir()) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for e in entries.flatten() {
        let Some(sid) = e.file_name().to_str().map(String::from) else {
            continue;
        };
        if !attend_heartbeat::is_fresh(&sid, attend_heartbeat::DEFAULT_GRACE) {
            continue;
        }
        if let Some(ref m) = members {
            if !m.iter().any(|id| id == &sid) {
                continue;
            }
        }
        if let Some(set) = attend_state::seen_keys_for(&sid) {
            out.push(set);
        }
    }
    out
}
