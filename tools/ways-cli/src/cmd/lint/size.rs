//! Delivered-size rule: a way's body against the hook context cap.
//!
//! Claude Code caps one hook's `additionalContext` at
//! [`HOOK_CONTEXT_CAP`] characters. Over the cap it saves the text to a file
//! and hands the model the path plus a 2,000-character preview, so the rest of
//! the way never reaches the model. The show path admits the first way of an
//! invocation even when it alone is over the cap, so an oversized way always
//! arrives as a preview. That is a defect, and it is an ERROR here.
//!
//! Every way and check shown in one invocation draws on the same cap, admitted
//! whole in match order. A way near the cap leaves room for little else: a
//! co-matching way larger than the remainder is withheld. That is a WARNING.
//!
//! The measured text is [`static_way_body`], the body `ways show` injects, in
//! the UTF-16 units [`context_chars`] counts. Macro output is produced at fire
//! time and cannot be measured here; it only adds to the total.

use std::fmt::Display;

use crate::cmd::show::{context_chars, static_way_body, HOOK_CONTEXT_CAP};

/// Above this many characters a way leaves co-matching ways less than a fifth
/// of the hook cap.
pub(super) const NEAR_CAP: usize = 8_000;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SizeVerdict {
    Under,
    NearCap,
    OverCap,
}

pub(super) fn classify(chars: usize) -> SizeVerdict {
    if chars > HOOK_CONTEXT_CAP {
        SizeVerdict::OverCap
    } else if chars > NEAR_CAP {
        SizeVerdict::NearCap
    } else {
        SizeVerdict::Under
    }
}

/// The lint line for a body of `chars` characters, or `None` when it is under
/// the warning threshold. Returned as (severity, message) so tests can pin the
/// exact text.
pub(super) fn message(rel: &impl Display, chars: usize) -> Option<(&'static str, String)> {
    const MACRO_NOTE: &str = "Measured on the static body; macro output adds to it at fire time.";
    match classify(chars) {
        SizeVerdict::Under => None,
        SizeVerdict::OverCap => Some((
            "ERROR",
            format!(
                "{rel} — delivered body is {chars} chars, over the {HOOK_CONTEXT_CAP}-char hook \
                 context cap. Claude Code delivers it as a file path and a 2,000-char preview, \
                 so the rest never reaches the model. Split it into child ways (ADR-105). \
                 {MACRO_NOTE}"
            ),
        )),
        SizeVerdict::NearCap => Some((
            "WARNING",
            format!(
                "{rel} — delivered body is {chars} chars, within {left} of the \
                 {HOOK_CONTEXT_CAP}-char hook context cap (warns above {NEAR_CAP}). Ways that \
                 match with it share the cap, and a co-matching way longer than {left} chars \
                 is withheld. Consider splitting it into child ways (ADR-105). {MACRO_NOTE}",
                left = HOOK_CONTEXT_CAP - chars
            ),
        )),
    }
}

/// Measure a way file's delivered body and report it.
pub(super) fn check_body_size(
    rel: &impl Display,
    content: &str,
    errors: &mut u32,
    warnings: &mut u32,
) {
    let chars = context_chars(&static_way_body(content));
    if let Some((severity, msg)) = message(rel, chars) {
        eprintln!("  {severity}: {msg}");
        match severity {
            "ERROR" => *errors += 1,
            _ => *warnings += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn way_with_body(body: &str) -> String {
        format!("---\ndescription: d\nvocabulary: v\n---\n{body}")
    }

    fn counts(content: &str) -> (u32, u32) {
        let (mut e, mut w) = (0, 0);
        check_body_size(&"x/x.md", content, &mut e, &mut w);
        (e, w)
    }

    #[test]
    fn classify_boundaries() {
        assert_eq!(classify(0), SizeVerdict::Under);
        assert_eq!(classify(NEAR_CAP), SizeVerdict::Under);
        assert_eq!(classify(NEAR_CAP + 1), SizeVerdict::NearCap);
        assert_eq!(classify(HOOK_CONTEXT_CAP), SizeVerdict::NearCap);
        assert_eq!(classify(HOOK_CONTEXT_CAP + 1), SizeVerdict::OverCap);
    }

    #[test]
    fn under_is_silent() {
        assert_eq!(counts(&way_with_body(&"a".repeat(NEAR_CAP))), (0, 0));
        assert!(message(&"x", 500).is_none());
    }

    #[test]
    fn near_cap_warns() {
        assert_eq!(counts(&way_with_body(&"a".repeat(NEAR_CAP + 1))), (0, 1));
        let (sev, msg) = message(&"a/b.md", 9_000).unwrap();
        assert_eq!(sev, "WARNING");
        assert!(msg.contains("9000 chars"));
        assert!(msg.contains("within 1000 of the 10000-char"));
        assert!(msg.contains("longer than 1000 chars is withheld"));
        assert!(msg.contains("macro output"));
    }

    #[test]
    fn over_cap_errors() {
        assert_eq!(
            counts(&way_with_body(&"a".repeat(HOOK_CONTEXT_CAP + 1))),
            (1, 0)
        );
        let (sev, msg) = message(&"a/b.md", 14_216).unwrap();
        assert_eq!(sev, "ERROR");
        assert!(msg.contains("14216 chars"));
        assert!(msg.contains("10000-char"));
        assert!(msg.contains("ADR-105"));
        assert!(msg.contains("macro output"));
    }

    #[test]
    fn frontmatter_is_not_counted() {
        // A large frontmatter with a small body is under the threshold: only the
        // delivered body counts.
        let content = format!(
            "---\ndescription: {}\nvocabulary: v\n---\nshort body\n",
            "d".repeat(HOOK_CONTEXT_CAP)
        );
        assert_eq!(counts(&content), (0, 0));
    }

    #[test]
    fn counts_utf16_units() {
        // 5,001 emoji are 5,001 chars but 10,002 UTF-16 units: over the cap.
        assert_eq!(counts(&way_with_body(&"😀".repeat(5_001))), (1, 0));
    }
}
