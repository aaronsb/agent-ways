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
use crate::groups::KnownGroup;

/// Which sigil-kind of mention we're matching. A single grammar
/// serves both `@agent` and `#group`; they differ only in the sigil
/// char and the completion pool.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Sigil {
    Agent,
    Group,
}

/// Output of parsing the input buffer for a trailing `<sigil>partial`.
#[derive(Debug, PartialEq, Eq)]
pub struct Mention<'a> {
    /// The part of the input up to (and including) the sigil.
    /// Completion replaces everything after it without touching
    /// what came before.
    pub prefix: &'a str,
    /// The characters already typed after the sigil, or `""` if the
    /// user just typed the sigil and hasn't typed anything else yet.
    pub partial: &'a str,
    /// Which sigil kicked this mention off.
    pub sigil: Sigil,
}

/// Parse a trailing mention at the end of `input`.
///
/// A mention is `@<word>` or `#<word>` where `<word>` contains only
/// ASCII alnum characters plus `-` (mirrors what attend's group-name
/// validator allows; nicknames are a strict subset). Returns `None`
/// if the last word doesn't start with a mention sigil, or if there's
/// a space after it (caret isn't inside the mention any more).
///
/// When both `@` and `#` appear trailing, the rightmost sigil wins —
/// that's the one the caret is adjacent to.
///
/// Cursor position isn't consulted — we only offer completion when
/// the cursor is at the end of the buffer, which the caller enforces.
pub fn find_trailing_mention(input: &str) -> Option<Mention<'_>> {
    let (pos, sigil) = [
        (input.rfind('@'), Sigil::Agent),
        (input.rfind('#'), Sigil::Group),
    ]
    .into_iter()
    .filter_map(|(p, s)| p.map(|p| (p, s)))
    .max_by_key(|(p, _)| *p)?;
    let after = &input[pos + 1..];
    // Everything after the sigil must be ASCII alnum or `-` for
    // us to treat it as a mention in progress. `-` is allowed
    // because attend's group-name validator permits it
    // (`my-group` is a real thing).
    if !after.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    // The char *before* the sigil must be whitespace or
    // start-of-input — otherwise we're inside an email address,
    // a URL, or the middle of some other token, not a mention.
    if pos > 0 {
        // `pos > 0` means `input[..pos]` is non-empty, so
        // `.chars().last()` always yields Some — the expect is
        // safe by construction.
        let prev = input[..pos]
            .chars()
            .last()
            .expect("pos > 0 implies non-empty prefix");
        if !prev.is_whitespace() {
            return None;
        }
    }
    Some(Mention {
        prefix: &input[..=pos],
        partial: after,
        sigil,
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

/// Every claude nickname that prefix-matches `partial`, in registry
/// order. Used by Tab-cycle: the first Tab takes [0], the second
/// takes [1], etc., wrapping at the end. Same case-insensitive
/// match rule as [`best_completion`] — recency-biased ordering
/// flows from `known_identities` putting active peers first.
pub fn all_completions(
    partial: &str,
    known: &[KnownIdentity],
) -> Vec<String> {
    let lc = partial.to_ascii_lowercase();
    known
        .iter()
        .filter(|k| k.is_claude && k.nickname.to_ascii_lowercase().starts_with(&lc))
        .map(|k| k.nickname.clone())
        .collect()
}

/// Group-side counterpart to [`all_completions`]. Same prefix rule
/// applied to the channel pool (`KnownGroup`).
pub fn all_group_completions(
    partial: &str,
    known: &[KnownGroup],
) -> Vec<String> {
    let lc = partial.to_ascii_lowercase();
    known
        .iter()
        .filter(|k| k.group.name.to_ascii_lowercase().starts_with(&lc))
        .map(|k| k.group.name.clone())
        .collect()
}

/// Find the best completion for `partial` among `known` groups.
///
/// Parallel to `best_completion` for agents — shared grammar, two
/// distinct registries. The `-` is tolerated in group names so
/// `#my-g` → `my-group` matches.
pub fn best_group_completion<'a>(
    partial: &str,
    known: &'a [KnownGroup],
) -> Option<&'a KnownGroup> {
    let lc = partial.to_ascii_lowercase();
    known
        .iter()
        .find(|k| k.group.name.to_ascii_lowercase().starts_with(&lc))
}

