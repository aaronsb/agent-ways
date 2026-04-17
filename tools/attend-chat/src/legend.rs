//! Agent legend strip — the `@Name @Name ...` row below the input.
//!
//! Two jobs:
//! - **Display**: render every known identity as its own styled chip
//!   so the user can see who's addressable and in what color.
//! - **Completion**: given a partial `@xxx` at the end of the input
//!   buffer, return the best-matching nickname so Tab can complete
//!   to a full `@Nickname `.
//!
//! The strip is *always* the completion UI — no popover, no arrow
//! navigation. Keeps the surface small and keeps the visible color
//! table doing double duty as the autocomplete cue.

use agent_identity::TermCaps;
use iocraft::prelude::*;

use crate::chip::{color_for, KnownIdentity};

/// Output of parsing the input buffer for a trailing `@partial`.
#[derive(Debug, PartialEq, Eq)]
pub struct Mention<'a> {
    /// The part of the input up to (and including) the `@` sigil.
    /// Completion replaces everything after the `@` without touching
    /// what came before.
    pub prefix: &'a str,
    /// The characters already typed after the `@`, or `""` if the
    /// user just typed `@` and hasn't typed anything else yet.
    pub partial: &'a str,
}

/// Parse a trailing mention at the end of `input`.
///
/// A mention is `@<word>` where `<word>` contains only ASCII alnum
/// characters (mirrors what our nickname pool allows). Returns `None`
/// if the last word doesn't start with `@`, or if there's a space
/// after the `@<word>` (caret isn't inside the mention any more).
///
/// Cursor position isn't consulted — we only offer completion when
/// the cursor is at the end of the buffer, which the caller enforces.
pub fn find_trailing_mention(input: &str) -> Option<Mention<'_>> {
    let at_pos = input.rfind('@')?;
    let after_at = &input[at_pos + 1..];
    // Everything after the `@` must be ASCII alnum (no spaces, no
    // punctuation) for us to treat it as a mention in progress.
    if !after_at.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    // The char *before* the `@` must be whitespace or start-of-input —
    // otherwise we're inside an email address or URL, not a mention.
    if at_pos > 0 {
        // `at_pos > 0` means `input[..at_pos]` is non-empty, so
        // `.chars().last()` always yields Some — the unwrap is safe
        // by construction.
        let prev = input[..at_pos]
            .chars()
            .last()
            .expect("at_pos > 0 implies non-empty prefix");
        if !prev.is_whitespace() {
            return None;
        }
    }
    Some(Mention {
        prefix: &input[..=at_pos],
        partial: after_at,
    })
}

/// Find the best completion for `partial` among `known` identities.
///
/// Case-insensitive prefix match. First hit in registry order wins —
/// because `known_identities` surfaces the most-recent-seen identity
/// first, that means active peers beat quiet ones. When several
/// identities share the same prefix (e.g. `partial="Ta"` against a
/// registry containing `Tamsin` and `Tal`), the caller gets whichever
/// appears first. No longer-is-better heuristic; recency is the
/// tiebreak.
pub fn best_completion<'a>(
    partial: &str,
    known: &'a [KnownIdentity],
) -> Option<&'a KnownIdentity> {
    let lc = partial.to_ascii_lowercase();
    known
        .iter()
        .find(|k| k.nickname.to_ascii_lowercase().starts_with(&lc))
}

/// If `msg` is addressed to an `@Nickname` as its first token,
/// return the nickname (without the `@`). Routing uses this to
/// decide whether to broadcast or direct-send to that claude's cwd
/// dir. Keeps the `@Nickname` in the outgoing body so receivers
/// still see the address.
pub fn parse_addressed(msg: &str) -> Option<&str> {
    let trimmed = msg.trim_start();
    let rest = trimmed.strip_prefix('@')?;
    // First alnum run is the nickname; anything after must be
    // whitespace or nothing. Matches the nickname pool's constraints
    // (ASCII letters only, short).
    let end = rest
        .char_indices()
        .find(|(_, c)| !c.is_ascii_alphanumeric())
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    let nick = &rest[..end];
    let after = &rest[end..];
    // Allow `@Nick` alone (entire message is the address, followed by
    // nothing) — lets users send a bare ping. Otherwise require a
    // space so `@Nick,` doesn't route.
    if after.is_empty() || after.starts_with(char::is_whitespace) {
        Some(nick)
    } else {
        None
    }
}

/// Render one legend row: `@Name @Name ...` across the width of the
/// input, each name in its own identity color. If the user is in the
/// middle of typing a mention (`current_partial` is `Some`), names
/// that prefix-match get an underline so the Tab target is visible.
///
/// Returns a `Vec` so the caller can splat it into an `element!`
/// children slot with `#(...)`.
pub fn legend_row(
    known: &[KnownIdentity],
    current_partial: Option<&str>,
) -> Vec<AnyElement<'static>> {
    let caps = TermCaps::detect();
    let lc_partial = current_partial.map(|p| p.to_ascii_lowercase());
    let chips: Vec<AnyElement<'static>> = known
        .iter()
        .map(|k| {
            let matches = lc_partial
                .as_ref()
                .map(|p| !p.is_empty() && k.nickname.to_ascii_lowercase().starts_with(p))
                .unwrap_or(false);
            let color = color_for(k.palette, caps);
            let weight = if k.style.bold || matches { Weight::Bold } else { Weight::Normal };
            let italic = k.style.italic;
            let decoration = if matches {
                TextDecoration::Underline
            } else {
                TextDecoration::None
            };
            let content = format!("@{} ", k.nickname);
            element! {
                Text(color, weight, italic, decoration, content, wrap: TextWrap::NoWrap)
            }
            .into_any()
        })
        .collect();
    vec![element! {
        View(
            flex_direction: FlexDirection::Row,
            padding_left: 1,
            padding_right: 1,
            height: 1u32,
            flex_shrink: 0.0,
            overflow: Overflow::Hidden,
        ) {
            #(chips)
        }
    }
    .into_any()]
}

