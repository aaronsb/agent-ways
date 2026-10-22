//! The shell lexer: a command string into raw segments of tokens.
//!
//! It follows the shell's own word rules closely enough to find segment
//! boundaries: quotes and escapes, `$( )`, `$(( ))`, `${ }`, backticks,
//! process substitution, redirections, heredocs, comments, subshells and
//! `[[ ]]`. It never runs or expands anything, and it never fails: an
//! unclosed construct runs to the end of the input.

use std::ops::Range;

use crate::tables::LEAD_KEYWORDS;
use crate::Connector;

/// Substitutions nested deeper than this are skipped, not parsed.
const MAX_DEPTH: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenKind {
    Word,
    /// A redirection operator, normalised without its fd: `>`, `>>`, `<`,
    /// `<<`, `<<-`, `<<<`, `>&`, `<&`, `>|`, `<>`, `&>`, `&>>`.
    RedirectOp,
    /// The word a redirection operator applies to (a heredoc's delimiter
    /// included).
    RedirectTarget,
}

#[derive(Debug, Clone)]
pub(crate) struct Token {
    pub span: Range<usize>,
    /// The word with quotes and escapes removed; substitutions kept raw.
    pub value: String,
    pub kind: TokenKind,
}

#[derive(Debug)]
pub(crate) struct RawSegment {
    pub connector: Connector,
    pub depth: usize,
    pub tokens: Vec<Token>,
}

pub(crate) struct Lexed {
    pub segments: Vec<RawSegment>,
    /// Heredoc bodies, in source order: cut from every segment's text.
    pub excluded: Vec<Range<usize>>,
}

