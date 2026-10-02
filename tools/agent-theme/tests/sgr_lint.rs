//! The raw-colour lint (ADR-504 §6): a raw SGR escape literal in a Rust
//! source under `tools/` outside `agent-theme` and `tools/spikes` is a
//! finding. Colour goes through agent-theme's painter, which honours the
//! theme, the terminal's depth and `NO_COLOR`; a literal does none of that.
//!
//! The second test plants a literal in a fixture tree and requires the scan
//! to report it, and the first requires the real scan to have visited more
//! than 100 files including a known one, so a scan that silently matches or
//! reads nothing cannot pass.
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

/// Directories the scan does not enter: the engine itself, the spikes,
/// build output and hidden directories.
fn skipped(dir: &Path, root: &Path) -> bool {
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name == "target" || name.starts_with('.') {
        return true;
    }
    let rel = dir.strip_prefix(root).unwrap_or(dir);
    rel == Path::new("agent-theme") || rel == Path::new("spikes")
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
            if NEEDLES.iter().any(|n| lower.contains(n)) {
                let rel = f.strip_prefix(root).unwrap_or(f);
                found.push(format!("{}:{}: {}", rel.display(), i + 1, line.trim()));
            }
        }
    }
    (found, files)
}

fn tools_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("agent-theme sits in tools/").to_path_buf()
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
    write("agent-theme/src/paint.rs", &planted);
    write("spikes/demo/src/main.rs", &planted);
    write("some-crate/target/debug/build.rs", &planted);
    write("some-crate/src/notes.txt", &planted);

    let (found, files) = scan(&root);
    let _ = std::fs::remove_dir_all(&root);
    assert_eq!(files.len(), 3, "the excluded trees and the .txt file are not read: {files:#?}");
    assert_eq!(found.len(), 2, "{found:#?}");
    assert!(found[0].starts_with(&format!("other{}src{}main.rs:1:", std::path::MAIN_SEPARATOR, std::path::MAIN_SEPARATOR)), "{found:#?}");
    assert!(found[1].starts_with(&format!("some-crate{}src{}lib.rs:2:", std::path::MAIN_SEPARATOR, std::path::MAIN_SEPARATOR)), "{found:#?}");
}
