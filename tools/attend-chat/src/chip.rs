//! Sender chip: the small bordered box to the left of each message
//! that identifies who sent it.
//!
//! Pure display logic — takes the wire `from`/`project`/`cwd` triple,
//! derives a stable nickname + color + style via `agent_identity`, and
//! returns a `ChipInfo` the renderer can feed into iocraft components.
//!
//! Lives in its own module because PR 2 (agent legend) and later PRs
//! (@ autocomplete, focus-group sidebar) will layer on top of the same
//! identity derivation — keeping the chip logic separate from the
//! iocraft component tree means one reviewable seam per PR rather
//! than a growing `app.rs`.

use agent_identity::{Identity, PaletteEntry, Style, TermCaps};
use iocraft::prelude::Color;

use crate::signal::Signal;

/// Width of the chip box in columns. Interior width for text =
/// `CHIP_WIDTH - 2 (border) - 2 (padding)`.
pub const CHIP_WIDTH: u32 = 20;

/// Display-layer facts for one signal's sender chip.
pub struct ChipInfo {
    /// First line — bold, colored with the identity palette.
    pub primary: String,
    /// Second line — dim, cwd basename or the broadcast fallback.
    pub secondary: String,
    pub palette: PaletteEntry,
    pub style: Style,
    /// Session UUID for claude senders, `None` for humans or
    /// unknown prefixes. The chip's group-glyph decoration cross-
    /// references this against `_groups.yaml` membership. Humans
    /// will get their own derivable key in PR 4 once the CRUD
    /// path needs to address them.
    pub session_id: Option<String>,
}

/// Derive the chip from the wire `from`/`project`/`cwd` triple.
///
/// Claudes get a stable nickname keyed on their full cwd path (see
/// `agent_identity::Identity::for_cwd`). Humans keep their username
/// but still pick up a color + style from the identity table so the
/// avatar is visually consistent everywhere the same user shows up.
///
/// This function never touches the wire format — identity is pure
/// receiver-side rendering. If the signal's `from` doesn't match a
/// known prefix, we fall through to showing the raw value.
pub fn chip_for(from: &str, project: &str, cwd: &str, caps: TermCaps) -> ChipInfo {
    let interior = (CHIP_WIDTH as usize).saturating_sub(4);
    let scope_src = if cwd.is_empty() { project } else { cwd };
    let scope_segment = scope_src.rsplit('/').next().unwrap_or(scope_src);
    let scope = if scope_segment.is_empty() {
        "broadcast".to_string()
    } else {
        scope_segment.to_string()
    };

    if let Some(uuid) = from.strip_prefix("claude:") {
        // For claude senders the cwd is the stable identity key. We
        // don't hash the session UUID — two sequential claudes in the
        // same dir should wear the same name, matching the user's
        // mental model of "the agent that lives here". The UUID
        // flows to ChipInfo.session_id so the chip can look up
        // group membership against _groups.yaml.
        let id = Identity::for_cwd(cwd, caps);
        ChipInfo {
            primary: truncate(id.nickname, interior),
            secondary: truncate(&scope, interior),
            palette: id.palette,
            style: id.style,
            session_id: Some(uuid.to_string()),
        }
    } else if let Some(rest) = from.strip_prefix("external:") {
        // Strip the terminal suffix ("aaron@kitty" → "aaron") so the
        // chip reads as the human, not their terminal emulator.
        let username = rest.split('@').next().unwrap_or(rest);
        let id = Identity::for_user(username, &scope, caps);
        ChipInfo {
            primary: truncate(username, interior),
            secondary: truncate(&scope, interior),
            palette: id.palette,
            style: id.style,
            session_id: None,
        }
    } else {
        // Unknown sender kind — key the color off the raw `from` so
        // different unknowns at least colorise differently instead of
        // all landing on a default.
        let id = Identity::for_user(from, &scope, caps);
        ChipInfo {
            primary: truncate(from, interior),
            secondary: truncate(&scope, interior),
            palette: id.palette,
            style: id.style,
            session_id: None,
        }
    }
}

/// Map a palette entry to an iocraft `Color`. Rich terminals get
/// RGB; basic ones get a named ANSI bright to avoid truecolor on
/// terminals that would approximate it poorly.
pub fn color_for(p: PaletteEntry, caps: TermCaps) -> Color {
    match caps {
        TermCaps::Rich => Color::Rgb { r: p.rgb.0, g: p.rgb.1, b: p.rgb.2 },
        TermCaps::Basic | TermCaps::Mono => Color::AnsiValue(p.ansi16),
    }
}

