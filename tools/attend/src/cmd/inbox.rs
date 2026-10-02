//! `attend inbox` — read pending messages from peers.
//!
//! Re-exports agent-identity's ADR-120 wire-format parser for its callers
//! here; `cmd::send` consults `is_valid_signal_id` when validating `--re`
//! ids, so the parser owns what a valid id looks like.

use attend_groups::Room;
use attend_instances::view::render_sender_label;
use attend_instances::SnapshotCache;
use crate::util::{get_groups, own_session_id, signals_base};

pub(crate) use agent_identity::{is_valid_signal_id, parse_signal};

pub(crate) fn cmd_inbox_read(msg_id: &str) {
    let cwd = crate::util::own_origin_cwd();
    let scan_dirs = get_groups().receive_dirs(&cwd);

    // Search for the signal file by ID
    let target = format!("{msg_id}.signal");
    for dir in &scan_dirs {
        let path = dir.path.join(&target);
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
        // The process painter is plain on a pipe (#388).
        let instances = SnapshotCache::new();
        let sender = render_sender_label(sig.from, sig.cwd, &agent_theme::painter(), &instances);
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
    let cwd = crate::util::own_origin_cwd();
    let own_session_id = own_session_id().unwrap_or_default();

    // The same receive set as the peer sensor and the drain.
    let scan_dirs = get_groups().receive_dirs(&cwd);

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
    // One registry snapshot per distinct cwd for this listing.
    let instances = SnapshotCache::new();

    for dir in &scan_dirs {
        let dir_entries = match std::fs::read_dir(&dir.path) {
            Ok(e) => e,
            Err(_) => continue,
        };

        let scope = match dir.room {
            Room::Open => "#open",
            Room::Project => "project",
            Room::Channel(_) => "channel",
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

            // The process painter is plain on a pipe (#388); a TTY keeps
            // the identity colors.
            let sender = render_sender_label(sig.from, sig.cwd, &agent_theme::painter(), &instances);

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
        let now = std::time::SystemTime::now();
        let mut t = agent_fmt::Table::new(&["When", "Scope", "From", "ID", "Re", "Message", "Source"]);
        t.max_width(1, 10);
        t.max_width(2, 24);
        t.max_width(3, 20);
        t.max_width(4, 20);
        for entry in page_entries {
            t.add(vec![
                &agent_fmt::compact_time(entry.mtime, now),
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
        let now = std::time::SystemTime::now();
        for entry in page_entries {
            println!("[{}] {}", entry.scope, entry.sender);
            println!("  when:    {}", agent_fmt::compact_time(entry.mtime, now));
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

/// One scanned pending message. Module scope (not fn-local) so the scan
/// core is testable without the identity/HOME plumbing around it.
struct Drained {
    mtime: std::time::SystemTime,
    when: String,
    sender: String,
    /// The wire `from` field — the canonical sender id (ADR-171). The
    /// display label is presentation over it, never a substitute.
    sender_id: String,
    scope: String,
    id: String,
    body: String,
    source_cwd: String,
}
impl DrainedView for Drained {
    fn when(&self) -> &str { &self.when }
    fn sender(&self) -> &str { &self.sender }
    fn sender_id(&self) -> &str { &self.sender_id }
    fn scope(&self) -> &str { &self.scope }
    fn id(&self) -> &str { &self.id }
    fn body(&self) -> &str { &self.body }
}

/// What one drain scan found: the messages to deliver, the seen-set keys
/// to record, and on a cold start the line announcing what it held back.
struct Scan {
    delivered: Vec<Drained>,
    mark: Vec<String>,
    note: Option<String>,
}

/// The scope label a drained message carries for its room.
fn room_label(room: &Room) -> String {
    match room {
        Room::Project => "project".to_string(),
        Room::Open => "#open".to_string(),
        // "@group" reads naturally as the channel name.
        Room::Channel(name) => format!("@{name}"),
    }
}

/// Scan core: walk the receive set and split unseen signals into
/// deliverable messages and keys to mark. Pure with respect to identity
/// and config: the caller supplies the seen-set, the session id and the
/// baselining decision, so tests drive it with temp dirs (PR #385 review,
/// finding 6).
///
/// Own messages are marked without delivering (dedup bookkeeping). Under
/// `baselining` (no seen-set yet) the messages from others go through
/// `attend_state::cold_start`, the rule the peers sensor applies too:
/// addressed mail is delivered whatever its age, old `#open` and channel
/// backlog is marked without being shown, and the scan reports a note
/// counting what it held back.
fn scan_pending(
    scan_dirs: &[attend_groups::ReceiveDir],
    seen: &std::collections::HashSet<String>,
    own_session_id: &str,
    baselining: bool,
) -> Scan {
    struct Found {
        room: usize,
        key: String,
        path: std::path::PathBuf,
        mtime: std::time::SystemTime,
        content: String,
    }
    let mut mark: Vec<String> = Vec::new();
    let mut found: Vec<Found> = Vec::new();
    for (room, dir) in scan_dirs.iter().enumerate() {
        let Ok(entries) = std::fs::read_dir(&dir.path) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let filename = match path.file_name().and_then(|f| f.to_str()) {
                Some(f) if f.ends_with(".signal") => f.to_string(),
                _ => continue,
            };
            // The key the peers sensor uses: the signal's filename.
            let key = attend_state::seen_key(&filename);
            if seen.contains(&key) {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else { continue };
            let Some(sig) = parse_signal(content.trim()) else { continue };
            // Own messages: mark (dedup bookkeeping) but never deliver.
            if sig.from.split_once(':').is_some_and(|(_, identity)| identity == own_session_id) {
                mark.push(key);
                continue;
            }
            let mtime = std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::UNIX_EPOCH);
            found.push(Found { room, key, path, mtime, content });
        }
    }

    let plan = baselining.then(|| {
        let pending: Vec<_> = found
            .iter()
            .map(|f| (&scan_dirs[f.room].room, f.mtime.elapsed().unwrap_or_default()))
            .collect();
        attend_state::cold_start::plan(&pending)
    });

    let mut delivered: Vec<Drained> = Vec::new();
    // One registry snapshot per distinct cwd for this scan.
    let instances = SnapshotCache::new();
    for (i, f) in found.into_iter().enumerate() {
        mark.push(f.key);
        if plan.as_ref().is_some_and(|p| !p.deliver[i]) {
            continue;
        }
        let Some(sig) = parse_signal(f.content.trim()) else { continue };
        // Escape-free by construction: the drain's output is
        // hook-injection text (or a pipe), never a styled terminal,
        // so it is drawn plain whatever the environment says (#388).
        // The label is the same form the peers sensor shows under
        // Monitor (#534); the wire `from` rides along as the id.
        delivered.push(Drained {
            when: agent_fmt::compact_time(f.mtime, std::time::SystemTime::now()),
            mtime: f.mtime,
            sender: render_sender_label(sig.from, sig.cwd, &agent_theme::Painter::plain(), &instances),
            sender_id: sig.from.to_string(),
            scope: room_label(&scan_dirs[f.room].room),
            id: f.path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string(),
            body: sig.message.to_string(),
            source_cwd: sig.cwd.to_string(),
        });
    }
    Scan { delivered, mark, note: plan.and_then(|p| p.note()) }
}

/// Drain pending authored messages for this session: deliver the unseen,
/// record their consumption in the shared seen-set, and (in `hook`
/// format) emit the Stop-hook block JSON that injects them into the
/// ending turn. Every guard degrades to "deliver nothing, mark nothing":
/// unresolved identity and the re-entry ceiling leave the tray for the
/// Monitor conduit; a cold start applies `attend_state::cold_start`,
/// announces what it held back, and always persists the state file so it
/// happens exactly once.
pub(crate) fn cmd_inbox_drain(format: &str) {
    let hook_mode = match format {
        "hook" => true,
        "plain" => false,
        other => {
            eprintln!("unknown --format '{other}' for --drain (expected plain|hook)");
            std::process::exit(2);
        }
    };

    // Resolved-gate (ADR-172 Decision 4): marking consumption under a
    // pid-fallback identity would alias sessions and corrupt the shared
    // seen-set. Under unresolved identity, Monitor remains the conduit.
    let ident = attend_presence::session::identity();
    if !ident.resolved() {
        if !hook_mode {
            eprintln!("(identity unresolved — drain is a no-op; the Monitor poller still delivers)");
        }
        return;
    }
    let session_id = ident.session_id.clone();
    // Enrollment gate (#720): a session that never ran attend and joined
    // no channel has chosen not to take part, so the drain delivers it
    // nothing and writes nothing, not even the heartbeat below.
    if !attend_presence::enrollment::is_enrolled(&session_id) {
        // `/clear` gives the process a new id. An enrollment recorded for
        // this same Claude Code process under its previous id moves here,
        // with its seen-set and channels, as `attend run` would move it.
        let previous = ident
            .claude_key()
            .and_then(|key| attend_presence::enrollment::previous_id(&key, &session_id));
        let Some(old) = previous else { return };
        crate::util::move_session(&old, &session_id, &ident.origin_path);
    }
    let base = signals_base();
    let r = crate::groups::Groups::new(&base, &session_id);

    // Liveness: a drain-only session (no Monitor running) must still
    // look alive to /purge's consumer consult, or the Decision 5
    // protection this PR co-ships would skip exactly the sessions that
    // depend on it (PR #385 review, finding 4).
    attend_presence::heartbeat::touch(&session_id).ok();

    // Re-entry guard (Decision 6). Only the hook path carries the
    // harness's stop_hook_active signal on stdin; a manual plain-mode
    // drain counts as a fresh boundary. The counter resets on every
    // empty drain (below), so continuations forced by OTHER Stop hooks
    // cannot inflate it across turns.
    let stop_active = hook_mode && stdin_stop_hook_active();
    // The ceiling fails closed: a forced continuation that cannot record
    // its round (state dir unwritable) would read round 1 forever.
    let rounds = match bump_drain_rounds(&session_id, stop_active) {
        Some(r) => r,
        None if stop_active => {
            eprintln!("(cannot record the drain round — deferring to the Monitor conduit)");
            return;
        }
        None => 1,
    };
    if rounds > MAX_DRAIN_ROUNDS {
        eprintln!("(drain round {rounds} > {MAX_DRAIN_ROUNDS} — deferring to the Monitor conduit)");
        return;
    }

    let store = attend_state::StateStore::new(Some(session_id.clone()));

    // Cold start = no conduit has applied the cold-start rule yet: no
    // state file, or one without `baselined` (the peers sensor checkpoints
    // on its first poll, before it scans messages). Load once; the same
    // snapshot supplies the seen-set.
    let snapshot = store.load();
    let baselining = !snapshot.as_ref().is_some_and(|s| s.baselined);
    let seen = snapshot.map(|s| s.seen_signals).unwrap_or_default();

    let cwd = ident.origin_path.clone();
    let Scan { mut delivered, mark, note } = scan_pending(&r.receive_dirs(&cwd), &seen, &session_id, baselining);
    delivered.sort_by_key(|d| d.mtime);

    if delivered.is_empty() && note.is_none() {
        // Empty drain: reset the re-entry counter (this boundary chain
        // is ending) and say nothing in hook mode so the turn ends —
        // the termination property the re-entry design leans on.
        reset_drain_rounds(&session_id);
        // A cold start must still persist the (possibly empty) baseline,
        // or the next drain re-baselines and swallows what arrived in
        // between (PR #385 review, blocking finding).
        if baselining {
            store.baseline(mark);
        } else {
            store.mark_seen(mark);
        }
        if !hook_mode {
            println!("no pending messages");
        }
        return;
    }

    // Deliver, THEN record: a crash between the two re-delivers at the
    // next boundary (the seen-set never got the marks), which dedup
    // tolerates — at-least-once. The reverse order would mark messages
    // consumed with no conduit having shown them: loss on BOTH conduits,
    // since the sensor imports drain marks (PR #385 review, finding 2).
    if hook_mode {
        let reason = render_drain_reason(&delivered, note.as_deref());
        println!(
            "{{\"decision\": \"block\", \"reason\": \"{}\"}}",
            json_escape(&reason)
        );
    } else {
        for d in &delivered {
            println!("[{}] {}", d.scope, d.sender);
            println!("  when:    {}", d.when);
            println!("  id:      {}", d.id);
            // The label carries only the cwd basename; the wire `from`
            // (with an external sender's terminal suffix) and the full
            // source path stay reachable here.
            println!("  from:    {}", d.sender_id);
            println!("  cwd:     {}", d.source_cwd);
            println!("  message: {}", d.body);
            println!();
        }
        if let Some(note) = &note {
            println!("{note}");
        }
        println!("{} message(s) drained and marked consumed", delivered.len());
    }

    if baselining {
        store.baseline(mark);
    } else {
        store.mark_seen(mark);
    }

    // `attend reply` should target what the drain delivered, exactly as
    // it targets what the sensor surfaced.
    #[cfg(feature = "sensor-peers")]
    if let Some(newest) = delivered.iter().rev().find(|d| d.source_cwd != cwd) {
        sensor_peers::last_inbound::record(&session_id, &newest.id);
    }
    #[cfg(not(feature = "sensor-peers"))]
    let _ = cwd;
}

/// The injected turn-continuation text. Sober and contract-preserving:
/// a drained message informs the turn — the standing messaging
/// guidance (reply autonomy, silence-is-valid) rides along verbatim so
/// turn-boundary delivery never reads as a command to respond.
///
/// `note` is the cold start's count of messages it held back; it rides the
/// first delivery after enrollment, or goes alone when nothing else does.
fn render_drain_reason(delivered: &[impl DrainedView], note: Option<&str>) -> String {
    if delivered.is_empty() {
        return format!("[attend] {}", note.unwrap_or("no peer messages"));
    }
    let mut out = format!(
        "[attend] {} peer message(s) delivered at the turn boundary (ADR-172 drain):\n",
        delivered.len()
    );
    if let Some(note) = note {
        out.push_str(&format!("({note})\n"));
    }
    for d in delivered.iter().take(DRAIN_RENDER_MAX) {
        // The canonical id is usually already the id stem's prefix;
        // spell it out only when it is not, so the reason does not
        // repeat ~45 characters per row.
        let from = if from_is_id_prefix(d.sender_id(), d.id()) {
            String::new()
        } else {
            format!(", from {}", d.sender_id())
        };
        out.push_str(&format!(
            "\n[{}] {} ({}, id {}{}):\n{}\n",
            d.when(),
            d.sender(),
            d.scope(),
            d.id(),
            from,
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

/// Whether the canonical sender id is already legible from the message
/// id. Id stems are `<sender-id>-<timestamp>-<seq>`
/// (`agent_identity::signal_filename`), so for a claude sender the
/// session id opens the stem. External ids are sanitized in the stem
/// (`aaron@kitty` → `aaron-kitty`) and so do not match; those rows, and
/// hand-written ids, get an explicit `from`.
fn from_is_id_prefix(from: &str, id: &str) -> bool {
    let ident = from.split_once(':').map(|(_, i)| i).unwrap_or(from);
    !ident.is_empty()
        && id
            .strip_prefix(ident)
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('-'))
}

/// View trait so `render_drain_reason` is testable without the
/// filesystem-shaped `Drained` struct.
trait DrainedView {
    fn when(&self) -> &str;
    fn sender(&self) -> &str;
    /// Canonical sender id — the wire `from` (`claude:<session-id>`).
    fn sender_id(&self) -> &str;
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
fn drain_rounds_path(session_id: &str) -> std::path::PathBuf {
    attend_presence::cache::state_dir().join(format!("{session_id}.drain-rounds"))
}

/// The round this drain is, or `None` when the counter cannot be written.
fn bump_drain_rounds(session_id: &str, stop_active: bool) -> Option<u32> {
    let path = drain_rounds_path(session_id);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).ok();
    }
    let rounds = if stop_active {
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0)
            + 1
    } else {
        1
    };
    // tmp + rename: a torn write that parses as garbage would read as
    // "fresh boundary" forever and quietly disable the ceiling.
    let tmp = path.with_extension(format!("drain-rounds.tmp.{}", std::process::id()));
    let written = std::fs::write(&tmp, rounds.to_string()).is_ok() && std::fs::rename(&tmp, &path).is_ok();
    written.then_some(rounds)
}

/// An empty drain ends the boundary chain: remove the counter (also the
/// sidecar's cleanup — no stale `.drain-rounds` files accumulate).
fn reset_drain_rounds(session_id: &str) {
    std::fs::remove_file(drain_rounds_path(session_id)).ok();
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
        when: String,
        sender: String,
        sender_id: String,
        scope: String,
        id: String,
        body: String,
    }
    impl DrainedView for FakeMsg {
        fn when(&self) -> &str { &self.when }
        fn sender(&self) -> &str { &self.sender }
        fn sender_id(&self) -> &str { &self.sender_id }
        fn scope(&self) -> &str { &self.scope }
        fn id(&self) -> &str { &self.id }
        fn body(&self) -> &str { &self.body }
    }
    fn msg(n: usize) -> FakeMsg {
        FakeMsg {
            when: "11:4{}".replace("{}", &(n % 10).to_string()),
            sender: format!("peer-{n}"),
            sender_id: format!("claude:session-{n}"),
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
        // The quoted-key pattern keeps bare prose mentions from matching.
        // Known accepted limit: a JSON string VALUE embedding the exact
        // quoted `"stop_hook_active": true` token would still false-
        // positive — the harness controls this payload, so a real parser
        // is not worth the dependency.
        assert!(!parse_stop_hook_active(
            r#"{"note": "stop_hook_active is unrelated here", "stop_hook_active": false}"#
        ));
    }

    #[test]
    fn drain_reason_renders_messages_and_contract_line() {
        let msgs = vec![msg(1), msg(2)];
        let reason = render_drain_reason(&msgs, None);
        assert!(reason.contains("2 peer message(s)"));
        assert!(reason.contains("peer-1 (#open, id id-1, from claude:session-1):"));
        assert!(reason.contains("body 2"));
        assert!(reason.contains("silence is a valid reply"));
    }

    // --- scan_pending core (PR #385 review, finding 6) ---

    fn scan_fixture(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "attend-drain-scan-{tag}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Write a signal file; returns its seen-set key.
    fn write_signal(dir: &std::path::Path, name: &str, from: &str, msg: &str) -> String {
        let file = format!("{name}.signal");
        std::fs::write(dir.join(&file), format!("{from}|proj|/src/cwd|{msg}\n")).unwrap();
        attend_state::seen_key(&file)
    }

    fn dirs(path: &std::path::Path, room: Room) -> Vec<attend_groups::ReceiveDir> {
        vec![attend_groups::ReceiveDir { path: path.to_path_buf(), room }]
    }

    fn age(dir: &std::path::Path, name: &str, by: std::time::Duration) {
        let f = std::fs::File::options().write(true).open(dir.join(format!("{name}.signal"))).unwrap();
        f.set_modified(std::time::SystemTime::now() - by).unwrap();
    }

    #[test]
    fn a_signal_moved_between_trays_stays_seen() {
        // Read in one tray, then moved into another: keyed by directory
        // it would be delivered again. The key is the filename.
        let root = scan_fixture("moved");
        let (old, new) = (root.join("tray-a"), root.join("tray-b"));
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        let key = write_signal(&old, "m1", "claude:other-session", "once");
        let seen: std::collections::HashSet<String> = [key].into_iter().collect();
        std::fs::rename(old.join("m1.signal"), new.join("m1.signal")).unwrap();
        let scan = scan_pending(&dirs(&new, Room::Project), &seen, "my-session", false);
        assert!(scan.delivered.is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn scan_delivers_unseen_and_filters_seen() {
        let dir = scan_fixture("seen");
        let k1 = write_signal(&dir, "peer-1", "claude:other-session", "already seen");
        let k2 = write_signal(&dir, "peer-2", "claude:other-session", "new message");
        let seen: std::collections::HashSet<String> = [k1].into_iter().collect();
        let Scan { delivered, mark, note } = scan_pending(&dirs(&dir, Room::Open), &seen, "my-session", false);
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].body, "new message");
        assert_eq!(delivered[0].scope, "#open");
        assert_eq!(mark, vec![k2]);
        assert_eq!(note, None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn scan_marks_own_messages_without_delivering() {
        let dir = scan_fixture("own");
        let k = write_signal(&dir, "self-1", "claude:my-session", "my own send");
        let Scan { delivered, mark, .. } = scan_pending(&dirs(&dir, Room::Open), &Default::default(), "my-session", true);
        assert!(delivered.is_empty(), "own message must not deliver");
        assert_eq!(mark, vec![k], "own message must still be marked (dedup bookkeeping)");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Regression for the PR #385 blocking finding: under a cold-start
    /// baseline, a FRESH message (live conversation racing the first
    /// checkpoint) must deliver, not be silently marked consumed.
    #[test]
    fn baseline_delivers_fresh_messages() {
        let dir = scan_fixture("baseline-fresh");
        let k = write_signal(&dir, "peer-1", "claude:other-session", "arrived just now");
        let Scan { delivered, mark, note } = scan_pending(&dirs(&dir, Room::Open), &Default::default(), "my-session", true);
        assert_eq!(delivered.len(), 1, "fresh message must survive the baseline");
        assert_eq!(mark, vec![k]);
        assert_eq!(note, None);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The flip side: stale `#open` backlog is marked without being shown,
    /// and counted in the note, never consumed silently.
    #[test]
    fn baseline_marks_stale_backlog_and_announces_it() {
        let dir = scan_fixture("baseline-stale");
        let k = write_signal(&dir, "peer-1", "claude:other-session", "durable backlog");
        age(&dir, "peer-1", std::time::Duration::from_secs(600));

        let Scan { delivered, mark, note } = scan_pending(&dirs(&dir, Room::Open), &Default::default(), "my-session", true);
        assert!(delivered.is_empty(), "stale backlog must not flood a cold start");
        assert_eq!(mark, vec![k], "stale backlog must be marked so it baselines once");
        assert_eq!(note.as_deref(), Some("1 earlier message not shown; attend inbox"));
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Addressed mail survives a cold start whatever its age.
    #[test]
    fn baseline_delivers_old_addressed_mail() {
        let dir = scan_fixture("baseline-addressed");
        write_signal(&dir, "peer-1", "claude:other-session", "sent with --to an hour ago");
        age(&dir, "peer-1", std::time::Duration::from_secs(3600));

        let scan = scan_pending(&dirs(&dir, Room::Project), &Default::default(), "my-session", true);
        assert_eq!(scan.delivered.len(), 1);
        assert_eq!(scan.note, None);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Issue #534: one message, two conduits, one name. A real
    /// `PeerSensor` polls the fixture tray (the Monitor conduit) and the
    /// scan core drains it (the Stop-hook conduit); the sender text must
    /// be identical, or a receiver has to know both forms to correlate a
    /// notification with a later drain row.
    #[cfg(feature = "sensor-peers")]
    #[test]
    fn monitor_and_drain_render_the_same_sender() {
        use sensor_trait::{Focus, Sensor};

        let dir = scan_fixture("conduits");
        write_signal(&dir, "peer-1", "claude:other-session", "same name on both");
        // Drain path.
        let Scan { delivered, .. } = scan_pending(&dirs(&dir, Room::Project), &Default::default(), "my-session", false);
        assert_eq!(delivered.len(), 1);
        let drained = &delivered[0];

        // Monitor path: the sensor scans only the fixture, as its project
        // tray. A non-empty state import skips the cold-start baseline
        // that would otherwise swallow the fixture, and
        // `reply_hint_shown` keeps the hint off the body.
        let mut sensor = sensor_peers::PeerSensor::new();
        let fixture = dir.clone();
        sensor.set_receive_dirs_provider(std::sync::Arc::new(move |_: &str| {
            vec![attend_groups::ReceiveDir { path: fixture.clone(), room: attend_groups::Room::Project }]
        }));
        sensor.import_state(&[("reply_hint_shown".to_string(), "true".to_string())]);
        // The fixture's cwd doubles as the focus so the sensor's
        // `attend reply` bookkeeping (keyed on the host session) sees
        // an own-project message and records nothing.
        let focus = Focus {
            description: String::new(),
            working_dir: "/src/cwd".to_string(),
            keywords: Vec::new(),
        };
        let _ = sensor.poll(&focus); // first poll: peer-presence baseline, emits nothing
        let observations = sensor.poll(&focus);

        let expected = format!("message from {}: same name on both", drained.sender);
        assert!(
            observations.iter().any(|(_, line)| line == &expected),
            "Monitor conduit did not emit {expected:?}; got {:?}",
            observations.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>()
        );
        // Persona-plus-project form on both — not the pre-#534 Monitor
        // form `claude//src/cwd`.
        assert!(drained.sender.ends_with(" (cwd)"), "{:?}", drained.sender);
        assert!(!drained.sender.contains('/'), "{:?}", drained.sender);
        // The canonical id rides the row, so the label never has to be
        // the key (ADR-171).
        assert_eq!(drained.sender_id, "claude:other-session");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The id stem already opens with the sender's session id, so the
    /// hook reason spells `from` out only when the id does not carry it.
    #[test]
    fn drain_reason_omits_from_when_id_carries_it() {
        let carried = FakeMsg {
            when: "11:40".into(),
            sender: "Jovan-alpha (ws)".into(),
            sender_id: "claude:abc123".into(),
            scope: "project".into(),
            id: "abc123-1712345-0".into(),
            body: "hi".into(),
        };
        let reason = render_drain_reason(&[carried], None);
        assert!(reason.contains("Jovan-alpha (ws) (project, id abc123-1712345-0):"), "{reason}");
        assert!(!reason.contains("from claude:"), "{reason}");

        assert!(from_is_id_prefix("claude:abc", "abc-1-0"));
        assert!(from_is_id_prefix("claude:abc", "abc"));
        assert!(!from_is_id_prefix("claude:abc", "abcd-1-0"), "partial match is not a prefix");
        assert!(!from_is_id_prefix("external:aaron@kitty", "aaron-kitty-1-0"));
        assert!(!from_is_id_prefix("claude:", "-1-0"));
    }

    /// A cold start with nothing to deliver sends the note alone, not a
    /// "0 peer message(s) delivered" header.
    #[test]
    fn note_only_reason_is_the_note() {
        let reason = render_drain_reason(&[] as &[FakeMsg], Some("3 earlier messages not shown; attend inbox"));
        assert!(reason.starts_with("[attend] 3 earlier messages not shown; attend inbox"), "{reason}");
        assert!(!reason.contains("0 peer message"), "{reason}");
    }

    #[test]
    fn drain_reason_caps_render_and_counts_remainder() {
        let msgs: Vec<FakeMsg> = (0..14).map(msg).collect();
        let reason = render_drain_reason(&msgs, None);
        assert!(reason.contains("14 peer message(s)"));
        assert!(reason.contains("body 9"));
        assert!(!reason.contains("body 10"));
        assert!(reason.contains("(+4 more — attend inbox for the rest)"));
    }
}
