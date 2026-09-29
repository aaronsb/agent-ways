//! Admission order for the ways one scan lane matched.
//!
//! Every way shown in one hook invocation draws on one [`ContextBudget`],
//! admitted whole in the order the lane shows them; the first body that does
//! not fit closes the budget. The order therefore decides which ways reach the
//! model when a lane matches more than the cap holds. Walk order is directory
//! order, which differs between filesystems and checkouts, so a lane first
//! collects its hits and then orders them here.
//!
//! The order:
//!
//! 1. **Family rank, descending.** A family is a matched way together with its
//!    matched descendants. Its rank is the best rank of any member, so a family
//!    is admitted as a unit at the position its strongest member earns.
//! 2. **Family root id**, ascending, as a deterministic tiebreak between
//!    families of equal rank.
//! 3. **Tree order within a family**: path segments compared in order, so a
//!    parent precedes its children and a child precedes its own children. A
//!    child's guidance presumes its parent's (ADR-105), so the parent goes first.
//!
//! A hit's rank is its score. A hit with no score came through an explicit
//! author trigger (a `files:`, `commands:` or keyword match, which carry no
//! probability) and ranks above every scored hit. All hits in the file lane are
//! of that kind, so there the order is family root id, then tree order.
//!
//! [`ContextBudget`]: crate::cmd::show::ContextBudget

use std::cmp::Ordering;

/// One matched way awaiting admission. `payload` carries whatever the lane
/// needs to show it (channel, span, the candidate itself).
pub(crate) struct Hit<T> {
    pub id: String,
    pub score: Option<f64>,
    pub payload: T,
}

fn rank(score: Option<f64>) -> f64 {
    score.unwrap_or(f64::INFINITY)
}

/// `ancestor` is `id` itself or a way above it in the tree.
fn is_ancestor_or_self(ancestor: &str, id: &str) -> bool {
    id == ancestor
        || (id.len() > ancestor.len()
            && id.starts_with(ancestor)
            && id.as_bytes()[ancestor.len()] == b'/')
}

fn tree_cmp(a: &str, b: &str) -> Ordering {
    a.split('/').cmp(b.split('/'))
}

/// Sort `hits` into admission order (see the module docs).
pub(crate) fn order_hits<T>(hits: &mut Vec<Hit<T>>) {
    let ids: Vec<String> = hits.iter().map(|h| h.id.clone()).collect();
    // Family root: the topmost matched ancestor of each hit (itself if none).
    let root_of = |id: &str| -> String {
        ids.iter()
            .filter(|a| is_ancestor_or_self(a, id))
            .min_by_key(|a| a.len())
            .cloned()
            .unwrap_or_else(|| id.to_string())
    };
    let roots: Vec<String> = hits.iter().map(|h| root_of(&h.id)).collect();
    let family_rank = |root: &str| -> f64 {
        hits.iter()
            .zip(&roots)
            .filter(|(_, r)| r.as_str() == root)
            .map(|(h, _)| rank(h.score))
            .fold(f64::NEG_INFINITY, f64::max)
    };
    let mut keyed: Vec<(f64, String, Hit<T>)> = Vec::with_capacity(hits.len());
    let fam: Vec<f64> = roots.iter().map(|r| family_rank(r)).collect();
    for ((hit, root), fr) in hits.drain(..).zip(roots).zip(fam) {
        keyed.push((fr, root, hit));
    }
    keyed.sort_by(|(fa, ra, ha), (fb, rb, hb)| {
        fb.partial_cmp(fa)
            .unwrap_or(Ordering::Equal)
            .then_with(|| tree_cmp(ra, rb))
            .then_with(|| tree_cmp(&ha.id, &hb.id))
    });
    hits.extend(keyed.into_iter().map(|(_, _, h)| h));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn order(input: &[(&str, Option<f64>)]) -> Vec<String> {
        let mut hits: Vec<Hit<()>> = input
            .iter()
            .map(|(id, s)| Hit {
                id: id.to_string(),
                score: *s,
                payload: (),
            })
            .collect();
        order_hits(&mut hits);
        hits.into_iter().map(|h| h.id).collect()
    }

    #[test]
    fn file_lane_is_family_then_tree_order_regardless_of_walk_order() {
        // The way-file edit that motivated this: walk order put the children
        // and the markdown way ahead of the authoring parent, which the budget
        // then withheld.
        let walked = [
            ("meta/knowledge/authoring/pii-free", None),
            ("documentation/markdown", None),
            ("meta/knowledge/authoring/tool-agnostic", None),
            ("softwaredev/delivery/branching", None),
            ("meta/knowledge/authoring", None),
        ];
        let want = vec![
            "documentation/markdown",
            "meta/knowledge/authoring",
            "meta/knowledge/authoring/pii-free",
            "meta/knowledge/authoring/tool-agnostic",
            "softwaredev/delivery/branching",
        ];
        assert_eq!(order(&walked), want);
        let mut reversed = walked;
        reversed.reverse();
        assert_eq!(order(&reversed), want);
    }

    #[test]
    fn parent_precedes_a_higher_scoring_child() {
        assert_eq!(
            order(&[("a/b/c", Some(0.9)), ("a/b", Some(0.6))]),
            vec!["a/b", "a/b/c"]
        );
    }

    #[test]
    fn families_rank_by_their_best_member() {
        // x's family carries 0.9 through its child, so it goes ahead of y (0.8).
        assert_eq!(
            order(&[("y", Some(0.8)), ("x/child", Some(0.9)), ("x", Some(0.55))]),
            vec!["x", "x/child", "y"]
        );
    }

    #[test]
    fn explicit_triggers_rank_above_scored_hits() {
        assert_eq!(
            order(&[("sem", Some(0.99)), ("regex", None)]),
            vec!["regex", "sem"]
        );
    }

    #[test]
    fn prefix_that_is_not_a_parent_is_a_separate_family() {
        // "a/b-x" shares the text prefix "a/b" but is a sibling, not a child.
        assert_eq!(
            order(&[("a/b-x", None), ("a/b/c", None), ("a/b", None)]),
            vec!["a/b", "a/b/c", "a/b-x"]
        );
    }

    #[test]
    fn child_without_its_parent_orders_by_its_own_id() {
        assert_eq!(
            order(&[("z", None), ("m/child", None), ("a", None)]),
            vec!["a", "m/child", "z"]
        );
    }
}
