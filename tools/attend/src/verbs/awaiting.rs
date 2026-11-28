// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! `awaiting`: the messages this session sent that still wait for an answer.
//!
//! A message waits when this session sent it as a direct message, to a peer
//! session or to a person's member id, and no recipient has answered it. A
//! recipient answers with a later message that threads under it or is
//! addressed to this session, in a room this session receives. Where a
//! message names several sessions, an answer from any one of them answers
//! it. A post to `#open` or to a named channel never waits, even one
//! addressed to someone.
//!
//! A session that has exited, or that carries the finished mark, never
//! answers, so a message to it does not wait. A session counts as live as
//! the send path judges it. A message to a person always waits until they
//! answer.
//!
//! A message waits only while it lives on the bus: its file is still there,
//! and no delete or tombstone has marked it. An answer counts while its file
//! is there, marked or not, so a delete that clears read mail does not reopen
//! what it answered.
//!
//! The query reads the bus and writes nothing. It marks nothing read and
//! consults no seen-set.
//!
//! References:
//! - ADR-418 §6, §10 and §11: a question awaiting an answer counts as pending.
//! - ADR-136 item 3: a message lives until its tray or its sender's project
//!   is reaped.

use crate::util::{current_session, groups_for, signals_base};
use agent_identity::{parse_signal, signal_send_order, wire_line};
use attend_groups::tombstones::Marks;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The messages this session sent that wait for an answer.
#[derive(Debug, Clone, Serialize)]
pub struct Awaiting {
    /// How many messages wait.
    pub count: usize,
    /// How many of them went to a person, by member id.
    pub operator: usize,
    /// Each message that waits, oldest first.
    pub items: Vec<Item>,
}

/// One message that waits for an answer.
#[derive(Debug, Clone, Serialize)]
pub struct Item {
    /// The signal's id, which an answer threads under.
    pub id: String,
    /// The short `~id` handle a delivery line shows.
    pub handle: String,
    /// The recipients' ids: session ids, or one person's member id.
    pub to: Vec<String>,
    /// The recipients as `attend peer list` names them, or the member id.
    pub recipient: String,
    /// Whether it went to a person rather than to a session.
    pub operator: bool,
    /// When it was sent, in seconds since the epoch.
    pub ts: u64,
    /// How long ago it was sent, in seconds.
    pub age: u64,
    /// The first words of the message, on one line.
    pub preview: String,
}

/// The count alone, for attend code that needs no listing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Count {
    /// How many messages wait.
    pub count: usize,
    /// How many of them went to a person, by member id.
    pub operator: usize,
}

/// A message this session sent that no recipient has answered.
#[derive(Debug, Clone)]
struct Waiting {
    id: String,
    to: Vec<String>,
    operator: bool,
    mtime: SystemTime,
    message: String,
}

/// How many messages this session sent wait for an answer, and how many of
/// those went to a person. It names no recipient, so it reads no peer list.
pub fn count() -> Count {
    tally(&waiting_for_session())
}

/// The messages this session sent that wait for an answer, with each
/// recipient named.
pub fn awaiting() -> Awaiting {
    let waiting = waiting_for_session();
    let Count { count, operator } = tally(&waiting);
    let names = if waiting.iter().any(|w| !w.operator) { session_names() } else { Vec::new() };
    let now = SystemTime::now();
    let items = waiting
        .into_iter()
        .map(|w| {
            let recipient = if w.operator {
                w.to.join(", ")
            } else {
                w.to.iter().map(|id| name_of(&names, id)).collect::<Vec<_>>().join(", ")
            };
            Item {
                handle: agent_identity::signal::short_id(&w.id),
                recipient,
                operator: w.operator,
                ts: secs(w.mtime),
                age: now.duration_since(w.mtime).map_or(0, |d| d.as_secs()),
                preview: preview(&w.message),
                to: w.to,
                id: w.id,
            }
        })
        .collect();
    Awaiting { count, operator, items }
}