/// Parsed routing hint extracted from a message's first token.
/// Either the message addresses a specific agent (`@Nick body`) or
/// a focus group (`#group body`), or neither. The routing layer in
/// `app.rs` dispatches on this.
#[derive(Debug, PartialEq, Eq)]
pub enum Addressed<'a> {
    Agent(&'a str),
    Group(&'a str),
}

/// If `msg` starts with an addressing sigil + name, return the kind and
/// name (without the sigil). Both `@Name` and `#Name` work. Keeps the
/// original token in the outgoing body so receivers still see the
/// address they were matched on.
pub fn parse_addressed(msg: &str) -> Option<Addressed<'_>> {
    let trimmed = msg.trim_start();
    let mut chars = trimmed.chars();
    let sigil = chars.next()?;
    let kind = match sigil {
        '@' => Sigil::Agent,
        '#' => Sigil::Group,
        _ => return None,
    };
    let rest = &trimmed[sigil.len_utf8()..];
    // First alnum + `-` run is the name; anything after must be
    // whitespace or nothing. Groups allow `-`; nicknames don't use
    // it today but accepting it in both keeps the grammar uniform
    // (and worst case the resolver just returns None).
    let end = rest
        .char_indices()
        .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '-'))
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    let name = &rest[..end];
    let after = &rest[end..];
    // Allow sigil-plus-name alone (a bare ping), otherwise require a
    // space so `@Nick,` or `#group.` don't route.
    if after.is_empty() || after.starts_with(char::is_whitespace) {
        Some(match kind {
            Sigil::Agent => Addressed::Agent(name),
            Sigil::Group => Addressed::Group(name),
        })
    } else {
        None
    }
}

/// Render the **agent** legend row: `@Name @Name ...` across the
/// width of the input, each name in its own identity color. If the
/// user is in the middle of typing an agent mention (`@partial`),
/// names that prefix-match get an underline so the Tab target is
/// visible.
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

