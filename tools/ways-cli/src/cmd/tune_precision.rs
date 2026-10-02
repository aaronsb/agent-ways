//! Relevance / precision audit from fire telemetry (ADR-134 Decision 3).
//!
//! The cadence audit (`tune-curves`) asks *how often* a way fires; this asks
//! *whether it fired in the right places*. It reads the events log (`paths::events_log()`)
//! and, for each way, estimates how often its fires landed in sessions whose
//! actual activity never touched the way's domain — the "17 of 47 fires landed
//! off-domain" signal that motivated ADR-134.
//!
//! **This is a heuristic flag, not a verdict** — the same contract as the
//! locale-fidelity audit. It writes nothing (apply is a separate concern,
//! ADR-134 task D / #11). It is deliberately conservative: it under-flags
//! rather than nag, and it separates two failure modes that look identical to a
//! naive counter:
//!
//!   - **mis-targeted** — a narrow way that keeps firing into the *same* wrong
//!     kind of session. A real precision problem; remedy is a threshold raise,
//!     vocabulary narrowing, or trigger-channel change.
//!   - **cross-cutting** — a way that fires across *many* different session
//!     kinds because it is broad by design (`meta/todos`, `freshness`).
//!     ADR-134's Negative section names exactly this false positive. We detect
//!     it from breadth (`spread`) and label it as such, never as a defect — and
//!     a flag here must NEVER drive an automatic vocabulary change.
//!
//! ### Method (all from existing `way_fired` fields — no new telemetry)
//!
//! - **Family** = a way id at depth 2 (`softwaredev/delivery`, `meta/todos`).
//!   The top-level domain (`softwaredev`) is too coarse — nearly every way is
//!   `softwaredev/*`, so it would call almost everything corroborated.
//! - **Session activity class** = the families whose share of that session's
//!   fires clears `ACTIVE_SHARE`, plus the single top family. This is what the
//!   session was *about*.
//! - A fire of W is **off-class** if W's family is not an active family of the
//!   session — W fired incidentally while the work was elsewhere. A focused
//!   docs session makes `docs` high-share (on-class); a lone docs fire into a
//!   delivery-dominated session is off-class. Single-way families are handled
//!   correctly by this share test, which family-vs-family corroboration is not.
//! - **irrelevance rate** = off-class sessions / sessions the way fired in.
//! - **spread** = how many *distinct* activity classes the way fired off-class
//!   into. High spread ⇒ broad-by-design, not mis-targeted.

use agent_fmt::{Align, Table};
use anyhow::Result;
use std::collections::{BTreeMap, BTreeSet, HashMap};


/// A family's share of a session's fires must clear this to count as part of
/// the session's activity class. 0.15 keeps incidental single fires out of the
/// class while admitting any genuinely co-active domain.
const ACTIVE_SHARE: f64 = 0.15;

/// A way flagged off-class into at least this many *distinct* activity classes
/// is labeled cross-cutting-by-design rather than mis-targeted. Breadth is the
/// signal that separates "fires everywhere on purpose" from "keeps firing into
/// the one wrong place."
const CROSSCUT_SPREAD: usize = 4;

struct Fire {
    way: String,
    family: String,
    session: String,
    trigger: String,
}

/// Family of a way id = its parent path (the way minus its last segment), so
/// that siblings — ways representing the same kind of work — share a family.
/// A fixed depth would be wrong: the corpus mixes 2-deep namespaces (`kg/api`,
/// `project/database`) with 3-deep ones (`softwaredev/delivery/migrations`).
/// Parent-grouping normalizes both — `kg/api`+`kg/cli` → `kg`,
/// `delivery/migrations`+`delivery/github` → `softwaredev/delivery` — which is
/// exactly the "same category" relation the activity class needs. A top-level
/// id with no parent (`build`) is its own family.
fn family_of(way: &str) -> String {
    match way.rsplit_once('/') {
        Some((parent, _)) => parent.to_string(),
        None => way.to_string(),
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Flag {
    Ok,
    LowN,
    MisTargeted,
    CrossCutting,
}

impl Flag {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Flag::Ok => "ok",
            Flag::LowN => "low-n",
            Flag::MisTargeted => "mis-targeted",
            Flag::CrossCutting => "cross-cutting",
        }
    }
    pub(crate) fn remedy(self) -> &'static str {
        match self {
            Flag::Ok => "-",
            Flag::LowN => "insufficient sample",
            Flag::MisTargeted => "narrow vocab / scope trigger / demote keyword to vocabulary",
            Flag::CrossCutting => "broad by design? scope by trigger — do NOT narrow vocab",
        }
    }
}

