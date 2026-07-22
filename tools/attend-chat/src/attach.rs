//! Path attachments (#390): a token that is an existing absolute
//! file path renders as an attachment chip — in the compose box and
//! in message cells.
//!
//! On the local-first bus (ADR-136) both ends share a filesystem, so
//! the path IS the attachment: no blob transport, and the wire body
//! carries the path as plain text — old clients and `attend inbox`
//! degrade to showing the path. Terminals deliver drag-drop as pasted
//! text (typically a shell-quoted absolute path), which is why the
//! tokenizer speaks shell-paste: quoted runs and backslash-escaped
//! spaces.
//!
//! Degradation is one-way and silent: a file deleted or moved after
//! send simply loses its chip while the raw path stays in the body —
//! a cell never errors on a stale path.

use std::path::Path;

use agent_identity::TermCaps;
use iocraft::prelude::*;

/// Tokenize `text` shell-paste style and keep the absolute-path
/// candidates, unquoted/unescaped. Pure string work — existence is
/// the caller's concern ([`existing_attachments`] for render sites,
/// an injected set in tests) so detection stays fs-free and fast on
/// ordinary chat lines.
pub fn candidate_paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
            continue;
        }
        let mut tok = String::new();
        if c == '\'' || c == '"' {
            // Quoted run: everything to the matching quote is one
            // token, spaces included — the bracketed-paste shape most
            // terminals produce for a dragged file.
            let quote = c;
            chars.next();
            for ch in chars.by_ref() {
                if ch == quote {
                    break;
                }
                tok.push(ch);
            }
        } else {
            while let Some(&ch) = chars.peek() {
                if ch.is_whitespace() {
                    break;
                }
                if ch == '\\' {
                    // Backslash escape: `my\ file.png` — take the
                    // escaped char literally.
                    chars.next();
                    if let Some(&esc) = chars.peek() {
                        tok.push(esc);
                        chars.next();
                    }
                } else {
                    tok.push(ch);
                    chars.next();
                }
            }
        }
        if tok.starts_with('/') {
            out.push(tok);
        }
    }
    out
}

/// Candidates that are files on disk right now. The fs probe runs
/// only on absolute-path tokens, so messages without a `/`-leading
/// token cost nothing beyond the scan.
pub fn existing_attachments(text: &str) -> Vec<String> {
    candidate_paths(text)
        .into_iter()
        .filter(|p| Path::new(p).is_file())
        .collect()
}

/// Render one flat chip per attachment: glyph + basename, inverse on
/// dark grey so a file reads as an object, not body text. Only
/// existing files reach here (the caller filters), which is the
/// never-error degradation: a vanished file loses its chip, nothing
/// more.
pub fn attachment_chips(paths: &[String], caps: TermCaps) -> Vec<AnyElement<'static>> {
    let glyph = match caps {
        TermCaps::Rich => "⎘ ",
        TermCaps::Basic | TermCaps::Mono => "file: ",
    };
    paths
        .iter()
        .map(|p| {
            let name = Path::new(p)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(p.as_str());
            let content = format!(" {glyph}{name} ");
            element! {
                View(background_color: Color::DarkGrey, margin_right: 1) {
                    Text(color: Color::White, content, wrap: TextWrap::NoWrap)
                }
            }
            .into_any()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_has_no_candidates() {
        assert!(candidate_paths("hello world").is_empty());
        assert!(candidate_paths("").is_empty());
        // Relative paths and bare filenames are not attachments.
        assert!(candidate_paths("see foo/bar.png").is_empty());
        assert!(candidate_paths("shot.png").is_empty());
    }

    #[test]
    fn bare_absolute_token_is_a_candidate() {
        assert_eq!(candidate_paths("/tmp/shot.png"), vec!["/tmp/shot.png"]);
        // Position doesn't matter — mid-message paths count too.
        assert_eq!(
            candidate_paths("take a look at /tmp/shot.png please"),
            vec!["/tmp/shot.png"]
        );
    }

    #[test]
    fn quoted_paths_keep_their_spaces() {
        // The drag-drop shape: terminals paste a shell-quoted path.
        assert_eq!(
            candidate_paths("'/tmp/my shot.png'"),
            vec!["/tmp/my shot.png"]
        );
        assert_eq!(
            candidate_paths("\"/tmp/my shot.png\" and text"),
            vec!["/tmp/my shot.png"]
        );
    }

    #[test]
    fn backslash_escaped_spaces_join_the_token() {
        assert_eq!(
            candidate_paths(r"/tmp/my\ shot.png"),
            vec!["/tmp/my shot.png"]
        );
    }

    #[test]
    fn multiple_candidates_in_order() {
        assert_eq!(
            candidate_paths("/a/one.png then '/b/two three.png'"),
            vec!["/a/one.png", "/b/two three.png"]
        );
    }

    #[test]
    fn quoted_non_path_is_ignored() {
        assert!(candidate_paths("'just a quote'").is_empty());
    }

    #[test]
    fn existing_filter_degrades_to_nothing_when_file_vanishes() {
        // The #390 degradation contract end to end: chip while the
        // file exists, silently none once it's gone — never an error.
        let dir = std::env::temp_dir().join(format!(
            "attend-chat-attach-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("shot.png");
        std::fs::write(&file, b"png").unwrap();
        let msg = format!("screenshot: {}", file.display());
        assert_eq!(existing_attachments(&msg), vec![file.display().to_string()]);
        // Directories are not attachments.
        assert!(existing_attachments(&format!("see {}", dir.display())).is_empty());
        std::fs::remove_file(&file).unwrap();
        assert!(existing_attachments(&msg).is_empty());
    }
}
