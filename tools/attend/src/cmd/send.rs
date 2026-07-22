//! `attend send` — broadcast a signal to peer sessions.
//! `attend reply` — `send --re <last-inbound>` sugar, feature-gated.

use crate::cmd::inbox::is_valid_signal_id;
use crate::util::{encode_project, get_groups, own_session_id, signals_base};

pub(crate) fn cmd_send(
    broadcast: bool,
    target_dir: Option<String>,
    target_focus: Option<String>,
    reply_to: Option<String>,
    message_parts: Vec<String>,
) {
    // A signal id must match the same character class the parser uses to
    // disambiguate threaded records from legacy messages that happen to
    // start with "re:". Signal filename stems are
    // `<sanitized-sender>-<nanos>-<seq>` (see `agent_identity::signal_filename`),
    // so `[A-Za-z0-9_-]+` comfortably covers the real shape and rejects
    // anything that would break the wire format (pipes, whitespace,
    // control chars) or trip the ambiguity fence in parse_signal.
    if let Some(ref id) = reply_to {
        if !is_valid_signal_id(id) {
            eprintln!("attend send: --re signal id must be non-empty and match [A-Za-z0-9_-]+");
            std::process::exit(1);
        }
    }

    let message = message_parts.join(" ");
    if message.is_empty() {
        eprintln!("usage: attend send <message>");
        eprintln!("  (reaches every peer and Aaron — no routing flags needed)");
        eprintln!("  tip: wrap message in double quotes to avoid shell expansion");
        std::process::exit(1);
    }

    // Fence: detect probable shell glob expansion.
    // If any message part is an existing file path, the shell likely
    // expanded a metachar (e.g. zsh expanded "hello?" into filenames).
    let suspect_expansion = message_parts.iter().any(|part| {
        std::path::Path::new(part).exists() && !part.contains(' ')
    });
    if suspect_expansion {
        eprintln!("[attend] warning: message contains existing file paths — shell may have expanded metacharacters");
        eprintln!("[attend] did you mean: attend send \"{}\"", message);
        eprintln!("[attend] sending anyway, but wrap in quotes next time");
    }

    let base = signals_base();
    // Wire identity rides this cwd (issue #378): the session record's
    // origin path, so a stray shell `cd` can't relabel our sends.
    let cwd = crate::util::own_origin_cwd();

    // Validate --to path against active peers
    if let Some(ref path) = target_dir {
        let resolved = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.clone());

        #[cfg(feature = "sensor-peers")]
        let peers = {
            let sensor = crate::sensors::PeerSensor::new();
            sensor.list_peers()
        };
        #[cfg(not(feature = "sensor-peers"))]
        let peers: Vec<(String, String, String, String, f64)> = Vec::new();
        let peer_paths: Vec<&str> = peers.iter().map(|(_, cwd, _, _, _)| cwd.as_str()).collect();

        if !peer_paths.contains(&resolved.as_str()) {
            eprintln!("error: no active peer at {}", resolved);
            if peers.is_empty() {
                eprintln!("\nno active peer sessions found");
            } else {
                eprintln!("\nactive peers:");
                for (_sid, peer_cwd, project, _, _) in &peers {
                    eprintln!("  {} ({})", peer_cwd, project);
                }
                // Fuzzy suggest: find closest match by path suffix
                if let Some(suggestion) = find_closest_peer(&resolved, &peer_paths) {
                    eprintln!("\ndid you mean: {}?", suggestion);
                }
            }
            std::process::exit(1);
        }
    }

    let r = get_groups();

    // Validate --focus name against live `_groups.yaml` membership. A
    // signal written to a group nobody is *currently* listening on sits
    // unread in `@<name>/` until cleanup sweeps it; the sender only sees
    // "signal written" and assumes delivery. Mirror --to's liveness
    // discipline: `_groups.yaml` membership is intersected with
    // `PeerSensor::live_session_ids` so a peer that joined-and-died
    // does not let the validation pass on a phantom member.
    if let Some(ref name) = target_focus {
        let members = r.members(name);
        let self_id = own_session_id();
        #[cfg(feature = "sensor-peers")]
        let live_ids: std::collections::HashSet<String> = {
            let sensor = crate::sensors::PeerSensor::new();
            sensor.live_session_ids()
        };
        #[cfg(not(feature = "sensor-peers"))]
        let live_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        // A member is live if it's a running claude session OR its
        // heartbeat is fresh. The heartbeat arm covers human members
        // (ADR-170): attend-chat heartbeats the username while open,
        // and a human never appears in the claude-process scan — so
        // without it, a human-only group would reject agent sends
        // with a phantom "no live peers".
        let live_peer_count: usize = match &members {
            Some(ids) => ids
                .iter()
                .filter(|sid| {
                    (live_ids.contains(*sid)
                        || attend_heartbeat::is_fresh(sid, attend_heartbeat::DEFAULT_GRACE))
                        && self_id.as_ref().map(|s| s != *sid).unwrap_or(true)
                })
                .count(),
            None => 0,
        };
        if live_peer_count == 0 {
            let self_in_group = members
                .as_ref()
                .zip(self_id.as_ref())
                .map(|(ids, sid)| ids.iter().any(|m| m == sid))
                .unwrap_or(false);
            if members.is_none() {
                eprintln!("error: no focus group named '{}'", name);
            } else if self_in_group {
                eprintln!("error: no live peers in focus group '{}' (you are the only listener)", name);
            } else {
                eprintln!("error: no live peers in focus group '{}'", name);
            }
            let groups = r.all_groups();
            if groups.is_empty() {
                eprintln!("\nno active focus groups");
            } else {
                eprintln!("\nactive focus groups (yaml count, live peers may be fewer):");
                for (gname, count, pinned) in &groups {
                    let pin = if *pinned { " (pinned)" } else { "" };
                    let suffix = if *count == 1 { "" } else { "s" };
                    eprintln!("  {} — {} member{}{}", gname, count, suffix, pin);
                }
            }
            eprintln!("\ndrop --focus to broadcast (reaches every peer):");
            eprintln!("  attend send <message>");
            std::process::exit(1);
        }
    }

    // Determine target directories.
    // Default is broadcast — simplest possible routing: every send reaches
    // every peer. Escape hatches remain for humans and scripts:
    //   --to <path>: specific project only
    //   --focus <name>: specific focus group only
    //   --broadcast: explicit (same as default)
    let dest_dirs: Vec<std::path::PathBuf> = if let Some(ref focus_name) = target_focus {
        vec![r.group_dir(focus_name)]
    } else if let Some(ref path) = target_dir {
        let resolved = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.clone());
        vec![base.join(encode_project(&resolved))]
    } else {
        // Default (and --broadcast): reach everyone via the broadcast dir.
        let _ = broadcast; // flag now redundant, kept for compat
        vec![base.join("_broadcast")]
    };

    let (sender_id, source_kind) = identify_sender();
    let project = cwd.rsplit('/').next().unwrap_or("?");

    let from = format!("{}:{}", source_kind, sender_id);
    // Build the filename stem, which doubles as the signal id `re:<id>`
    // replies reference. `signal_filename` normalizes the sender id
    // (external senders are `$USER@<terminal>`; the raw `@`/`.` would fail
    // `is_valid_signal_id` and break `attend reply` auto-threading —
    // issue #368) and makes the name collision-proof. The `from` field
    // above keeps the un-normalized identity.
    let filename = agent_identity::signal_filename(&sender_id);
    // Wire format: `from|project|cwd|message` (legacy) or
    // `from|project|cwd|re:signal-id|message` (threaded reply). The `re:`
    // field is only emitted when --re was given; unthreaded sends stay
    // byte-identical to the pre-ADR-120 format.
    //
    // **Wire-format mirror.** `tools/attend-chat/src/signal.rs::write_broadcast`
    // produces the legacy branch of this format. Keep the two in
    // lockstep; there is no shared crate gating the contract.
    let content = match &reply_to {
        Some(id) => format!("{}|{}|{}|re:{}|{}\n", from, project, cwd, id, message),
        None => format!("{}|{}|{}|{}\n", from, project, cwd, message),
    };

    let scope = if target_focus.is_some() {
        "focus"
    } else if target_dir.is_some() {
        "directed"
    } else {
        "#open"
    };

    for dest_dir in &dest_dirs {
        std::fs::create_dir_all(dest_dir).ok();
        let path = dest_dir.join(&filename);
        let tmp_path = dest_dir.join(format!("{}.tmp", filename));

        match std::fs::write(&tmp_path, &content) {
            Ok(_) => {
                if let Err(e) = std::fs::rename(&tmp_path, &path) {
                    eprintln!("[attend] error renaming signal: {}", e);
                    std::fs::remove_file(&tmp_path).ok();
                }
            }
            Err(e) => eprintln!(
                "[attend] error writing signal to {}: {}",
                dest_dir.display(),
                e
            ),
        }
    }

    eprintln!(
        "[attend] signal written ({}, {} dirs): {}",
        scope,
        dest_dirs.len(),
        filename
    );
}