pub(crate) struct WayPrecision {
    pub(crate) way: String,
    pub(crate) sessions: usize,
    pub(crate) off_class: usize,
    pub(crate) irrelevance: f64,
    pub(crate) spread: usize,
    pub(crate) top_off_trigger: String,
    pub(crate) flag: Flag,
}

/// The precision audit as data, for callers that render it themselves (the
/// session TUI) rather than printing. `content` is the events-log text.
/// `project` filters the fires loaded; `way` filters only the results, since a
/// session's activity class needs every way that fired in it.
#[allow(dead_code)] // consumed by the TUI tab, not yet wired
pub(crate) fn report(
    content: &str,
    min_sessions: usize,
    flag_threshold: f64,
    project: Option<&str>,
    way: Option<&str>,
) -> Vec<WayPrecision> {
    compute_precision(&load_fires(content, project), min_sessions, flag_threshold, way)
}

pub fn run(
    min_sessions: usize,
    flag_threshold: f64,
    project_filter: Option<String>,
    way_filter: Option<String>,
    json_output: bool,
) -> Result<()> {
    if crate::paths::events_log_sources().is_empty() {
        println!("no events log found at {}", crate::paths::events_log().display());
        println!("ways tune precision needs real firing data — run ways for a few sessions first.");
        return Ok(());
    }

    // Load ALL fires (project filter only). The way filter is applied at the
    // report stage, never here: a session's activity class must be computed
    // from every way that fired in it, not just the way under inspection.
    let fires = load_fires(&ways_core::firing::load_events_text(), project_filter.as_deref());
    if fires.is_empty() {
        println!("no way_fired events found in the selected window.");
        return Ok(());
    }

    let results = compute_precision(&fires, min_sessions, flag_threshold, way_filter.as_deref());

    if json_output {
        println!("{}", json_text(&results));
    } else {
        emit_report(&results, min_sessions, flag_threshold);
    }
    Ok(())
}

fn load_fires(content: &str, project_filter: Option<&str>) -> Vec<Fire> {
    let mut fires = Vec::new();

    for line in content.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let row: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        // Load only `way_fired` events and dedup per (session, way) downstream;
        // `way_redisclosed` is a separate event type (cadence signal, tune-curves)
        // and never reaches here. So each (session, way) lands once — placement,
        // not cadence.
        if row.get("event").and_then(|v| v.as_str()) != Some("way_fired") {
            continue;
        }
        if let Some(pat) = project_filter {
            match row.get("project").and_then(|v| v.as_str()) {
                Some(p) if p.contains(pat) => {}
                _ => continue,
            }
        }
        let way = match row.get("way").and_then(|v| v.as_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        let session = match row.get("session").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        let trigger = row
            .get("trigger")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let family = family_of(&way);
        fires.push(Fire { way, family, session, trigger });
    }
    fires
}

/// Per-session view: which families fired, and how many distinct ways landed.
struct SessionProfile {
    /// family -> number of distinct ways in it that fired this session
    family_counts: HashMap<String, usize>,
    total: usize,
}

impl SessionProfile {
    /// The session's activity class: families clearing ACTIVE_SHARE, plus the
    /// single most active family (so even a low-volume session has a class).
    fn active_families(&self) -> BTreeSet<String> {
        let mut active: BTreeSet<String> = self
            .family_counts
            .iter()
            .filter(|(_, &c)| self.total > 0 && (c as f64 / self.total as f64) >= ACTIVE_SHARE)
            .map(|(f, _)| f.clone())
            .collect();
        if let Some(top) = self.dominant_family() {
            active.insert(top);
        }
        active
    }

    /// The single most active family — the session's theme. Used as the spread
    /// key: a way's spread is how many distinct *themes* it fires off-class
    /// into, NOT how many distinct family-combinations (which would be nearly
    /// per-session and inflate every broad way into "cross-cutting").
    fn dominant_family(&self) -> Option<String> {
        self.family_counts
            .iter()
            .max_by_key(|(f, &c)| (c, (*f).clone()))
            .map(|(f, _)| f.clone())
    }
}

