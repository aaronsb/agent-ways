//! Slash-command parser, registry, and autocomplete.
//!
//! Slash commands are a TUI-local grammar: they never touch the
//! signal bus. A message beginning with `/` is intercepted in the
//! Enter handler and dispatched here; everything else falls through
//! to the `@`/`#`/plain-text send paths in `app.rs`.
//!
//! The grammar differs from `@Name` and `#group`:
//! - mentions can appear anywhere in a message body
//! - slash commands are start-of-input only — a `/` mid-sentence is a
//!   literal slash, not a command sigil
//!
//! Because of that asymmetry, parse + completion live in their own
//! module instead of being retrofitted into [`crate::legend`]. Agent
//! / group legends and slash completion share only the spirit of
//! "Tab completes the partial you're typing" — the plumbing is
//! distinct.
//!
//! **Scope.** Every registered command dispatches (ADR-173 Decision 5
//! graduated the last Planned rows, `/peers` and `/whois`). The
//! [`Status::Planned`] machinery stays: a future roadmap entry gets
//! autocomplete + `/help` visibility and a "not implemented yet"
//! dispatch until its handler arm lands.
//!
//! Dispatch stays IO-free: commands with side effects return a
//! [`SlashOutcome`] variant describing the effect (clear the
//! transcript, join/leave a group) and the key handler in
//! `crate::app::keys` executes it. That keeps this module's parse +
//! validate logic unit-testable without a filesystem.

use iocraft::prelude::*;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Wired into [`dispatch`]. Runs when invoked.
    Implemented,
    /// Listed for autocomplete + `/help` visibility, but dispatch
    /// short-circuits to a "coming soon" status message.
    Planned,
}

/// What registry the helper row should switch to once the user is
/// past a command's name and typing its argument. The app-side
/// state machine in `crate::helper` reads this to pick the right
/// chip source — keeping the "which commands expect which kind of
/// argument" data next to the command definition itself, so adding
/// a command means adding one row here and nothing elsewhere.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ArgKind {
    /// Command takes no argument. Helper stays on the default
    /// (agents) once the name is complete.
    None,
    /// Expects an `@Agent`. Helper switches to the agent legend.
    Agent,
    /// Expects a `#group`. Helper switches to the channel legend.
    Group,
}

/// One row of the slash-command registry.
#[derive(Copy, Clone, Debug)]
pub struct SlashCommand {
    pub name: &'static str,
    pub description: &'static str,
    pub status: Status,
    pub arg_kind: ArgKind,
}

/// Every slash command the TUI knows about. Implemented ones
/// dispatch; planned ones autocomplete and show up in `/help` so
/// agents can see the roadmap without reading code.
pub const REGISTRY: &[SlashCommand] = &[
    SlashCommand {
        name: "help",
        description: "List available commands",
        status: Status::Implemented,
        arg_kind: ArgKind::None,
    },
    SlashCommand {
        name: "whois",
        description: "Show a peer's session id + origin root",
        status: Status::Implemented,
        arg_kind: ArgKind::Agent,
    },
    SlashCommand {
        name: "peers",
        description: "List live peer sessions",
        status: Status::Implemented,
        arg_kind: ArgKind::None,
    },
    SlashCommand {
        name: "join",
        description: "Join a focus group",
        status: Status::Implemented,
        arg_kind: ArgKind::Group,
    },
    SlashCommand {
        name: "leave",
        description: "Leave a focus group",
        status: Status::Implemented,
        arg_kind: ArgKind::Group,
    },
    SlashCommand {
        name: "dissolve",
        description: "Remove a group with no live members",
        status: Status::Implemented,
        arg_kind: ArgKind::Group,
    },
    SlashCommand {
        name: "channels",
        description: "List channels with member counts",
        status: Status::Implemented,
        arg_kind: ArgKind::None,
    },
    SlashCommand {
        name: "invite",
        description: "Invite a peer to a channel (they join themselves)",
        status: Status::Implemented,
        arg_kind: ArgKind::Agent,
    },
    SlashCommand {
        name: "kick",
        description: "Remove a member from a channel",
        status: Status::Implemented,
        arg_kind: ArgKind::Agent,
    },
    SlashCommand {
        name: "purge",
        description: "Delete a channel's on-disk history",
        status: Status::Implemented,
        arg_kind: ArgKind::Group,
    },
    SlashCommand {
        name: "clear",
        description: "Clear the message buffer",
        status: Status::Implemented,
        arg_kind: ArgKind::None,
    },
];

