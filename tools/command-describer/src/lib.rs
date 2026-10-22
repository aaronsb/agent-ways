//! The command describer of ADR-188 item 4: one shell command string in,
//! its ordered segments out.
//!
//! A Bash call is often several tools joined by connectors: `cd x && make
//! test`, `git log | grep fix | head`, `git push && gh pr create --body
//! "$(cat <<'EOF' … EOF)"`. [`parse`] splits the string the way the shell
//! would, without running anything, and describes each simple command it
//! finds as a [`Segment`]:
//!
//! - the [`Connector`] that joins it to what came before;
//! - a [`Role`]: navigation (`cd`, `export`, `echo`, an assignment), filter
//!   (the consumer side of a pipe: `grep`, `head`, `sort`, `jq`, …) or
//!   operative (everything else);
//! - the program, after `FOO=1`, `sudo`, `env`, `timeout N`, `nohup` and the
//!   other wrappers in [`tables::WRAPPERS`];
//! - the subcommand, for programs that have one ([`tables::SUBCOMMAND_DEPTH`]);
//! - the flags;
//! - for an operative segment, a [`Class`] from the ordered rules in
//!   [`tables::CLASS_RULES`].
//!
//! Quoting is respected, heredoc bodies are dropped, and the bodies of
//! subshells, `$( )`, backticks, process substitutions and loops become
//! segments of their own. [`operative_texts`] gives each operative segment's
//! own text, which the `commands:` lane matches its regexes against
//! (ADR-188 item 3a).
//!
//! The rules are ported from the Bash-surface study behind ADR-707 (its
//! segmenter and class tables); the tables live in [`tables`] as data so
//! later slices can extend them. This crate knows nothing of ways,
//! embeddings, the judge or hook events.

mod classify;
mod lex;
// Laid out by hand: one rule or list per block reads as data.
#[rustfmt::skip]
pub mod tables;

use std::ops::Range;

/// What joins a segment to the one before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Connector {
    /// The first segment of the command.
    Start,
    /// `;` (and a `case` arm's `;;`).
    Semicolon,
    /// `&&`.
    And,
    /// `||`.
    Or,
    /// `|` or `|&`.
    Pipe,
    /// `&`: the segment before ran in the background.
    Background,
    /// An unquoted newline.
    Newline,
    /// The first segment inside `( … )`.
    Subshell,
    /// The first segment inside `$( … )`, backticks, `<( … )` or `>( … )`.
    CommandSubstitution,
    /// The first segment of a loop body, after `do`.
    LoopBody,
}

impl Connector {
    /// A short stable label: `start`, `;`, `&&`, `||`, `|`, `&`, `newline`,
    /// `subshell`, `$()`, `do`.
    pub fn as_str(self) -> &'static str {
        match self {
            Connector::Start => "start",
            Connector::Semicolon => ";",
            Connector::And => "&&",
            Connector::Or => "||",
            Connector::Pipe => "|",
            Connector::Background => "&",
            Connector::Newline => "newline",
            Connector::Subshell => "subshell",
            Connector::CommandSubstitution => "$()",
            Connector::LoopBody => "do",
        }
    }
}

/// What a segment does within the call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// Moves or sets up the shell: `cd`, `pushd`, `export`, `echo`, `sleep`,
    /// an assignment alone, or a word that is not a program name.
    Navigation,
    /// A filter program reading a pipe: `grep`, `head`, `sort`, `jq`, …
    /// (`sed -i` is never a filter).
    Filter,
    /// Everything else: the commands that carry the call's intent.
    Operative,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Navigation => "navigation",
            Role::Filter => "filter",
            Role::Operative => "operative",
        }
    }
}

