//! `attend inbox` — read pending messages from peers.
//!
//! Owns the `ParsedSignal` ADR-120 wire-format parser because `cmd_inbox`
//! / `cmd_inbox_read` are its hottest callers. `cmd::send` re-uses
//! `is_valid_signal_id` from here when validating `--re` ids, which is
//! the right direction of dependency: the parser owns what a valid id
//! looks like, senders consult it.

use crate::util::{encode_project, get_groups, own_session_id, signals_base};

/// Parsed signal record (ADR-120 wire format).
///
/// Legacy signals have no `reply_to`; threaded replies carry the original
/// signal's ID in that field. Borrows from the input to keep the parse
/// allocation-free at the hot path.
pub(crate) struct ParsedSignal<'a> {
    pub(crate) from: &'a str,
    pub(crate) project: &'a str,
    pub(crate) cwd: &'a str,
    pub(crate) reply_to: Option<&'a str>,
    pub(crate) message: &'a str,
}

/// Signal IDs are filename stems in the form `<sender-id>-<timestamp>`,
/// which is always `[A-Za-z0-9_-]+`. Using this char class as the
/// discriminator fence keeps legacy prose that happens to start with
/// "re:" from being misparsed as threaded — e.g. `attend send "re: the
/// thing we discussed|still open"` stays a 4-field legacy message
/// because `the thing we discussed` has a space.
pub(crate) fn is_valid_signal_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Parse a single-line signal. Accepts both the legacy 4-field format and
/// the 5-field threaded format; the discriminator is a `re:<id>|` prefix
/// on the field that follows `cwd`, where `<id>` matches
/// `is_valid_signal_id`. A malformed or ambiguous `re:` prefix degrades
/// to legacy interpretation so real prose round-trips cleanly.
pub(crate) fn parse_signal(content: &str) -> Option<ParsedSignal<'_>> {
    let parts: Vec<&str> = content.splitn(4, '|').collect();
    if parts.len() < 4 {
        return None;
    }
    let tail = parts[3];
    let (reply_to, message) = match tail.strip_prefix("re:").and_then(|rest| rest.split_once('|')) {
        Some((id, msg)) if is_valid_signal_id(id) => (Some(id), msg),
        // Either not threaded, or the `re:` prefix is followed by text
        // that doesn't look like a signal id — fall back to legacy so
        // prose like "re: the thing we discussed" stays intact.
        _ => (None, tail),
    };
    Some(ParsedSignal {
        from: parts[0],
        project: parts[1],
        cwd: parts[2],
        reply_to,
        message,
    })
}

pub(crate) fn cmd_inbox_read(msg_id: &str) {
    let base = signals_base();
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let own_encoded = encode_project(&cwd);
    let r = get_groups();
    let mut scan_dirs = vec![
        base.join(&own_encoded),
        base.join("_broadcast"),
    ];
    for name in r.joined_group_names() {
        scan_dirs.push(r.group_dir(&name));
    }

    // Search for the signal file by ID
    let target = format!("{msg_id}.signal");
    for dir in &scan_dirs {
        let path = dir.join(&target);
        if !path.is_file() {
            continue;
        }
        // File exists: from here on, any failure is a corrupt-file
        // condition, not a benign "already consumed" miss. Distinguish
        // them so operators can tell partial-write / disk-full bugs
        // from ordinary races.
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("(signal {msg_id} exists but could not be read: {e})");
                return;
            }
        };
        let sig = match parse_signal(content.trim()) {
            Some(s) => s,
            None => {
                eprintln!("(signal {msg_id} exists but its wire format is corrupt)");
                return;
            }
        };
        let from = sig.from;
        let project = sig.project;
        let source_cwd = sig.cwd;
        let (kind, identity) = from.split_once(':').unwrap_or(("?", from));
        let sender = match kind {
            "claude" => format!("claude/{source_cwd}"),
            "external" => identity.to_string(),
            _ => format!("{project} ({from})"),
        };
        println!("From: {sender}");
        println!("ID:   {msg_id}");
        if let Some(re_id) = sig.reply_to {
            println!("Re:   {re_id}");
        }
        println!();
        println!("{}", sig.message);
        return;
    }
    // Benign miss — message may already be consumed or expired.
    // Exit 0 so callers don't treat a normal race as an error.
    println!("(no message by that id — already consumed or expired)");
}

