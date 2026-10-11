//! One description per message verb, read by every frontend (ADR-501 item 4).
//!
//! The CLI renders each as the verb's clap `about`, so `--help` and the
//! generated `docs/cli/attend.md` say exactly this. A frontend that adds
//! guidance of its own, such as a tool description, starts from the same
//! string.

pub const SEND: &str = "Send a signal to peer sessions (defaults to #open base channel)";
pub const REPLY: &str = "Reply to the most recent peer message (auto-threaded)";
pub const INBOX: &str = "Read pending messages from peers";
pub const PEERS: &str = "List active Claude Code sessions and their channels";
pub const STATUS: &str = "Show running instances, signals, and channel state";
pub const JOIN: &str = "Join a channel (creates it if absent)";
pub const LEAVE: &str = "Leave a channel";
pub const CHANNELS: &str = "Channel lifecycle (default: list all, joined ones marked)";

/// Every message verb with its description, by the CLI name.
pub const VERBS: &[(&str, &str)] = &[
    ("send", SEND),
    ("reply", REPLY),
    ("inbox", INBOX),
    ("peers", PEERS),
    ("status", STATUS),
    ("join", JOIN),
    ("leave", LEAVE),
    ("channels", CHANNELS),
];
