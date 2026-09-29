//! Admission order for the ways one scan lane matched.
//!
//! Every way shown in one hook invocation draws on one [`ContextBudget`],
//! admitted whole in the order the lane shows them. A body that does not fit is
//! skipped and admission continues, so the order decides which ways get the
//! room first when a lane matches more than the cap holds. Walk order is
//! directory order, which differs between filesystems and checkouts, so a lane
//! collects its hits and orders them here.
//!
//! ## Rank
//!
//! Each hit has a rank. A hit with a score (a semantic fire) ranks by that
//! score. A hit without one came through an explicit author trigger (`files:`,
//! `commands:`, or a keyword `pattern:`) and ranks above every scored hit;
//! among explicit hits, the more specific trigger ranks first.
//!
//! **Specificity** is the longest run of the matched text that the pattern
//! spells out literally: the longest common substring, ignoring case, of the
//! matched span and the pattern's literal characters, where every regex
//! metacharacter breaks a run. A pattern naming the file outranks one matching
//! it by extension or glob:
//!
//! | pattern on `docs/README.md` | matched span | specificity |
//! |---|---|---|
//! | `README\.md$` | `README.md` | 9 (`README.md`) |
//! | `\.md$` | `.md` | 3 (`.md`) |
//! | `\.(md\|rs\|sh)$` | `.md` | 2 (`md`: the group breaks `.` from `md`) |
//!
//! ## Order
//!
//! Matched ways form a forest: a hit's parent is its nearest matched ancestor
//! in the way tree. The forest is walked depth first:
//!
//! 1. Trees, and the children within a tree, are ordered by the best rank in
//!    their subtree, highest first, then by id.
//! 2. A parent precedes its own subtree. A child's guidance presumes its
//!    parent's (ADR-105), so the parent gets the room first.
//!
//! So on an edit to `README.md`, `documentation` (a `README` pattern) leads
//! its tree, its `validate` child (also a `README` pattern) comes before its
//! `markdown` child (`\.md$`), and the extension-wide `branching` way comes
//! after the whole tree.
//!
//! [`ContextBudget`]: crate::cmd::show::ContextBudget

use std::cmp::Ordering;

/// One matched way awaiting admission. `payload` carries whatever the lane
/// needs to show it (channel, span).
pub(crate) struct Hit<T> {
    pub id: String,
    /// Semantic score; `None` for an explicit trigger.
    pub score: Option<f64>,
    /// [`specificity`] of an explicit trigger; ignored for a scored hit.
    pub specificity: usize,
    pub payload: T,
}

impl<T> Hit<T> {
    /// A hit from an explicit trigger: `pattern` matched `span`.
    pub fn explicit(id: &str, pattern: &str, span: &str, payload: T) -> Self {
        Hit {
            id: id.to_string(),
            score: None,
            specificity: specificity(pattern, span),
            payload,
        }
    }

    /// A hit from a semantic fire, or a keyword fire whose span is unknown.
    pub fn scored(id: &str, score: Option<f64>, payload: T) -> Self {
        Hit {
            id: id.to_string(),
            score,
            specificity: 0,
            payload,
        }
    }
}

/// Rank as a comparable pair: explicit triggers (tier 1) above scored hits
/// (tier 0); within a tier, larger is better.
fn rank<T>(h: &Hit<T>) -> (u8, f64) {
    match h.score {
        None => (1, h.specificity as f64),
        Some(s) => (0, s),
    }
}

fn rank_cmp(a: (u8, f64), b: (u8, f64)) -> Ordering {
    a.0.cmp(&b.0)
        .then(a.1.partial_cmp(&b.1).unwrap_or(Ordering::Equal))
}