/// How `attend reply` should thread, given the recorded last-inbound id.
/// Pure decision, split out from `cmd_reply` so the degradation rule
/// (issue #368) is unit-testable without touching the filesystem or the
/// process exit path.
#[cfg(feature = "sensor-peers")]
#[derive(Debug, PartialEq, Eq)]
enum ReplyTarget {
    /// No prior inbound at all — reply has nothing to thread against.
    NoInbound,
    /// A prior inbound exists but its id can't be threaded; degrade to an
    /// unthreaded send rather than leak the `send --re` validation error.
    Unthreaded,
    /// A valid threadable id.
    Threaded(String),
}

#[cfg(feature = "sensor-peers")]
fn classify_reply_target(last_inbound: Option<String>) -> ReplyTarget {
    match last_inbound {
        None => ReplyTarget::NoInbound,
        Some(id) if is_valid_signal_id(&id) => ReplyTarget::Threaded(id),
        Some(_) => ReplyTarget::Unthreaded,
    }
}

/// `attend reply <message>` — thin sugar over `attend send --re <last-inbound>`.
///
/// Reads the most-recent inbound signal id from per-session state that
/// `sensor-peers::read_signals` writes every time it emits a peer
/// observation. If no prior inbound exists the command exits with a
/// clear error rather than silently falling through to an unthreaded
/// send — threaded-vs-unthreaded is a semantic distinction and
/// guessing is the wrong default. If a prior inbound exists but its id
/// is not threadable (issue #368), it degrades to an unthreaded send
/// rather than leaking the internal `send --re` validation error.
///
/// The entire point of this subcommand is to keep the 50-char signal
/// uuid out of the agent's context window. A caller never sees the
/// id, never has to hunt for it in `attend inbox`, and never reaches
/// into `~/.cache/attend/signals/` to find it. Delegating to
/// `cmd_send` preserves every existing `send` flag (`--focus`,
/// `--to`, `--broadcast`) without duplication.
#[cfg(feature = "sensor-peers")]
pub(crate) fn cmd_reply(
    broadcast: bool,
    target_dir: Option<String>,
    target_focus: Option<String>,
    message: Vec<String>,
) {
    let session_id =
        own_session_id().unwrap_or_else(|| format!("pid-{}", std::process::id()));
    let reply_to = match classify_reply_target(sensor_peers::last_inbound::read(&session_id)) {
        // Genuine "nothing to reply to" — the agent needs to do something
        // different (start a new topic), so this stays a hard error.
        ReplyTarget::NoInbound => {
            eprintln!("attend reply: no prior inbound signal to thread against.");
            eprintln!("  (reply is for responding to a peer message your sensor surfaced.)");
            eprintln!("  if you are starting a new topic, use `attend send` instead.");
            std::process::exit(1);
        }
        // There *is* a prior inbound, but its recorded id is not
        // threadable. A standing guard, not a transition shim: whatever
        // the source of a bad id (a stale pre-#368 record from an external
        // `$USER@<terminal>` sender, a hand-edited state file, some future
        // writer that skips `signal_filename`), the agent did nothing
        // wrong and can't fix it, so we never punish it with the internal
        // `send --re` validation error. Threading is cosmetic —
        // `parse_signal` drops the `re:` id and no peer renders it — so
        // degrading to an unthreaded send is lossless: the message still
        // lands. Note it on stderr and carry on.
        ReplyTarget::Unthreaded => {
            eprintln!(
                "[attend] note: last inbound id is not threadable; sending unthreaded (message still delivered)."
            );
            None
        }
        ReplyTarget::Threaded(id) => Some(id),
    };
    // Inject the resolved signal id as `reply_to` and delegate to cmd_send.
    // All other routing flags (--focus, --to, --broadcast) flow through
    // untouched.
    cmd_send(broadcast, target_dir, target_focus, reply_to, message);
}