/// The waiting messages of the session this process runs in: what it sent
/// under its id or an id it had before a `/clear`, against the rooms it
/// receives.
fn waiting_for_session() -> Vec<Waiting> {
    let me = current_session();
    let ids = attend_presence::enrollment::reading_ids(&me);
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    let answer_dirs: Vec<PathBuf> = groups_for(&me).receive_dirs(&me.origin_path).into_iter().map(|d| d.path).collect();
    let base = signals_base();
    let marks = Marks::load(&base);
    let live = crate::verbs::send::live_session_ids();
    let can_answer = |id: &str| answerable(id, &live, &attend_presence::enrollment::is_finished);
    scan(&base, &ids, &answer_dirs, &marks, &can_answer)
}

/// Whether session `id` can still answer: it is live by the send path's
/// test against the running sessions `live`, and `finished` does not hold
/// for it.
fn answerable(id: &str, live: &std::collections::HashSet<String>, finished: &dyn Fn(&str) -> bool) -> bool {
    attend_presence::alive(id, live) && !finished(id)
}

/// The direct messages under `base` that a sender reading as `ids` sent and
/// no recipient answered in `answer_dirs`, oldest first. A session
/// recipient counts only while `can_answer` holds for it; a person always
/// counts.
fn scan(base: &Path, ids: &[&str], answer_dirs: &[PathBuf], marks: &Marks, can_answer: &dyn Fn(&str) -> bool) -> Vec<Waiting> {
    let mut sent: Vec<(Order, Waiting)> = Vec::new();
    for (dir, operator) in direct_dirs(base) {
        for (name, mtime, content) in signals(&dir) {
            if marks.hides(&attend_state::seen_key(&name)) {
                continue;
            }
            let Some(sig) = parse_signal(wire_line(&content)) else { continue };
            let sender = sig.sender_id();
            if !ids.contains(&sender.as_str()) || marks.hides_sender(&sender, mtime) {
                continue;
            }
            // A record with no `to` was sent to a project by an older
            // writer, and names no recipient to wait for.
            let Some(to) = sig.to else { continue };
            let to: Vec<String> = to
                .split(',')
                .filter(|t| !t.is_empty() && !ids.contains(t) && (operator || can_answer(t)))
                .map(str::to_string)
                .collect();
            if to.is_empty() {
                continue;
            }
            let id = name.trim_end_matches(".signal").to_string();
            let order = (mtime, signal_send_order(&id));
            sent.push((order, Waiting { id, to, operator, mtime, message: sig.message.to_string() }));
        }
    }
    if sent.is_empty() {
        return Vec::new();
    }

    // What the recipients sent into the rooms this session receives: who,
    // when, the id it threads under, and whether it names this session.
    let mut answers: Vec<(String, Order, Option<String>, bool)> = Vec::new();
    for dir in answer_dirs {
        for (name, mtime, content) in signals(dir) {
            let Some(sig) = parse_signal(wire_line(&content)) else { continue };
            let order = (mtime, signal_send_order(&name));
            let to_me = ids.iter().any(|id| sig.addressed_to(id));
            answers.push((sig.sender_id(), order, sig.reply_to.map(str::to_string), to_me));
        }
    }

    sent.retain(|(at, w)| {
        !answers
            .iter()
            .any(|(from, order, re, to_me)| w.to.contains(from) && order > at && (*to_me || re.as_deref() == Some(w.id.as_str())))
    });
    sent.sort_by_key(|a| a.0);
    sent.into_iter().map(|(_, w)| w).collect()
}

/// When a signal was written: its file's mtime, then the send order its id
/// carries, for signals that share an mtime.
type Order = (SystemTime, (u128, u64));

/// The directories under `base` a direct message is written to, each with
/// whether it is a person's tray: every project tray, and every person tray
/// under [`attend_groups::PEOPLE_DIR`]. `#open`, the channels and attend's
/// own stores are left out.
fn direct_dirs(base: &Path) -> Vec<(PathBuf, bool)> {
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(base).into_iter().flatten().flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        if name == attend_groups::PEOPLE_DIR {
            let people = std::fs::read_dir(entry.path()).into_iter().flatten().flatten();
            dirs.extend(people.filter(|p| p.file_type().is_ok_and(|t| t.is_dir())).map(|p| (p.path(), true)));
        } else if !name.starts_with(['_', '@', '.']) {
            dirs.push((entry.path(), false));
        }
    }
    dirs
}