/// The pattern's literal characters, lowercased, with `\0` wherever a regex
/// construct stands: an escaped punctuation character is a literal, an escaped
/// letter (`\b`, `\d`, `\s`) and every unescaped metacharacter is a break, and
/// a bracket class or `{m,n}` quantifier is one break.
fn literal_text(pattern: &str) -> Vec<char> {
    let mut out = Vec::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some(e) if e.is_ascii_punctuation() || e == ' ' => out.push(e),
                _ => out.push('\0'),
            },
            '[' => {
                // Skip to the closing bracket, honoring escapes.
                while let Some(e) = chars.next() {
                    if e == '\\' {
                        chars.next();
                    } else if e == ']' {
                        break;
                    }
                }
                out.push('\0');
            }
            '{' => {
                for e in chars.by_ref() {
                    if e == '}' {
                        break;
                    }
                }
                out.push('\0');
            }
            '.' | '*' | '+' | '?' | '(' | ')' | '|' | '^' | '$' => out.push('\0'),
            _ => out.extend(c.to_lowercase()),
        }
    }
    out
}

/// Longest run of `span` (ignoring case) that `pattern` spells out literally.
pub(crate) fn specificity(pattern: &str, span: &str) -> usize {
    let lit = literal_text(pattern);
    let text: Vec<char> = span.chars().flat_map(char::to_lowercase).collect();
    // Longest common substring, one DP row at a time.
    let mut best = 0;
    let mut prev = vec![0usize; lit.len() + 1];
    for &t in &text {
        let mut cur = vec![0usize; lit.len() + 1];
        for (j, &l) in lit.iter().enumerate() {
            if l != '\0' && l == t {
                cur[j + 1] = prev[j] + 1;
                best = best.max(cur[j + 1]);
            }
        }
        prev = cur;
    }
    best
}

/// `ancestor` is a way above `id` in the tree.
fn is_proper_ancestor(ancestor: &str, id: &str) -> bool {
    id.len() > ancestor.len() && id.starts_with(ancestor) && id.as_bytes()[ancestor.len()] == b'/'
}

