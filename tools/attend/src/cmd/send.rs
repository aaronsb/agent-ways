//! `attend send` and `attend reply`: argv in, `verbs::send` called, its
//! result rendered. The verb bodies live in the library (ADR-501 item 2);
//! what stays here is what only a shell caller needs: the flag-shaped
//! message fence, the glob-expansion warning, and the CLI's wording.

#[cfg(feature = "sensor-peers")]
use crate::verbs::send::Threading;
use crate::verbs::send::{self, SendError, SendRequest};


/// The flags `attend send` takes, as a caller might write them after the
/// message by mistake.
pub(crate) const SEND_FLAGS: &[&str] = &["--to", "--channel", "--re", "--json"];
/// The flags `attend reply` takes.
pub(crate) const REPLY_FLAGS: &[&str] = &["--to", "--channel", "--json"];

/// The message is a trailing, hyphen-tolerant argument, so clap hands any
/// unknown flag (a removed `--broadcast` or `--focus`, or a typo such as
/// `--chanel`) over as message text, and the send would go out with the flag
/// in its body. So does any of the verb's own flags (`own_flags`) written
/// after the message: `attend send hi --json` would send "hi --json". A
/// leading token shaped like a long flag, or one of the verb's own flags
/// anywhere in the message, is refused unless a `--` sits immediately before
/// the message in the raw arguments (the escape hatch for a message that
/// really contains one); a later `--` in the text does not count.
fn flag_like_refusal(own_flags: &[&str], message: &[String], raw_args: &[String]) -> Option<String> {
    let first = message.first()?.as_str();
    // The message is the tail of the raw arguments, so the escape is the
    // token just before that tail. Matching `--` followed by the first word
    // anywhere would let `send ok --json -- ok` through.
    let escape = raw_args.len().checked_sub(message.len() + 1).map(|i| raw_args[i].as_str());
    if escape == Some("--") {
        return None;
    }
    let name_of = |token: &str| token.split('=').next().unwrap_or(token).to_string();
    let name = name_of(first);
    let flag_like = name.len() > 2
        && name.starts_with("--")
        && name[2..].starts_with(|c: char| c.is_ascii_lowercase())
        && name[2..].chars().all(|c| c.is_ascii_lowercase() || c == '-');
    if flag_like {
        let hint = match name.as_str() {
            // transition: removed by #717 (ADR-506)
            "--broadcast" => " It was removed: a send with no routing flag already reaches everyone.",
            // transition: removed by #717 (ADR-506)
            "--focus" => " It was removed: use --channel.",
            _ => "",
        };
        return Some(format!(
            "`{name}` is not a flag this command accepts.{hint} To send it as text, put `--` before the message."
        ));
    }
    let late = message[1..].iter().map(|t| name_of(t)).find(|n| own_flags.contains(&n.as_str()))?;
    Some(format!(
        "`{late}` comes after the message, so it would be sent as text. Put flags before the message; to send it as text, put `--` before the message."
    ))
}

/// Exit 2 when `message` carries a flag-shaped token and no `--` escape. A
/// refusal is JSON when `--json` was given, before the message or caught in it.
pub(crate) fn reject_flag_like(verb: &str, own_flags: &[&str], message: &[String], json: bool) {
    let raw: Vec<String> = std::env::args().collect();
    if let Some(why) = flag_like_refusal(own_flags, message, &raw) {
        let json = json || message.iter().any(|t| t == "--json");
        crate::cmd::refuse_usage(&format!("attend {verb}: {why}"), &why, json);
    }
}


/// `attend send`: the verb is `verbs::send::send`; this renders it.
pub(crate) fn cmd_send(
    target_dir: Option<String>,
    target_channel: Option<String>,
    reply_to: Option<String>,
    message_parts: Vec<String>,
    json: bool,
) {
    let req = SendRequest { message: message_parts.join(" "), to: target_dir, channel: target_channel, re: reply_to };
    warn_on_expansion(&req, &message_parts);
    match send::send(&req) {
        Ok(sent) => {
            report(&sent);
            if json {
                crate::cmd::print_json(&sent);
            }
        }
        Err(e) => fail(&e, json, SEND_USAGE),
    }
}

