//! Helper-row state machine.
//!
//! The row below the input box shows one of the registries — agents,
//! groups, slash commands, a subcommand level, or a free-token hint —
//! depending on what the user is typing. This module is the single
//! source of truth for that decision.
//!
//! [`derive`] is a pure function of the input buffer. Given any
//! string, it returns the [`HelperMode`] the render path should use.
//! Deterministic, side-effect free, and covered by a table of unit
//! tests that double as the state machine's visible specification
//! — scroll to the bottom of this file to see every state and the
//! input pattern that reaches it.
//!
//! ## States
//!
//! | State         | Entered when …                                | Partial underlined |
//! |---------------|-----------------------------------------------|--------------------|
//! | `Slash`       | buffer starts with `/`, still typing the name | yes, on command    |
//! | `Agents`      | trailing `@partial`, OR default               | yes, on agent      |
//! | `Groups`      | trailing `#partial`                           | yes, on group      |
//! | `SubCommands` | grammar walk sits at a subcommand choice      | yes, on subcommand |
//! | `FreeText`    | grammar walk sits at a free token             | — (hint only)      |
//! | `Agents`/`Groups` | grammar walk sits at an `@`/`#` token     | once typing starts |
//! | `Agents`      | grammar satisfied (past the end)              | —                  |
//!
//! ## Extending
//!
//! Adding a new slash command that routes the helper: add a row to
//! [`crate::slash::REGISTRY`] with the right grammar. No changes
//! here — [`derive`] walks the registry's grammars dynamically
//! (#405); state ascends/descends purely by re-deriving from the
//! string, so backspace unwinds levels with no hidden mode.

use crate::grammar::{self, SubCommand};
use crate::legend::{find_trailing_mention, Sigil};
use crate::slash;

/// Which registry the helper row should render right now. The
/// `Option<String>` carries the current partial (what the user has
/// typed after the sigil) so matching chips can underline for
/// Tab-target affordance. `None` means "no active partial" — render
/// the row without any underline.
#[derive(Debug, PartialEq, Eq)]
pub enum HelperMode {
    Agents(Option<String>),
    Groups(Option<String>),
    Slash(Option<String>),
    /// Grammar walk sits at a subcommand choice (#405) — render that
    /// level's candidates through the same chip row as the top-level
    /// slash list.
    SubCommands {
        choices: &'static [SubCommand],
        partial: Option<String>,
    },
    /// Grammar walk sits at a free token — render its hint dimmed
    /// (`<name>`, `[description…]`).
    FreeText(&'static str),
}

/// Derive the helper mode from the current input buffer.
///
/// Precedence:
///
/// 1. **Slash active.** Input starts with `/`:
///    - still typing the name → [`HelperMode::Slash`] with partial.
///    - past the name → walk the command's grammar (#405): the
///      position picks the registry (agents / groups / subcommand
///      level / free-token hint), descending one level per completed
///      token + space.
///    - past the name, unknown command → stay on slash registry so
///      the user can see valid options.
/// 2. **Trailing mention.** A trailing `@partial` or `#partial`
///    picks Agents or Groups respectively, with the partial under-
///    lined.
/// 3. **Default.** [`HelperMode::Agents`] with no partial — the
///    "who's around" glance.
pub fn derive(input: &str) -> HelperMode {
    if let Some(mode) = derive_slash(input) {
        return mode;
    }
    if let Some(mode) = derive_mention(input) {
        return mode;
    }
    HelperMode::Agents(None)
}

fn derive_slash(input: &str) -> Option<HelperMode> {
    // Leading-whitespace tolerant, matching `slash::parse` and
    // `slash::find_slash_partial`. " /help" is the same state as
    // "/help" for routing purposes — see PR #66 review item S2.
    if !input.trim_start().starts_with('/') {
        return None;
    }
    // Phase 1: still typing the command name.
    if let Some(partial) = slash::find_slash_partial(input) {
        return Some(HelperMode::Slash(Some(partial.partial.to_string())));
    }
    // Phase 2: past the name — the grammar walk owns the rest. Its
    // position maps 1:1 onto a helper mode; `Done` falls back to the
    // default agents glance (nothing left to help with).
    let (name, args) = slash::parse(input)?;
    let Some(cmd) = slash::lookup(name) else {
        // Unknown command — keep the slash registry visible so the
        // user can see what they might have meant.
        return Some(HelperMode::Slash(None));
    };
    Some(match grammar::walk(cmd.grammar, args) {
        grammar::Pos::Done => HelperMode::Agents(None),
        grammar::Pos::Sub { choices, partial } => HelperMode::SubCommands { choices, partial },
        grammar::Pos::Agent(p) => HelperMode::Agents(p),
        grammar::Pos::Group(p) => HelperMode::Groups(p),
        grammar::Pos::Free(hint) => HelperMode::FreeText(hint),
    })
}

fn derive_mention(input: &str) -> Option<HelperMode> {
    let ctx = find_trailing_mention(input)?;
    Some(match ctx.sigil {
        Sigil::Agent => HelperMode::Agents(Some(ctx.partial.to_string())),
        Sigil::Group => HelperMode::Groups(Some(ctx.partial.to_string())),
    })
}

#[cfg(test)]
mod tests {
    //! State machine's visible specification.
    //!
    //! Each test corresponds to one state-entry condition. Reading
    //! the test names top-to-bottom is reading the rule table.