#[cfg(not(feature = "sensor-peers"))]
pub(crate) fn cmd_reply(
    _broadcast: bool,
    _target_dir: Option<String>,
    _target_focus: Option<String>,
    _message: Vec<String>,
) {
    eprintln!("attend reply: sensor-peers feature is not compiled in this build");
    std::process::exit(1);
}

// --- Sender identity helpers ---

/// Determine who is sending this signal.
/// Returns (identity_string, source_kind) where source_kind is "claude" or "external".
fn identify_sender() -> (String, &'static str) {
    // First, try to find a Claude session ID (we're inside a Claude session)
    if let Some(sid) = own_session_id() {
        return (sid, "claude");
    }

    // Not inside Claude — build identity from environment
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string());

    // Detect terminal: check common terminal-specific env vars
    let terminal = detect_terminal();

    let identity = if !terminal.is_empty() {
        format!("{}@{}", user, terminal)
    } else {
        user
    };

    (identity, "external")
}

/// Best-effort terminal detection from environment variables.
fn detect_terminal() -> String {
    // Specific terminal emulators set their own env vars
    if std::env::var("KITTY_PID").is_ok() {
        return "kitty".to_string();
    }
    if std::env::var("ALACRITTY_SOCKET").is_ok() {
        return "alacritty".to_string();
    }
    if std::env::var("WEZTERM_PANE").is_ok() {
        return "wezterm".to_string();
    }
    if std::env::var("TMUX").is_ok() {
        return "tmux".to_string();
    }
    if std::env::var("STY").is_ok() {
        return "screen".to_string();
    }
    // TERM_PROGRAM is set by some terminals (macOS Terminal, iTerm2, VS Code)
    if let Ok(tp) = std::env::var("TERM_PROGRAM") {
        return tp.to_lowercase();
    }
    // SSH session
    if std::env::var("SSH_CONNECTION").is_ok() {
        return "ssh".to_string();
    }
    // Fallback: try TERMINAL or just use the shell
    if let Ok(t) = std::env::var("TERMINAL") {
        return t.rsplit('/').next().unwrap_or(&t).to_string();
    }
    String::new()
}