/// `attend reply`: the verb is `verbs::send::reply`; this renders it.
#[cfg(feature = "sensor-peers")]
pub(crate) fn cmd_reply(
    target_dir: Option<String>,
    target_channel: Option<String>,
    message: Vec<String>,
    json: bool,
) {
    let req = SendRequest { message: message.join(" "), to: target_dir, channel: target_channel, re: None };
    match send::reply(&req) {
        Ok(replied) => {
            if replied.threading == Threading::Unthreaded {
                eprintln!(
                    "[attend] note: last inbound id is not threadable; sending unthreaded (message still delivered)."
                );
            }
            warn_on_expansion(&req, &message);
            report(&replied.sent);
            if json {
                crate::cmd::print_json(&replied);
            }
        }
        // Nothing to reply to: the caller needs to start a new topic, and
        // the glob warning about a message it will not send is noise.
        Err(e @ SendError::NoInbound) => fail(&e, json, REPLY_USAGE),
        Err(e) => {
            warn_on_expansion(&req, &message);
            fail(&e, json, REPLY_USAGE)
        }
    }
}

#[cfg(not(feature = "sensor-peers"))]
pub(crate) fn cmd_reply(
    _target_dir: Option<String>,
    _target_channel: Option<String>,
    _message: Vec<String>,
    json: bool,
) {
    let why = "sensor-peers feature is not compiled in this build";
    crate::cmd::refuse_usage(&format!("attend reply: {why}"), why, json);
}

/// Fence: detect probable shell glob expansion. If any message part is an
/// existing file path, the shell likely expanded a metachar (e.g. zsh
/// expanded "hello?" into filenames). Only a send that passed the id and
/// empty-message checks gets here, as before the verb moved.
fn warn_on_expansion(req: &SendRequest, message_parts: &[String]) {
    if req.re.as_deref().is_some_and(|id| !agent_identity::is_valid_signal_id(id)) || req.message.is_empty() {
        return;
    }
    let suspect_expansion = message_parts.iter().any(|part| std::path::Path::new(part).exists() && !part.contains(' '));
    if suspect_expansion {
        eprintln!("[attend] warning: message contains existing file paths — shell may have expanded metacharacters");
        eprintln!("[attend] did you mean: attend send \"{}\"", req.message);
        eprintln!("[attend] sending anyway, but wrap in quotes next time");
    }
}

/// The stderr lines for a message written: each tray that failed, then the
/// signal's id, one line per part for a message sent in parts.
fn report(sent: &send::Sent) {
    for f in &sent.failed {
        eprintln!("[attend] error writing signal to {}: {}", f.dir.display(), f.error);
    }
    let n = sent.parts.len();
    if n == 1 {
        eprintln!("[attend] signal written ({}, {} dirs): {}.signal", sent.scope, sent.dirs, sent.id);
        return;
    }
    for (i, id) in sent.parts.iter().enumerate() {
        eprintln!("[attend] signal written ({}, part {}/{n}): {id}.signal", sent.scope, i + 1);
    }
}

const SEND_USAGE: &str = "usage: attend send <message>\n  (reaches every peer and Aaron — no routing flags needed)\n  tip: wrap message in double quotes to avoid shell expansion";

#[cfg(feature = "sensor-peers")]
const REPLY_USAGE: &str = "usage: attend reply <message>\n  (answers the newest peer message delivered to this session, where it was said)\n  tip: wrap message in double quotes to avoid shell expansion";

/// Print why the send wrote nothing, as the CLI always has, and exit 1. An
/// empty message is a malformed command line: exit 2 with the verb's
/// `usage`, `kind: usage` under `--json` (ADR-185, note of 2026-10-10).
fn fail(e: &SendError, json: bool, usage: &str) -> ! {
    if let SendError::EmptyMessage = e {
        crate::cmd::refuse_usage(usage, &e.to_string(), json);
    }
    if json {
        crate::cmd::print_json_error(e, &e.to_string());
    }
    match e {
        SendError::InvalidReId => {
            eprintln!("attend send: --re signal id must be non-empty and match [A-Za-z0-9_-]+");
        }
        SendError::EmptyMessage => unreachable!("an empty message is refused as usage above"),
        SendError::NoPeerAt { path, peers, suggestion } => {
            eprintln!("error: no active peer at {}", path);
            if peers.is_empty() {
                eprintln!("\nno active peer sessions found");
            } else {
                eprintln!("\nactive peers:");
                for p in peers {
                    eprintln!("  {} ({})", p.cwd, p.project);
                }
                if let Some(s) = suggestion {
                    eprintln!("\ndid you mean: {}?", s);
                }
            }
        }
        SendError::NoChannel { name, channels } | SendError::NoLivePeers { name, channels, .. } => {
            match e {
                SendError::NoChannel { .. } => eprintln!("error: no channel named '{}'", name),
                SendError::NoLivePeers { only_self: true, .. } => {
                    eprintln!("error: no live peers in channel '{}' (you are the only listener)", name)
                }
                _ => eprintln!("error: no live peers in channel '{}'", name),
            }
            if channels.is_empty() {
                eprintln!("\nno active channels");
            } else {
                eprintln!("\nactive channels (yaml count, live peers may be fewer):");
                for c in channels {
                    let pin = if c.pinned { " (pinned)" } else { "" };
                    let suffix = if c.members == 1 { "" } else { "s" };
                    eprintln!("  {} — {} member{}{}", c.name, c.members, suffix, pin);
                }
            }
            eprintln!("\ndrop --channel to broadcast (reaches every peer):");
            eprintln!("  attend send <message>");
        }
        SendError::NoInbound => {
            eprintln!("attend reply: no prior inbound signal to thread against.");
            eprintln!("  (reply is for responding to a peer message your sensor surfaced.)");
            eprintln!("  if you are starting a new topic, use `attend send` instead.");
        }
        SendError::ParentChannelLeft { .. }
        | SendError::ParentChannelGone { .. }
        | SendError::ParentRoomUnknown => eprintln!("attend reply: {e}"),
        SendError::OwnProject { .. } | SendError::Incomplete { .. } => eprintln!("error: {e}"),
    }
    std::process::exit(1);
}

