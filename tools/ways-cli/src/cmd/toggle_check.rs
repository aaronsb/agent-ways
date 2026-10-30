//! Per-way toggles (ADR-131) that name no way, and toggle patterns that do
//! not mean what they say. A key in a project's `.claude/ways.yaml` `ways:`
//! map is a toggle pattern over way ids (`ways_core::config::toggle_matches`).
//! A key that matches no way switches nothing: after a way is renamed, the
//! toggle that switched it off stays in the file and the way runs again. The
//! overlay accepts any key, so `ways status` and `ways settings lint` report
//! these.
//!
//! The way ids are read from the roots a scan reads (`paths::ways_roots`):
//! the project's own ways, the user's, then the shipped ways. Without the
//! shipped ways every toggle naming one would look unmatched, so nothing is
//! judged then.

use agent_settings::load::Layer;
use agent_settings::LayerScope;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A toggle key that matches no way, with the existing id it most likely
/// meant when one stands out.
#[derive(Debug, Clone, PartialEq)]
pub struct Unmatched {
    pub key: String,
    pub nearest: Option<String>,
}

/// What a check of a set of toggle keys found.
#[derive(Debug, PartialEq)]
pub enum Check {
    /// The keys were judged; these match no way.
    Checked(Vec<Unmatched>),
    /// The shipped ways were not found, so the keys were not judged.
    NoShippedWays,
}

/// Judge `keys` against the ways a session in `project` can see. No keys
/// is a clean check, without reading any way root.
pub fn check(keys: &[String], project: &Path) -> Check {
    if keys.is_empty() {
        return Check::Checked(Vec::new());
    }
    match judged_ids(project) {
        Some(ids) => Check::Checked(unmatched(keys.iter().map(String::as_str), &ids)),
        None => Check::NoShippedWays,
    }
}

/// The way ids toggles in `project` are judged against; `None` without the
/// shipped ways.
pub fn judged_ids(project: &Path) -> Option<BTreeSet<String>> {
    ways_core::paths::shipped_ways_root().is_dir().then(|| known_ids(project))
}