/// Sort `hits` into admission order (see the module docs).
pub(crate) fn order_hits<T>(hits: &mut Vec<Hit<T>>) {
    let n = hits.len();
    // Parent in the hit forest: the nearest (longest) matched proper ancestor.
    let parent: Vec<Option<usize>> = (0..n)
        .map(|i| {
            (0..n)
                .filter(|&j| is_proper_ancestor(&hits[j].id, &hits[i].id))
                .max_by_key(|&j| hits[j].id.len())
        })
        .collect();
    // Best rank in each hit's subtree.
    let best: Vec<(u8, f64)> = (0..n)
        .map(|i| {
            (0..n)
                .filter(|&j| j == i || is_proper_ancestor(&hits[i].id, &hits[j].id))
                .map(|j| rank(&hits[j]))
                .max_by(|a, b| rank_cmp(*a, *b))
                .unwrap_or((0, f64::NEG_INFINITY))
        })
        .collect();
    let by_rank_then_id = |a: &usize, b: &usize| {
        rank_cmp(best[*b], best[*a]).then_with(|| hits[*a].id.cmp(&hits[*b].id))
    };

    let mut order = Vec::with_capacity(n);
    let mut roots: Vec<usize> = (0..n).filter(|&i| parent[i].is_none()).collect();
    roots.sort_by(by_rank_then_id);
    // Depth-first: each node, then its children in rank order.
    let mut stack: Vec<usize> = roots.into_iter().rev().collect();
    while let Some(i) = stack.pop() {
        order.push(i);
        let mut children: Vec<usize> = (0..n).filter(|&j| parent[j] == Some(i)).collect();
        children.sort_by(by_rank_then_id);
        stack.extend(children.into_iter().rev());
    }

    let mut slots: Vec<Option<Hit<T>>> = hits.drain(..).map(Some).collect();
    hits.extend(order.into_iter().filter_map(|i| slots[i].take()));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (id, score, specificity)
    fn order(input: &[(&str, Option<f64>, usize)]) -> Vec<String> {
        let mut hits: Vec<Hit<()>> = input
            .iter()
            .map(|(id, s, sp)| Hit {
                id: id.to_string(),
                score: *s,
                specificity: *sp,
                payload: (),
            })
            .collect();
        order_hits(&mut hits);
        hits.into_iter().map(|h| h.id).collect()
    }

    #[test]
    fn specificity_counts_the_literal_run_of_the_match() {
        assert_eq!(specificity(r"README\.md$|docs/.*\.md$", "README.md"), 9);
        assert_eq!(specificity(r"\.md$", ".md"), 3);
        assert_eq!(specificity(r"\.(md|rs|sh)$", ".md"), 2);
        assert_eq!(
            specificity(
                r"(^|/)(\.claude/ways|hooks/ways)/.*\.md$",
                "/hooks/ways/a/b.md"
            ),
            10
        );
        assert_eq!(specificity(r"^git\ commit", "git commit"), 10);
        assert_eq!(
            specificity(r"(?i)\bpr\b", "PR"),
            2,
            "case is ignored; \\b is a break"
        );
        assert_eq!(specificity(r"[a-z]+\.env$", "x.env"), 4);
        assert_eq!(specificity(r".*", "anything"), 0);
    }

    #[test]
    fn readme_edit_puts_the_named_file_before_the_extension_glob() {
        // The tier-1 fixture's README.md edit: documentation and its validate
        // child name README; markdown and branching match by extension.
        let hits = [
            ("softwaredev/delivery/branching", None, 2),
            ("documentation/markdown", None, 3),
            ("documentation/validate", None, 9),
            ("documentation", None, 9),
        ];
        let want = vec![
            "documentation",
            "documentation/validate",
            "documentation/markdown",
            "softwaredev/delivery/branching",
        ];
        assert_eq!(order(&hits), want);
        let mut reversed = hits;
        reversed.reverse();
        assert_eq!(order(&reversed), want);
    }

    #[test]
    fn a_parent_precedes_its_subtree_even_when_a_child_ranks_higher() {
        assert_eq!(
            order(&[("a/b/c", Some(0.9), 0), ("a/b", Some(0.6), 0)]),
            vec!["a/b", "a/b/c"]
        );
    }

    #[test]
    fn trees_rank_by_their_best_member_not_their_id() {
        // "z" sorts last by id but its child carries the best score, so z's
        // tree goes first; tree-id order would put "a" first.
        assert_eq!(
            order(&[
                ("a", Some(0.8), 0),
                ("z/child", Some(0.95), 0),
                ("z", Some(0.55), 0)
            ]),
            vec!["z", "z/child", "a"]
        );
    }

    #[test]
    fn siblings_order_by_subtree_rank_then_id() {
        assert_eq!(
            order(&[
                ("p", None, 5),
                ("p/a", None, 3),
                ("p/b", None, 9),
                ("p/a/x", None, 12)
            ]),
            vec!["p", "p/a", "p/a/x", "p/b"],
            "p/a's subtree holds the 12, so it leads p/b (9)"
        );
    }

    #[test]
    fn explicit_triggers_rank_above_scored_hits() {
        assert_eq!(
            order(&[("sem", Some(0.99), 0), ("regex", None, 1)]),
            vec!["regex", "sem"]
        );
    }

    #[test]
    fn prefix_that_is_not_a_parent_is_a_separate_tree() {
        // "a/b-x" shares the text prefix "a/b" but is a sibling, not a child.
        assert_eq!(
            order(&[("a/b-x", None, 1), ("a/b/c", None, 1), ("a/b", None, 1)]),
            vec!["a/b", "a/b/c", "a/b-x"]
        );
    }

    #[test]
    fn nearest_matched_ancestor_is_the_parent() {
        // a/b is unmatched, so a/b/c hangs from a directly.
        assert_eq!(
            order(&[("a/b/c", None, 1), ("a", None, 1), ("a/d", None, 1)]),
            vec!["a", "a/b/c", "a/d"]
        );
    }
}
