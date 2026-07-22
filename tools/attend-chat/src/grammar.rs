//! Command grammars and the hierarchical narrowing walk (#405).
//!
//! Every slash command carries a grammar — a sequence of [`Token`]s,
//! optionally opening with a subcommand choice — instead of the old
//! single-valued `ArgKind`. The helper row narrows through it the way
//! Rhino 3D / AutoCAD command lines do: while a token is being typed,
//! the current level's candidates render with prefix highlight; a
//! completed token + space descends one level; backspacing ascends
//! naturally because [`walk`] re-derives the position from the input
//! string on every render — there is no hidden mode state.
//!
//! The walk is pure string work over `&'static` grammar data, so the
//! whole state space is unit-testable without a terminal. Structural
//! tokens split on whitespace (the same rule `slash::dispatch` uses
//! to parse arguments, so the helper can never disagree with what
//! Enter will do); a [`Token::Rest`] swallows everything after it,
//! which is where free-text descriptions live.

/// One token position in a slash command's grammar.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Token {
    /// An `@agent` argument — the helper shows the agent legend.
    Agent { required: bool },
    /// A `#channel` argument — the helper shows the channel legend.
    Group { required: bool },
    /// A single free-word token (e.g. a NEW channel name that no
    /// legend can offer). The hint carries its own `<>`/`[]`
    /// brackets and renders verbatim in the helper and in help.
    Word(&'static str),
    /// Free text to end of line. Hint carries its own brackets.
    /// Nothing can follow a `Rest` — it swallows the tail.
    Rest(&'static str),
    /// A subcommand choice steering into per-branch grammars. By
    /// convention this is a command's sole top-level token (slash
    /// commands stay top-level; subcommands steer into them), and
    /// it is optional — a bare command falls back to its default
    /// behavior, so the signature renders in `[]`.
    Subcommands(&'static [SubCommand]),
}

/// One branch of a [`Token::Subcommands`] choice.
#[derive(Debug, PartialEq, Eq)]
pub struct SubCommand {
    pub name: &'static str,
    pub description: &'static str,
    pub grammar: &'static [Token],
}

/// Where in a grammar the caret currently sits. Derived fresh from
/// the args string each render — see module docs.
#[derive(Debug, PartialEq, Eq)]
pub enum Pos {
    /// At a subcommand choice; `partial` is the token being typed
    /// (`None` right after a descent, before any narrowing).
    Sub {
        choices: &'static [SubCommand],
        partial: Option<String>,
    },
    /// At an `@agent` token. Partial is sigil-normalized: `@` is
    /// stripped, a bare word passes through, the wrong sigil yields
    /// `None` (no hijacking the other legend's narrowing).
    Agent(Option<String>),
    /// At a `#channel` token; same normalization, `#` expected.
    Group(Option<String>),
    /// At a free token — the hint to render dimmed in the helper.
    Free(&'static str),
    /// Grammar satisfied; nothing more is expected.
    Done,
}

/// Walk `grammar` against the args string (everything after the
/// command name, leading whitespace already trimmed by
/// `slash::parse`). Trailing whitespace is significant: it is the
/// descend gesture — a token followed by a space is completed, a
/// token without one is still being typed at its level.
pub fn walk(grammar: &'static [Token], args: &str) -> Pos {
    let mut toks: Vec<&str> = args.split_whitespace().collect();
    let current: Option<&str> = if !args.is_empty() && !args.ends_with(char::is_whitespace) {
        toks.pop()
    } else {
        None
    };
    walk_tokens(grammar, &toks, current)
}

fn walk_tokens(grammar: &'static [Token], completed: &[&str], current: Option<&str>) -> Pos {
    let mut specs = grammar.iter();
    let mut spec = specs.next();
    let mut i = 0;
    // Consume completed tokens against the grammar sequence.
    while i < completed.len() {
        match spec {
            // More typed than the grammar knows — past the end.
            None => return Pos::Done,
            Some(Token::Subcommands(subs)) => {
                return match subs.iter().find(|s| s.name == completed[i]) {
                    // A completed subcommand descends: its branch
                    // grammar takes over for everything after it.
                    Some(sub) => walk_tokens(sub.grammar, &completed[i + 1..], current),
                    // Unknown completed token: stay at this level with
                    // no narrowing so the user can see valid options —
                    // same posture as an unknown top-level command.
                    None => Pos::Sub {
                        choices: subs,
                        partial: None,
                    },
                };
            }
            // Rest swallows the tail — the position never advances.
            Some(Token::Rest(h)) => return Pos::Free(h),
            Some(Token::Agent { .. } | Token::Group { .. } | Token::Word(_)) => {
                i += 1;
                spec = specs.next();
            }
        }
    }
    // Completed tokens all consumed — `current` sits at `spec`.
    match spec {
        None => Pos::Done,
        Some(Token::Subcommands(subs)) => Pos::Sub {
            choices: subs,
            partial: current.map(str::to_string),
        },
        Some(Token::Agent { .. }) => Pos::Agent(sigil_partial(current, '@', '#')),
        Some(Token::Group { .. }) => Pos::Group(sigil_partial(current, '#', '@')),
        Some(Token::Word(h)) | Some(Token::Rest(h)) => Pos::Free(h),
    }
}

/// Normalize the token being typed at a sigil position: the expected
/// sigil strips off; a bare word narrows as-is (the Rhino model —
/// candidates match without ceremony); the *other* sigil yields no
/// partial rather than narrowing the wrong legend.
fn sigil_partial(current: Option<&str>, expect: char, other: char) -> Option<String> {
    let t = current?;
    if let Some(stripped) = t.strip_prefix(expect) {
        return Some(stripped.to_string());
    }
    if t.starts_with(other) {
        return None;
    }
    Some(t.to_string())
}

/// Render a grammar as the usage tail for help lines — `" <@name>
/// [#channel]"` — leading space per token so it concatenates directly
/// after a command name. Word/Rest hints carry their own brackets.
pub fn signature(grammar: &[Token]) -> String {
    let mut out = String::new();
    for t in grammar {
        out.push(' ');
        match t {
            Token::Agent { required: true } => out.push_str("<@name>"),
            Token::Agent { required: false } => out.push_str("[@name]"),
            Token::Group { required: true } => out.push_str("<#channel>"),
            Token::Group { required: false } => out.push_str("[#channel]"),
            Token::Word(h) | Token::Rest(h) => out.push_str(h),
            Token::Subcommands(subs) => {
                let names: Vec<&str> = subs.iter().map(|s| s.name).collect();
                out.push('[');
                out.push_str(&names.join("|"));
                out.push(']');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    //! The grammar walk's visible specification — the same
    //! table-of-states idiom as `helper`'s tests. Fixtures mirror
    //! the real registry shapes (channels / invite) without
    //! depending on the registry itself.

    use super::*;

    const CREATE: &[Token] = &[Token::Word("<name>"), Token::Rest("[description…]")];
    const DESCRIBE: &[Token] = &[
        Token::Group { required: true },
        Token::Rest("[description…]"),
    ];
    const SUBS: &[SubCommand] = &[
        SubCommand { name: "list", description: "list them", grammar: &[] },
        SubCommand { name: "create", description: "create one", grammar: CREATE },
        SubCommand { name: "describe", description: "describe one", grammar: DESCRIBE },
    ];
    const FAMILY: &[Token] = &[Token::Subcommands(SUBS)];
    const MEMBERSHIP: &[Token] = &[
        Token::Agent { required: true },
        Token::Group { required: false },
    ];

    // ── Top of a grammar ───────────────────────────────────────

    #[test]
    fn empty_grammar_is_done_immediately() {
        assert_eq!(walk(&[], ""), Pos::Done);
        // Stray args past the grammar are past-the-end, not an error.
        assert_eq!(walk(&[], "extra"), Pos::Done);
        assert_eq!(walk(&[], "extra tokens "), Pos::Done);
    }

    #[test]
    fn subcommand_level_narrows_by_prefix() {
        // Right after the command name + space: full choice list.
        assert_eq!(walk(FAMILY, ""), Pos::Sub { choices: SUBS, partial: None });
        // Typing narrows at this level, exactly like the top level.
        assert_eq!(
            walk(FAMILY, "cre"),
            Pos::Sub { choices: SUBS, partial: Some("cre".into()) }
        );
        // A full name without the space is still a partial — descent
        // is the space, not the spelling.
        assert_eq!(
            walk(FAMILY, "create"),
            Pos::Sub { choices: SUBS, partial: Some("create".into()) }
        );
    }

    #[test]
    fn completed_subcommand_descends_into_its_branch() {
        assert_eq!(walk(FAMILY, "create "), Pos::Free("<name>"));
        assert_eq!(walk(FAMILY, "describe "), Pos::Group(None));
        // A branch with an empty grammar is immediately done.
        assert_eq!(walk(FAMILY, "list "), Pos::Done);
    }

    #[test]
    fn unknown_completed_subcommand_stays_at_level_unnarrowed() {
        assert_eq!(walk(FAMILY, "bogus "), Pos::Sub { choices: SUBS, partial: None });
        assert_eq!(walk(FAMILY, "bogus more "), Pos::Sub { choices: SUBS, partial: None });
    }

    // ── Per-level narrowing below a descent ────────────────────

    #[test]
    fn word_token_hints_while_typing_then_advances() {
        assert_eq!(walk(FAMILY, "create pla"), Pos::Free("<name>"));
        assert_eq!(walk(FAMILY, "create plans "), Pos::Free("[description…]"));
    }

    #[test]
    fn rest_token_swallows_the_tail() {
        assert_eq!(walk(FAMILY, "create plans roadmap"), Pos::Free("[description…]"));
        assert_eq!(
            walk(FAMILY, "create plans roadmap talk for q3 "),
            Pos::Free("[description…]")
        );
    }

    #[test]
    fn group_token_narrows_with_and_without_sigil() {
        assert_eq!(walk(DESCRIBE, "#pla"), Pos::Group(Some("pla".into())));
        // Bare word narrows too — the Rhino model, no ceremony.
        assert_eq!(walk(DESCRIBE, "pla"), Pos::Group(Some("pla".into())));
        // Wrong sigil doesn't hijack the channel legend's narrowing.
        assert_eq!(walk(DESCRIBE, "@who"), Pos::Group(None));
        // Bare sigil: committed, nothing typed — empty partial.
        assert_eq!(walk(DESCRIBE, "#"), Pos::Group(Some("".into())));
    }

    #[test]
    fn sequence_walks_agent_then_optional_group() {
        assert_eq!(walk(MEMBERSHIP, ""), Pos::Agent(None));
        assert_eq!(walk(MEMBERSHIP, "@Cl"), Pos::Agent(Some("Cl".into())));
        // Completed agent + space descends to the channel level.
        assert_eq!(walk(MEMBERSHIP, "@Cleo "), Pos::Group(None));
        assert_eq!(walk(MEMBERSHIP, "@Cleo #dep"), Pos::Group(Some("dep".into())));
        // Both consumed: grammar satisfied.
        assert_eq!(walk(MEMBERSHIP, "@Cleo #deploy "), Pos::Done);
        assert_eq!(walk(MEMBERSHIP, "@Cleo #deploy extra"), Pos::Done);
    }

    #[test]
    fn ascent_is_free_because_state_derives_from_the_string() {
        // The backspace path retraces the descend path in reverse —
        // no mode to unwind, just shorter strings.
        assert_eq!(walk(FAMILY, "create plans "), Pos::Free("[description…]"));
        assert_eq!(walk(FAMILY, "create plans"), Pos::Free("<name>"));
        assert_eq!(
            walk(FAMILY, "create"),
            Pos::Sub { choices: SUBS, partial: Some("create".into()) }
        );
        assert_eq!(walk(FAMILY, ""), Pos::Sub { choices: SUBS, partial: None });
    }

    // ── Signatures ─────────────────────────────────────────────

    #[test]
    fn signatures_render_grammar_shapes() {
        assert_eq!(signature(MEMBERSHIP), " <@name> [#channel]");
        assert_eq!(signature(FAMILY), " [list|create|describe]");
        assert_eq!(signature(CREATE), " <name> [description…]");
        assert_eq!(signature(DESCRIBE), " <#channel> [description…]");
        assert_eq!(signature(&[]), "");
    }
}