/// Look up a registered command by name (exact match, case-sensitive).
/// The helper state machine uses this to inspect `arg_kind` once the
/// user is past the name; callers that need prefix matching use
/// [`best_slash_completion`] instead.
pub fn lookup(name: &str) -> Option<&'static SlashCommand> {
    REGISTRY.iter().find(|c| c.name == name)
}

/// Parse a slash command from the input buffer.
///
/// Returns `Some((name, args))` if `input` begins (optionally after
/// leading whitespace) with `/` followed by a non-empty name; args
/// is whatever follows the first whitespace run, trimmed of its
/// leading spaces. Returns `None` for plain text, a bare `/`, or a
/// `/ ` followed by whitespace.
///
/// Leading-whitespace tolerance means `" /help"` parses identically
/// to `"/help"` — the user who stray-spaces their command shouldn't
/// get silently routed onto the signal bus as a peer message.
pub fn parse(input: &str) -> Option<(&str, &str)> {
    let rest = input.trim_start().strip_prefix('/')?;
    let end = rest
        .char_indices()
        .find(|(_, c)| c.is_whitespace())
        .map(|(i, _)| i)
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    let name = &rest[..end];
    let args = rest[end..].trim_start();
    Some((name, args))
}

/// Partial slash command at the head of the input — the autocomplete
/// gate. Returns `Some` only while the user is still typing the
/// *name*: a space after the name means completion is done and args
/// are being typed.
#[derive(Debug, PartialEq, Eq)]
pub struct SlashPartial<'a> {
    pub partial: &'a str,
}

/// Inline help for the status slot while composing (#398): when
/// exactly one registry command has `partial` as a prefix, return its
/// one-line help; otherwise `None` and the caller keeps showing the
/// last executed result. An empty partial matches everything, so the
/// bare `/` menu keeps the result visible beneath the full list.
pub fn single_match_help(partial: &str) -> Option<String> {
    let mut matches = REGISTRY.iter().filter(|c| c.name.starts_with(partial));
    match (matches.next(), matches.next()) {
        (Some(cmd), None) => {
            let arg = match cmd.arg_kind {
                ArgKind::None => "",
                ArgKind::Agent => " <@name>",
                ArgKind::Group => " <#channel>",
            };
            let planned = if cmd.status == Status::Planned {
                " (planned)"
            } else {
                ""
            };
            Some(format!("/{}{} — {}{}", cmd.name, arg, cmd.description, planned))
        }
        _ => None,
    }
}

pub fn find_slash_partial(input: &str) -> Option<SlashPartial<'_>> {
    // Mirror [`parse`]'s leading-whitespace tolerance so every caller
    // (Enter interceptor, Tab completion, helper-row state machine)
    // sees the same "starts with `/`" predicate.
    let rest = input.trim_start().strip_prefix('/')?;
    if rest.contains(char::is_whitespace) {
        return None;
    }
    Some(SlashPartial { partial: rest })
}

/// Find the best match for `partial` in the registry. Case-
/// insensitive prefix match; first hit in registry order wins (so
/// the order in [`REGISTRY`] doubles as completion priority).
pub fn best_slash_completion(partial: &str) -> Option<&'static SlashCommand> {
    let lc = partial.to_ascii_lowercase();
    REGISTRY
        .iter()
        .find(|c| c.name.to_ascii_lowercase().starts_with(&lc))
}

/// Apply a completion: replace the whole buffer with `/<name> ` and
/// place the cursor after the trailing space, ready for args.
pub fn apply_slash_completion(full_name: &str) -> (String, usize) {
    let out = format!("/{full_name} ");
    let cursor = out.chars().count();
    (out, cursor)
}

