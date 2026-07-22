//! `attend inbox` — read pending messages from peers.
//!
//! Owns the `ParsedSignal` ADR-120 wire-format parser because `cmd_inbox`
//! / `cmd_inbox_read` are its hottest callers. `cmd::send` re-uses
//! `is_valid_signal_id` from here when validating `--re` ids, which is
//! the right direction of dependency: the parser owns what a valid id
//! looks like, senders consult it.

use crate::identity_view::render_sender_label;
use crate::util::{encode_project, get_groups, own_session_id, signals_base};
use agent_identity::TermCaps;

/// Parsed signal record (ADR-120 wire format).
///
/// Legacy signals have no `reply_to`; threaded replies carry the original
/// signal's ID in that field. Borrows from the input to keep the parse
/// allocation-free at the hot path.
pub(crate) struct ParsedSignal<'a> {
    pub(crate) from: &'a str,
    /// Parsed but currently unread by any caller. Retained so future
    /// sender-hint rendering (non-cwd) can read it without re-parsing.
    #[allow(dead_code)]
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
    let cwd = crate::util::own_origin_cwd();
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
        let caps = TermCaps::detect();
        let sender = render_sender_label(sig.from, sig.cwd, caps);
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

pub(crate) fn cmd_inbox(limit: usize, page: usize, before: Option<u64>) {
    let base = signals_base();
    let cwd = crate::util::own_origin_cwd();
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
            "#open"
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

            let caps = TermCaps::detect();
            let sender = render_sender_label(sig.from, sig.cwd, caps);

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

    // Cursor filter: keep only entries strictly older than `before`.
    if let Some(ts) = before {
        entries.retain(|e| mtime_secs(e.mtime) < ts);
    }

    if entries.is_empty() {
        println!("no messages");
        return;
    }

    // Page over the oldest-first ledger; page 1 = the newest `limit`, and
    // higher page numbers walk back into history. The never-reaped ledger
    // can be long, so a bounded page keeps `attend inbox` (and the digest's
    // "attend inbox for detail" pull) usable.
    let limit = limit.max(1);
    let page = page.max(1);
    let total = entries.len();
    let end = total.saturating_sub((page - 1) * limit);
    let start = end.saturating_sub(limit);
    if start >= end {
        let pages = total.div_ceil(limit);
        println!("no messages on page {page} ({total} total, {pages} page(s))");
        return;
    }
    let older = start; // entries older than this page's oldest
    let page_entries = &entries[start..end];
    // Cursor for the next-older page = the oldest entry shown here.
    let cursor_ts = mtime_secs(page_entries[0].mtime);

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
        for entry in page_entries {
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
        print_inbox_footer(page, total, page_entries.len(), older, cursor_ts);
    } else {
        // Non-TTY: one block per message, full-width fields.
        for entry in page_entries {
            println!("[{}] {}", entry.scope, entry.sender);
            println!("  id:      {}", entry.id);
            if !entry.re.is_empty() {
                println!("  re:      {}", entry.re);
            }
            println!("  source:  {}", entry.source);
            println!("  message: {}", entry.message);
            println!();
        }
        print_inbox_footer(page, total, page_entries.len(), older, cursor_ts);
    }
}