    use super::*;

    // ── Default + mention states ───────────────────────────────

    #[test]
    fn empty_input_defaults_to_agents() {
        assert_eq!(derive(""), HelperMode::Agents(None));
    }

    #[test]
    fn plain_text_defaults_to_agents() {
        assert_eq!(derive("hello world"), HelperMode::Agents(None));
    }

    #[test]
    fn trailing_at_partial_selects_agents() {
        assert_eq!(derive("hi @Tam"), HelperMode::Agents(Some("Tam".into())));
    }

    #[test]
    fn trailing_hash_partial_selects_groups() {
        assert_eq!(derive("hi #dep"), HelperMode::Groups(Some("dep".into())));
    }

    // ── Slash: typing the command name ─────────────────────────

    #[test]
    fn bare_slash_shows_slash_with_empty_partial() {
        assert_eq!(derive("/"), HelperMode::Slash(Some("".into())));
    }

    #[test]
    fn slash_partial_name_shows_slash() {
        assert_eq!(derive("/he"), HelperMode::Slash(Some("he".into())));
    }

    // ── Slash: past the name, grammar routing ──────────────────

    #[test]
    fn slash_help_space_stays_on_agents_as_default() {
        // Empty grammar → satisfied → default registry (agents).
        assert_eq!(derive("/help "), HelperMode::Agents(None));
    }

    #[test]
    fn slash_whois_space_switches_to_agents_waiting_for_at() {
        // Agent token, no @ typed yet — Agents with no partial.
        assert_eq!(derive("/whois "), HelperMode::Agents(None));
    }

    #[test]
    fn slash_whois_with_at_partial_underlines_agent() {
        // Agent token, user is now typing @Ur — underline "Ur".
        assert_eq!(
            derive("/whois @Ur"),
            HelperMode::Agents(Some("Ur".into()))
        );
    }

    #[test]
    fn slash_join_space_switches_to_groups() {
        // Group token, no # typed yet — Groups with no partial.
        assert_eq!(derive("/join "), HelperMode::Groups(None));
    }

    #[test]
    fn slash_join_with_hash_partial_underlines_group() {
        assert_eq!(
            derive("/join #dep"),
            HelperMode::Groups(Some("dep".into()))
        );
    }

    #[test]
    fn slash_leave_space_switches_to_groups() {
        // Second Group-token command — confirms grammar routing isn't
        // tied to a single command name.
        assert_eq!(derive("/leave "), HelperMode::Groups(None));
    }

    // ── Slash: unknown / malformed ─────────────────────────────

    #[test]
    fn slash_unknown_command_keeps_slash_registry() {
        // So the user can see what they might have meant.
        assert_eq!(derive("/bogus "), HelperMode::Slash(None));
        assert_eq!(derive("/bogus arg"), HelperMode::Slash(None));
    }

    // ── Precedence: slash wins over any other sigil ────────────

    #[test]
    fn slash_precedes_trailing_mention() {
        // `/help @Tam` — even though there's a trailing @, the
        // buffer starts with `/` and we're past the command name.
        // the satisfied grammar returns Agents(None), which tests the
        // state machine's "slash wins" invariant.
        assert_eq!(derive("/help @Tam"), HelperMode::Agents(None));
    }

    // ── Leading whitespace tolerance (PR #66 review S2) ────────

    #[test]
    fn slash_with_leading_space_enters_slash_state() {
        // Stray leading space must not drop the buffer into mention
        // or default mode — otherwise the Enter interceptor and the
        // helper-row mode disagree on what the user is doing.
        assert_eq!(derive(" /he"), HelperMode::Slash(Some("he".into())));
        assert_eq!(derive("\t/"), HelperMode::Slash(Some("".into())));
    }

    #[test]
    fn slash_with_leading_space_routes_arg_kind() {
        // Past-the-name routing must also tolerate leading
        // whitespace — otherwise `" /whois "` silently shows agents
        // (the default) instead of agents-because-grammar.
        // The observable state is the same, but the reasoning path
        // differs; we want the state machine to take the Slash
        // branch for consistency with Enter / Tab.
        assert_eq!(derive(" /whois "), HelperMode::Agents(None));
        assert_eq!(derive(" /join #dep"), HelperMode::Groups(Some("dep".into())));
    }