#[cfg(test)]
mod removed_flag_tests {
    use super::{flag_like_refusal as refusal, REPLY_FLAGS, SEND_FLAGS};

    fn v(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    fn flag_like_refusal(message: &[String], raw: &[String]) -> Option<String> {
        refusal(SEND_FLAGS, message, raw)
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
    fn the_verbs_own_flags_after_the_message_are_refused() {
        let raw = v(&["attend", "send"]);
        for late in ["--json", "--to", "--to=/x", "--channel", "--re"] {
            let why = flag_like_refusal(&v(&["hello", late, "x"]), &raw).unwrap_or_else(|| panic!("{late}"));
            assert!(why.contains("after the message"), "{why}");
        }
        assert!(refusal(REPLY_FLAGS, &v(&["thanks", "--json"]), &raw).is_some());
        assert!(refusal(REPLY_FLAGS, &v(&["see", "--re"]), &raw).is_none(), "reply takes no --re");
    }

    #[test]
    fn explicit_double_dash_escapes() {
        let raw = v(&["attend", "send", "--", "--broadcast", "is", "gone"]);
        assert!(flag_like_refusal(&v(&["--broadcast", "is", "gone"]), &raw).is_none());
        let raw = v(&["attend", "send", "--", "pass", "--json", "to", "it"]);
        assert!(flag_like_refusal(&v(&["pass", "--json", "to", "it"]), &raw).is_none());
    }

    #[test]
    fn a_later_double_dash_does_not_escape() {
        let raw = v(&["attend", "send", "--chanel", "deploy", "deploying", "now", "--", "ETA", "5m"]);
        let msg = v(&["--chanel", "deploy", "deploying", "now", "--", "ETA", "5m"]);
        assert!(flag_like_refusal(&msg, &raw).is_some());
        let raw = v(&["attend", "send", "hi", "--", "--json"]);
        assert!(flag_like_refusal(&v(&["hi", "--", "--json"]), &raw).is_some());
        // A `--` before a later copy of the first word is not the escape.
        let raw = v(&["attend", "send", "ok", "--json", "--", "ok"]);
        assert!(flag_like_refusal(&v(&["ok", "--json", "--", "ok"]), &raw).is_some());
        assert!(flag_like_refusal(&v(&["ok", "--json", "ok"]), &raw).is_some());
        // The escape after the verb's own flags still counts.
        let raw = v(&["attend", "send", "--json", "--", "--broadcast", "hi"]);
        assert!(flag_like_refusal(&v(&["--broadcast", "hi"]), &raw).is_none());
    }

    #[test]
    fn ordinary_messages_pass() {
        let raw = v(&["attend", "send"]);
        assert!(flag_like_refusal(&v(&["hello", "--broadcast"]), &raw).is_none());
        assert!(flag_like_refusal(&v(&["--"]), &raw).is_none());
        assert!(flag_like_refusal(&v(&["--2x", "text"]), &raw).is_none());
        assert!(flag_like_refusal(&v(&["-5", "degrees"]), &raw).is_none());
        assert!(flag_like_refusal(&v(&["pass", "--jsonish", "--total"]), &raw).is_none());
        assert!(flag_like_refusal(&[], &raw).is_none());
    }
}