/// Seconds since the epoch for a file mtime (0 if before the epoch).
fn mtime_secs(t: std::time::SystemTime) -> u64 {
    t.duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Pagination footer: what page this is, and how to walk further back.
fn print_inbox_footer(page: usize, total: usize, shown: usize, older: usize, cursor_ts: u64) {
    println!("page {page} · showing {shown} of {total} message(s)");
    if older > 0 {
        println!(
            "  ↑ {older} older — attend inbox --page {} (or --before {cursor_ts})",
            page + 1
        );
    }
}

// ---------------------------------------------------------------------------
// `attend inbox --drain` — the ADR-172 turn-boundary consumption path.
// ---------------------------------------------------------------------------

/// Consecutive drain-fired continuations allowed before the drain defers
/// to the Monitor conduit (ADR-172 Decision 6). Two actively conversing
/// sessions can otherwise injection-trigger each other's turns without
/// bound. Deferring delivers nothing and marks nothing — the messages
/// stay pending for the poller or the next fresh turn boundary.
const MAX_DRAIN_ROUNDS: u32 = 5;

/// Cap on messages rendered in full in one drain; the remainder is
/// counted and pointed at `attend inbox`. Keeps a rejoin-after-gap
/// burst from flooding a single turn injection.
const DRAIN_RENDER_MAX: usize = 10;

/// Drain pending authored messages for this session: deliver the unseen,
/// record their consumption in the shared seen-set, and (in `hook`
/// format) emit the Stop-hook block JSON that injects them into the
/// ending turn. Every guard degrades to "deliver nothing, mark nothing":
/// unresolved identity, cold start, and the re-entry ceiling all leave
/// the tray for the Monitor conduit rather than risking the seen-set.
pub(crate) fn cmd_inbox_drain(format: &str) {
    let hook_mode = format == "hook";

    // Resolved-gate (ADR-172 Decision 4): marking consumption under a
    // pid-fallback identity would alias sessions and corrupt the shared
    // seen-set. Under unresolved identity, Monitor remains the conduit.
    let ident = attend_session::identity();
    if !ident.resolved() {
        if !hook_mode {
            eprintln!("(identity unresolved — drain is a no-op; the Monitor poller still delivers)");
        }
        return;
    }
    let session_id = ident.session_id.clone();

    // Re-entry guard (Decision 6). Only the hook path carries the
    // harness's stop_hook_active signal on stdin; a manual plain-mode
    // drain counts as a fresh boundary.
    let stop_active = hook_mode && stdin_stop_hook_active();
    let rounds = bump_drain_rounds(&session_id, stop_active);
    if rounds > MAX_DRAIN_ROUNDS {
        eprintln!("(drain round {rounds} > {MAX_DRAIN_ROUNDS} — deferring to the Monitor conduit)");
        return;
    }

    let store = attend_state::StateStore::new(Some(session_id.clone()));

    // Cold start (no seen-set on disk): baseline the whole backlog as
    // seen WITHOUT delivering — the same flood guard the peers sensor
    // applies on a checkpoint-less first scan. Detail stays available
    // via `attend inbox`.
    let baselining = store.load().is_none();

    let base = signals_base();
    let cwd = crate::util::own_origin_cwd();
    let own_encoded = encode_project(&cwd);
    let r = get_groups();
    let mut scan_dirs = vec![base.join(&own_encoded), base.join("_broadcast")];
    for name in r.joined_group_names() {
        scan_dirs.push(r.group_dir(&name));
    }

    let seen = store.load().map(|s| s.seen_signals).unwrap_or_default();

    struct Drained {
        mtime: std::time::SystemTime,
        sender: String,
        scope: String,
        id: String,
        body: String,
        source_cwd: String,
    }
    impl DrainedView for Drained {
        fn sender(&self) -> &str { &self.sender }
        fn scope(&self) -> &str { &self.scope }
        fn id(&self) -> &str { &self.id }
        fn body(&self) -> &str { &self.body }
    }
    let mut delivered: Vec<Drained> = Vec::new();
    let mut mark: Vec<String> = Vec::new();

    for dir in &scan_dirs {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let dir_name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        let scope = if dir_name == "_broadcast" {
            "#open"
        } else if dir_name == own_encoded {
            "project"
        } else {
            dir_name // "@group" reads naturally as the channel name
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let filename = match path.file_name().and_then(|f| f.to_str()) {
                Some(f) if f.ends_with(".signal") => f.to_string(),
                _ => continue,
            };
            // Same collision-proof key the peers sensor uses.
            let key = format!("{}:{}", dir.display(), filename);
            if seen.contains(&key) {
                continue;
            }
            if baselining {
                mark.push(key);
                continue;
            }
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let sig = match parse_signal(content.trim()) {
                Some(s) => s,
                None => continue,
            };
            // Own messages: mark (dedup bookkeeping) but never deliver.
            if let Some((_, identity)) = sig.from.split_once(':') {
                if identity == session_id {
                    mark.push(key);
                    continue;
                }
            }
            let mtime = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::UNIX_EPOCH);
            // Always Mono: the drain's output is hook-injection text (or
            // a pipe), never a styled terminal — env-probed detection
            // would leak raw ANSI codes into the turn (live-test find).
            delivered.push(Drained {
                mtime,
                sender: render_sender_label(sig.from, sig.cwd, TermCaps::Mono),
                scope: scope.to_string(),
                id: path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string(),
                body: sig.message.to_string(),
                source_cwd: sig.cwd.to_string(),
            });
            mark.push(key);
        }
    }

    delivered.sort_by_key(|d| d.mtime);

    // Consumption is recorded for everything we are about to deliver
    // (plus own-message bookkeeping) — the atomic "return AND record"
    // contract. Recording before printing means a crash between the two
    // loses a delivery to this conduit, not deliver-once: the message
    // file itself is untouched and `attend inbox` still lists it.
    store.mark_seen(mark);

    // `attend reply` should target what the drain delivered, exactly as
    // it targets what the sensor surfaced.
    #[cfg(feature = "sensor-peers")]
    if let Some(newest) = delivered.iter().rev().find(|d| d.source_cwd != cwd) {
        sensor_peers::last_inbound::record(&session_id, &newest.id);
    }

    if baselining {
        if !hook_mode {
            eprintln!("(cold start — baselined the existing backlog without delivering; see attend inbox)");
        }
        return;
    }
    if delivered.is_empty() {
        // Empty drain: say nothing in hook mode so the turn ends —
        // the termination property the re-entry design leans on.
        if !hook_mode {
            println!("no pending messages");
        }
        return;
    }

    if hook_mode {
        let reason = render_drain_reason(&delivered);
        println!(
            "{{\"decision\": \"block\", \"reason\": \"{}\"}}",
            json_escape(&reason)
        );
    } else {
        for d in &delivered {
            println!("[{}] {}", d.scope, d.sender);
            println!("  id:      {}", d.id);
            println!("  message: {}", d.body);
            println!();
        }
        println!("{} message(s) drained and marked consumed", delivered.len());
    }
}

