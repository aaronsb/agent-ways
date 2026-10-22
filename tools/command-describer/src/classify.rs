//! A raw segment into a described [`Segment`]: keywords stripped, program
//! resolved past assignments and wrappers, subcommand and flags read, role
//! and class assigned from the tables.

use std::ops::Range;

use crate::lex::{RawSegment, Token, TokenKind};
use crate::tables::{
    self, ClassInput, FILTERS, GIT_SECOND_WORD, LEAD_KEYWORDS, NAVIGATION, SKIP_KEYWORDS, WRAPPERS,
};
use crate::{Connector, Role, Segment};

pub(crate) fn build(src: &str, excluded: &[Range<usize>], raw: RawSegment) -> Option<Segment> {
    let (connector, tokens) = strip_keywords(raw.connector, &raw.tokens)?;
    let span = tokens.first()?.span.start..tokens.last()?.span.end;
    let text = cut(src, &span, excluded);

    let words: Vec<&str> = tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Word)
        .map(|t| t.value.as_str())
        .collect();
    let redirect = writes_file(tokens);

    let at = program_index(&words);
    let program = words.get(at).map(|w| basename(w)).filter(|p| program_ok(p));
    let Some(program) = program else {
        return Some(Segment {
            connector,
            depth: raw.depth,
            role: Role::Navigation,
            program: None,
            subcommand: None,
            flags: Vec::new(),
            class: None,
            span,
            text,
        });
    };
    let rest = &words[at + 1..];
    let (subcommand, flags) = subcommand_and_flags(program, rest);

    let role = if NAVIGATION.contains(&program) {
        Role::Navigation
    } else if connector == Connector::Pipe
        && FILTERS.contains(&program)
        && !(program == "sed" && sed_in_place(rest))
    {
        Role::Filter
    } else {
        Role::Operative
    };
    let class = (role == Role::Operative).then(|| {
        let positional = positional_words(program, rest);
        tables::classify(&ClassInput {
            program,
            tokens: rest,
            words: &positional,
            redirect,
        })
    });

    Some(Segment {
        connector,
        depth: raw.depth,
        role,
        program: Some(program.to_string()),
        subcommand: (!subcommand.is_empty()).then(|| subcommand.join(" ")),
        flags,
        class,
        span,
        text,
    })
}

/// Drop leading shell keywords and function headers; `None` when what is
/// left is structure (`for … in`, `done`, `esac`) or nothing.
fn strip_keywords(mut connector: Connector, tokens: &[Token]) -> Option<(Connector, &[Token])> {
    let mut i = 0;
    while let Some(t) = tokens.get(i) {
        if t.kind != TokenKind::Word {
            break;
        }
        let v = t.value.as_str();
        if LEAD_KEYWORDS.contains(&v) {
            if v == "do" {
                connector = Connector::LoopBody;
            }
            i += 1;
        } else if v == "function" {
            // `function name [()] {`
            i += 2;
            if tokens.get(i).is_some_and(|t| t.value == "()") {
                i += 1;
            }
        } else if v.len() > 2 && v.ends_with("()") {
            i += 1;
        } else {
            break;
        }
    }
    let rest = tokens.get(i..).filter(|r| !r.is_empty())?;
    if rest[0].kind == TokenKind::Word && SKIP_KEYWORDS.contains(&rest[0].value.as_str()) {
        return None;
    }
    Some((connector, rest))
}

/// `src[span]` with any excluded range (a heredoc body) cut out.
fn cut(src: &str, span: &Range<usize>, excluded: &[Range<usize>]) -> String {
    let mut text = String::new();
    let mut p = span.start;
    for r in excluded {
        if r.end <= p || r.start >= span.end {
            continue;
        }
        text.push_str(&src[p..r.start.max(p)]);
        p = r.end.min(span.end);
    }
    text.push_str(&src[p..span.end]);
    text
}

