//! A raw segment into a described [`Segment`]: keywords stripped, program
//! resolved past assignments and wrappers, subcommand and flags read, role
//! and class assigned from the tables.

use std::ops::Range;

use crate::lex::{RawSegment, Token, TokenKind};
use crate::tables::{
    self, ClassInput, FILTERS, IN_SHELL_WRAPPERS, LEAD_KEYWORDS, NAVIGATION, SHELLS,
    SHELL_VALUE_LONG_OPTIONS, SHELL_VALUE_OPTIONS, SKIP_KEYWORDS, WRAPPERS,
};
use crate::{Class, Connector, Role, Segment};

/// The segment, and the words it runs as a command string, if any (see
/// [`command_string`]).
pub(crate) fn build(
    src: &str,
    excluded: &[Range<usize>],
    raw: RawSegment,
) -> Option<(Segment, Option<Vec<Token>>)> {
    let (connector, tokens) = strip_keywords(raw.connector, &raw.tokens)?;
    let span = tokens.first()?.span.start..tokens.last()?.span.end;
    let text = cut(src, &span, excluded);

    let word_tokens: Vec<&Token> = tokens
        .iter()
        .filter(|t| t.kind == TokenKind::Word)
        .collect();
    let words: Vec<&str> = word_tokens.iter().map(|t| t.value.as_str()).collect();
    let redirect = writes_file(tokens);

    let at = program_index(&words);
    let program = words.get(at).map(|w| basename(w)).filter(|p| program_ok(p));
    let Some(program) = program else {
        // A segment that names no program still writes a file by redirect
        // (`> out.txt`, `exec > log`).
        let (role, class) = if redirect {
            (Role::Operative, Some(Class::FileWrite))
        } else {
            (Role::Navigation, None)
        };
        let segment = Segment {
            connector,
            depth: raw.depth,
            role,
            program: None,
            subcommand: None,
            flags: Vec::new(),
            class,
            span,
            text,
            program_at: 0,
        };
        return Some((segment, None));
    };
    let program_at = text_offset(word_tokens[at].span.start, &span, excluded);
    let rest = &words[at + 1..];
    let (subcommand, flags) = subcommand_and_flags(program, rest);

    // A navigation program behind a wrapper runs as a process: `sudo -n
    // true` checks a credential, `sudo mkdir` writes as root. `time` and
    // `builtin` leave it what it was (`time echo hi`, `builtin cd /tmp`).
    let wrapped = words[..at].iter().any(|w| {
        let w = basename(w);
        WRAPPERS.contains(&w) && !IN_SHELL_WRAPPERS.contains(&w)
    });
    // A redirect to a file makes any segment operative: `echo x >>
    // ~/.bashrc` and `… | jq . > out.json` write.
    let role = if redirect {
        Role::Operative
    } else if NAVIGATION.contains(&program) && !wrapped {
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
        let tokens = split_flag_values(program, rest);
        let positional = positional_words(program, rest);
        let class = tables::classify(&ClassInput {
            program,
            tokens: &tokens,
            words: &positional,
            redirect,
        });
        // A redirect makes the class at least a file write: `grep x > out`
        // writes, `cargo test > log` still builds.
        if redirect {
            class.min(Class::FileWrite)
        } else {
            class
        }
    });

    let string = command_string(program, &word_tokens[at + 1..]);
    let segment = Segment {
        connector,
        depth: raw.depth,
        role,
        program: Some(program.to_string()),
        subcommand: (!subcommand.is_empty()).then(|| subcommand.join(" ")),
        flags,
        class,
        span,
        text,
        program_at,
    };
    Some((segment, string))
}

