//! Hide messages here (ADR-405, amendment of 2026-10-13): a watermark per
//! tab, local to this chat. Hiding a tab's messages records the send order
//! of the newest message the tab holds; the tab then shows only what is
//! newer ([`crate::tabs::shows`]). Nothing on the bus changes, and other
//! tabs, and other people, still see those messages.
//!
//! The watermarks persist across restarts in attend-chat's own state file,
//! `$XDG_STATE_HOME/attend-chat/hidden` (`~/.local/state/attend-chat/hidden`
//! when the variable is unset), one tab a line: the tab's key, then the
//! watermark's seconds, nanoseconds and sequence, tab-separated.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::signal::Signal;
use crate::tabs::Tab;

/// Where a message falls in send order: its file's time in seconds, then
/// the nanoseconds and sequence its id carries
/// ([`agent_identity::signal_send_order`]). A local notice's id carries
/// neither, so it orders by its seconds alone.
pub type Order = (u64, u128, u64);

/// The send order of `sig`.
pub fn order(sig: &Signal) -> Order {
    let (nanos, seq) = agent_identity::signal_send_order(&sig.id);
    (sig.ts, nanos, seq)
}

/// Each tab's watermark: the tab hides every message at or before it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hidden {
    marks: BTreeMap<String, Order>,
}

/// The key a tab's watermark is kept under.
fn key(tab: &Tab) -> String {
    match tab {
        Tab::Merged => "all".into(),
        Tab::Channel(c) => format!("#{c}"),
        Tab::Dm(m) => format!("dm:{m}"),
        Tab::Review(m) => format!("review:{m}"),
    }
}

impl Hidden {
    /// Whether `tab` hides `sig`: it is at or before the tab's watermark.
    pub fn hides(&self, tab: &Tab, sig: &Signal) -> bool {
        self.marks.get(&key(tab)).is_some_and(|mark| order(sig) <= *mark)
    }

    /// Hide in `tab` every message at or before `upto`. A watermark only
    /// moves forward.
    pub fn hide(&mut self, tab: &Tab, upto: Order) {
        let mark = self.marks.entry(key(tab)).or_insert(upto);
        *mark = (*mark).max(upto);
    }

    /// The watermarks kept at `path`; none when it is missing or unreadable.
    /// A line that does not parse is passed over.
    pub fn load(path: &Path) -> Hidden {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        let marks = text
            .lines()
            .filter_map(|line| {
                let mut f = line.split('\t');
                let (k, ts, nanos, seq) = (f.next()?, f.next()?, f.next()?, f.next()?);
                Some((k.to_string(), (ts.parse().ok()?, nanos.parse().ok()?, seq.parse().ok()?)))
            })
            .collect();
        Hidden { marks }
    }

    /// Write the watermarks to `path`, whole.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let body: String = self.marks.iter().map(|(k, (ts, nanos, seq))| format!("{k}\t{ts}\t{nanos}\t{seq}\n")).collect();
        agent_settings::writer::write_atomic(path, body)
    }
}

/// attend-chat's state file for the watermarks.
pub fn default_path() -> PathBuf {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| claude_sessions::home_dir().join(".local").join("state"));
    state.join("attend-chat").join("hidden")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal::Channel;

    fn sig(id: &str, ts: u64, channel: Channel) -> Signal {
        Signal { id: id.into(), from: "claude:a1".into(), project: String::new(), cwd: String::new(), reply_to: None, message: String::new(), ts, channel, tray: "t".into(), beside_post: false }
    }

    #[test]
    fn a_watermark_hides_up_to_itself_in_its_own_tab_only() {
        let open = Tab::Channel("open".into());
        let old = sig("p-1790949000000000100-1", 10, Channel::Open);
        let same_second_later = sig("p-1790949000000000200-1", 10, Channel::Open);
        let mut h = Hidden::default();
        assert!(!h.hides(&open, &old), "nothing hidden yet");
        h.hide(&open, order(&old));
        assert!(h.hides(&open, &old), "the newest message held is hidden");
        assert!(!h.hides(&open, &same_second_later), "one sent later in the same second shows");
        assert!(!h.hides(&Tab::Merged, &old), "another tab keeps it");
        h.hide(&open, (1, 0, 0));
        assert!(h.hides(&open, &old), "a watermark never moves back");
    }

    #[test]
    fn the_watermarks_survive_a_restart() {
        let dir = crate::test_dir::unique("hidden");
        let path = dir.join("attend-chat").join("hidden");
        let mut h = Hidden::default();
        h.hide(&Tab::Dm("a1".into()), (5, 6, 7));
        h.hide(&Tab::Merged, (8, 9, 10));
        h.save(&path).unwrap();
        assert_eq!(Hidden::load(&path), h);
        assert_eq!(Hidden::load(&dir.join("missing")), Hidden::default());
        std::fs::write(&path, "all\t1\t2\t3\nbroken line\n#x\tq\t2\t3\n").unwrap();
        let read = Hidden::load(&path);
        assert!(read.hides(&Tab::Merged, &sig("x", 1, Channel::Open)) && !read.hides(&Tab::Channel("x".into()), &sig("x", 0, Channel::Open)), "{read:?}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_state_file_is_under_xdg_state_home() {
        let _env = crate::test_dir::env_lock();
        let prev = std::env::var_os("XDG_STATE_HOME");
        std::env::set_var("XDG_STATE_HOME", "/tmp/st");
        assert_eq!(default_path(), PathBuf::from("/tmp/st/attend-chat/hidden"));
        match prev {
            Some(v) => std::env::set_var("XDG_STATE_HOME", v),
            None => std::env::remove_var("XDG_STATE_HOME"),
        }
    }
}
