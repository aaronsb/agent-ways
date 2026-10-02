//! The ADR-120 signal wire format: one line, `from|project|cwd|message`, or
//! `from|project|cwd|re:<signal-id>|message` for a threaded reply.
//!
//! Lives beside [`crate::signal_filename`], which mints the ids a reply
//! references, so every reader (the attend CLI, the attend-chat TUI) applies
//! one rule for what counts as threaded.

/// A parsed signal line. Legacy signals have no `reply_to`; threaded replies
/// carry the original signal's id there. Borrows from the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSignal<'a> {
    pub from: &'a str,
    pub project: &'a str,
    pub cwd: &'a str,
    pub reply_to: Option<&'a str>,
    pub message: &'a str,
}

/// Signal ids are filename stems in the form `<sender-id>-<timestamp>`,
/// which is always `[A-Za-z0-9_-]+`. This char class is the discriminator
/// fence that keeps legacy prose starting with "re:" from being misparsed as
/// threaded: `attend send "re: the thing we discussed|still open"` stays a
/// legacy message because `the thing we discussed` has a space.
pub fn is_valid_signal_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Parse a single-line signal. The discriminator is a `re:<id>|` prefix on
/// the field that follows `cwd`, where `<id>` passes [`is_valid_signal_id`].
/// A malformed or ambiguous `re:` prefix degrades to the legacy reading, so
/// real prose round-trips intact. Pipes in the message are kept.
pub fn parse_signal(content: &str) -> Option<ParsedSignal<'_>> {
    let parts: Vec<&str> = content.splitn(4, '|').collect();
    if parts.len() < 4 {
        return None;
    }
    let tail = parts[3];
    let (reply_to, message) = match tail.strip_prefix("re:").and_then(|rest| rest.split_once('|')) {
        Some((id, msg)) if is_valid_signal_id(id) => (Some(id), msg),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threaded_reply_carries_its_id() {
        let s = parse_signal("claude:a|p|/x|re:sender-123-0|reply body").unwrap();
        assert_eq!(s.reply_to, Some("sender-123-0"));
        assert_eq!(s.message, "reply body");
    }

    #[test]
    fn prose_starting_re_stays_legacy() {
        let s = parse_signal("claude:a|p|/x|re: prose|x").unwrap();
        assert_eq!(s.reply_to, None);
        assert_eq!(s.message, "re: prose|x");
    }

    #[test]
    fn legacy_body_keeps_its_pipes() {
        let s = parse_signal("claude:a|p|/x|a | b").unwrap();
        assert_eq!((s.reply_to, s.message), (None, "a | b"));
        assert!(parse_signal("claude:a|p|/x").is_none());
    }
}