/// One identity known to the TUI, extracted from buffered signals.
/// Carries enough to render the legend and resolve an `@name` to its
/// routing destination.
#[derive(Clone, Debug)]
pub struct KnownIdentity {
    /// Nickname derived from the sender's cwd (claude) or username
    /// (external). This is what `@foo` in the input matches against.
    pub nickname: String,
    /// Full cwd of the sender. For claudes this is the routing key:
    /// a directed `@Nickname` message writes to `signals_base/<encoded-cwd>/`.
    /// For humans the cwd is their working directory when they sent —
    /// we still carry it so the legend can show `@aaron (Projects)`.
    pub cwd: String,
    /// Whether this identity is a claude (routable) or an external
    /// human (not routable to a cwd inbox).
    pub is_claude: bool,
    pub palette: PaletteEntry,
    pub style: Style,
}

/// Build the registry of known identities from a slice of buffered
/// signals. Pure function — no IO, no state. Called every render;
/// the buffer cap keeps the work bounded.
///
/// Ordering: most-recently-seen first, then stable by nickname. That
/// way the legend strip tends to put active peers at the front
/// without flickering if two peers have the same last-seen tick.
pub fn known_identities(signals: &[Signal], caps: TermCaps) -> Vec<KnownIdentity> {
    // Walk from newest to oldest so the first time we see a cwd we
    // also capture its most-recent-seen position. A HashSet of cwd
    // strings keeps dedup O(n) without depending on a hasher.
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<KnownIdentity> = Vec::new();
    for sig in signals.iter().rev() {
        let (primary_label, is_claude, id) = if sig.from.strip_prefix("claude:").is_some() {
            let id = Identity::for_cwd(&sig.cwd, caps);
            (id.nickname.to_string(), true, id)
        } else if let Some(rest) = sig.from.strip_prefix("external:") {
            let username = rest.split('@').next().unwrap_or(rest).to_string();
            let scope = agent_identity::cwd_basename(&sig.cwd);
            let id = Identity::for_user(&username, &scope, caps);
            (username, false, id)
        } else {
            continue; // unknown prefix — don't pollute the legend
        };

        // Dedupe on (nickname, is_claude, cwd) — same claude can appear
        // many times in the buffer; same username across different
        // cwds should still surface once per cwd so legend can show
        // them distinctly.
        let key = format!("{}\x1f{}\x1f{}", primary_label, is_claude as u8, sig.cwd);
        if seen.insert(key) {
            out.push(KnownIdentity {
                nickname: primary_label,
                cwd: sig.cwd.clone(),
                is_claude,
                palette: id.palette,
                style: id.style,
            });
        }
    }
    out
}

