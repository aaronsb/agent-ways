//! The raw-colour lint (ADR-504 §6): a raw SGR escape literal in a Rust
//! source under `tools/` outside `agent-theme` is a finding. Colour goes through agent-theme's painter, which honours the
//! theme, the terminal's depth and `NO_COLOR`; a literal does none of that.
//!
//! The second test plants a literal in a fixture tree and requires the scan
//! to report it, and the first requires the real scan to have visited more
//! than 100 files including a known one, so a scan that silently matches or
//! reads nothing cannot pass.
//!
//! A CSI with a private-mode prefix (`<`, `>`, `?`, `=`) is a terminal mode
//! control, not colour, and is not a finding.
//!
//! Known gaps: the scan matches one literal per needle, so an escape built
//! from pieces gets past it: `"\x1b"` followed by `"["`, a `'\x1b'` char with
//! `[` pushed after it, `char::from(27)`, or the 8-bit CSI `\u{9b}`. The
//! `'\x1b'` chars in the tree today are ANSI parsers that measure or strip
//! escapes (agent-fmt's width module, markdown) and a width test's erase-line
//! fixture, which is legitimate.

use std::path::{Path, PathBuf};

/// The escape spellings that start a control sequence in Rust source text,
/// lowercased.
const NEEDLES: [&str; 5] = ["\\x1b[", "\\033[", "\\u{1b}[", "\\u{001b}[", "\\u{00001b}["];

/// Directories the scan does not enter: the engine itself, build output
/// and hidden directories. A spike under `tools/spikes` is scanned like any
/// other source.
fn skipped(dir: &Path, root: &Path) -> bool {
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name == "target" || name.starts_with('.') {
        return true;
    }
    let rel = dir.strip_prefix(root).unwrap_or(dir);
    rel == Path::new("agent-theme")
}

fn walk(dir: &Path, root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = rd.filter_map(Result::ok).map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            if !skipped(&p, root) {
                walk(&p, root, out);
            }
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Whether `line` holds `needle` followed by something that can open an SGR.
/// A private-mode prefix (`<`, `>`, `?`, `=`) after the CSI marks a terminal
/// mode control, such as the kitty keyboard protocol's push and pop, which
/// never sets colour.
fn starts_sgr(line: &str, needle: &str) -> bool {
    line.match_indices(needle)
        .any(|(i, _)| !matches!(line[i + needle.len()..].chars().next(), Some('<' | '>' | '?' | '=')))
}

/// Every raw SGR literal under `root`, as `file:line: text`, and every file
/// scanned. A line whose code starts with `//` is a comment and not a literal.
fn scan(root: &Path) -> (Vec<String>, Vec<PathBuf>) {
    let mut files = Vec::new();
    walk(root, root, &mut files);
    let mut found = Vec::new();
    for f in &files {
        let Ok(text) = std::fs::read_to_string(f) else { continue };
        for (i, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            let lower = line.to_ascii_lowercase();
            if NEEDLES.iter().any(|n| starts_sgr(&lower, n)) {
                let rel = f.strip_prefix(root).unwrap_or(f);
                found.push(format!("{}:{}: {}", rel.display(), i + 1, line.trim()));
            }
        }
    }
    (found, files)
}

fn tools_root() -> PathBuf {
    // Read at run time: a test binary reused from another checkout keeps the
    // path it was built at.
    std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_else(|| env!("CARGO_MANIFEST_DIR").into())).parent().expect("agent-theme sits in tools/").to_path_buf()
}

#[test]
fn no_raw_sgr_literals_outside_agent_theme() {
    let (found, files) = scan(&tools_root());
    assert!(files.len() > 100, "scanned only {} files under {}", files.len(), tools_root().display());
    let known = Path::new("ways-cli").join("src").join("cmd").join("render.rs");
    assert!(files.iter().any(|f| f.ends_with(&known)), "the scan never visited {}", known.display());
    assert!(
        found.is_empty(),
        "{} raw SGR literal(s); draw colour through agent_theme (paint, sgr, Style, Role) instead:\n  {}",
        found.len(),
        found.join("\n  ")
    );
}

#[test]
fn a_planted_literal_is_reported() {
    let root = std::env::temp_dir().join(format!("sgr-lint-plant-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let write = |rel: &str, body: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    };
    // Built from parts so this file's own text is not what is being tested.
    let esc = ["\\", "x1b["].concat();
    let planted = format!("fn f() {{ println!(\"{esc}31mred{esc}0m\"); }}\n");
    write("some-crate/src/lib.rs", &format!("// header\n{planted}"));
    write("other/src/main.rs", &format!("fn g() {{ let _ = \"{}1m\"; }}\n", ["\\", "u{1B}["].concat()));
    write("other/src/doc.rs", &format!("/// writes `{esc}0m` to reset\nfn h() {{}}\n"));
    // A private-mode control is not colour; a colour code later on the same
    // line is still caught.
    write("other/src/mode.rs", &format!("const POP: &[u8] = b\"{esc}<u\";\nconst P2: &str = \"{esc}?25l{esc}1m\";\n"));
    write("agent-theme/src/paint.rs", &planted);
    write("spikes/demo/src/main.rs", &planted);
    write("some-crate/target/debug/build.rs", &planted);
    write("some-crate/src/notes.txt", &planted);

    let (found, files) = scan(&root);
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(files.len(), 5, "the excluded trees and the .txt file are not read: {files:#?}");
    assert_eq!(found.len(), 4, "{found:#?}");
    let sep = std::path::MAIN_SEPARATOR;
    assert!(found[0].starts_with(&format!("other{sep}src{sep}main.rs:1:")), "{found:#?}");
    assert!(found[1].starts_with(&format!("other{sep}src{sep}mode.rs:2:")), "colour after a mode control: {found:#?}");
    assert!(found[2].starts_with(&format!("some-crate{sep}src{sep}lib.rs:2:")), "{found:#?}");
    assert!(found[3].starts_with(&format!("spikes{sep}demo{sep}src{sep}main.rs:1:")), "a spike is not exempt: {found:#?}");
}