/// Outcome of dispatching a parsed slash command.
///
/// `Ok`/`Err` carry a status string and end here; the effect variants
/// describe work the key handler must perform — dispatch itself never
/// touches the filesystem, so join/leave validation stays testable
/// without a signals base on disk.
#[derive(Debug, PartialEq, Eq)]
pub enum SlashOutcome {
    /// Status message to show; clear the input buffer.
    Ok(String),
    /// Status message to show; *keep* the input so the user can
    /// edit + retry (typical for unknown / malformed commands).
    Err(String),
    /// `/clear` — empty the transcript buffer.
    ClearTranscript,
    /// `/join` — join the named focus group as the human member
    /// (ADR-170). Name is already normalized (`#` stripped) and
    /// validated.
    Join(String),
    /// `/leave [#g]` — leave a focus group. Name normalized; `None`
    /// scopes to the foreground tab (#393). Existence/membership are
    /// checked at execution time against the live state.
    Leave(Option<String>),
    /// `/dissolve [#g]` — remove a group entirely: yaml entry and
    /// `@dir`, including orphan dirs the yaml doesn't know about
    /// (ADR-170). `None` scopes to the foreground tab (#393); the
    /// live-member guard runs at execution time.
    Dissolve(Option<String>),
    /// `/channels` — render every channel with live/total member
    /// counts into the status row.
    ListChannels,
    /// `/purge` — delete a channel's on-disk signal history (ADR-136
    /// durability, deliberately overridden by explicit operator
    /// action). `None` targets the base channel's `_broadcast/`.
    Purge(Option<String>),
    /// `/peers` — render the live-session roster as a transcript
    /// status block (ADR-173 Decision 5: operator discovery parity).
    Peers,
    /// `/whois @name` — resolve a display name to its session id +
    /// origin root. Name normalized (`@` stripped); resolution
    /// happens at execution time against the live roster.
    Whois(String),
    /// `/invite @name [#channel]` — send a DIRECTED invitation to the
    /// peer's project tray. Never a force-add: entry changes what
    /// reaches a session, so it requires the invitee's own
    /// `attend join`; silence declines (consent asymmetry, ADR-173 /
    /// issue #393).
    Invite {
        member: String,
        channel: Option<String>,
    },
    /// `/kick @name [#channel]` — remove a member from a channel.
    /// Operator power tool, the membership sibling of `/purge`:
    /// removal only *reduces* the target's routing reach, so it needs
    /// no consent. The kicked member gets a directed notice so their
    /// focus state doesn't drift silently; rejoin stays allowed.
    Kick {
        member: String,
        channel: Option<String>,
    },
}

/// Normalize an agent argument: first whitespace-separated token,
/// leading `@` stripped (users copy the legend spelling). Returns
/// `None` when no argument was given.
fn agent_arg(args: &str) -> Option<&str> {
    let first = args.split_whitespace().next()?;
    Some(first.strip_prefix('@').unwrap_or(first))
}

/// Normalize a group argument: first whitespace-separated token,
/// leading `#` stripped (users copy the channel-bar spelling).
/// Returns `None` when no argument was given.
fn group_arg(args: &str) -> Option<&str> {
    let first = args.split_whitespace().next()?;
    Some(first.strip_prefix('#').unwrap_or(first))
}