fn compute_precision(
    fires: &[Fire],
    min_sessions: usize,
    flag_threshold: f64,
    way_filter: Option<&str>,
) -> Vec<WayPrecision> {
    // session -> profile (distinct ways per family). A way fires once per
    // session via way_fired, so a (session, way) pair is counted once.
    let mut sessions: HashMap<String, HashMap<String, BTreeSet<String>>> = HashMap::new();
    for f in fires {
        sessions
            .entry(f.session.clone())
            .or_default()
            .entry(f.family.clone())
            .or_default()
            .insert(f.way.clone());
    }

    let profiles: HashMap<String, SessionProfile> = sessions
        .into_iter()
        .map(|(sid, fam_ways)| {
            let family_counts: HashMap<String, usize> =
                fam_ways.iter().map(|(fam, ways)| (fam.clone(), ways.len())).collect();
            let total = family_counts.values().sum();
            (sid, SessionProfile { family_counts, total })
        })
        .collect();

    // Per-session spread key = the session's dominant family (its theme).
    // Spread then counts distinct themes a way fires off-class into.
    let class_key: HashMap<String, String> = profiles
        .iter()
        .filter_map(|(sid, p)| p.dominant_family().map(|d| (sid.clone(), d)))
        .collect();

    // Accumulate per-way placement stats. One entry per (session, way).
    struct Acc {
        sessions: usize,
        off_class: usize,
        off_classes: BTreeSet<String>,
        off_triggers: BTreeMap<String, usize>,
    }
    let mut per_way: BTreeMap<String, Acc> = BTreeMap::new();
    // Dedup (session, way) so multiple way_fired lines for the same pair (should
    // not happen, but the log is append-only and tolerant) count once.
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();

    for f in fires {
        if !seen.insert((f.session.clone(), f.way.clone())) {
            continue;
        }
        let profile = match profiles.get(&f.session) {
            Some(p) => p,
            None => continue,
        };
        let on_class = profile.active_families().contains(&f.family);
        let acc = per_way.entry(f.way.clone()).or_insert_with(|| Acc {
            sessions: 0,
            off_class: 0,
            off_classes: BTreeSet::new(),
            off_triggers: BTreeMap::new(),
        });
        acc.sessions += 1;
        if !on_class {
            acc.off_class += 1;
            if let Some(k) = class_key.get(&f.session) {
                acc.off_classes.insert(k.clone());
            }
            *acc.off_triggers.entry(f.trigger.clone()).or_default() += 1;
        }
    }

    let mut out: Vec<WayPrecision> = Vec::new();
    for (way, acc) in per_way {
        if let Some(pat) = way_filter {
            if !way.contains(pat) {
                continue;
            }
        }
        let irrelevance = if acc.sessions > 0 {
            acc.off_class as f64 / acc.sessions as f64
        } else {
            0.0
        };
        let spread = acc.off_classes.len();
        let top_off_trigger = acc
            .off_triggers
            .iter()
            // Explicit (count, name) tiebreak so determinism doesn't silently
            // depend on the map's iteration order (mirrors dominant_family).
            .max_by_key(|(t, &c)| (c, (*t).clone()))
            .map(|(t, _)| t.clone())
            .unwrap_or_else(|| "-".to_string());

        let flag = if acc.sessions < min_sessions {
            Flag::LowN
        } else if irrelevance < flag_threshold {
            Flag::Ok
        } else if spread >= CROSSCUT_SPREAD {
            Flag::CrossCutting
        } else {
            Flag::MisTargeted
        };

        out.push(WayPrecision {
            way,
            sessions: acc.sessions,
            off_class: acc.off_class,
            irrelevance,
            spread,
            top_off_trigger,
            flag,
        });
    }

    // Flagged first, by irrelevance descending; then the rest alphabetically.
    out.sort_by(|a, b| {
        b.irrelevance
            .partial_cmp(&a.irrelevance)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.way.cmp(&b.way))
    });
    out
}

/// The text report in three pieces: the table is printed by `agent_fmt`
/// directly to stdout, so the text around it and its rows are built here.
struct ReportText {
    /// Everything before the table (the whole report when nothing is flagged).
    head: String,
    /// One row per flagged way, in table column order. Empty ⇒ no table.
    rows: Vec<Vec<String>>,
    /// Everything after the table (the summary line).
    foot: String,
}