/// Output goes to a file: `>`, `>>`, `>|`, `&>` or `&>>` to a target other
/// than `/dev/null`. An fd duplication (`2>&1`) is not a file.
fn writes_file(tokens: &[Token]) -> bool {
    tokens.windows(2).any(|w| {
        w[0].kind == TokenKind::RedirectOp
            && matches!(w[0].value.as_str(), ">" | ">>" | ">|" | "&>" | "&>>")
            && w[1].value != "/dev/null"
    })
}

fn is_assignment(w: &str) -> bool {
    let Some(eq) = w.find('=') else { return false };
    let name = w[..eq].strip_suffix('+').unwrap_or(&w[..eq]);
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_duration(w: &str) -> bool {
    let digits = w.strip_suffix(['s', 'm', 'h', 'd']).unwrap_or(w);
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// The index of the program: past leading assignments and each wrapper
/// with its flags, flag values, durations and assignments.
fn program_index(words: &[&str]) -> usize {
    let mut i = 0;
    while words.get(i).is_some_and(|w| is_assignment(w)) {
        i += 1;
    }
    while let Some(w) = words.get(i) {
        let wrapper = basename(w);
        if !WRAPPERS.contains(&wrapper) {
            break;
        }
        i += 1;
        while let Some(t) = words.get(i) {
            if t.starts_with('-') {
                let takes = tables::takes_value(wrapper, t) && !t.contains('=');
                i += if takes { 2 } else { 1 };
            } else if is_duration(t) || is_assignment(t) {
                i += 1;
            } else {
                break;
            }
        }
    }
    i
}

fn basename(w: &str) -> &str {
    w.rsplit('/').next().unwrap_or(w)
}

/// A program name: a letter, then letters, digits and `_.+-`.
fn program_ok(p: &str) -> bool {
    let mut chars = p.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || "_.+-".contains(c))
}

/// A subcommand word: lowercase letter, then lowercase, digits and `-`.
fn is_word(t: &str) -> bool {
    let mut chars = t.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The flag's name when `t` is a flag: `-x`, `-rn`, `-j4`, `--long`,
/// `--long=value` (named `--long`), or a cargo `+toolchain`.
fn flag_name<'a>(program: &str, t: &'a str) -> Option<&'a str> {
    let alnum = |s: &str| s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if let Some(long) = t.strip_prefix("--") {
        let name = long.split('=').next().unwrap_or(long);
        let ok = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && alnum(name);
        return ok.then(|| &t[..2 + name.len()]);
    }
    if let Some(short) = t.strip_prefix('-') {
        let ok = short
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
            && short.chars().all(|c| c.is_ascii_alphanumeric());
        return ok.then_some(t);
    }
    if program == "cargo" && t.len() > 1 && t.starts_with('+') {
        return Some(t);
    }
    None
}

/// Subcommand words and flags. A program with subcommands keeps up to its
/// depth of leading words and stops at its first argument; flags are read
/// throughout. `python -m mod` reads `mod` as its subcommand.
fn subcommand_and_flags(program: &str, rest: &[&str]) -> (Vec<String>, Vec<String>) {
    let depth = tables::subcommand_depth(program);
    let mut want = depth.unwrap_or(0);
    let mut words: Vec<String> = Vec::new();
    let mut flags: Vec<String> = Vec::new();
    let mut j = 0;
    while let Some(t) = rest.get(j) {
        j += 1;
        if let Some(name) = flag_name(program, t) {
            flags.push(name.to_string());
            if takes_value_here(program, name, t, rest.get(j)) {
                j += 1;
            }
            continue;
        }
        if words.len() < want && is_word(t) {
            words.push(t.to_string());
            if program == "git" && words.len() == 1 && GIT_SECOND_WORD.contains(t) {
                want = 2;
            }
            continue;
        }
        if depth.is_some() {
            want = words.len();
        }
    }
    if matches!(program, "python" | "python3") && rest.first() == Some(&"-m") {
        if let Some(module) = rest.get(1) {
            words = vec![module.to_string()];
        }
    }
    (words, flags)
}

/// The flag takes the next word as its value here: a value flag for the
/// program, written without `=`, followed by a word that is not a flag.
fn takes_value_here(program: &str, name: &str, raw: &str, next: Option<&&str>) -> bool {
    tables::takes_value(program, name)
        && !raw.contains('=')
        && next.is_some_and(|n| !n.starts_with('-'))
}