/// Dispatch a parsed slash command.
pub fn dispatch(name: &str, args: &str) -> SlashOutcome {
    let Some(cmd) = REGISTRY.iter().find(|c| c.name == name) else {
        return SlashOutcome::Err(format!("unknown: /{name} (try /help)"));
    };
    match cmd.status {
        Status::Implemented => match cmd.name {
            "help" => SlashOutcome::Ok(help_message()),
            "clear" => SlashOutcome::ClearTranscript,
            "join" => match group_arg(args) {
                None => SlashOutcome::Err("usage: /join <group>".into()),
                // Friendlier than the generic "'open' is reserved":
                // the base channel isn't joinable because everyone is
                // already in it (ADR-124).
                Some("open") => SlashOutcome::Err(
                    "#open is the base channel — everyone is already there".into(),
                ),
                Some(g) => match attend_groups::validate_group_name(g) {
                    Ok(()) => SlashOutcome::Join(g.to_string()),
                    Err(e) => SlashOutcome::Err(format!("/join: {e}")),
                },
            },
            // Bare `/leave` and `/dissolve` scope to the foreground
            // tab (#393) — the key handler owns tab state and also
            // re-applies the base-channel rejection when the
            // foreground resolves to `#open`.
            "leave" => match group_arg(args) {
                None => SlashOutcome::Leave(None),
                Some("open") => SlashOutcome::Err(
                    "#open is the base channel — you can't leave it".into(),
                ),
                Some(g) => SlashOutcome::Leave(Some(g.to_string())),
            },
            // No name validation here — dissolve targets *existing*
            // state, and its main quarry is exactly the orphan dirs
            // whose names a strict validator might reject.
            "dissolve" => match group_arg(args) {
                None => SlashOutcome::Dissolve(None),
                Some("open") => SlashOutcome::Err(
                    "#open is the base channel — you can't dissolve it".into(),
                ),
                Some(g) => SlashOutcome::Dissolve(Some(g.to_string())),
            },
            "channels" => SlashOutcome::ListChannels,
            // Bare `/purge` = foreground-tab scope, resolved
            // downstream. An explicit `/purge open` stays EXPLICIT:
            // collapsing it into `None` here would reinterpret it as
            // foreground scope and purge whatever tab the operator
            // happens to be on (PR #395 blocking finding —
            // destructive intent inversion). `run_purge` maps the
            // resolved base name onto the on-disk `None` target.
            "purge" => match group_arg(args) {
                None => SlashOutcome::Purge(None),
                Some(g) => SlashOutcome::Purge(Some(g.to_string())),
            },
            "peers" => SlashOutcome::Peers,
            "whois" => match agent_arg(args) {
                None => SlashOutcome::Err("usage: /whois <@name>".into()),
                Some(name) => SlashOutcome::Whois(name.to_string()),
            },
            // Shared `@name [#channel]` grammar for the membership
            // pair. A missing channel means "scope to the foreground
            // tab" — resolved by the key handler, which owns tab
            // state; dispatch stays context-free.
            "invite" | "kick" => match agent_arg(args) {
                None => SlashOutcome::Err(format!("usage: /{} <@name> [#channel]", cmd.name)),
                Some(member) => {
                    let channel = args
                        .split_whitespace()
                        .nth(1)
                        .map(|c| c.strip_prefix('#').unwrap_or(c).to_string());
                    match (cmd.name, channel.as_deref()) {
                        ("invite", Some("open")) => SlashOutcome::Err(
                            "#open is the base channel — everyone is already there".into(),
                        ),
                        ("kick", Some("open")) => SlashOutcome::Err(
                            "#open is the base channel — you can't kick from it".into(),
                        ),
                        ("invite", _) => SlashOutcome::Invite {
                            member: member.to_string(),
                            channel,
                        },
                        _ => SlashOutcome::Kick {
                            member: member.to_string(),
                            channel,
                        },
                    }
                }
            },
            // Keeps the match exhaustive — if a registry entry flips
            // to Implemented without a handler arm, this fires at
            // runtime rather than silently treating it as planned.
            other => SlashOutcome::Err(format!("/{other}: registered but unhandled")),
        },
        Status::Planned => SlashOutcome::Err(format!(
            "/{}: {} — not implemented yet",
            cmd.name, cmd.description
        )),
    }
}