fn report_text(results: &[WayPrecision], min_sessions: usize, flag_threshold: f64) -> ReportText {
    let flagged: Vec<&WayPrecision> = results
        .iter()
        .filter(|r| matches!(r.flag, Flag::MisTargeted | Flag::CrossCutting))
        .collect();

    let mut head = format!(
        "\n  Precision audit — heuristic relevance flags (min-sessions={min_sessions}, flag≥{:.0}%)\n",
        flag_threshold * 100.0
    );
    head.push_str("  A flag is a place to look, not a verdict. Cross-cutting ways fire broadly by\n");
    head.push_str("  design and are expected here — never auto-narrow a way's vocabulary from this.\n");

    if flagged.is_empty() {
        head.push_str(&format!(
            "\n  no ways cleared the flag threshold. {} ways measured.\n",
            results.len()
        ));
        return ReportText { head, rows: Vec::new(), foot: String::new() };
    }
    head.push('\n');

    let rows = flagged
        .iter()
        .map(|r| {
            vec![
                r.way.clone(),
                r.sessions.to_string(),
                r.off_class.to_string(),
                format!("{:.0}%", r.irrelevance * 100.0),
                r.spread.to_string(),
                r.top_off_trigger.clone(),
                r.flag.label().to_string(),
                r.flag.remedy().to_string(),
            ]
        })
        .collect();

    let mis = flagged.iter().filter(|r| r.flag == Flag::MisTargeted).count();
    let cross = flagged.iter().filter(|r| r.flag == Flag::CrossCutting).count();
    let low_n = results.iter().filter(|r| r.flag == Flag::LowN).count();
    let foot = format!(
        "\n  {mis} mis-targeted, {cross} cross-cutting, {low_n} low-n (below min-sessions), \
         {} ok of {} measured.\n",
        results.len() - flagged.len() - low_n,
        results.len()
    );
    ReportText { head, rows, foot }
}

fn emit_report(results: &[WayPrecision], min_sessions: usize, flag_threshold: f64) {
    let parts = report_text(results, min_sessions, flag_threshold);
    print!("{}", parts.head);
    if parts.rows.is_empty() {
        return;
    }

    let mut t = Table::new(&["Way", "Sess", "Off", "Off%", "Spread", "OffTrigger", "Flag", "Remedy"]);
    t.max_width(0, 34);
    t.align(1, Align::Right);
    t.align(2, Align::Right);
    t.align(3, Align::Right);
    t.align(4, Align::Right);
    t.max_width(7, 48);
    for row in parts.rows {
        t.add_owned(row);
    }
    t.print();
    print!("{}", parts.foot);
}