/// The injected turn-continuation text. Sober and contract-preserving:
/// a drained message informs the turn — the standing messaging
/// guidance (reply autonomy, silence-is-valid) rides along verbatim so
/// turn-boundary delivery never reads as a command to respond.
fn render_drain_reason(delivered: &[impl DrainedView]) -> String {
    let mut out = format!(
        "[attend] {} peer message(s) delivered at the turn boundary (ADR-172 drain):\n",
        delivered.len()
    );
    for d in delivered.iter().take(DRAIN_RENDER_MAX) {
        out.push_str(&format!(
            "\n{} ({}, id {}):\n{}\n",
            d.sender(),
            d.scope(),
            d.id(),
            d.body()
        ));
    }
    if delivered.len() > DRAIN_RENDER_MAX {
        out.push_str(&format!(
            "\n(+{} more — attend inbox for the rest)\n",
            delivered.len() - DRAIN_RENDER_MAX
        ));
    }
    out.push_str(
        "\nYou may reply (attend reply \"...\" auto-threads to the newest), \
         start a new thread (attend send), or continue your work — \
         silence is a valid reply.",
    );
    out
}

/// View trait so `render_drain_reason` is testable without the
/// filesystem-shaped `Drained` struct.
trait DrainedView {
    fn sender(&self) -> &str;
    fn scope(&self) -> &str;
    fn id(&self) -> &str;
    fn body(&self) -> &str;
}

/// Read the Stop-hook stdin payload and extract `stop_hook_active`.
/// Tolerant token scan rather than a JSON dependency: the harness may
/// emit compact or pretty JSON; we only need one boolean.
fn stdin_stop_hook_active() -> bool {
    use std::io::Read;
    let mut buf = String::new();
    if std::io::stdin().read_to_string(&mut buf).is_err() {
        return false;
    }
    parse_stop_hook_active(&buf)
}

