//! Per-way toggles (ADR-131) that name no way. A key in a project's
//! `.claude/ways.yaml` `ways:` map is a way id, or a `dir/*` prefix that
//! covers the way at `dir` and every way under it (ADR-701 §1). A key that
//! matches no way switches nothing: after a way is renamed, the toggle that
//! switched it off stays in the file and the way runs again. The overlay
//! accepts any key, so `ways status` and `ways settings lint` report these.
//!
//! The way ids are read from the roots a scan reads (`paths::ways_roots`):
//! the project's own ways, the user's, then the shipped ways.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A toggle key that matches no way, with the existing id it most likely
/// meant when one stands out.
#[derive(Debug, Clone, PartialEq)]
pub struct Unmatched {
    pub key: String,
    pub nearest: Option<String>,
}

/// Every way id a session in `project` can see, across the scan's roots.
pub fn known_ids(project: &Path) -> BTreeSet<String> {
    ways_core::paths::ways_roots(Some(project))
        .iter()
        .flat_map(|r| crate::cmd::scan::candidates::way_ids(r))
        .collect()
}

/// Whether a toggle key matches at least one way, by the rule of
/// `config::way_disabled`: a bare key matches the way with that id, and a
/// `dir/*` key matches the way at `dir` and every way under it.
pub fn names_a_way(key: &str, ids: &BTreeSet<String>) -> bool {
    match key.strip_suffix("/*") {
        None => ids.contains(key),
        Some(dir) => ids.iter().any(|id| id == dir || (id.starts_with(dir) && id.as_bytes().get(dir.len()) == Some(&b'/'))),
    }
}

/// The keys among `keys` that match no way in `ids`, in the order given.
/// With no ways found at all nothing can be judged, and nothing is reported.
pub fn unmatched<'a>(keys: impl IntoIterator<Item = &'a str>, ids: &BTreeSet<String>) -> Vec<Unmatched> {
    if ids.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<Unmatched> = Vec::new();
    for key in keys {
        if names_a_way(key, ids) || out.iter().any(|u| u.key == key) {
            continue;
        }
        out.push(Unmatched { key: key.to_string(), nearest: nearest(key, ids) });
    }
    out
}

/// The project whose ways a settings file's toggles are judged against: the
/// directory holding its `.claude/`, else `fallback`.
pub fn project_of(file: &Path, fallback: &Path) -> PathBuf {
    match file.parent() {
        Some(dir) if dir.file_name().is_some_and(|n| n == ".claude") => dir.parent().map(Path::to_path_buf).unwrap_or_else(|| fallback.to_path_buf()),
        _ => fallback.to_path_buf(),
    }
}

/// The 1-based line of `key` in a `ways:` map, found textually; `None` when
/// the text does not hold it as a plain or quoted mapping key.
pub fn line_of(text: &str, key: &str) -> Option<usize> {
    let forms = [format!("{key}:"), format!("\"{key}\":"), format!("'{key}':")];
    text.lines().position(|l| forms.iter().any(|f| l.trim_start().starts_with(f.as_str()))).map(|i| i + 1)
}

/// The suggestion for a key that matches nothing. A bare key that is a
/// directory of ways is the prefix form `dir/*`. Otherwise the sibling way
/// (same parent) whose name shares a word stem with the key's, or failing
/// that is within half its length in edit distance; `None` when no sibling
/// is close or two are equally close.
fn nearest(key: &str, ids: &BTreeSet<String>) -> Option<String> {
    if key.strip_suffix("/*").is_some() {
        return None;
    }
    let dir_form = format!("{key}/*");
    if names_a_way(&dir_form, ids) {
        return Some(dir_form);
    }
    let (parent, leaf) = key.rsplit_once('/').unwrap_or(("", key));
    let mut ranked: Vec<(bool, usize, &str)> = ids
        .iter()
        .filter_map(|id| {
            let (p, l) = id.rsplit_once('/').unwrap_or(("", id.as_str()));
            (p == parent).then_some((id.as_str(), l))
        })
        .filter_map(|(id, l)| {
            let stem = shares_stem(leaf, l);
            let d = distance(leaf, l);
            (stem || d * 2 <= leaf.len().max(l.len())).then_some((!stem, d, id))
        })
        .collect();
    ranked.sort();
    match ranked.as_slice() {
        [] => None,
        [(s0, d0, _), (s1, d1, _), ..] if s0 == s1 && d0 == d1 => None,
        [(_, _, id), ..] => Some(id.to_string()),
    }
}

/// Whether two way names share a word: a hyphen-separated part of one
/// contains a part of the other, or the two start with the same three
/// letters (`documentation`, `schema-docs`).
fn shares_stem(a: &str, b: &str) -> bool {
    a.split('-').any(|x| {
        b.split('-').any(|y| {
            let common = x.chars().zip(y.chars()).take_while(|(p, q)| p == q).count();
            (x.len() >= 3 && y.contains(x)) || (y.len() >= 3 && x.contains(y)) || common >= 3
        })
    })
}

/// Levenshtein distance over chars.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = if ca == *cb { prev } else { 1 + prev.min(cur).min(row[j]) };
            prev = cur;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_renamed_way_is_unmatched_and_its_new_name_suggested() {
        let ids = ids(&["data", "data/schema-docs", "data/migrations", "workstation/shell", "workstation/shell/shell-prompt"]);
        let got = unmatched(["data/documentation", "workstation/shell/prompt"], &ids);
        assert_eq!(
            got,
            vec![
                Unmatched { key: "data/documentation".into(), nearest: Some("data/schema-docs".into()) },
                Unmatched { key: "workstation/shell/prompt".into(), nearest: Some("workstation/shell/shell-prompt".into()) },
            ]
        );
    }

    #[test]
    fn an_existing_way_or_a_covering_prefix_is_matched() {
        let ids = ids(&["data", "data/schema-docs", "softwaredev/code/quality"]);
        assert!(unmatched(["data/schema-docs", "data/*", "softwaredev/*", "softwaredev/code/*", "data"], &ids).is_empty());
    }

    #[test]
    fn a_prefix_covering_no_way_is_unmatched() {
        let ids = ids(&["data/schema-docs"]);
        assert_eq!(unmatched(["itops/*", "dat/*"], &ids).iter().map(|u| u.key.as_str()).collect::<Vec<_>>(), ["itops/*", "dat/*"]);
    }

    #[test]
    fn a_bare_directory_key_suggests_its_prefix_form() {
        let ids = ids(&["softwaredev/code/quality"]);
        assert_eq!(unmatched(["softwaredev"], &ids)[0].nearest.as_deref(), Some("softwaredev/*"));
    }

    #[test]
    fn no_ways_found_judges_nothing() {
        assert!(unmatched(["data/documentation"], &BTreeSet::new()).is_empty());
    }

    #[test]
    fn a_line_is_found_for_plain_and_quoted_keys() {
        let text = "ways:\n  data/documentation: false\n  'itops/*':\n    enabled: false\n";
        assert_eq!(line_of(text, "data/documentation"), Some(2));
        assert_eq!(line_of(text, "itops/*"), Some(3));
        assert_eq!(line_of(text, "nope"), None);
    }

    #[test]
    fn the_project_of_an_overlay_is_above_its_claude_dir() {
        assert_eq!(project_of(Path::new("/p/.claude/ways.yaml"), Path::new("/f")), PathBuf::from("/p"));
        assert_eq!(project_of(Path::new("/tmp/copy.yaml"), Path::new("/f")), PathBuf::from("/f"));
    }
}
