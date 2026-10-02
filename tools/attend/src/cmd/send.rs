//! `attend send` — broadcast a signal to peer sessions.
//! `attend reply` — `send --re <last-inbound>` sugar, feature-gated.

use crate::cmd::inbox::is_valid_signal_id;
use crate::util::{encode_project, get_groups, own_session_id, signals_base};

/// The message is a trailing, hyphen-tolerant argument, so clap hands any
/// unknown flag (a removed `--broadcast` or `--focus`, or a typo such as
/// `--chanel`) over as message text, and the send would go out with the flag in
/// its body. A leading token shaped like a long flag is refused unless a `--`
/// sits immediately before it in the raw arguments (the escape hatch for a
/// message that really starts with one); a later `--` in the text does not
/// count.
fn flag_like_refusal(message: &[String], raw_args: &[String]) -> Option<String> {
    let first = message.first()?.as_str();
    let name = first.split('=').next().unwrap_or(first);
    let flag_like = name.len() > 2
        && name.starts_with("--")
        && name[2..].starts_with(|c: char| c.is_ascii_lowercase())
        && name[2..].chars().all(|c| c.is_ascii_lowercase() || c == '-');
    let escaped = raw_args.windows(2).any(|w| w[0] == "--" && w[1] == first);
    if !flag_like || escaped {
        return None;
    }
    let hint = match name {
        // transition: removed by #717 (ADR-506)
        "--broadcast" => " It was removed: a send with no routing flag already reaches everyone.",
        // transition: removed by #717 (ADR-506)
        "--focus" => " It was removed: use --channel.",
        _ => "",
    };
    Some(format!(
        "`{name}` is not a flag this command accepts.{hint} To send it as text, put `--` before the message."
    ))
}

/// Exit 2 when `message` leads with a flag-shaped token and no `--` escape.
pub(crate) fn reject_flag_like(verb: &str, message: &[String]) {
    let raw: Vec<String> = std::env::args().collect();
    if let Some(why) = flag_like_refusal(message, &raw) {
        eprintln!("attend {verb}: {why}");
        std::process::exit(2);
    }
}

