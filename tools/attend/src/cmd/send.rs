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
    // start with "re:". Signal filename stems are `<sender-id>-<ts>`, so
    // `[A-Za-z0-9_-]+` comfortably covers the real shape and rejects
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
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

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
        let live_peer_count: usize = match &members {
            Some(ids) => ids
                .iter()
                .filter(|sid| {
                    live_ids.contains(*sid)
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
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let from = format!("{}:{}", source_kind, sender_id);
    let filename = format!("{}-{}.signal", sender_id.replace('/', "-"), ts);
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

/// `attend reply <message>` — thin sugar over `attend send --re <last-inbound>`.
///
/// Reads the most-recent inbound signal id from per-session state that
/// `sensor-peers::read_signals` writes every time it emits a peer
/// observation. If no prior inbound exists the command exits with a
/// clear error rather than silently falling through to an unthreaded
/// send — threaded-vs-unthreaded is a semantic distinction and
/// guessing is the wrong default.
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
    let last_id = match sensor_peers::last_inbound::read(&session_id) {
        Some(id) => id,
        None => {
            eprintln!("attend reply: no prior inbound signal to thread against.");
            eprintln!("  (reply is for responding to a peer message your sensor surfaced.)");
            eprintln!("  if you are starting a new topic, use `attend send` instead.");
            std::process::exit(1);
        }
    };
    // Inject the resolved signal id as `reply_to` and delegate to cmd_send.
    // All other routing flags (--focus, --to, --broadcast) flow through
    // untouched.
    cmd_send(broadcast, target_dir, target_focus, Some(last_id), message);
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