    // ── Extra states suggested in PR #66 review ────────────────

    #[test]
    fn agent_cmd_with_wrong_sigil_falls_back_to_default_agents() {
        // `/whois #dep` — Agent token but user typed `#`. The
        // trailing-# doesn't satisfy the Agent filter, so the state
        // is "agents waiting for @" (no partial) rather than
        // hijacking to groups.
        assert_eq!(derive("/whois #dep"), HelperMode::Agents(None));
    }

    #[test]
    fn slash_agent_cmd_bare_at_yields_empty_partial() {
        // The user committed to addressing but hasn't typed letters
        // yet. Agents(Some("")) is the correct "everything matches
        // trivially" state — no chip gets underlined because the
        // underline rule requires a non-empty partial.
        assert_eq!(derive("/whois @"), HelperMode::Agents(Some("".into())));
    }

    #[test]
    fn leading_whitespace_no_slash_stays_on_default() {
        // Plain leading whitespace without a slash is still default
        // mode — the tolerance is surgical, not blanket.
        assert_eq!(derive(" hello"), HelperMode::Agents(None));
        assert_eq!(derive("   "), HelperMode::Agents(None));
    }

    // ── Hierarchical narrowing (#405) ──────────────────────────

    #[test]
    fn channels_space_descends_to_subcommand_level() {
        // Completed name + space: the choice level's full candidate
        // list, no narrowing yet.
        assert_eq!(
            derive("/channels "),
            HelperMode::SubCommands {
                choices: slash::CHANNELS_SUBS,
                partial: None
            }
        );
    }

    #[test]
    fn subcommand_partial_narrows_at_its_level() {
        assert_eq!(
            derive("/channels cr"),
            HelperMode::SubCommands {
                choices: slash::CHANNELS_SUBS,
                partial: Some("cr".into())
            }
        );
    }

    #[test]
    fn completed_subcommand_descends_to_its_first_token() {
        // create → free-word name hint; describe → channel legend.
        assert_eq!(derive("/channels create "), HelperMode::FreeText("<name>"));
        assert_eq!(derive("/channels describe "), HelperMode::Groups(None));
        assert_eq!(
            derive("/channels describe #pla"),
            HelperMode::Groups(Some("pla".into()))
        );
        // list's branch grammar is empty — immediately satisfied.
        assert_eq!(derive("/channels list "), HelperMode::Agents(None));
    }

    #[test]
    fn free_text_tail_holds_its_hint() {
        assert_eq!(
            derive("/channels create plans "),
            HelperMode::FreeText("[description…]")
        );
        assert_eq!(
            derive("/channels create plans roadmap talk for"),
            HelperMode::FreeText("[description…]")
        );
        assert_eq!(
            derive("/channels describe #plans road"),
            HelperMode::FreeText("[description…]")
        );
    }

    #[test]
    fn unknown_subcommand_keeps_the_level_unnarrowed() {
        // Same posture as an unknown top-level command: show the
        // valid options rather than guessing.
        assert_eq!(
            derive("/channels bogus "),
            HelperMode::SubCommands {
                choices: slash::CHANNELS_SUBS,
                partial: None
            }
        );
    }

    #[test]
    fn membership_pair_walks_agent_then_channel() {
        // invite/kick: <@agent> [#channel] — the second level opens
        // once the first token completes.
        assert_eq!(derive("/invite "), HelperMode::Agents(None));
        assert_eq!(derive("/invite @Cl"), HelperMode::Agents(Some("Cl".into())));
        assert_eq!(derive("/invite @Cleo "), HelperMode::Groups(None));
        assert_eq!(
            derive("/kick @Cleo #dep"),
            HelperMode::Groups(Some("dep".into()))
        );
        // Grammar satisfied → default glance.
        assert_eq!(derive("/kick @Cleo #deploy "), HelperMode::Agents(None));
    }

    #[test]
    fn backspacing_ascends_because_state_is_derived() {
        // The exact reverse of the descend sequence — no mode state
        // to unwind, just re-derivation from shorter strings.
        assert_eq!(
            derive("/channels create plans "),
            HelperMode::FreeText("[description…]")
        );
        assert_eq!(derive("/channels create plans"), HelperMode::FreeText("<name>"));
        assert_eq!(derive("/channels create "), HelperMode::FreeText("<name>"));
        assert_eq!(
            derive("/channels create"),
            HelperMode::SubCommands {
                choices: slash::CHANNELS_SUBS,
                partial: Some("create".into())
            }
        );
        assert_eq!(derive("/channels"), HelperMode::Slash(Some("channels".into())));
        assert_eq!(derive("/"), HelperMode::Slash(Some("".into())));
    }
}