/// The verb class of an operative segment, most consequential first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Class {
    /// Reaches outside the machine: `git push`, `gh pr create`, `gh api -X POST`.
    Publish,
    /// Changes repository state: `git commit`, `git checkout`, `git stash`.
    VcsWrite,
    /// Starts, stops or signals a service or process.
    Service,
    /// Installs or removes packages.
    Package,
    /// Builds, tests or lints: `cargo test`, `make`, `pytest`.
    BuildTest,
    /// Writes files: `cp`, `rm`, `sed -i`, `tee`, a redirect.
    FileWrite,
    /// Reads repository state: `git status`, `git log`, `gh pr view`.
    VcsRead,
    /// Searches: `grep`, `rg`, `find`, `fd`.
    Search,
    /// Reads or inspects: `cat`, `ls`, `ps`, `systemctl status`.
    ReadInspect,
    /// Anything the rules do not name.
    Other,
}

impl Class {
    /// Every class, most consequential first.
    pub const ALL: [Class; 10] = [
        Class::Publish,
        Class::VcsWrite,
        Class::Service,
        Class::Package,
        Class::BuildTest,
        Class::FileWrite,
        Class::VcsRead,
        Class::Search,
        Class::ReadInspect,
        Class::Other,
    ];

    /// The six classes that change something; the study's action classes.
    pub fn is_action(self) -> bool {
        self <= Class::FileWrite
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Class::Publish => "publish",
            Class::VcsWrite => "vcs-write",
            Class::Service => "service",
            Class::Package => "package",
            Class::BuildTest => "build-test",
            Class::FileWrite => "file-write",
            Class::VcsRead => "vcs-read",
            Class::Search => "search",
            Class::ReadInspect => "read-inspect",
            Class::Other => "other",
        }
    }
}

/// One simple command within a shell command string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// What joins this segment to the one before it.
    pub connector: Connector,
    /// Nesting depth: 0 at top level, one more inside each subshell or
    /// substitution.
    pub depth: usize,
    pub role: Role,
    /// The program's base name (`/usr/bin/git` gives `git`), after
    /// assignments and wrappers. `None` when the segment runs no program
    /// name: an assignment alone, `$CMD args`, `(( i++ ))`.
    pub program: Option<String>,
    /// The subcommand words, space-joined: `pr create` for `gh pr create`,
    /// `test` for `make test`, `stash pop` for `git stash pop`.
    pub subcommand: Option<String>,
    /// The flags after the program, in order, each without an `=value`.
    pub flags: Vec<String>,
    /// The verb class; set only for an operative segment.
    pub class: Option<Class>,
    /// Byte range in the original command, from the segment's first word
    /// (shell keywords such as `do` or `then` excluded) to its last.
    pub span: Range<usize>,
    /// The original text over `span`, with any heredoc body inside it cut out.
    pub text: String,
}

impl Segment {
    pub fn is_operative(&self) -> bool {
        self.role == Role::Operative
    }

    /// Program, subcommand and flags, space-joined, with arguments and paths
    /// dropped: the segment's verb, as ADR-188 item 3b's matcher reads it.
    pub fn verb(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if let Some(p) = &self.program {
            parts.push(p);
        }
        if let Some(s) = &self.subcommand {
            parts.push(s);
        }
        parts.extend(self.flags.iter().map(String::as_str));
        parts.join(" ")
    }
}

/// Split a shell command string into its ordered segments.
///
/// Segments come in source order; the segments of a substitution follow the
/// segment whose word contains it. Shell keywords (`if`, `then`, `do`,
/// `done`, `for … in …`, `case`, braces) are not segments; a loop body's
/// first segment carries [`Connector::LoopBody`]. Malformed input (an
/// unclosed quote or parenthesis) never fails: the rest of the string is
/// read as far as it goes.
pub fn parse(command: &str) -> Vec<Segment> {
    let lexed = lex::lex(command);
    lexed
        .segments
        .into_iter()
        .filter_map(|raw| classify::build(command, &lexed.excluded, raw))
        .collect()
}

/// Each operative segment's own text, in order: what the `commands:` lane
/// matches its regexes against (ADR-188 item 3a).
pub fn operative_texts(segments: &[Segment]) -> Vec<&str> {
    segments
        .iter()
        .filter(|s| s.is_operative())
        .map(|s| s.text.as_str())
        .collect()
}
