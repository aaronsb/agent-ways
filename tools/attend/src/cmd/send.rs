//! `attend send` and `attend reply`: argv in, `verbs::send` called, its
//! result rendered. The verb bodies live in the library (ADR-501 item 2);
//! what stays here is what only a shell caller needs: the flag-shaped
//! message fence, the glob-expansion warning, and the CLI's wording.

#[cfg(feature = "sensor-peers")]
use crate::verbs::send::ReplyTarget;
use crate::verbs::send::{self, SendError, SendRequest};


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
    finish(send::send(&req), json);
}

/// `attend reply`: `send` threaded under the newest peer message.
#[cfg(feature = "sensor-peers")]
pub(crate) fn cmd_reply(
    target_dir: Option<String>,
    target_channel: Option<String>,
    message: Vec<String>,
    json: bool,
) {
    let re = match send::reply_target() {
        // Genuine "nothing to reply to" — the agent needs to do something
        // different (start a new topic), so this stays a hard error.
        ReplyTarget::NoInbound => fail(&SendError::NoInbound, json),
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
    cmd_send(target_dir, target_channel, re, message, json);
}

#[cfg(not(feature = "sensor-peers"))]
pub(crate) fn cmd_reply(
    _target_dir: Option<String>,
    _target_channel: Option<String>,
    _message: Vec<String>,
    _json: bool,
) {
    eprintln!("attend reply: sensor-peers feature is not compiled in this build");
    std::process::exit(1);
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

fn finish(result: Result<send::Sent, SendError>, json: bool) {
    let sent = match result {
        Ok(sent) => sent,
        Err(e) => fail(&e, json),
    };
    for f in &sent.failed {
        eprintln!("[attend] error writing signal to {}: {}", f.dir.display(), f.error);
    }
    eprintln!("[attend] signal written ({}, {} dirs): {}.signal", sent.scope, sent.dirs, sent.id);
    if json {
        crate::cmd::print_json(&sent);
    }
}

/// Print why the send wrote nothing, as the CLI always has, and exit 1.
fn fail(e: &SendError, json: bool) -> ! {
    if json {
        crate::cmd::print_json_error(e, &e.to_string());
    }
    match e {
        SendError::InvalidReId => {
            eprintln!("attend send: --re signal id must be non-empty and match [A-Za-z0-9_-]+");
        }
        SendError::EmptyMessage => {
            eprintln!("usage: attend send <message>");
            eprintln!("  (reaches every peer and Aaron — no routing flags needed)");
            eprintln!("  tip: wrap message in double quotes to avoid shell expansion");
        }
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
    }
    std::process::exit(1);
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
