//! A command string the shell parses again: the arguments of `eval`, or the
//! operand of `bash -c`. Its text is the words' values joined by spaces, as
//! bash joins `eval`'s arguments, and its spans map back to the source.

use std::ops::Range;

use crate::lex::Token;

pub(crate) struct CommandString {
    /// The string as the shell reads it: quotes and escapes removed.
    pub text: String,
    words: Vec<Word>,
}

struct Word {
    /// Where the word's value starts in `text`.
    at: usize,
    len: usize,
    /// The word's span in the source.
    span: Range<usize>,
    /// Where the value starts in the source, when the source holds it as
    /// written: unquoted, or inside one pair of quotes with no escapes.
    verbatim: Option<usize>,
}

impl CommandString {
    /// The string run from `words`, which is not empty.
    pub fn new(src: &str, words: &[Token]) -> Self {
        let mut text = String::new();
        let mut out = Vec::with_capacity(words.len());
        for (i, w) in words.iter().enumerate() {
            if i > 0 {
                text.push(' ');
            }
            let raw = &src[w.span.clone()];
            let quoted = ['\'', '"']
                .iter()
                .any(|&q| raw.strip_prefix(q).and_then(|r| r.strip_suffix(q)) == Some(&w.value));
            let verbatim = if raw == w.value {
                Some(w.span.start)
            } else if quoted {
                Some(w.span.start + 1)
            } else {
                None
            };
            out.push(Word {
                at: text.len(),
                len: w.value.len(),
                span: w.span.clone(),
                verbatim,
            });
            text.push_str(&w.value);
        }
        CommandString { text, words: out }
    }

    /// The source span of a span in `text`. Where the words it starts and
    /// ends in are verbatim it is exact; otherwise it runs from the start of
    /// the first of those words to the end of the last.
    pub fn source_span(&self, r: &Range<usize>) -> Range<usize> {
        let first = self
            .words
            .iter()
            .rposition(|w| w.at <= r.start)
            .unwrap_or(0);
        let last = self
            .words
            .iter()
            .position(|w| r.end <= w.at + w.len)
            .unwrap_or(self.words.len() - 1);
        let (a, b) = (&self.words[first], &self.words[last]);
        match (a.verbatim, b.verbatim) {
            (Some(sa), Some(sb)) => {
                sa + (r.start - a.at).min(a.len)..sb + r.end.saturating_sub(b.at).min(b.len)
            }
            _ => a.span.start..b.span.end,
        }
    }
}