/// Resolve an `@Nickname` token to a routable cwd.
///
/// Returns `Some(cwd)` only for claude identities — humans don't have
/// a signal inbox we can post into. Case-insensitive match to be
/// gentle on typos (the nickname pool is case-distinct, but a user
/// typing `@tamsin` should still hit `Tamsin`).
pub fn resolve_nickname(
    name: &str,
    known: &[KnownIdentity],
) -> Option<String> {
    let lc = name.to_ascii_lowercase();
    known
        .iter()
        .find(|k| k.is_claude && k.nickname.to_ascii_lowercase() == lc)
        .map(|k| k.cwd.clone())
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else if max <= 1 {
        s.chars().take(max).collect()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_sender_uses_derived_nickname() {
        // A claude in /home/aaron/.claude gets a nickname from the
        // agent-identity pool, scope comes from cwd basename.
        let chip = chip_for(
            "claude:e74a4a4b-7e3b-49bc-8404-216162e54ba8",
            "claude",
            "/home/aaron/.claude",
            TermCaps::Rich,
        );
        let expected = Identity::for_cwd("/home/aaron/.claude", TermCaps::Rich);
        assert_eq!(chip.primary, expected.nickname);
        assert_eq!(chip.secondary, ".claude");
    }

    #[test]
    fn claude_nickname_stable_across_session_ids() {
        // Two sequential claudes in the same cwd should show the same
        // name — the session UUID changes but identity is keyed on
        // cwd, not session.
        let a = chip_for("claude:aaaa-1", "p", "/home/x", TermCaps::Rich);
        let b = chip_for("claude:bbbb-2", "p", "/home/x", TermCaps::Rich);
        assert_eq!(a.primary, b.primary);
    }

    #[test]
    fn scope_prefers_cwd_basename_over_project() {
        let chip = chip_for("external:aaron", "ignored", "/home/aaron/temp", TermCaps::Rich);
        assert_eq!(chip.secondary, "temp");
    }

    #[test]
    fn external_strips_terminal_suffix() {
        let chip = chip_for("external:aaron@kitty", "proj", "/home/aaron", TermCaps::Rich);
        assert_eq!(chip.primary, "aaron");
    }

    #[test]
    fn scope_truncates_long_segment() {
        // Interior is 16 chars for CHIP_WIDTH=20 (2 borders + 2 padding).
        let chip = chip_for(
            "external:aaron",
            "x",
            "/tmp/some-very-long-directory-name",
            TermCaps::Rich,
        );
        assert!(chip.secondary.chars().count() <= 16);
        assert!(chip.secondary.ends_with('…'));
    }

    #[test]
    fn unknown_sender_still_colored() {
        // Something that isn't claude: or external: — we don't crash,
        // we show the raw value and pick a color off it.
        let a = chip_for("mystery:abc", "", "/tmp", TermCaps::Rich);
        let b = chip_for("mystery:xyz", "", "/tmp", TermCaps::Rich);
        // Different senders → different colors most of the time. We
        // don't assert inequality (palette is finite), just that the
        // code path doesn't panic and produces valid output.
        assert_eq!(a.primary, "mystery:abc");
        assert_eq!(b.primary, "mystery:xyz");
    }

    #[test]
    fn color_for_rich_is_rgb() {
        let p = PaletteEntry { rgb: (10, 20, 30), ansi16: 3, name: "test" };
        match color_for(p, TermCaps::Rich) {
            Color::Rgb { r, g, b } => assert_eq!((r, g, b), (10, 20, 30)),
            other => panic!("expected Rgb, got {other:?}"),
        }
    }

    #[test]
    fn color_for_basic_is_ansi() {
        let p = PaletteEntry { rgb: (10, 20, 30), ansi16: 9, name: "test" };
        match color_for(p, TermCaps::Basic) {
            Color::AnsiValue(v) => assert_eq!(v, 9),
            other => panic!("expected AnsiValue, got {other:?}"),
        }
    }

    fn sig(from: &str, cwd: &str) -> Signal {
        Signal {
            id: "t".into(),
            from: from.into(),
            project: cwd.rsplit('/').next().unwrap_or("?").into(),
            cwd: cwd.into(),
            reply_to: None,
            message: "msg".into(),
            ts: 0,
        }
    }

    #[test]
    fn registry_dedupes_repeat_senders() {
        let buf = vec![
            sig("claude:a", "/home/x"),
            sig("claude:a", "/home/x"), // same cwd, should dedup
            sig("claude:b", "/home/y"),
        ];
        let reg = known_identities(&buf, TermCaps::Rich);
        assert_eq!(reg.len(), 2, "expected 2 unique identities, got {}", reg.len());
    }

    #[test]
    fn registry_newest_first() {
        let buf = vec![
            sig("claude:a", "/home/x"),
            sig("claude:b", "/home/y"),
        ];
        let reg = known_identities(&buf, TermCaps::Rich);
        // Buffer order is oldest→newest; registry should surface the
        // most-recent cwd first so active peers lead the legend.
        let y_id = Identity::for_cwd("/home/y", TermCaps::Rich);
        assert_eq!(reg[0].nickname, y_id.nickname);
    }

    #[test]
    fn registry_includes_humans() {
        let buf = vec![sig("external:aaron@kitty", "/home/aaron/Projects")];
        let reg = known_identities(&buf, TermCaps::Rich);
        assert_eq!(reg.len(), 1);
        assert_eq!(reg[0].nickname, "aaron");
        assert!(!reg[0].is_claude);
    }

    #[test]
    fn registry_skips_unknown_prefix() {
        let buf = vec![sig("mystery:abc", "/tmp")];
        let reg = known_identities(&buf, TermCaps::Rich);
        assert!(reg.is_empty(), "unknown prefix should be ignored, got {reg:?}");
    }

    #[test]
    fn resolve_nickname_case_insensitive_claude_only() {
        let buf = vec![
            sig("claude:a", "/home/repo"),
            sig("external:aaron@kitty", "/home/aaron/Projects"),
        ];
        let reg = known_identities(&buf, TermCaps::Rich);
        let claude_nick = &reg.iter().find(|k| k.is_claude).unwrap().nickname;
        // Case-insensitive match hits the claude cwd.
        let lowered = claude_nick.to_ascii_lowercase();
        assert_eq!(resolve_nickname(&lowered, &reg), Some("/home/repo".into()));
        // Humans are not routable — `@aaron` returns None even
        // though the human appears in the registry.
        assert_eq!(resolve_nickname("aaron", &reg), None);
        // Unknown nickname → None.
        assert_eq!(resolve_nickname("NotReal", &reg), None);
    }
}
