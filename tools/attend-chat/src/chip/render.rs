//! Sender-chip rendering: pure display facts derived from a wire
//! `from`/`project`/`cwd` triple.
//!
//! No IO, no state — everything here is a function of the inputs plus
//! an `instances` cache (read by `with_instance` for the per-session
//! suffix). The chip the renderer ultimately draws is a function of
//! `ChipInfo`.

use agent_identity::{Identity, PaletteEntry, Style, TermCaps};
use attend_instances::SnapshotCache;
use iocraft::prelude::Color;

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
/// `agent_identity::Identity::for_cwd`) plus an instance suffix
/// resolved through `instances` (ADR-129). Humans keep their
/// username but still pick up a color + style from the identity
/// table so the avatar is visually consistent everywhere the same
/// user shows up.
///
/// `instances` is a per-render cache that collapses repeat lookups
/// for the same cwd into a single registry read. Build it once at
/// the top of a render pass and pass it through to every chip.
///
/// This function never touches the wire format — identity is pure
/// receiver-side rendering. If the signal's `from` doesn't match a
/// known prefix, we fall through to showing the raw value.
pub fn chip_for(
    from: &str,
    project: &str,
    cwd: &str,
    caps: TermCaps,
    instances: &SnapshotCache,
) -> ChipInfo {
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
        // same dir wear the same nickname stem, with a per-session
        // instance suffix (ADR-129) appended to disambiguate.
        let id = Identity::for_cwd(cwd, caps);
        let display = with_instance(id.nickname, cwd, uuid, instances);
        ChipInfo {
            primary: truncate(&display, interior),
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

/// Compose `<nickname>-<instance>` for a claude session (ADR-129).
/// Falls back to the bare nickname when the registry has no entry —
/// only happens in the moments before a session has registered, or
/// when the registry file is unreadable.
///
/// Reads through `instances`, which caches per-cwd snapshots for
/// the lifetime of one render pass. Without this cache the render
/// path read + parsed the registry yaml once per chip; with it,
/// the read is amortized to once per distinct cwd per render.
///
/// Visible to sibling submodules (`registry`) so the legend builds
/// the same display strings the chip does.
pub(super) fn with_instance(
    nickname: &str,
    cwd: &str,
    session_id: &str,
    instances: &SnapshotCache,
) -> String {
    match instances.lookup(cwd, session_id) {
        Some(inst) => format!("{nickname}-{inst}"),
        None => nickname.to_string(),
    }
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

    /// Synthetic test session_ids never have registry entries, so a
    /// fresh cache always returns `None` from `lookup` and the suffix
    /// path falls back to bare nicknames.
    fn empty_cache() -> SnapshotCache {
        SnapshotCache::new()
    }

    #[test]
    fn claude_sender_uses_derived_nickname() {
        // A claude in /home/aaron/.claude gets a nickname from the
        // agent-identity pool, scope comes from cwd basename.
        let chip = chip_for(
            "claude:e74a4a4b-7e3b-49bc-8404-216162e54ba8",
            "claude",
            "/home/aaron/.claude",
            TermCaps::Rich,
            &empty_cache(),
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
        let a = chip_for("claude:aaaa-1", "p", "/home/x", TermCaps::Rich, &empty_cache());
        let b = chip_for("claude:bbbb-2", "p", "/home/x", TermCaps::Rich, &empty_cache());
        assert_eq!(a.primary, b.primary);
    }

    #[test]
    fn scope_prefers_cwd_basename_over_project() {
        let chip = chip_for("external:aaron", "ignored", "/home/aaron/temp", TermCaps::Rich, &empty_cache());
        assert_eq!(chip.secondary, "temp");
    }

    #[test]
    fn external_strips_terminal_suffix() {
        let chip = chip_for("external:aaron@kitty", "proj", "/home/aaron", TermCaps::Rich, &empty_cache());
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
            &empty_cache(),
        );
        assert!(chip.secondary.chars().count() <= 16);
        assert!(chip.secondary.ends_with('…'));
    }

    #[test]
    fn unknown_sender_still_colored() {
        // Something that isn't claude: or external: — we don't crash,
        // we show the raw value and pick a color off it.
        let a = chip_for("mystery:abc", "", "/tmp", TermCaps::Rich, &empty_cache());
        let b = chip_for("mystery:xyz", "", "/tmp", TermCaps::Rich, &empty_cache());
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
}