/// Each signal file in `dir`: its file name, its mtime and its content.
fn signals(dir: &Path) -> Vec<(String, SystemTime, String)> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_string();
            if !name.ends_with(".signal") {
                return None;
            }
            let mtime = entry.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            let content = agent_identity::signal::read_signal_file(&entry.path()).ok()?;
            Some((name, mtime, content))
        })
        .collect()
}

fn tally(waiting: &[Waiting]) -> Count {
    Count { count: waiting.len(), operator: waiting.iter().filter(|w| w.operator).count() }
}

/// Each live peer session's id and its name as `attend peer list` shows it.
fn session_names() -> Vec<(String, String)> {
    crate::verbs::peers::peers()
        .peers
        .into_iter()
        .filter_map(|p| p.session_id.map(|id| (id, p.name)))
        .collect()
}

/// The name `names` gives session `id`, else its short id.
fn name_of(names: &[(String, String)], id: &str) -> String {
    names
        .iter()
        .find(|(sid, _)| sid == id)
        .map_or_else(|| format!("session {}", id.chars().take(8).collect::<String>()), |(_, name)| name.clone())
}

/// Characters of a message the preview keeps before it cuts at a word.
const PREVIEW_CHARS: usize = 60;

/// The first words of `message` on one line, cut at a word boundary with an
/// ellipsis once it passes [`PREVIEW_CHARS`].
fn preview(message: &str) -> String {
    let mut out = String::new();
    for word in message.split_whitespace() {
        if !out.is_empty() && out.chars().count() + 1 + word.chars().count() > PREVIEW_CHARS {
            out.push('…');
            return out;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

fn secs(t: SystemTime) -> u64 {
    t.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_identity::Envelope;

    const ME: &str = "aaaaaaaa-0000-0000-0000-00000000000a";
    const PEER: &str = "bbbbbbbb-0000-0000-0000-00000000000b";
    const OTHER: &str = "cccccccc-0000-0000-0000-00000000000c";

    struct Store {
        base: PathBuf,
    }

    impl Store {
        fn new(tag: &str) -> Self {
            let base = std::env::temp_dir().join(format!("attend-awaiting-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&base);
            std::fs::create_dir_all(&base).unwrap();
            Store { base }
        }

        fn my_tray(&self) -> PathBuf {
            self.base.join("-proj-a")
        }

        fn peer_tray(&self) -> PathBuf {
            self.base.join("-proj-b")
        }

        fn open(&self) -> PathBuf {
            self.base.join(attend_groups::BROADCAST_DIR)
        }

        fn person(&self, member: &str) -> PathBuf {
            attend_groups::person_dir(&self.base, member)
        }

        /// Write a signal from `sender` into `dir`, addressed to `to`.
        fn put(&self, dir: &Path, sender: &str, to: &str, re: Option<&str>, msg: &str) -> String {
            let human = !attend_groups::is_agent_member(sender);
            let from = if human { format!("external:{sender}@term") } else { format!("claude:{sender}") };
            let kind = if human { "human" } else { "agent" };
            let env = Envelope { from_kind: Some(kind), from_id: Some(sender), on_behalf_of: None, to: Some(to) };
            let content = agent_identity::signal::format_tagged(&from, "proj", "/proj", &env, re, msg);
            let filename = agent_identity::signal_filename(sender);
            agent_identity::signal::write_signal_file(dir, &filename, &content).unwrap();
            filename.trim_end_matches(".signal").to_string()
        }

        fn count(&self) -> Count {
            self.count_while(&|_| true)
        }

        fn count_while(&self, can_answer: &dyn Fn(&str) -> bool) -> Count {
            tally(&scan(&self.base, &[ME], &[self.my_tray(), self.open()], &Marks::load(&self.base), can_answer))
        }
    }

    impl Drop for Store {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    /// The card's fixture: a send to a peer waits until the peer replies, an
    /// `#open` post never waits, and a send to a member id waits as the
    /// operator's.
    #[test]
    fn a_direct_message_waits_until_its_recipient_answers() {
        let s = Store::new("card");
        let sent = s.put(&s.peer_tray(), ME, PEER, None, "can you review the branch?");
        assert_eq!(s.count(), Count { count: 1, operator: 0 });

        s.put(&s.my_tray(), PEER, ME, Some(&sent), "on it");
        assert_eq!(s.count(), Count { count: 0, operator: 0 });

        s.put(&s.open(), ME, "*", None, "anyone free?");
        assert_eq!(s.count(), Count { count: 0, operator: 0 });

        s.put(&s.person("aaron"), ME, "aaron", None, "may I merge it?");
        assert_eq!(s.count(), Count { count: 1, operator: 1 });
    }

    /// Only the recipient answers, and only after the message: another
    /// session's message to this one, and the recipient's own message from
    /// before the send, leave it waiting. An unthreaded message from the
    /// recipient addressed to this session answers it, as attend-chat sends.
    #[test]
    fn only_a_later_message_from_the_recipient_answers() {
        let s = Store::new("who");
        s.put(&s.my_tray(), "aaron", ME, None, "earlier note");
        s.put(&s.person("aaron"), ME, "aaron", None, "may I merge it?");
        s.put(&s.my_tray(), OTHER, ME, None, "not the recipient");
        assert_eq!(s.count(), Count { count: 1, operator: 1 });

        s.put(&s.my_tray(), "aaron", ME, None, "yes");
        assert_eq!(s.count(), Count { count: 0, operator: 0 });
    }

    /// An answer that threads under the message answers it wherever this
    /// session reads it, even one addressed to no one.
    #[test]
    fn a_threaded_answer_in_open_answers() {
        let s = Store::new("thread");
        let sent = s.put(&s.peer_tray(), ME, PEER, None, "which tag?");
        s.put(&s.open(), PEER, "*", None, "unrelated news");
        assert_eq!(s.count(), Count { count: 1, operator: 0 });

        s.put(&s.open(), PEER, "*", Some(&sent), "v0.38.2");
        assert_eq!(s.count(), Count { count: 0, operator: 0 });
    }

    /// A message waits only while it lives and only as a direct message: one
    /// a delete marked, one in a channel and one from a tombstoned session do
    /// not wait.
    #[test]
    fn a_marked_or_channel_message_does_not_wait() {
        let s = Store::new("life");
        let marked = s.put(&s.peer_tray(), ME, PEER, None, "marked");
        s.put(&s.base.join("@deploy"), ME, PEER, None, "in a channel");
        assert_eq!(s.count(), Count { count: 1, operator: 0 });

        attend_groups::tombstones::mark_messages(&s.base, &[attend_state::seen_key(&format!("{marked}.signal"))]).unwrap();
        assert_eq!(s.count(), Count { count: 0, operator: 0 });

        s.put(&s.peer_tray(), ME, PEER, None, "before the tombstone");
        attend_groups::tombstones::mark_session(&s.base, ME).unwrap();
        assert_eq!(s.count(), Count { count: 0, operator: 0 });
    }

    /// A message to a session that exited or finished does not wait, since
    /// no answer can come. A message to a person waits whatever the session
    /// check says.
    #[test]
    fn a_session_that_cannot_answer_leaves_nothing_waiting() {
        let s = Store::new("gone");
        s.put(&s.peer_tray(), ME, PEER, None, "still there?");
        s.put(&s.person("aaron"), ME, "aaron", None, "may I merge it?");
        let running: std::collections::HashSet<String> = [PEER.to_string()].into();
        let none = std::collections::HashSet::new();
        let finished_peer = |id: &str| id == PEER;
        let nobody_finished = |_: &str| false;

        let live = |id: &str| answerable(id, &running, &nobody_finished);
        assert_eq!(s.count_while(&live), Count { count: 2, operator: 1 });

        let exited = |id: &str| answerable(id, &none, &nobody_finished);
        assert_eq!(s.count_while(&exited), Count { count: 1, operator: 1 });

        let finished = |id: &str| answerable(id, &running, &finished_peer);
        assert_eq!(s.count_while(&finished), Count { count: 1, operator: 1 });
    }

    #[test]
    fn the_preview_cuts_at_a_word() {
        assert_eq!(preview("short\nmessage"), "short message");
        let long = "word ".repeat(30);
        let p = preview(&long);
        assert!(p.ends_with('…') && p.chars().count() <= PREVIEW_CHARS + 1, "{p}");
    }
}