/// Apply a completion to `input`: replace the trailing `@partial`
/// with `@<full> ` so the caret lands past a trailing space ready for
/// the message body.
///
/// Returns the new buffer and the new char-cursor position.
pub fn apply_completion(input: &str, mention: &Mention<'_>, full_name: &str) -> (String, usize) {
    // `prefix` already includes the `@`. Drop the `@` and append the
    // expansion so we control whether there's a space after it.
    let prefix_no_at = &mention.prefix[..mention.prefix.len() - 1];
    let mut out = String::with_capacity(input.len() + full_name.len() + 1);
    out.push_str(prefix_no_at);
    out.push('@');
    out.push_str(full_name);
    out.push(' ');
    let new_cursor = out.chars().count();
    (out, new_cursor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_identity::{PaletteEntry, Style};

    fn ident(nick: &str, cwd: &str, is_claude: bool) -> KnownIdentity {
        KnownIdentity {
            nickname: nick.to_string(),
            cwd: cwd.to_string(),
            is_claude,
            palette: PaletteEntry { rgb: (0, 0, 0), ansi16: 7, name: "t" },
            style: Style::default(),
        }
    }

    #[test]
    fn trailing_mention_after_space() {
        let m = find_trailing_mention("hi @Tam").unwrap();
        assert_eq!(m.prefix, "hi @");
        assert_eq!(m.partial, "Tam");
    }

    #[test]
    fn trailing_mention_at_start() {
        let m = find_trailing_mention("@Tam").unwrap();
        assert_eq!(m.prefix, "@");
        assert_eq!(m.partial, "Tam");
    }

    #[test]
    fn bare_at_sign_is_empty_partial() {
        let m = find_trailing_mention("hello @").unwrap();
        assert_eq!(m.partial, "");
    }

    #[test]
    fn no_mention_on_plain_text() {
        assert_eq!(find_trailing_mention("hello world"), None);
        assert_eq!(find_trailing_mention(""), None);
    }

    #[test]
    fn mention_rejected_if_followed_by_space() {
        // "@Tam " is complete — don't offer completion anymore.
        assert_eq!(find_trailing_mention("hi @Tam "), None);
    }

    #[test]
    fn at_in_email_is_not_a_mention() {
        // "hello aaron@kitty" — the char before `@` is `n`, not
        // whitespace, so it's an email/handle and we leave it alone.
        assert_eq!(find_trailing_mention("mail aaron@kitty"), None);
    }

    #[test]
    fn mention_with_punctuation_rejected() {
        // Nickname pool is ASCII alnum only; `@Tam,` is not a
        // mention-in-progress because `,` isn't alnum.
        assert_eq!(find_trailing_mention("@Tam,"), None);
    }

    #[test]
    fn completion_picks_case_insensitive_prefix() {
        let known = vec![ident("Tamsin", "/a", true), ident("Urban", "/b", true)];
        let m = best_completion("tam", &known).unwrap();
        assert_eq!(m.nickname, "Tamsin");
    }

    #[test]
    fn completion_returns_none_if_no_match() {
        let known = vec![ident("Tamsin", "/a", true)];
        assert!(best_completion("zzz", &known).is_none());
    }

    #[test]
    fn completion_on_empty_partial_picks_first() {
        let known = vec![ident("Tamsin", "/a", true), ident("Urban", "/b", true)];
        let m = best_completion("", &known).unwrap();
        assert_eq!(m.nickname, "Tamsin");
    }

    #[test]
    fn apply_completion_replaces_partial_and_adds_space() {
        let m = find_trailing_mention("hi @Tam").unwrap();
        let (out, cursor) = apply_completion("hi @Tam", &m, "Tamsin");
        assert_eq!(out, "hi @Tamsin ");
        assert_eq!(cursor, "hi @Tamsin ".chars().count());
    }

    #[test]
    fn apply_completion_at_start() {
        let m = find_trailing_mention("@Tam").unwrap();
        let (out, _) = apply_completion("@Tam", &m, "Tamsin");
        assert_eq!(out, "@Tamsin ");
    }

    #[test]
    fn apply_completion_on_bare_at() {
        let m = find_trailing_mention("hi @").unwrap();
        let (out, _) = apply_completion("hi @", &m, "Urban");
        assert_eq!(out, "hi @Urban ");
    }

    #[test]
    fn addressed_at_start_with_body() {
        assert_eq!(parse_addressed("@Tamsin hello"), Some("Tamsin"));
    }

    #[test]
    fn addressed_bare_ping() {
        // `@Nick` with nothing after is a valid addressed ping.
        assert_eq!(parse_addressed("@Urban"), Some("Urban"));
    }

    #[test]
    fn addressed_with_leading_whitespace() {
        assert_eq!(parse_addressed("   @Tamsin hello"), Some("Tamsin"));
    }

    #[test]
    fn not_addressed_if_not_at_start() {
        assert_eq!(parse_addressed("hi @Tamsin"), None);
    }

    #[test]
    fn not_addressed_if_trailing_punctuation() {
        // `@Tamsin,` — punctuation immediately after the name means
        // the user isn't really addressing, just referring.
        assert_eq!(parse_addressed("@Tamsin, hi"), None);
    }

    #[test]
    fn not_addressed_on_bare_at_sign() {
        assert_eq!(parse_addressed("@ hi"), None);
        assert_eq!(parse_addressed("@"), None);
    }
}