/// Find the closest matching peer path by comparing path suffixes.
fn find_closest_peer<'a>(target: &str, peers: &[&'a str]) -> Option<&'a str> {
    // Try matching the last N segments of the target against peer paths
    let target_parts: Vec<&str> = target.rsplit('/').collect();
    let mut best: Option<(&str, usize)> = None;

    for peer in peers {
        let peer_parts: Vec<&str> = peer.rsplit('/').collect();
        let common = target_parts
            .iter()
            .zip(peer_parts.iter())
            .take_while(|(a, b)| a == b)
            .count();
        if common > 0 && (best.is_none() || common > best.unwrap().1) {
            best = Some((peer, common));
        }
    }

    best.map(|(p, _)| p)
}

#[cfg(all(test, feature = "sensor-peers"))]
mod tests {
    use super::*;

    #[test]
    fn no_inbound_is_a_hard_error_case() {
        assert_eq!(classify_reply_target(None), ReplyTarget::NoInbound);
    }

    #[test]
    fn valid_id_threads() {
        assert_eq!(
            classify_reply_target(Some("claude-2f2632d7-1712345".to_string())),
            ReplyTarget::Threaded("claude-2f2632d7-1712345".to_string())
        );
    }

    #[test]
    fn external_at_bearing_id_degrades_to_unthreaded() {
        // The regression from issue #368: a last-inbound id from an
        // external `$USER@<terminal>` sender must NOT reach cmd_send's
        // `--re` validation — it degrades to an unthreaded send instead.
        assert_eq!(
            classify_reply_target(Some("aaron@kitty-1712345".to_string())),
            ReplyTarget::Unthreaded
        );
        assert_eq!(
            classify_reply_target(Some("aaron@iterm.app-1712345".to_string())),
            ReplyTarget::Unthreaded
        );
    }
}