/// The `--json` form: a pretty-printed array, one object per way.
fn json_text(results: &[WayPrecision]) -> String {
    let arr: Vec<serde_json::Value> = results
        .iter()
        .map(|r| {
            serde_json::json!({
                "way": r.way,
                "sessions": r.sessions,
                "off_class": r.off_class,
                "irrelevance_rate": r.irrelevance,
                "spread": r.spread,
                "top_off_trigger": r.top_off_trigger,
                "flag": r.flag.label(),
            })
        })
        .collect();
    serde_json::to_string_pretty(&serde_json::Value::Array(arr)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fire(way: &str, session: &str, trigger: &str) -> Fire {
        Fire {
            way: way.to_string(),
            family: family_of(way),
            session: session.to_string(),
            trigger: trigger.to_string(),
        }
    }

    #[test]
    fn family_is_parent_path() {
        assert_eq!(family_of("softwaredev/delivery/migrations"), "softwaredev/delivery");
        assert_eq!(family_of("kg/api"), "kg");
        assert_eq!(family_of("meta/todos"), "meta");
        assert_eq!(family_of("solo"), "solo");
    }

    #[test]
    fn on_class_fire_is_not_flagged() {
        // A delivery-dominated session: migrations fires alongside its family.
        let fires = vec![
            fire("softwaredev/delivery/migrations", "s1", "prompt"),
            fire("softwaredev/delivery/github", "s1", "prompt"),
            fire("softwaredev/delivery/commits", "s1", "prompt"),
        ];
        let r = compute_precision(&fires, 1, 0.5, None);
        let mig = r.iter().find(|w| w.way.ends_with("migrations")).unwrap();
        assert_eq!(mig.off_class, 0);
        assert!(matches!(mig.flag, Flag::Ok));
    }

    /// Fill a session with `n` distinct ways all under `family`, so that family
    /// dominates the session's activity class (mirrors a real focused session,
    /// where an incidental single fire is a small share — not the 33% it would
    /// be in a toy 3-fire session, which the share test correctly treats as
    /// on-class).
    fn filler(family: &str, n: usize, session: &str) -> Vec<Fire> {
        (0..n).map(|i| fire(&format!("{family}/w{i}"), session, "prompt")).collect()
    }

    #[test]
    fn lone_fire_into_foreign_session_is_off_class() {
        // s1..s6: docs dominates (6 ways); a single migrations fire rides in at
        // 1/7 ≈ 14% < ACTIVE_SHARE → off-class.
        let mut fires = Vec::new();
        for s in 1..=6 {
            let sid = format!("s{s}");
            fires.extend(filler("softwaredev/docs", 6, &sid));
            fires.push(fire("softwaredev/delivery/migrations", &sid, "bash"));
        }
        let r = compute_precision(&fires, 5, 0.5, None);
        let mig = r.iter().find(|w| w.way.ends_with("migrations")).unwrap();
        assert_eq!(mig.sessions, 6);
        assert_eq!(mig.off_class, 6);
        // All six off-class sessions share one activity class (docs) → low spread.
        assert_eq!(mig.spread, 1);
        assert!(matches!(mig.flag, Flag::MisTargeted));
        assert_eq!(mig.top_off_trigger, "bash");
    }

    #[test]
    fn broad_way_across_many_classes_is_cross_cutting() {
        // tracking rides into 5 differently-themed sessions, each dominated by
        // its own theme family (6 ways) so tracking is a ~14% off-class share.
        let themes = [
            "softwaredev/delivery",
            "softwaredev/docs",
            "softwaredev/code",
            "softwaredev/architecture",
            "meta/knowledge",
        ];
        let mut fires = Vec::new();
        for (i, fam) in themes.iter().enumerate() {
            let sid = format!("s{i}");
            fires.extend(filler(fam, 6, &sid));
            fires.push(fire("meta/todos", &sid, "state"));
        }
        let r = compute_precision(&fires, 5, 0.5, None);
        let track = r.iter().find(|w| w.way == "meta/todos").unwrap();
        assert_eq!(track.off_class, 5);
        assert!(track.spread >= CROSSCUT_SPREAD);
        assert!(matches!(track.flag, Flag::CrossCutting));
    }

    #[test]
    fn below_min_sessions_is_low_n() {
        let fires = vec![fire("softwaredev/delivery/migrations", "s1", "bash")];
        let r = compute_precision(&fires, 5, 0.5, None);
        let mig = &r[0];
        assert!(matches!(mig.flag, Flag::LowN));
    }

    /// A probe way rides off-class into `n_themes` distinctly-themed sessions
    /// (each dominated by 6 filler ways in its own family), so spread == n_themes.
    fn probe_into_themes(probe: &str, n_themes: usize) -> Vec<Fire> {
        let mut fires = Vec::new();
        for i in 0..n_themes {
            let sid = format!("t{i}");
            fires.extend(filler(&format!("dom{i}/area"), 6, &sid));
            fires.push(fire(probe, &sid, "prompt"));
        }
        fires
    }

    #[test]
    fn spread_boundary_gates_cross_cutting() {
        // This boundary gates the no-auto-apply path: at/above CROSSCUT_SPREAD a
        // way is cross-cutting (vocab-narrowing suppressed); below it is
        // mis-targeted. Pin both sides against off-by-one drift.
        let below = compute_precision(&probe_into_themes("probe/way", CROSSCUT_SPREAD - 1), 1, 0.5, None);
        let b = below.iter().find(|w| w.way == "probe/way").unwrap();
        assert_eq!(b.spread, CROSSCUT_SPREAD - 1);
        assert!(matches!(b.flag, Flag::MisTargeted));

        let at = compute_precision(&probe_into_themes("probe/way", CROSSCUT_SPREAD), 1, 0.5, None);
        let a = at.iter().find(|w| w.way == "probe/way").unwrap();
        assert_eq!(a.spread, CROSSCUT_SPREAD);
        assert!(matches!(a.flag, Flag::CrossCutting));
    }

    #[test]
    fn dominant_family_tiebreak_is_deterministic() {
        // Two equal-count families must resolve to the lexicographically-max
        // name regardless of map order — the determinism fix this test locks in.
        let p = SessionProfile {
            family_counts: HashMap::from([("a/x".to_string(), 2), ("b/x".to_string(), 2)]),
            total: 4,
        };
        assert_eq!(p.dominant_family().as_deref(), Some("b/x"));
    }

    #[test]
    fn sole_fire_in_every_session_is_never_flagged() {
        // The load-bearing conservative property: a way that is the only fire in
        // each session is 100%-share → always on-class → never flagged.
        let fires: Vec<Fire> = (0..6).map(|s| fire("lonely/way", &format!("s{s}"), "prompt")).collect();
        let r = compute_precision(&fires, 5, 0.5, None);
        let w = r.iter().find(|w| w.way == "lonely/way").unwrap();
        assert_eq!(w.off_class, 0);
        assert!(matches!(w.flag, Flag::Ok));
    }

    /// Build an events-log text: five sessions, each with 6 docs ways plus one
    /// migrations fire (off-class), plus a malformed line and a non-fire event.
    fn log_fixture() -> String {
        let mut lines = Vec::new();
        let ev = |way: &str, session: &str, trigger: &str| {
            serde_json::json!({
                "event": "way_fired", "way": way, "session": session,
                "trigger": trigger, "project": "proj",
            })
            .to_string()
        };
        for s in 0..5 {
            let sid = format!("s{s}");
            for i in 0..6 {
                lines.push(ev(&format!("softwaredev/docs/w{i}"), &sid, "prompt"));
            }
            lines.push(ev("softwaredev/delivery/migrations", &sid, "bash"));
        }
        lines.push("not json".to_string());
        lines.push(serde_json::json!({"event": "way_redisclosed", "way": "x/y", "session": "s0"}).to_string());
        lines.join("\n")
    }

    #[test]
    fn report_and_texts_on_synthetic_log() {
        let r = report(&log_fixture(), 5, 0.5, None, None);
        let mig = r.iter().find(|w| w.way == "softwaredev/delivery/migrations").unwrap();
        assert_eq!((mig.sessions, mig.off_class, mig.spread), (5, 5, 1));
        assert_eq!(mig.irrelevance, 1.0);
        assert_eq!(mig.top_off_trigger, "bash");
        assert!(matches!(mig.flag, Flag::MisTargeted));
        let docs = r.iter().find(|w| w.way == "softwaredev/docs/w0").unwrap();
        assert_eq!((docs.sessions, docs.off_class, docs.irrelevance), (5, 0, 0.0));
        assert_eq!(docs.top_off_trigger, "-");
        assert!(matches!(docs.flag, Flag::Ok));

        // The way filter narrows results, and the project filter drops fires.
        assert_eq!(report(&log_fixture(), 5, 0.5, None, Some("migrations")).len(), 1);
        assert!(report(&log_fixture(), 5, 0.5, Some("other"), None).is_empty());

        let json: serde_json::Value =
            serde_json::from_str(&json_text(&report(&log_fixture(), 5, 0.5, None, Some("migrations")))).unwrap();
        assert_eq!(
            json,
            serde_json::json!([{
                "way": "softwaredev/delivery/migrations",
                "sessions": 5,
                "off_class": 5,
                "irrelevance_rate": 1.0,
                "spread": 1,
                "top_off_trigger": "bash",
                "flag": "mis-targeted",
            }])
        );

        let text = report_text(&r, 5, 0.5);
        assert!(text.head.starts_with("\n  Precision audit — heuristic relevance flags (min-sessions=5, flag≥50%)\n"));
        assert_eq!(text.rows.len(), 1);
        assert_eq!(
            text.rows[0],
            vec![
                "softwaredev/delivery/migrations", "5", "5", "100%", "1", "bash", "mis-targeted",
                "narrow vocab / scope trigger / demote keyword to vocabulary",
            ]
        );
        assert_eq!(text.foot, "\n  1 mis-targeted, 0 cross-cutting, 0 low-n (below min-sessions), 6 ok of 7 measured.\n");

        let clean = report_text(&report(&log_fixture(), 5, 0.5, None, Some("docs/w0")), 5, 0.5);
        assert!(clean.rows.is_empty());
        assert!(clean.head.ends_with("\n  no ways cleared the flag threshold. 1 ways measured.\n"));
    }
}
