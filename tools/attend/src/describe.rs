//! One description per message verb, read by every frontend (ADR-501 item 4).
//!
//! The CLI renders each as the verb's clap `about`, so `--help` and the
//! generated `docs/cli/attend.md` say exactly this. A frontend that adds
//! guidance of its own, such as a tool description, starts from the same
//! string.

pub const SEND: &str = "Send a message: a direct message with --to, a channel post with --channel, else to #open";
pub const REPLY: &str = "Reply to the most recent peer message: a direct message to its sender, or in its named channel";
pub const INBOX: &str = "Read pending messages from peers";
pub const PEERS: &str = "List active Claude Code sessions and their channels";
pub const STATUS: &str = "Show every running `attend run` with the session it serves, signals, and channel state";
pub const JOIN: &str = "Join a channel (creates it if absent)";
pub const LEAVE: &str = "Leave a channel";
/// What the verb does with no subcommand, which is all a tool offers. The
/// CLI's lifecycle subcommands (create, describe, pin, unpin) list
/// themselves under it in `--help`.
pub const CHANNELS: &str = "List every channel, marking the ones this session is in";

pub const LISTEN: &str = "Turn listening on or off for this session without joining a channel, or show it (default: status)";
pub const UNREAD: &str = "Count this session's unread messages by room, marking none of them read";

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
    ("listen", LISTEN),
    ("unread", UNREAD),
];