/// The words a program runs as a command string: every argument of `eval`
/// (after a leading `--`), or the first operand of a shell in
/// [`SHELLS`] whose options include `-c`, alone or in a cluster (`-lc`,
/// `-ec`, `-euo pipefail -c`). `None` for any other program, for `eval`
/// alone, and for a shell that runs a script (`bash deploy.sh`) or reads
/// its input (`sh -s`, `bash <<EOF`).
fn command_string(program: &str, args: &[&Token]) -> Option<Vec<Token>> {
    if program == "eval" {
        let args = match args.first() {
            Some(t) if t.value == "--" => &args[1..],
            _ => args,
        };
        return (!args.is_empty()).then(|| args.iter().map(|t| (*t).clone()).collect());
    }
    if !SHELLS.contains(&program) {
        return None;
    }
    // The shell reads its options, then takes its first operand as the
    // string when `-c` was among them.
    let mut dash_c = false;
    let mut i = 0;
    while let Some(v) = args.get(i).map(|t| t.value.as_str()) {
        if v == "--" {
            i += 1;
            break;
        }
        if v.starts_with("--") {
            i += 1 + usize::from(SHELL_VALUE_LONG_OPTIONS.contains(&v));
            continue;
        }
        let Some(cluster) = v.strip_prefix(['-', '+']).filter(|c| !c.is_empty()) else {
            break;
        };
        dash_c |= v.starts_with('-') && cluster.contains('c');
        i += 1 + cluster.matches(SHELL_VALUE_OPTIONS).count();
    }
    let operand = args.get(i).filter(|_| dash_c)?;
    Some(vec![(*operand).clone()])
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

/// The byte offset in the segment's text of source position `pos`: its
/// distance from the span's start, less any excluded range cut before it.
fn text_offset(pos: usize, span: &Range<usize>, excluded: &[Range<usize>]) -> usize {
    let cut_before: usize = excluded
        .iter()
        .map(|r| r.start.max(span.start)..r.end.min(pos))
        .filter(|r| r.start < r.end)
        .map(|r| r.end - r.start)
        .sum();
    pos - span.start - cut_before
}

/// Output goes to a file: `>`, `>>`, `>|`, `&>` or `&>>` to a target other
/// than `/dev/null`. An fd duplication (`2>&1`) is not a file.
fn writes_file(tokens: &[Token]) -> bool {
    tokens.windows(2).any(|w| {
        w[0].kind == TokenKind::RedirectOp
            && matches!(w[0].value.as_str(), ">" | ">>" | ">|" | "&>" | "&>>")
            && !is_stream_target(&w[1].value)
    })
}

/// A redirect target that is a stream, not a file: `/dev/null`,
/// `/dev/stdout`, `/dev/stderr`, `/dev/tty` and `/dev/fd/N`.
fn is_stream_target(target: &str) -> bool {
    matches!(
        target,
        "/dev/null" | "/dev/stdout" | "/dev/stderr" | "/dev/tty"
    ) || target.starts_with("/dev/fd/")
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
/// with its flags, flag values, durations and assignments. `command -v x`
/// looks `x` up without running it, so `command` is the program there.
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
        if wrapper == "command" && words.get(i + 1).is_some_and(|t| matches!(*t, "-v" | "-V")) {
            break;
        }
        i += 1;
        while let Some(t) = words.get(i) {
            if t.starts_with('-') {
                i += if value_follows(wrapper, t, words.get(i + 1)) {
                    2
                } else {
                    1
                };
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
/// throughout, and `make` variable assignments are skipped. `python -m mod`
/// reads `mod` as its subcommand.
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
            if value_follows(program, t, rest.get(j)) {
                j += 1;
            }
            continue;
        }
        if program == "make" && is_assignment(t) {
            continue;
        }
        if words.len() < want && is_word(t) {
            words.push(t.to_string());
            if words.len() == 1 && tables::takes_second_word(program, t) {
                want = 2;
            }
            continue;
        }
        if depth.is_some() {
            want = words.len();
        }
    }
    if matches!(program, "python" | "python3") {
        // `-m` among the interpreter's own flags, before any script.
        let m = rest
            .iter()
            .take_while(|t| t.starts_with('-'))
            .position(|t| *t == "-m");
        if let Some(module) = m.and_then(|m| rest.get(m + 1)) {
            words = vec![module.to_string()];
        }
    }
    (words, flags)
}

/// The flag `t` takes the next word, `next`, as its value: a value flag for
/// the program written without `=`, or a short-flag cluster whose last flag
/// is one (`sudo -iu postgres`), followed by a word that is not a flag. A
/// numeric-value flag takes it only when it is a number (`make -j 4`).
fn value_follows(program: &str, t: &str, next: Option<&&str>) -> bool {
    let Some(next) = next.filter(|n| !n.starts_with('-')) else {
        return false;
    };
    let numeric = || next.parse::<f64>().is_ok();
    if t.starts_with("--") {
        return !t.contains('=')
            && (tables::takes_value(program, t)
                || tables::takes_numeric_value(program, t) && numeric());
    }
    let Some(short) = t.strip_prefix('-') else {
        return false;
    };
    // getopt: the first value flag in a cluster takes the rest of the word,
    // or the next word when it ends the cluster.
    for (i, c) in short.char_indices() {
        let flag = format!("-{c}");
        let last = i + c.len_utf8() == short.len();
        if tables::takes_value(program, &flag) {
            return last;
        }
        if tables::takes_numeric_value(program, &flag) {
            return last && numeric();
        }
    }
    false
}

/// The tokens with each flag split from a value written into it:
/// `--method=DELETE` gives `--method`, `DELETE`; `-XPOST` gives `-X`, `POST`
/// when `-X` is a value flag for the program.
fn split_flag_values<'a>(program: &str, rest: &[&'a str]) -> Vec<&'a str> {
    let mut out = Vec::with_capacity(rest.len());
    for t in rest {
        if t.starts_with("--") {
            if let Some((name, value)) = t.split_once('=') {
                out.extend([name, value]);
                continue;
            }
        } else if let Some(short) = t.strip_prefix('-') {
            let attached = short.char_indices().find_map(|(i, c)| {
                let end = 1 + i + c.len_utf8();
                (tables::takes_value(program, &format!("-{c}")) && end < t.len()).then_some(end)
            });
            if let Some(end) = attached {
                out.extend([&t[..end], &t[end..]]);
                continue;
            }
        }
        out.push(*t);
    }
    out
}