fn parse_stop_hook_active(payload: &str) -> bool {
    payload
        .split("\"stop_hook_active\"")
        .nth(1)
        .and_then(|rest| rest.split_once(':'))
        .map(|(_, after)| after.trim_start().starts_with("true"))
        .unwrap_or(false)
}

/// Track consecutive drain-fired continuations in a sidecar next to the
/// state file. A fresh boundary (stop_hook_active=false) resets to 1;
/// each hook-forced continuation increments. The file is tiny and
/// self-healing — an unreadable count is treated as a fresh boundary.
fn bump_drain_rounds(session_id: &str, stop_active: bool) -> u32 {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let dir = std::path::PathBuf::from(home)
        .join(".cache")
        .join("attend")
        .join("state");
    std::fs::create_dir_all(&dir).ok();
    let path = dir.join(format!("{session_id}.drain-rounds"));
    let rounds = if stop_active {
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0)
            + 1
    } else {
        1
    };
    std::fs::write(&path, rounds.to_string()).ok();
    rounds
}

/// Minimal JSON string escaping for the hook `reason` field.
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod drain_tests {
    use super::*;

    struct FakeMsg {
        sender: String,
        scope: String,
        id: String,
        body: String,
    }
    impl DrainedView for FakeMsg {
        fn sender(&self) -> &str { &self.sender }
        fn scope(&self) -> &str { &self.scope }
        fn id(&self) -> &str { &self.id }
        fn body(&self) -> &str { &self.body }
    }
    fn msg(n: usize) -> FakeMsg {
        FakeMsg {
            sender: format!("peer-{n}"),
            scope: "#open".into(),
            id: format!("id-{n}"),
            body: format!("body {n}"),
        }
    }

    #[test]
    fn json_escape_covers_hook_reason_hazards() {
        assert_eq!(json_escape("plain"), "plain");
        assert_eq!(json_escape("a\"b"), "a\\\"b");
        assert_eq!(json_escape("a\\b"), "a\\\\b");
        assert_eq!(json_escape("line1\nline2"), "line1\\nline2");
        assert_eq!(json_escape("tab\there"), "tab\\there");
        assert_eq!(json_escape("bell\u{7}"), "bell\\u0007");
    }

    #[test]
    fn parse_stop_hook_active_tolerates_json_shapes() {
        assert!(parse_stop_hook_active(r#"{"stop_hook_active":true}"#));
        assert!(parse_stop_hook_active(r#"{ "stop_hook_active" : true }"#));
        assert!(parse_stop_hook_active("{\n  \"stop_hook_active\": true,\n  \"x\": 1\n}"));
        assert!(!parse_stop_hook_active(r#"{"stop_hook_active":false}"#));
        assert!(!parse_stop_hook_active(r#"{"other": true}"#));
        assert!(!parse_stop_hook_active(""));
        // A string VALUE mentioning the field must not trip the scan's
        // simple tokenizer into a false positive for `: true`.
        assert!(!parse_stop_hook_active(
            r#"{"note": "stop_hook_active is unrelated here", "stop_hook_active": false}"#
        ));
    }

    #[test]
    fn drain_reason_renders_messages_and_contract_line() {
        let msgs = vec![msg(1), msg(2)];
        let reason = render_drain_reason(&msgs);
        assert!(reason.contains("2 peer message(s)"));
        assert!(reason.contains("peer-1 (#open, id id-1):"));
        assert!(reason.contains("body 2"));
        assert!(reason.contains("silence is a valid reply"));
    }

    #[test]
    fn drain_reason_caps_render_and_counts_remainder() {
        let msgs: Vec<FakeMsg> = (0..14).map(msg).collect();
        let reason = render_drain_reason(&msgs);
        assert!(reason.contains("14 peer message(s)"));
        assert!(reason.contains("body 9"));
        assert!(!reason.contains("body 10"));
        assert!(reason.contains("(+4 more — attend inbox for the rest)"));
    }
}