/// Positional words after the program: tokens not starting with `-`, less
/// the values of value flags.
fn positional_words<'a>(program: &str, rest: &[&'a str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut j = 0;
    while let Some(t) = rest.get(j) {
        j += 1;
        if t.starts_with('-') {
            let name = t.split('=').next().unwrap_or(t);
            if takes_value_here(program, name, t, rest.get(j)) {
                j += 1;
            }
        } else {
            out.push(*t);
        }
    }
    out
}

fn sed_in_place(rest: &[&str]) -> bool {
    rest.iter()
        .any(|t| t.starts_with("-i") || t.starts_with("--in-place"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assignment_shapes() {
        assert!(is_assignment("FOO=1"));
        assert!(is_assignment("_x=a b"));
        assert!(is_assignment("PATH+=:/x"));
        assert!(!is_assignment("--flag=1"));
        assert!(!is_assignment("1X=2"));
        assert!(!is_assignment("cargo"));
    }

    #[test]
    fn program_index_skips_assignments_and_wrappers() {
        assert_eq!(program_index(&["FOO=1", "BAR=2", "cargo", "test"]), 2);
        assert_eq!(
            program_index(&["sudo", "-u", "root", "systemctl", "restart"]),
            3
        );
        assert_eq!(program_index(&["timeout", "-s", "KILL", "30s", "cargo"]), 4);
        assert_eq!(program_index(&["env", "-u", "X", "A=1", "make"]), 4);
        assert_eq!(program_index(&["nohup", "nice", "-n", "10", "./run.sh"]), 4);
        assert_eq!(program_index(&["xargs", "-I", "{}", "rm", "{}"]), 3);
    }

    #[test]
    fn flag_names() {
        assert_eq!(flag_name("ls", "-la"), Some("-la"));
        assert_eq!(flag_name("make", "-j4"), Some("-j4"));
        assert_eq!(flag_name("gh", "--title=x y"), Some("--title"));
        assert_eq!(flag_name("ls", "-1"), None);
        assert_eq!(flag_name("ls", "--"), None);
        assert_eq!(flag_name("cargo", "+nightly"), Some("+nightly"));
        assert_eq!(flag_name("ls", "+x"), None);
    }

    #[test]
    fn subcommand_depths() {
        let (w, f) = subcommand_and_flags("gh", &["pr", "create", "--title", "x"]);
        assert_eq!(
            (w, f),
            (
                vec!["pr".to_string(), "create".to_string()],
                vec!["--title".to_string()]
            )
        );
        let (w, _) = subcommand_and_flags("git", &["stash", "pop"]);
        assert_eq!(w, ["stash", "pop"]);
        let (w, _) = subcommand_and_flags("git", &["branch", "feature"]);
        assert_eq!(w, ["branch"]);
        let (w, _) = subcommand_and_flags("git", &["-C", "tools", "status"]);
        assert_eq!(w, ["status"]);
        let (w, _) = subcommand_and_flags("make", &["-C", "tools", "test"]);
        assert_eq!(w, ["test"]);
        let (w, _) = subcommand_and_flags("gh", &["api", "repos/x/y", "pulls"]);
        assert_eq!(w, ["api"]);
        let (w, _) = subcommand_and_flags("python3", &["-m", "pytest", "-q"]);
        assert_eq!(w, ["pytest"]);
        let (w, f) = subcommand_and_flags("grep", &["-rn", "foo", "src"]);
        assert!(w.is_empty());
        assert_eq!(f, ["-rn"]);
    }

    #[test]
    fn cut_removes_excluded_ranges() {
        let src = "aaa BODY bbb XX ccc";
        let excluded = [4..9, 13..16];
        assert_eq!(cut(src, &(0..19), &excluded), "aaa bbb ccc");
        assert_eq!(cut(src, &(0..3), &excluded), "aaa");
        assert_eq!(cut(src, &(6..12), &excluded), "bbb");
        assert_eq!(cut(src, &(10..19), &excluded), "bb ccc");
    }
}