/// Positional words after the program: tokens not starting with `-`, less
/// the values of value flags.
fn positional_words<'a>(program: &str, rest: &[&'a str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut j = 0;
    while let Some(t) = rest.get(j) {
        j += 1;
        if t.starts_with('-') {
            if value_follows(program, t, rest.get(j)) {
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
    fn value_flags_follow_getopt() {
        assert!(value_follows("sudo", "-u", Some(&"root")));
        assert!(value_follows("sudo", "-iu", Some(&"postgres")));
        assert!(!value_follows("sudo", "-uroot", Some(&"psql")));
        assert!(!value_follows("sudo", "-u", Some(&"-i")));
        assert!(value_follows("git", "--git-dir", Some(&"/x/.git")));
        assert!(!value_follows("git", "--git-dir=/x/.git", Some(&"status")));
        assert!(value_follows("make", "-j", Some(&"4")));
        assert!(!value_follows("make", "-j", Some(&"install")));
        assert!(!value_follows("grep", "-rn", Some(&"foo")));
    }

    #[test]
    fn flag_values_are_split_for_the_rules() {
        assert_eq!(
            split_flag_values("gh", &["api", "-XPOST", "x"]),
            ["api", "-X", "POST", "x"]
        );
        assert_eq!(
            split_flag_values("gh", &["api", "--method=DELETE"]),
            ["api", "--method", "DELETE"]
        );
        assert_eq!(split_flag_values("gh", &["-X", "PUT"]), ["-X", "PUT"]);
        assert_eq!(split_flag_values("ls", &["-la"]), ["-la"]);
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