pub(crate) fn cmd_inbox() {
    let base = signals_base();
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let own_session_id = own_session_id().unwrap_or_default();

    // Scan same dirs as the peer sensor: own project + broadcast + focus group
    let own_encoded = encode_project(&cwd);
    let r = get_groups();
    let mut scan_dirs = vec![
        base.join(&own_encoded),
        base.join("_broadcast"),
    ];
    // Add focus group dirs
    for name in r.joined_group_names() {
        scan_dirs.push(r.group_dir(&name));
    }

    // Collect all messages with mtime for chronological ordering
    struct InboxEntry {
        mtime: std::time::SystemTime,
        scope: String,
        sender: String,
        message: String,
        source: String,
        id: String,
        re: String,
    }
    let mut entries: Vec<InboxEntry> = Vec::new();

    for dir in &scan_dirs {
        let dir_entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        let dir_name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_string();
        let scope = if dir_name == "_broadcast" {
            "broadcast"
        } else if dir_name == own_encoded {
            "project"
        } else {
            "focus"
        };

        for entry in dir_entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("signal") {
                continue;
            }

            let mtime = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::UNIX_EPOCH);

            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let content = content.trim().to_string();
            let sig = match parse_signal(&content) {
                Some(s) => s,
                None => continue,
            };

            // Skip own messages
            if let Some((_, identity)) = sig.from.split_once(':') {
                if identity == own_session_id {
                    continue;
                }
            }

            let (kind, identity) = sig.from.split_once(':').unwrap_or(("unknown", sig.from));
            let sender = match kind {
                "claude" => format!("claude/{}", sig.cwd),
                "external" => identity.to_string(),
                _ => format!("{} ({})", sig.project, sig.from),
            };

            let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();

            entries.push(InboxEntry {
                mtime,
                scope: scope.to_string(),
                sender,
                message: sig.message.to_string(),
                source: sig.cwd.to_string(),
                id,
                re: sig.reply_to.map(|s| s.to_string()).unwrap_or_default(),
            });
        }
    }

    // Sort chronologically — oldest first (ledger order)
    entries.sort_by_key(|e| e.mtime);

    if entries.is_empty() {
        println!("no messages");
        return;
    }

    // Pipe-aware output: when stdout is a real terminal, render the
    // compact 6-column table (nice at-a-glance scan for humans). When
    // stdout is piped — Claude's Bash tool, `| less`, `>file`, etc. —
    // render one untruncated block per message so ids and bodies stay
    // legible. Mirrors the behavior of `ls` switching to one-per-line
    // output when it detects a pipe.
    use std::io::IsTerminal;
    if std::io::stdout().is_terminal() {
        let mut t = agent_fmt::Table::new(&["Scope", "From", "ID", "Re", "Message", "Source"]);
        t.max_width(0, 10);
        t.max_width(1, 24);
        t.max_width(2, 20);
        t.max_width(3, 20);
        for entry in &entries {
            t.add(vec![
                &entry.scope,
                &entry.sender,
                &entry.id,
                &entry.re,
                &entry.message,
                &entry.source,
            ]);
        }
        t.print();
        println!("  {} message(s)", entries.len());
    } else {
        // Non-TTY: one block per message, full-width fields.
        for entry in &entries {
            println!("[{}] {}", entry.scope, entry.sender);
            println!("  id:      {}", entry.id);
            if !entry.re.is_empty() {
                println!("  re:      {}", entry.re);
            }
            println!("  source:  {}", entry.source);
            println!("  message: {}", entry.message);
            println!();
        }
        println!("{} message(s)", entries.len());
    }
}