pub(crate) fn cmd_send(
    target_dir: Option<String>,
    target_channel: Option<String>,
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

    // Validate --channel name against live `_groups.yaml` membership. A
    // signal written to a group nobody is *currently* listening on sits
    // unread in `@<name>/` until cleanup sweeps it; the sender only sees
    // "signal written" and assumes delivery. Mirror --to's liveness
    // discipline: `_groups.yaml` membership is intersected with
    // `PeerSensor::live_session_ids` so a peer that joined-and-died
    // does not let the validation pass on a phantom member.
    if let Some(ref name) = target_channel {
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
                    attend_presence::alive(sid, &live_ids)
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
                eprintln!("error: no channel named '{}'", name);
            } else if self_in_group {
                eprintln!("error: no live peers in channel '{}' (you are the only listener)", name);
            } else {
                eprintln!("error: no live peers in channel '{}'", name);
            }
            let groups = r.all_groups();
            if groups.is_empty() {
                eprintln!("\nno active channels");
            } else {
                eprintln!("\nactive channels (yaml count, live peers may be fewer):");
                for (gname, count, pinned) in &groups {
                    let pin = if *pinned { " (pinned)" } else { "" };
                    let suffix = if *count == 1 { "" } else { "s" };
                    eprintln!("  {} — {} member{}{}", gname, count, suffix, pin);
                }
            }
            eprintln!("\ndrop --channel to broadcast (reaches every peer):");
            eprintln!("  attend send <message>");
            std::process::exit(1);
        }
    }

    // Determine target directories.
    // Default is broadcast — simplest possible routing: every send reaches
    // every peer. Escape hatches remain for humans and scripts:
    //   --to <path>: specific project only
    //   --channel <name>: specific channel only
    let dest_dirs: Vec<std::path::PathBuf> = if let Some(ref channel_name) = target_channel {
        vec![r.group_dir(channel_name)]
    } else if let Some(ref path) = target_dir {
        let resolved = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| path.clone());
        vec![base.join(encode_project(&resolved))]
    } else {
        // Default: reach everyone via the broadcast dir.
        vec![base.join(attend_groups::BROADCAST_DIR)]
    };

    let (sender_id, source_kind) = agent_identity::signal::identify_sender(own_session_id());
    let from = format!("{}:{}", source_kind, sender_id);
    // Build the filename stem, which doubles as the signal id `re:<id>`
    // replies reference. `signal_filename` normalizes the sender id
    // (external senders are `$USER@<terminal>`; the raw `@`/`.` would fail
    // `is_valid_signal_id` and break `attend reply` auto-threading —
    // issue #368) and makes the name collision-proof. The `from` field
    // above keeps the un-normalized identity.
    let filename = agent_identity::signal_filename(&sender_id);
    // Wire format (ADR-120): `from|project|cwd|message`, or with `re:<id>`
    // before the message when --re was given. attend-chat writes through
    // the same `agent_identity::signal` functions.
    let project = agent_identity::signal::project_label(&cwd);
    let content = agent_identity::signal::format_signal(&from, &project, &cwd, reply_to.as_deref(), &message);

    let scope = if target_channel.is_some() {
        "channel"
    } else if target_dir.is_some() {
        "directed"
    } else {
        "#open"
    };

    for dest_dir in &dest_dirs {
        if let Err(e) = agent_identity::signal::write_signal_file(dest_dir, &filename, &content) {
            eprintln!("[attend] error writing signal to {}: {}", dest_dir.display(), e);
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
/// `cmd_send` preserves every existing `send` flag (`--channel`,
/// `--to`) without duplication.
#[cfg(feature = "sensor-peers")]
pub(crate) fn cmd_reply(
    target_dir: Option<String>,
    target_channel: Option<String>,
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
    // All other routing flags (--channel, --to) flow through
    // untouched.
    cmd_send(target_dir, target_channel, reply_to, message);
}

#[cfg(not(feature = "sensor-peers"))]
pub(crate) fn cmd_reply(
    _target_dir: Option<String>,
    _target_channel: Option<String>,
    _message: Vec<String>,
) {
    eprintln!("attend reply: sensor-peers feature is not compiled in this build");
    std::process::exit(1);
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

#[cfg(test)]
mod removed_flag_tests {
    use super::flag_like_refusal;

    fn v(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn flag_shaped_leading_tokens_are_refused() {
        let raw = v(&["attend", "send"]);
        for first in ["--broadcast", "--focus", "--focus=x", "--chanel", "--to-all"] {
            assert!(flag_like_refusal(&v(&[first, "hello"]), &raw).is_some(), "{first}");
        }
        assert!(flag_like_refusal(&v(&["--broadcast", "hi"]), &raw).unwrap().contains("removed"));
        assert!(flag_like_refusal(&v(&["--focus", "x"]), &raw).unwrap().contains("--channel"));
    }

    #[test]
    fn explicit_double_dash_escapes() {
        let raw = v(&["attend", "send", "--", "--broadcast", "is", "gone"]);
        assert!(flag_like_refusal(&v(&["--broadcast", "is", "gone"]), &raw).is_none());
    }

    #[test]
    fn a_later_double_dash_does_not_escape() {
        let raw = v(&["attend", "send", "--chanel", "deploy", "deploying", "now", "--", "ETA", "5m"]);
        let msg = v(&["--chanel", "deploy", "deploying", "now", "--", "ETA", "5m"]);
        assert!(flag_like_refusal(&msg, &raw).is_some());
    }

    #[test]
    fn ordinary_messages_pass() {
        let raw = v(&["attend", "send"]);
        assert!(flag_like_refusal(&v(&["hello", "--broadcast"]), &raw).is_none());
        assert!(flag_like_refusal(&v(&["--"]), &raw).is_none());
        assert!(flag_like_refusal(&v(&["--2x", "text"]), &raw).is_none());
        assert!(flag_like_refusal(&v(&["-5", "degrees"]), &raw).is_none());
        assert!(flag_like_refusal(&[], &raw).is_none());
    }
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