pub(crate) fn lex(src: &str) -> Lexed {
    let mut lexer = Lexer {
        b: src.as_bytes(),
        pos: 0,
        heredocs: Vec::new(),
        excluded: Vec::new(),
    };
    let segments = lexer.list(0, Connector::Start, Term::Eof);
    Lexed {
        segments,
        excluded: lexer.excluded,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Term {
    Eof,
    Paren,
    Backtick,
}

struct Heredoc {
    delim: String,
    strip_tabs: bool,
}

struct Lexer<'a> {
    b: &'a [u8],
    pos: usize,
    /// Heredocs whose bodies start at the next unquoted newline.
    heredocs: Vec<Heredoc>,
    excluded: Vec<Range<usize>>,
}

/// One list's in-progress segment.
struct List {
    depth: usize,
    out: Vec<RawSegment>,
    cur: RawSegment,
    /// Segments of substitutions inside `cur`'s words; they follow `cur`.
    nested: Vec<RawSegment>,
    /// The redirection operator waiting for its target word.
    redirect: Option<&'static str>,
    /// Inside `[[ … ]]`, where `&&`, `||`, `<`, `>` and parens are words.
    dbracket: bool,
    /// The words before the next `)` are a `case` pattern: after `case … in`
    /// or after an arm's `;;`.
    case_pattern: bool,
}

impl List {
    fn new(depth: usize, first: Connector) -> Self {
        List {
            depth,
            out: Vec::new(),
            cur: RawSegment {
                connector: first,
                depth,
                tokens: Vec::new(),
            },
            nested: Vec::new(),
            redirect: None,
            dbracket: false,
            case_pattern: false,
        }
    }

    /// End the current segment; the next one starts with `next`.
    fn flush(&mut self, next: Connector) {
        let cur = std::mem::replace(
            &mut self.cur,
            RawSegment {
                connector: next,
                depth: self.depth,
                tokens: Vec::new(),
            },
        );
        if let Some(first) = cur.tokens.first() {
            self.case_pattern = first.value == "case";
            self.out.push(cur);
        }
        self.out.append(&mut self.nested);
        self.redirect = None;
        self.dbracket = false;
    }

    /// The current words are a `case` pattern, where `|` separates
    /// alternatives and `)` ends the pattern.
    fn in_case_pattern(&self) -> bool {
        match self.cur.tokens.first() {
            Some(t) if t.value == "esac" => false,
            Some(t) if t.value == "case" => true,
            _ => self.case_pattern,
        }
    }

    /// The current segment holds only leading keywords (`do`, `then`, `{`):
    /// a newline after them does not end it.
    fn only_keywords(&self) -> bool {
        self.cur
            .tokens
            .iter()
            .all(|t| t.kind == TokenKind::Word && LEAD_KEYWORDS.contains(&t.value.as_str()))
    }

    fn finish(mut self) -> Vec<RawSegment> {
        self.flush(Connector::Semicolon);
        self.out
    }
}

impl Lexer<'_> {
    fn at(&self, i: usize) -> Option<u8> {
        self.b.get(i).copied()
    }

    fn peek(&self, ahead: usize) -> Option<u8> {
        self.at(self.pos + ahead)
    }

    fn starts_with(&self, s: &[u8]) -> bool {
        self.b[self.pos..].starts_with(s)
    }

    /// Parse a list of segments until `term` (consumed) or the end.
    fn list(&mut self, depth: usize, first: Connector, term: Term) -> Vec<RawSegment> {
        let mut l = List::new(depth, first);
        while let Some(c) = self.peek(0) {
            match c {
                b' ' | b'\t' | b'\r' => self.pos += 1,
                b'\\' if self.peek(1) == Some(b'\n') => self.pos += 2,
                b'\n' => {
                    self.pos += 1;
                    // A newline after `&&`, `|` or a lone `do` continues the
                    // segment; elsewhere it ends one.
                    if !l.only_keywords() {
                        l.flush(Connector::Newline);
                    }
                    self.heredoc_bodies();
                }
                b'#' => {
                    while self.peek(0).is_some_and(|c| c != b'\n') {
                        self.pos += 1;
                    }
                }
                b'&' | b'|' | b'<' | b'>' | b'(' | b')' if l.dbracket => {
                    let start = self.pos;
                    while self.peek(0).is_some_and(|c| b"&|<>()".contains(&c)) {
                        self.pos += 1;
                    }
                    let value = String::from_utf8_lossy(&self.b[start..self.pos]).into_owned();
                    l.cur.tokens.push(Token {
                        span: start..self.pos,
                        value,
                        kind: TokenKind::Word,
                    });
                }
                b';' => {
                    let n = [&b";;&"[..], b";;", b";&"]
                        .iter()
                        .find(|op| self.starts_with(op))
                        .map_or(1, |op| op.len());
                    self.pos += n;
                    l.flush(Connector::Semicolon);
                    if n > 1 {
                        l.case_pattern = true;
                    }
                }
                b'&' => {
                    if self.starts_with(b"&&") {
                        self.pos += 2;
                        l.flush(Connector::And);
                    } else if self.starts_with(b"&>") {
                        self.redirect(&mut l);
                    } else {
                        self.pos += 1;
                        l.flush(Connector::Background);
                    }
                }
                b'|' if l.in_case_pattern() && !self.starts_with(b"||") => {
                    l.cur.tokens.push(Token {
                        span: self.pos..self.pos + 1,
                        value: "|".into(),
                        kind: TokenKind::Word,
                    });
                    self.pos += 1;
                }
                b'|' => {
                    if self.starts_with(b"||") {
                        self.pos += 2;
                        l.flush(Connector::Or);
                    } else {
                        self.pos += if self.starts_with(b"|&") { 2 } else { 1 };
                        l.flush(Connector::Pipe);
                    }
                }
                b'<' | b'>' if self.peek(1) == Some(b'(') => self.word(&mut l, term),
                b'<' | b'>' => self.redirect(&mut l),
                b'(' => self.open_paren(&mut l),
                b')' => {
                    self.pos += 1;
                    if term == Term::Paren {
                        return l.finish();
                    }
                    // A `)` closing a `case` pattern: the pattern is not a
                    // command. Any other stray `)` just ends the segment.
                    if l.in_case_pattern() {
                        l.cur.tokens.clear();
                        l.case_pattern = false;
                    }
                    l.flush(Connector::Semicolon);
                }
                b'`' if term == Term::Backtick => {
                    self.pos += 1;
                    return l.finish();
                }
                _ => self.word(&mut l, term),
            }
        }
        l.finish()
    }

    /// `(` between words: a subshell, an arithmetic command, or part of the
    /// segment (`name()`, `for ((…))`). At a segment's start, or after its
    /// leading keywords, it opens a subshell.
    fn open_paren(&mut self, l: &mut List) {
        // After leading keywords alone (`time (…)`, `! (…)`, `if (…)`) the
        // `(` still opens a subshell; the keywords are stripped later.
        if !l.only_keywords() {
            let start = self.pos;
            let end = self.skip_balanced(start, b'(', b')');
            self.pos = end;
            let raw = String::from_utf8_lossy(&self.b[start..end]).into_owned();
            match l.cur.tokens.last_mut() {
                Some(last) if last.span.end == start || raw == "()" => {
                    last.value.push_str(&raw);
                    last.span.end = end;
                }
                _ => l.cur.tokens.push(Token {
                    span: start..end,
                    value: raw,
                    kind: TokenKind::Word,
                }),
            }
            return;
        }
        if l.in_case_pattern() {
            // A pattern's optional leading `(`: `case x in (a) …`.
            self.pos += 1;
            return;
        }
        if self.peek(1) == Some(b'(') {
            // `(( expr ))`: one word, which names no program.
            let start = self.pos;
            self.pos = self.skip_balanced(start, b'(', b')');
            let value = String::from_utf8_lossy(&self.b[start..self.pos]).into_owned();
            l.cur.tokens.push(Token {
                span: start..self.pos,
                value,
                kind: TokenKind::Word,
            });
            return;
        }
        self.pos += 1;
        let inner = self.sub_list(l.depth, Connector::Subshell, Term::Paren);
        l.out.extend(inner);
    }

    /// Parse a nested list after its opener, or skip it past the depth cap.
    fn sub_list(&mut self, depth: usize, first: Connector, term: Term) -> Vec<RawSegment> {
        if depth + 1 < MAX_DEPTH {
            return self.list(depth + 1, first, term);
        }
        self.pos = match term {
            Term::Paren => self.skip_balanced(self.pos - 1, b'(', b')'),
            Term::Backtick => self.b[self.pos..]
                .iter()
                .position(|&c| c == b'`')
                .map_or(self.b.len(), |i| self.pos + i + 1),
            Term::Eof => self.b.len(),
        };
        Vec::new()
    }

    /// The end (exclusive) of the balanced `open … close` group at `start`,
    /// stepping over quotes and escapes; the input's end when unbalanced.
    fn skip_balanced(&self, start: usize, open: u8, close: u8) -> usize {
        let mut depth = 0usize;
        let mut i = start;
        let mut quote: Option<u8> = None;
        while let Some(c) = self.at(i) {
            match quote {
                Some(q) => {
                    if c == b'\\' && q == b'"' {
                        i += 1;
                    } else if c == q {
                        quote = None;
                    }
                }
                None => {
                    if c == b'\\' {
                        i += 1;
                    } else if c == b'\'' || c == b'"' {
                        quote = Some(c);
                    } else if c == open {
                        depth += 1;
                    } else if c == close {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            return i + 1;
                        }
                    }
                }
            }
            i += 1;
        }
        self.b.len()
    }

    /// A redirection operator, taking an adjacent fd number (`2>`) with it.
    fn redirect(&mut self, l: &mut List) {
        let mut start = self.pos;
        if let Some(last) = l.cur.tokens.last() {
            let fd = &last.value;
            let is_fd = !fd.is_empty() && fd.bytes().all(|c| c.is_ascii_digit())
                || fd.starts_with('{') && fd.ends_with('}');
            if last.kind == TokenKind::Word && last.span.end == start && is_fd {
                start = last.span.start;
                l.cur.tokens.pop();
            }
        }
        const OPS: &[&str] = &[
            "&>>", "&>", "<<<", "<<-", "<<", "<>", "<&", "<", ">>", ">&", ">|", ">",
        ];
        let op = OPS
            .iter()
            .find(|op| self.starts_with(op.as_bytes()))
            .copied()
            .unwrap_or(">");
        self.pos += op.len();
        l.cur.tokens.push(Token {
            span: start..self.pos,
            value: op.to_string(),
            kind: TokenKind::RedirectOp,
        });
        l.redirect = Some(op);
    }

    /// Read the heredoc bodies queued on the line just ended, and record
    /// each as excluded text.
    fn heredoc_bodies(&mut self) {
        for doc in std::mem::take(&mut self.heredocs) {
            let start = self.pos;
            let mut line_start = self.pos;
            let end = loop {
                if line_start >= self.b.len() {
                    break self.b.len();
                }
                let line_end = self.b[line_start..]
                    .iter()
                    .position(|&c| c == b'\n')
                    .map_or(self.b.len(), |i| line_start + i);
                let line = String::from_utf8_lossy(&self.b[line_start..line_end]);
                let line = if doc.strip_tabs {
                    line.trim_start_matches('\t')
                } else {
                    &line
                };
                if line.trim() == doc.delim {
                    break (line_end + 1).min(self.b.len());
                }
                // `EOF)` closing a `$(cat <<EOF …` on the delimiter's line:
                // the body ends at the delimiter and the `)` is read.
                let lead = line.len() - line.trim_start().len();
                let trimmed = line.trim_start();
                if let Some(after) = trimmed.strip_prefix(doc.delim.as_str()) {
                    if after.trim_start().starts_with(')') {
                        let line_len = self.b[line_start..line_end].len();
                        let shown = line.len();
                        break line_start + (line_len - shown) + lead + doc.delim.len();
                    }
                }
                line_start = line_end + 1;
            };
            self.excluded.push(start..end);
            self.pos = end;
        }
    }

    /// One word, with any substitutions inside it parsed into `l.nested`.
    fn word(&mut self, l: &mut List, term: Term) {
        let start = self.pos;
        let mut val: Vec<u8> = Vec::new();
        while let Some(c) = self.peek(0) {
            match c {
                b' ' | b'\t' | b'\r' | b'\n' | b';' | b'&' | b'|' | b')' => break,
                b'<' | b'>' => {
                    if self.pos == start && self.peek(1) == Some(b'(') {
                        // Process substitution.
                        self.pos += 2;
                        let inner =
                            self.sub_list(l.depth, Connector::CommandSubstitution, Term::Paren);
                        l.nested.extend(inner);
                        val.extend_from_slice(&self.b[start..self.pos]);
                        continue;
                    }
                    break;
                }
                b'(' => {
                    // `arr=( … )` and extglob `@( … )` stay in the word.
                    let prev = val.last().copied();
                    if matches!(prev, Some(b'=' | b'@' | b'?' | b'*' | b'+' | b'!')) {
                        let end = self.skip_balanced(self.pos, b'(', b')');
                        val.extend_from_slice(&self.b[self.pos..end]);
                        self.pos = end;
                        continue;
                    }
                    break;
                }
                b'`' => {
                    if term == Term::Backtick {
                        break;
                    }
                    self.backtick(l, &mut val);
                }
                b'\\' => self.escape(&mut val),
                b'\'' => {
                    let close = self.b[self.pos + 1..].iter().position(|&c| c == b'\'');
                    let end = close.map_or(self.b.len(), |i| self.pos + 1 + i);
                    val.extend_from_slice(&self.b[self.pos + 1..end]);
                    self.pos = (end + 1).min(self.b.len());
                }
                b'"' => self.double_quoted(l, term, &mut val),
                b'$' => self.dollar(l, &mut val),
                _ => {
                    val.push(c);
                    self.pos += 1;
                }
            }
        }
        if self.pos == start {
            // A character no rule consumes; step over it.
            self.pos += 1;
            return;
        }
        let value = String::from_utf8_lossy(&val).into_owned();
        let kind = match l.redirect.take() {
            Some(op) => {
                if op == "<<" || op == "<<-" {
                    self.heredocs.push(Heredoc {
                        delim: value.clone(),
                        strip_tabs: op == "<<-",
                    });
                }
                TokenKind::RedirectTarget
            }
            None => TokenKind::Word,
        };
        if kind == TokenKind::Word {
            match value.as_str() {
                "[[" if l
                    .cur
                    .tokens
                    .iter()
                    .all(|t| LEAD_KEYWORDS.contains(&t.value.as_str())) =>
                {
                    l.dbracket = true
                }
                "]]" => l.dbracket = false,
                _ => {}
            }
        }
        l.cur.tokens.push(Token {
            span: start..self.pos,
            value,
            kind,
        });
    }

    fn escape(&mut self, val: &mut Vec<u8>) {
        match self.peek(1) {
            Some(b'\n') => self.pos += 2,
            Some(c) => {
                val.push(c);
                self.pos += 2;
            }
            None => self.pos += 1,
        }
    }

    fn backtick(&mut self, l: &mut List, val: &mut Vec<u8>) {
        let start = self.pos;
        self.pos += 1;
        let inner = self.sub_list(l.depth, Connector::CommandSubstitution, Term::Backtick);
        l.nested.extend(inner);
        val.extend_from_slice(&self.b[start..self.pos]);
    }

    fn double_quoted(&mut self, l: &mut List, term: Term, val: &mut Vec<u8>) {
        self.pos += 1;
        while let Some(c) = self.peek(0) {
            match c {
                b'"' => {
                    self.pos += 1;
                    return;
                }
                b'\\' => match self.peek(1) {
                    Some(b'\n') => self.pos += 2,
                    Some(n @ (b'$' | b'`' | b'"' | b'\\')) => {
                        val.push(n);
                        self.pos += 2;
                    }
                    _ => {
                        val.push(b'\\');
                        self.pos += 1;
                    }
                },
                b'$' => self.dollar(l, val),
                b'`' if term == Term::Backtick => return,
                b'`' => self.backtick(l, val),
                _ => {
                    val.push(c);
                    self.pos += 1;
                }
            }
        }
    }

    /// `$( … )`, `$(( … ))`, `${ … }`, `$'…'`, `$"…"`, or a plain `$`.
    fn dollar(&mut self, l: &mut List, val: &mut Vec<u8>) {
        let start = self.pos;
        match self.peek(1) {
            Some(b'(') if self.peek(2) == Some(b'(') => {
                self.pos = self.skip_balanced(start + 1, b'(', b')');
            }
            Some(b'(') => {
                self.pos += 2;
                let inner = self.sub_list(l.depth, Connector::CommandSubstitution, Term::Paren);
                l.nested.extend(inner);
            }
            Some(b'{') => {
                self.pos = self.skip_balanced(start + 1, b'{', b'}');
            }
            Some(b'\'') => {
                self.pos += 2;
                while let Some(c) = self.peek(0) {
                    self.pos += if c == b'\\' { 2 } else { 1 };
                    if c == b'\'' {
                        break;
                    }
                    val.push(c);
                }
                self.pos = self.pos.min(self.b.len());
                return;
            }
            Some(b'"') => {
                self.pos += 1;
                return;
            }
            _ => {
                val.push(b'$');
                self.pos += 1;
                return;
            }
        }
        val.extend_from_slice(&self.b[start..self.pos]);
    }
}