/// Render the slash-command legend row — one chip per registered
/// command. Implemented entries render in a full-weight foreground
/// color; planned entries dim so the roadmap is visible without
/// looking equally available. Matching names underline when the
/// caller passes the current partial (Tab-target affordance, same
/// idiom as the agent / group legends).
pub fn slash_legend_row(current_partial: Option<&str>) -> Vec<AnyElement<'static>> {
    let lc_partial = current_partial.map(|p| p.to_ascii_lowercase());
    let chips: Vec<AnyElement<'static>> = REGISTRY
        .iter()
        .map(|cmd| {
            let matches = lc_partial
                .as_ref()
                .map(|p| !p.is_empty() && cmd.name.to_ascii_lowercase().starts_with(p))
                .unwrap_or(false);
            // Slash commands aren't identities, so they don't carry
            // palette hashing — a uniform Cyan for ready, DarkGrey
            // for planned keeps the bar readable without adding new
            // visual dimensions the user has to learn.
            let color = match cmd.status {
                Status::Implemented => Color::Cyan,
                Status::Planned => Color::DarkGrey,
            };
            let weight = if matches { Weight::Bold } else { Weight::Normal };
            let decoration = if matches {
                TextDecoration::Underline
            } else {
                TextDecoration::None
            };
            let content = format!("/{} ", cmd.name);
            element! {
                Text(color, weight, decoration, content, wrap: TextWrap::NoWrap)
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

fn help_message() -> String {
    let mut ready: Vec<String> = Vec::new();
    let mut planned: Vec<String> = Vec::new();
    for c in REGISTRY {
        let label = format!("/{}", c.name);
        match c.status {
            Status::Implemented => ready.push(label),
            Status::Planned => planned.push(label),
        }
    }
    if planned.is_empty() {
        format!("available: {}", ready.join(" "))
    } else {
        format!(
            "available: {} · planned: {}",
            ready.join(" "),
            planned.join(" ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bare_command() {
        assert_eq!(parse("/help"), Some(("help", "")));
    }

    #[test]
    fn parse_command_with_args() {
        assert_eq!(parse("/whois @Urban"), Some(("whois", "@Urban")));
    }

    #[test]
    fn parse_collapses_leading_arg_whitespace() {
        assert_eq!(parse("/whois    @Urban"), Some(("whois", "@Urban")));
    }

    #[test]
    fn parse_rejects_bare_slash() {
        assert_eq!(parse("/"), None);
    }

    #[test]
    fn parse_rejects_slash_space() {
        // `/ foo` — no command name, just a stray slash.
        assert_eq!(parse("/ foo"), None);
    }

    #[test]
    fn parse_rejects_plain_text() {
        assert_eq!(parse("hello /help"), None);
        assert_eq!(parse("hello"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn parse_tolerates_leading_whitespace() {
        // A stray space in front of `/help` should not leak onto
        // the signal bus as a peer message — the interceptor
        // relies on this predicate.
        assert_eq!(parse(" /help"), Some(("help", "")));
        assert_eq!(parse("\t/whois @Urban"), Some(("whois", "@Urban")));
    }

    #[test]
    fn slash_partial_while_typing_name() {
        let p = find_slash_partial("/hel").unwrap();
        assert_eq!(p.partial, "hel");
    }

    #[test]
    fn slash_partial_bare_slash_is_empty() {
        let p = find_slash_partial("/").unwrap();
        assert_eq!(p.partial, "");
    }

    #[test]
    fn slash_partial_stops_after_space() {
        // Past the command name — completion no longer applies.
        assert!(find_slash_partial("/help ").is_none());
        assert!(find_slash_partial("/whois @Ur").is_none());
    }

    #[test]
    fn slash_partial_rejects_non_slash_input() {
        assert!(find_slash_partial("hello").is_none());
        assert!(find_slash_partial("").is_none());
    }

    #[test]
    fn slash_partial_tolerates_leading_whitespace() {
        // Mirror of `parse_tolerates_leading_whitespace` for the
        // completion-gate path — the helper row and Tab handler
        // both depend on this agreeing with `parse`.
        let p = find_slash_partial(" /he").unwrap();
        assert_eq!(p.partial, "he");
        let p = find_slash_partial("  /").unwrap();
        assert_eq!(p.partial, "");
    }

    #[test]
    fn best_completion_matches_prefix_case_insensitive() {
        let c = best_slash_completion("he").unwrap();
        assert_eq!(c.name, "help");
        let c = best_slash_completion("HE").unwrap();
        assert_eq!(c.name, "help");
    }

    #[test]
    fn best_completion_empty_partial_picks_first() {
        // A bare `/` + Tab should complete to the first registry
        // entry — the most visible command gets the zero-friction
        // path.
        let c = best_slash_completion("").unwrap();
        assert_eq!(c.name, REGISTRY[0].name);
    }

    #[test]
    fn best_completion_none_on_miss() {
        assert!(best_slash_completion("zzzzz").is_none());
    }

    #[test]
    fn apply_completion_adds_trailing_space() {
        let (out, cursor) = apply_slash_completion("help");
        assert_eq!(out, "/help ");
        assert_eq!(cursor, "/help ".chars().count());
    }

    #[test]
    fn dispatch_help_lists_commands() {
        let SlashOutcome::Ok(s) = dispatch("help", "") else {
            panic!("help should dispatch Ok")
        };
        assert!(s.contains("/help"));
        // With nothing planned, no empty "planned:" stub renders.
        assert!(!s.contains("planned"));
    }

    #[test]
    fn dispatch_unknown_returns_err() {
        let SlashOutcome::Err(s) = dispatch("bogus", "") else {
            panic!("unknown should dispatch Err")
        };
        assert!(s.contains("unknown"));
        assert!(s.contains("/bogus"));
    }

    #[test]
    fn dispatch_clear_returns_effect() {
        assert_eq!(dispatch("clear", ""), SlashOutcome::ClearTranscript);
        // Stray args after /clear are ignored, not an error.
        assert_eq!(dispatch("clear", "everything"), SlashOutcome::ClearTranscript);
    }

    #[test]
    fn dispatch_join_normalizes_and_validates() {
        assert_eq!(dispatch("join", "deploy"), SlashOutcome::Join("deploy".into()));
        // The `#` spelling from the channel bar is accepted.
        assert_eq!(dispatch("join", "#deploy"), SlashOutcome::Join("deploy".into()));
        // Missing arg → usage, input kept for retry.
        let SlashOutcome::Err(s) = dispatch("join", "") else {
            panic!("bare /join should be a usage error")
        };
        assert!(s.contains("usage"));
        // Base channel is not joinable.
        let SlashOutcome::Err(s) = dispatch("join", "open") else {
            panic!("/join open should be rejected")
        };
        assert!(s.contains("base channel"));
        // Invalid names bounce with the shared validator's message.
        let SlashOutcome::Err(s) = dispatch("join", "_internal") else {
            panic!("invalid name should be rejected")
        };
        assert!(s.contains("/join"));
    }

    #[test]
    fn dispatch_leave_normalizes() {
        assert_eq!(
            dispatch("leave", "#deploy"),
            SlashOutcome::Leave(Some("deploy".into()))
        );
        // Bare form scopes to the foreground tab (#393).
        assert_eq!(dispatch("leave", ""), SlashOutcome::Leave(None));
        let SlashOutcome::Err(s) = dispatch("leave", "open") else {
            panic!("/leave open should be rejected")
        };
        assert!(s.contains("base channel"));
    }

    #[test]
    fn dispatch_dissolve_normalizes() {
        assert_eq!(
            dispatch("dissolve", "#bg-test"),
            SlashOutcome::Dissolve(Some("bg-test".into()))
        );
        // Bare form scopes to the foreground tab (#393).
        assert_eq!(dispatch("dissolve", ""), SlashOutcome::Dissolve(None));
        let SlashOutcome::Err(s) = dispatch("dissolve", "open") else {
            panic!("/dissolve open should be rejected")
        };
        assert!(s.contains("base channel"));
    }

    #[test]
    fn dispatch_channels_returns_effect() {
        assert_eq!(dispatch("channels", ""), SlashOutcome::ListChannels);
    }

    #[test]
    fn dispatch_purge_defaults_to_base_channel() {
        // Bare /purge → foreground scope (None). Explicit "open" must
        // NOT collapse to None: from a channel tab that would purge
        // the foreground instead of the commons (PR #395 blocking).
        assert_eq!(dispatch("purge", ""), SlashOutcome::Purge(None));
        assert_eq!(dispatch("purge", "open"), SlashOutcome::Purge(Some("open".into())));
        assert_eq!(dispatch("purge", "#open"), SlashOutcome::Purge(Some("open".into())));
        assert_eq!(
            dispatch("purge", "#deploy"),
            SlashOutcome::Purge(Some("deploy".into()))
        );
    }

    #[test]
    fn dispatch_peers_returns_effect() {
        assert_eq!(dispatch("peers", ""), SlashOutcome::Peers);
    }

    #[test]
    fn dispatch_whois_normalizes() {
        assert_eq!(
            dispatch("whois", "@Tamsin-alpha"),
            SlashOutcome::Whois("Tamsin-alpha".into())
        );
        // Bare-name spelling is accepted too.
        assert_eq!(dispatch("whois", "Cleo"), SlashOutcome::Whois("Cleo".into()));
        let SlashOutcome::Err(s) = dispatch("whois", "") else {
            panic!("bare /whois should be a usage error")
        };
        assert!(s.contains("usage"));
    }

    #[test]
    fn dispatch_invite_parses_member_and_optional_channel() {
        assert_eq!(
            dispatch("invite", "@Tamsin-alpha #deploy"),
            SlashOutcome::Invite {
                member: "Tamsin-alpha".into(),
                channel: Some("deploy".into())
            }
        );
        // No channel → foreground-tab scope, resolved by the caller.
        assert_eq!(
            dispatch("invite", "@Cleo"),
            SlashOutcome::Invite {
                member: "Cleo".into(),
                channel: None
            }
        );
        let SlashOutcome::Err(s) = dispatch("invite", "") else {
            panic!("bare /invite should be a usage error")
        };
        assert!(s.contains("usage"));
        // Base channel: everyone is already there — nothing to invite to.
        let SlashOutcome::Err(s) = dispatch("invite", "@Cleo #open") else {
            panic!("/invite to open should be rejected")
        };
        assert!(s.contains("base channel"));
    }

    #[test]
    fn dispatch_kick_parses_member_and_optional_channel() {
        assert_eq!(
            dispatch("kick", "@Tamsin-alpha #deploy"),
            SlashOutcome::Kick {
                member: "Tamsin-alpha".into(),
                channel: Some("deploy".into())
            }
        );
        assert_eq!(
            dispatch("kick", "Cleo"),
            SlashOutcome::Kick {
                member: "Cleo".into(),
                channel: None
            }
        );
        let SlashOutcome::Err(s) = dispatch("kick", "") else {
            panic!("bare /kick should be a usage error")
        };
        assert!(s.contains("usage"));
        let SlashOutcome::Err(s) = dispatch("kick", "@Cleo open") else {
            panic!("/kick from open should be rejected")
        };
        assert!(s.contains("base channel"));
    }

    #[test]
    fn registry_has_no_planned_commands_left() {
        // ADR-173 Decision 5 closed the discovery gap — /peers and
        // /whois were the last Planned rows. Every registered command
        // now dispatches; a new Planned entry should be a deliberate
        // roadmap choice, not a leftover.
        assert!(REGISTRY.iter().all(|c| c.status == Status::Implemented));
    }
}

#[cfg(test)]
mod single_match_tests {
    use super::*;

    #[test]
    fn unique_prefix_yields_help_with_arg_hint() {
        // "/w" → only whois. Help carries the @name hint.
        let h = single_match_help("w").expect("whois is the only w-command");
        assert!(h.starts_with("/whois <@name> — "), "got: {h}");
    }

    #[test]
    fn ambiguous_and_empty_prefixes_yield_none() {
        // Empty matches everything → the bare / menu keeps the last
        // result visible (#398 step 2).
        assert!(single_match_help("").is_none());
        // "p" is ambiguous (peers, purge).
        assert!(single_match_help("p").is_none());
    }

    #[test]
    fn full_name_and_unknown_behave() {
        assert!(single_match_help("peers").is_some());
        assert!(single_match_help("zzz").is_none());
    }
}