/// Every key of the `ways:` map in a project-scope ways overlay, whatever
/// it sets; empty for any other layer.
pub fn overlay_keys(l: &Layer) -> Vec<String> {
    if !(l.file == ways_core::settings::FILE && l.scope == LayerScope::Project) {
        return Vec::new();
    }
    l.accepted
        .get("ways")
        .and_then(serde_yaml::Value::as_mapping)
        .map(|m| m.keys().filter_map(|k| k.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// Every toggle of the `ways:` map in a project-scope ways overlay, as
/// `(key, enabled)`, read as the config reads it: an entry that sets no
/// boolean `enabled` is left out. Empty for any other layer.
pub fn overlay_toggles(l: &Layer) -> Vec<(String, bool)> {
    if !(l.file == ways_core::settings::FILE && l.scope == LayerScope::Project) {
        return Vec::new();
    }
    let Some(map) = l.accepted.get("ways").and_then(serde_yaml::Value::as_mapping) else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(k, v)| {
            let enabled = match v {
                serde_yaml::Value::Bool(b) => Some(*b),
                serde_yaml::Value::Mapping(m) => m.get("enabled").and_then(serde_yaml::Value::as_bool),
                _ => None,
            };
            Some((k.as_str()?.to_string(), enabled?))
        })
        .collect()
}

/// Every way id a session in `project` can see, across the scan's roots.
pub fn known_ids(project: &Path) -> BTreeSet<String> {
    ways_core::paths::ways_roots(Some(project))
        .iter()
        .flat_map(|r| crate::cmd::scan::candidates::way_ids(r))
        .collect()
}

/// Whether a toggle key matches at least one way, by the rule of
/// `config::way_disabled`.
pub fn names_a_way(key: &str, ids: &BTreeSet<String>) -> bool {
    ids.iter().any(|id| ways_core::config::toggle_matches(key, id))
}

/// Whether a segment of `key` mixes text and a wildcard, as `doc*` or `a*b`
/// do. Such a key is no toggle pattern: the matcher reads it literally, so
/// it matches only a way id spelled with that asterisk.
pub fn mixes_wildcard(key: &str) -> bool {
    key.split('/').any(|s| s.contains('*') && s != "*" && s != "**")
}

/// What `lint` and the writers of `ways settings` say of a key that
/// [`mixes_wildcard`].
pub const MIXED_WILDCARD: &str = "a segment mixes text and a wildcard, so the key is read literally and matches no way; write `*` or `**` as a whole segment";

/// For a key `dir/*`, which before toggle patterns (ADR-710) covered `dir`
/// itself and every way under it, what lint says of it. The candidate
/// replacement is `dir/**: v`, plus `dir: v` when `dir` is a way in `ids` and
/// the file holds no `dir` key; a `dir` key already there stays. Lint names
/// it only when the file holds no `dir/**` key and, over the toggles after
/// it, every way in `ids` at or under `dir` has the state the pre-ADR-710
/// rule gives it over the file as it is, with no tie the file lacks.
/// Otherwise lint states the change and names the keys to read together.
/// `toggles` are the file's toggles as `(key, enabled)` and `keys` every key
/// it holds; with `ids` `None` or empty nothing is simulated. `None` unless `dir` is
/// one directory: non-empty and with no wildcard.
pub fn old_star_advice(key: &str, toggles: &[(String, bool)], keys: &[String], ids: Option<&BTreeSet<String>>) -> Option<String> {
    let dir = key.strip_suffix("/*").filter(|d| !d.is_empty() && !d.contains('*'))?;
    let value = |k: &str| toggles.iter().find(|(t, _)| t == k).map(|(_, on)| *on);
    let head = format!("`/*` matches only the ways directly under {dir}");
    let Some(on) = value(key) else {
        return Some(format!("{head}; this key sets no `enabled`, so it switches nothing; remove it"));
    };
    let sub = format!("{dir}/**");
    let ids = ids.filter(|ids| !ids.is_empty());
    let dir_is_way = ids.is_none_or(|ids| ids.contains(dir));
    let lost = if dir_is_way { format!("{dir} itself or the ways deeper down") } else { "the ways deeper down".to_string() };
    let under = |id: &str| id == dir || id.strip_prefix(dir).is_some_and(|r| r.starts_with('/'));
    // The other keys bearing on a way at or under `dir`. A key spelling one
    // way and setting a value decides that way under both rules.
    let mut others: Vec<&str> = keys
        .iter()
        .map(String::as_str)
        .filter(|k| *k != key && !(!ways_core::config::is_toggle_pattern(k) && value(k).is_some()))
        .filter(|k| match ids {
            Some(ids) => ids.iter().any(|id| under(id) && (ways_core::config::toggle_matches(k, id) || pre_pattern_rank(k, id).is_some())),
            None => overlaps(k, dir) || overlaps(k, &sub),
        })
        .collect();
    let together = |others: &[&str]| match others {
        [] => String::new(),
        _ => format!(", so read it together with {}", others.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", ")),
    };
    if keys.contains(&sub) {
        others.retain(|k| *k != sub);
        others.insert(0, &sub);
        return Some(format!("{head}, and on those ways it outranks `{sub}`; it no longer covers {lost}{}", together(&others)));
    }
    let Some(ids) = ids else {
        return Some(format!("{head}; it no longer covers {lost}, and with no shipped ways found no replacement is checked{}", together(&others)));
    };
    let has_children = ids.iter().any(|id| id.strip_prefix(dir).is_some_and(|r| r.starts_with('/')));
    if !dir_is_way && !has_children {
        return Some(format!("{head}; no way sits at or under {dir}, so it switches nothing; remove it"));
    }
    let state = |b: bool| if b { "on" } else { "off" };
    let mut add: Vec<(String, bool)> = Vec::new();
    let mut note = String::new();
    let mut covers_dir = false;
    if keys.iter().any(|k| k == dir) {
        match value(dir) {
            Some(own) if own == on => covers_dir = dir_is_way,
            Some(own) if dir_is_way => note = format!("; the existing `{dir}: {own}` stays, so {dir} itself stays {}", state(own)),
            None if dir_is_way => note = format!("; the existing `{dir}` key sets no `enabled`, so {dir} itself is not switched by it"),
            _ => {}
        }
    } else if dir_is_way {
        add.push((dir.to_string(), on));
        covers_dir = true;
    }
    if has_children {
        add.push((sub.clone(), on));
    }
    let after: Vec<(String, bool)> = toggles.iter().filter(|(k, _)| k != key).cloned().chain(add.iter().cloned()).collect();
    let states_kept = ids.iter().filter(|id| under(id)).all(|id| {
        let new_off = ways_core::config::way_toggled_off(after.iter().map(|(k, on)| (k.as_str(), *on)), id);
        (pre_pattern_toggle(toggles, id) == Some(false)) == new_off
    });
    let before = ties(toggles, ids);
    let ties_kept = ties(&after, ids).iter().all(|(k, _)| before.iter().any(|(b, _)| b == k));
    if !(states_kept && ties_kept) {
        return Some(format!("{head}; it no longer covers {lost}, and no replacement of this key alone keeps every way as it was{}", together(&others)));
    }
    if add.is_empty() {
        return Some(format!("{head}; no way sits under {dir}, and the `{dir}` key already in the file covers it, so remove this key{note}"));
    }
    let scope = match (covers_dir, has_children) {
        (true, true) => format!("{dir} and every way under it"),
        (true, false) => format!("{dir}, which has no ways under it"),
        _ => format!("every way under {dir}"),
    };
    let named: Vec<String> = add.iter().map(|(k, on)| format!("`{k}: {on}`")).collect();
    Some(format!("{head}; for {scope}, replace this key with {}{note}", named.join(" plus ")))
}

/// The pre-ADR-710 toggle rule, kept to check the `dir/*` advice: the value
/// of the toggle that switched `way_id`, where a key equal to the id wins,
/// else the longest `dir/*` key whose `dir` is the id or one of its
/// ancestors.
fn pre_pattern_toggle(toggles: &[(String, bool)], way_id: &str) -> Option<bool> {
    let mut best: Option<(usize, bool)> = None;
    for (key, enabled) in toggles {
        let Some(rank) = pre_pattern_rank(key, way_id) else { continue };
        if best.is_none_or(|(r, _)| rank > r) {
            best = Some((rank, *enabled));
        }
    }
    best.map(|(_, enabled)| enabled)
}

/// How specific `key` is for `way_id` under the pre-ADR-710 rule; `None`
/// when it does not cover the way.
fn pre_pattern_rank(key: &str, way_id: &str) -> Option<usize> {
    match key.strip_suffix("/*") {
        None => (key == way_id).then_some(usize::MAX),
        Some(dir) => (way_id == dir || way_id.strip_prefix(dir).is_some_and(|r| r.len() > 1 && r.starts_with('/'))).then_some(dir.len()),
    }
}

/// Whether toggle patterns `a` and `b` match at least one way id in common.
fn overlaps(a: &str, b: &str) -> bool {
    // Both patterns consume one id segment per step; a `**` may stay to
    // consume the next one too.
    fn step(a: &[&str], b: &[&str]) -> bool {
        let (Some((x, ra)), Some((y, rb))) = (a.split_first(), b.split_first()) else {
            return a.is_empty() && b.is_empty();
        };
        let wild = |s: &str| s == "*" || s == "**";
        if !(x == y || wild(x) || wild(y)) {
            return false;
        }
        step(ra, rb) || (*x == "**" && step(a, rb)) || (*y == "**" && step(ra, b))
    }
    step(&a.split('/').collect::<Vec<_>>(), &b.split('/').collect::<Vec<_>>())
}

/// Each set of toggles tied at the highest rank with opposite values on a
/// way in `ids`, with the first such way. A tie resolves to `false`.
pub fn ties(toggles: &[(String, bool)], ids: &BTreeSet<String>) -> Vec<(Vec<String>, String)> {
    let mut out: Vec<(Vec<String>, String)> = Vec::new();
    for id in ids {
        let keys: Vec<String> = ways_core::config::tied_toggles(toggles.iter().map(|(k, on)| (k.as_str(), *on)), id).into_iter().map(str::to_string).collect();
        if !keys.is_empty() && !out.iter().any(|(k, _)| *k == keys) {
            out.push((keys, id.clone()));
        }
    }
    out
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
/// directory of ways is the pattern `dir/**`. Otherwise the sibling way
/// (same parent) whose name shares a word stem with the key's, or failing
/// that is within half its length in edit distance; `None` when no sibling
/// is close or two are equally close.
fn nearest(key: &str, ids: &BTreeSet<String>) -> Option<String> {
    if key.contains('*') {
        return None;
    }
    let dir_form = format!("{key}/**");
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
    fn an_existing_way_or_a_pattern_matching_one_is_matched() {
        let ids = ids(&["data", "data/schema-docs", "softwaredev/code/quality"]);
        assert!(unmatched(["data/schema-docs", "data/*", "softwaredev/**", "softwaredev/code/*", "*/code/quality", "data"], &ids).is_empty());
    }

    #[test]
    fn a_pattern_matching_no_way_is_unmatched() {
        let ids = ids(&["data/schema-docs", "workstation/shell/shell-prompt"]);
        let got = unmatched(["itops/**", "dat/*", "workstation/*", "workstation/shell/shell-prompt/*"], &ids);
        assert_eq!(
            got.iter().map(|u| u.key.as_str()).collect::<Vec<_>>(),
            ["itops/**", "dat/*", "workstation/*", "workstation/shell/shell-prompt/*"],
            "`*` reaches one segment down, and never the directory's own way"
        );
    }

    #[test]
    fn a_bare_directory_key_suggests_its_subtree_pattern() {
        let ids = ids(&["softwaredev/code/quality"]);
        assert_eq!(unmatched(["softwaredev"], &ids)[0].nearest.as_deref(), Some("softwaredev/**"));
    }

    #[test]
    fn a_segment_mixing_text_and_a_wildcard_is_flagged() {
        for key in ["doc*", "documentation/adr*", "a*b/c", "a/***"] {
            assert!(mixes_wildcard(key), "{key}");
        }
        for key in ["documentation/adr", "*/adr", "a/**", "a/*/c"] {
            assert!(!mixes_wildcard(key), "{key}");
        }
    }

    fn file(entries: &[(&str, bool)]) -> (Vec<(String, bool)>, Vec<String>) {
        (entries.iter().map(|(k, v)| (k.to_string(), *v)).collect(), entries.iter().map(|(k, _)| k.to_string()).collect())
    }

    /// The advice for the file's `/*` key, against the ways `data`,
    /// `data/a` and `data/a/b`.
    fn advice(entries: &[(&str, bool)]) -> String {
        advice_over(entries, &["data", "data/a", "data/a/b"])
    }

    fn advice_over(entries: &[(&str, bool)], ways: &[&str]) -> String {
        let (t, k) = file(entries);
        let key = entries.iter().map(|(k, _)| *k).find(|k| k.ends_with("/*")).expect("a `/*` key");
        old_star_advice(key, &t, &k, Some(&ids(ways))).expect("advice")
    }

    #[test]
    fn a_trailing_star_key_alone_is_replaced_by_both_keys() {
        assert_eq!(
            advice(&[("data/*", false)]),
            "`/*` matches only the ways directly under data; for data and every way under it, replace this key with `data: false` plus `data/**: false`"
        );
    }

    #[test]
    fn a_directory_that_is_no_way_is_not_named_to_add() {
        let (t, k) = file(&[("softwaredev/*", false)]);
        let ids = ids(&["softwaredev/code", "softwaredev/code/quality"]);
        assert_eq!(
            old_star_advice("softwaredev/*", &t, &k, Some(&ids)).as_deref(),
            Some("`/*` matches only the ways directly under softwaredev; for every way under softwaredev, replace this key with `softwaredev/**: false`")
        );
        let ids = super::tests::ids(&["softwaredev", "softwaredev/code"]);
        assert!(old_star_advice("softwaredev/*", &t, &k, Some(&ids)).expect("advice").contains("`softwaredev: false` plus `softwaredev/**: false`"));
    }

    #[test]
    fn an_existing_directory_key_is_kept_not_written_again() {
        assert_eq!(
            advice(&[("data", false), ("data/*", false)]),
            "`/*` matches only the ways directly under data; for data and every way under it, replace this key with `data/**: false`"
        );
        assert_eq!(
            advice(&[("data", true), ("data/*", false)]),
            "`/*` matches only the ways directly under data; for every way under data, replace this key with `data/**: false`; the existing `data: true` stays, so data itself stays on"
        );
    }

    #[test]
    fn an_existing_subtree_key_gets_no_replacement() {
        for entries in [&[("data/*", false), ("data/**", false)][..], &[("data/*", false), ("data/**", true)], &[("data", false), ("data/*", false), ("data/**", true)]] {
            assert_eq!(
                advice(entries),
                "`/*` matches only the ways directly under data, and on those ways it outranks `data/**`; it no longer covers data itself or the ways deeper down, so read it together with `data/**`",
                "{entries:?}"
            );
        }
    }

    #[test]
    fn a_star_key_that_sets_no_value_is_advised_away() {
        let k = vec!["data/*".to_string()];
        assert_eq!(
            old_star_advice("data/*", &[], &k, None).as_deref(),
            Some("`/*` matches only the ways directly under data; this key sets no `enabled`, so it switches nothing; remove it")
        );
    }

    #[test]
    fn an_existing_key_that_sets_no_value_is_named() {
        let k = vec!["data/*".to_string(), "data".to_string()];
        let ids = ids(&["data", "data/a"]);
        assert_eq!(
            old_star_advice("data/*", &[("data/*".to_string(), true)], &k, Some(&ids)).as_deref(),
            Some("`/*` matches only the ways directly under data; for every way under data, replace this key with `data/**: true`; the existing `data` key sets no `enabled`, so data itself is not switched by it")
        );
        assert_eq!(
            old_star_advice("data/*", &[("data/*".to_string(), false)], &k, Some(&ids)).as_deref(),
            Some("`/*` matches only the ways directly under data; it no longer covers data itself or the ways deeper down, and no replacement of this key alone keeps every way as it was, so read it together with `data`"),
            "data was off under the old rule and nothing would switch it"
        );
    }

    #[test]
    fn a_competitor_that_would_tie_with_the_subtree_key_gets_no_replacement() {
        assert_eq!(
            advice_over(&[("data/*", true), ("**/x", false)], &["data", "data/x"]),
            "`/*` matches only the ways directly under data; it no longer covers data itself or the ways deeper down, and no replacement of this key alone keeps every way as it was, so read it together with `**/x`"
        );
    }

    #[test]
    fn a_competitor_that_would_outrank_the_subtree_key_gets_no_replacement() {
        assert_eq!(
            advice_over(&[("data/*", false), ("*/x", true)], &["data", "data/x"]),
            "`/*` matches only the ways directly under data; it no longer covers data itself or the ways deeper down, and no replacement of this key alone keeps every way as it was, so read it together with `*/x`"
        );
    }

    #[test]
    fn a_deeper_competitor_that_would_tie_gets_no_replacement() {
        assert_eq!(
            advice_over(&[("data/*", false), ("*/x/**", true)], &["data", "data/x", "data/x/y"]),
            "`/*` matches only the ways directly under data; it no longer covers data itself or the ways deeper down, and no replacement of this key alone keeps every way as it was, so read it together with `*/x/**`"
        );
    }

    #[test]
    fn a_competitor_the_replacement_keeps_is_no_bar() {
        assert_eq!(
            advice_over(&[("data/*", false), ("*/x", false), ("data/a", true)], &["data", "data/a", "data/x"]),
            "`/*` matches only the ways directly under data; for data and every way under it, replace this key with `data: false` plus `data/**: false`"
        );
    }

    #[test]
    fn a_directory_that_is_no_way_is_not_claimed_as_covered() {
        assert_eq!(
            advice_over(&[("softwaredev", false), ("softwaredev/*", false)], &["softwaredev/code"]),
            "`/*` matches only the ways directly under softwaredev; for every way under softwaredev, replace this key with `softwaredev/**: false`"
        );
        assert_eq!(
            advice_over(&[("softwaredev/*", true), ("**/code", false)], &["softwaredev/code"]),
            "`/*` matches only the ways directly under softwaredev; it no longer covers the ways deeper down, and no replacement of this key alone keeps every way as it was, so read it together with `**/code`"
        );
    }

    #[test]
    fn without_the_ways_no_replacement_is_named() {
        let (t, k) = file(&[("data/*", false), ("*/x", true), ("other/**", true), ("data/a", true)]);
        assert_eq!(
            old_star_advice("data/*", &t, &k, None).as_deref(),
            Some("`/*` matches only the ways directly under data; it no longer covers data itself or the ways deeper down, and with no shipped ways found no replacement is checked, so read it together with `*/x`")
        );
        let (t, k) = file(&[("data/*", false)]);
        assert_eq!(
            old_star_advice("data/*", &t, &k, None).as_deref(),
            Some("`/*` matches only the ways directly under data; it no longer covers data itself or the ways deeper down, and with no shipped ways found no replacement is checked")
        );
    }

    #[test]
    fn two_patterns_overlap_where_one_id_matches_both() {
        for (a, b) in [("data", "*"), ("data/**", "*/x"), ("data/**", "**/x/**"), ("**", "**"), ("a/*/c", "*/b/**")] {
            assert!(overlaps(a, b) && overlaps(b, a), "{a} {b}");
        }
        for (a, b) in [("data", "data/**"), ("data/**", "other/**"), ("*/x", "*/*/x"), ("data", "*/*")] {
            assert!(!overlaps(a, b) && !overlaps(b, a), "{a} {b}");
        }
    }

    #[test]
    fn the_pre_pattern_rule_covers_a_directory_and_its_subtree() {
        let toggles = vec![("data/*".to_string(), false), ("data/a/*".to_string(), true), ("data/a/b".to_string(), false), ("**/c".to_string(), true)];
        for (id, want) in [("data", Some(false)), ("data/a", Some(true)), ("data/a/b", Some(false)), ("data/a/c", Some(true)), ("data/c", Some(false)), ("database", None)] {
            assert_eq!(pre_pattern_toggle(&toggles, id), want, "{id}");
        }
    }

    #[test]
    fn only_a_star_under_one_named_directory_gets_the_advice() {
        for key in ["/*", "*", "a/*/*", "doc*/*", "**/*", "data/**", "a/*/c"] {
            let (t, k) = file(&[(key, false)]);
            assert_eq!(old_star_advice(key, &t, &k, None), None, "{key}");
        }
    }

    #[test]
    fn a_directory_with_no_ways_under_it_gets_no_subtree_key() {
        let (t, k) = file(&[("data/*", false)]);
        assert_eq!(
            old_star_advice("data/*", &t, &k, Some(&ids(&["data", "other/a"]))).as_deref(),
            Some("`/*` matches only the ways directly under data; for data, which has no ways under it, replace this key with `data: false`")
        );
        let (t, k) = file(&[("gone/*", false)]);
        assert_eq!(
            old_star_advice("gone/*", &t, &k, Some(&ids(&["other/a"]))).as_deref(),
            Some("`/*` matches only the ways directly under gone; no way sits at or under gone, so it switches nothing; remove it")
        );
    }

    #[test]
    fn a_directory_key_that_is_no_way_gets_no_note() {
        let (t, k) = file(&[("softwaredev", true), ("softwaredev/*", false)]);
        assert_eq!(
            old_star_advice("softwaredev/*", &t, &k, Some(&ids(&["softwaredev/code", "softwaredev/code/x"]))).as_deref(),
            Some("`/*` matches only the ways directly under softwaredev; for every way under softwaredev, replace this key with `softwaredev/**: false`")
        );
    }

    #[test]
    fn an_empty_set_of_ways_checks_no_replacement() {
        let (t, k) = file(&[("data/*", false)]);
        assert_eq!(
            old_star_advice("data/*", &t, &k, Some(&BTreeSet::new())).as_deref(),
            Some("`/*` matches only the ways directly under data; it no longer covers data itself or the ways deeper down, and with no shipped ways found no replacement is checked")
        );
    }

    #[test]
    fn a_true_false_tie_is_reported_once_with_its_first_way() {
        let ids = ids(&["a/b/c", "a/b/d", "x/b/c", "z/b/c"]);
        let toggles = vec![("a/*/c".to_string(), true), ("*/b/c".to_string(), false), ("x/*/c".to_string(), false)];
        assert_eq!(ties(&toggles, &ids), [(vec!["*/b/c".to_string(), "a/*/c".to_string()], "a/b/c".to_string())]);
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