/// Render the **group** legend row: `<glyph> #name` per group, each
/// in its own hashed color. Sits at the top of the TUI so the user
/// can see at a glance what focus groups exist and in what color
/// they'll appear on agent chips.
///
/// If the user is mid-`#partial`, prefix-matching group names get
/// an underline so Tab-target is obvious. Glyphs stay single-width;
/// the name-color matches the glyph-color (same palette entry) so
/// visual recognition carries even if the glyph degrades on a
/// limited terminal.
pub fn group_legend_row(
    known: &[KnownGroup],
    current_partial: Option<&str>,
) -> Vec<AnyElement<'static>> {
    let caps = TermCaps::detect();
    let lc_partial = current_partial.map(|p| p.to_ascii_lowercase());
    let chips: Vec<AnyElement<'static>> = known
        .iter()
        .map(|k| {
            let matches = lc_partial
                .as_ref()
                .map(|p| !p.is_empty() && k.group.name.to_ascii_lowercase().starts_with(p))
                .unwrap_or(false);
            let color = color_for(k.group.palette, caps);
            // Base channel (`#open`) always renders bold as the
            // visual affordance for "this is the commons" —
            // ADR-124 §4. Partial-match underline still overlays.
            let weight = if k.is_base || k.group.style.bold || matches {
                Weight::Bold
            } else {
                Weight::Normal
            };
            let italic = k.group.style.italic;
            let decoration = if matches { TextDecoration::Underline } else { TextDecoration::None };
            let content = format!("{} #{} ", k.group.glyph, k.group.name);
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
        assert_eq!(m.sigil, Sigil::Agent);
    }

    #[test]
    fn trailing_mention_at_start() {
        let m = find_trailing_mention("@Tam").unwrap();
        assert_eq!(m.prefix, "@");
        assert_eq!(m.partial, "Tam");
        assert_eq!(m.sigil, Sigil::Agent);
    }

    #[test]
    fn bare_at_sign_is_empty_partial() {
        let m = find_trailing_mention("hello @").unwrap();
        assert_eq!(m.partial, "");
        assert_eq!(m.sigil, Sigil::Agent);
    }

    #[test]
    fn trailing_hash_is_group_mention() {
        let m = find_trailing_mention("hi #dep").unwrap();
        assert_eq!(m.partial, "dep");
        assert_eq!(m.sigil, Sigil::Group);
    }

    #[test]
    fn group_with_dash_accepted() {
        // attend's validator permits `-` in group names; the
        // mention grammar must match so `#my-g` is a valid
        // prefix of `my-group`.
        let m = find_trailing_mention("#my-g").unwrap();
        assert_eq!(m.partial, "my-g");
    }

    #[test]
    fn rightmost_sigil_wins() {
        // If both sigils appear and are both trailing-legal,
        // whichever is closer to the caret is the mention.
        let m = find_trailing_mention("#dep @Tam").unwrap();
        assert_eq!(m.sigil, Sigil::Agent);
        let m = find_trailing_mention("@Tam #dep").unwrap();
        assert_eq!(m.sigil, Sigil::Group);
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
    fn all_completions_returns_every_prefix_match_in_order() {
        // Tab-cycle relies on this: candidates [0], [1], … define
        // the cycle order, so the slice must preserve registry
        // order rather than re-sorting.
        let known = vec![
            ident("Tamsin-alpha", "/a", true),
            ident("Tamsin-beta", "/b", true),
            ident("Urban-alpha", "/c", true),
        ];
        let cands = all_completions("tam", &known);
        assert_eq!(cands, vec!["Tamsin-alpha", "Tamsin-beta"]);
    }

    #[test]
    fn all_completions_skips_humans() {
        // Only claudes route to a cwd inbox, so Tab-cycle must not
        // surface human entries even though they appear in the
        // legend for visibility.
        let known = vec![
            ident("Tamsin-alpha", "/a", true),
            ident("aaron", "/h", false),
        ];
        let cands = all_completions("", &known);
        assert_eq!(cands, vec!["Tamsin-alpha"]);
    }

    #[test]
    fn all_completions_empty_when_no_match() {
        let known = vec![ident("Tamsin", "/a", true)];
        let cands = all_completions("zzz", &known);
        assert!(cands.is_empty());
    }

    #[test]
    fn all_group_completions_returns_every_prefix_match() {
        use agent_identity::Group;
        use crate::groups::{GroupMembership, KnownGroup};
        let mk = |name: &str| KnownGroup {
            group: Group::for_name(name, TermCaps::Rich),
            membership: GroupMembership::default(),
            is_base: false,
        };
        let known = vec![mk("deploy"), mk("deploy-prod"), mk("infra")];
        let cands = all_group_completions("dep", &known);
        assert_eq!(cands, vec!["deploy", "deploy-prod"]);
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
        assert_eq!(parse_addressed("@Tamsin hello"), Some(Addressed::Agent("Tamsin")));
    }

    #[test]
    fn addressed_bare_ping() {
        // `@Nick` with nothing after is a valid addressed ping.
        assert_eq!(parse_addressed("@Urban"), Some(Addressed::Agent("Urban")));
    }

    #[test]
    fn addressed_with_leading_whitespace() {
        assert_eq!(parse_addressed("   @Tamsin hello"), Some(Addressed::Agent("Tamsin")));
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

    #[test]
    fn addressed_group_with_body() {
        assert_eq!(parse_addressed("#deploy rollout at 3pm"), Some(Addressed::Group("deploy")));
    }

    #[test]
    fn addressed_group_with_dash() {
        assert_eq!(parse_addressed("#my-team ping"), Some(Addressed::Group("my-team")));
    }

    #[test]
    fn addressed_group_bare_ping() {
        assert_eq!(parse_addressed("#infra"), Some(Addressed::Group("infra")));
    }

    #[test]
    fn addressed_group_rejects_punctuation() {
        assert_eq!(parse_addressed("#deploy, now"), None);
    }
}
