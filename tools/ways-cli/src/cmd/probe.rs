//! `ways author probe` — score a probe file through the real scan (ADR-701).
//!
//! Each row of the probe file (`prompt`, `expected_way`, `kind`, `role`,
//! `must_not`) runs as a fresh session through the scan the hooks run
//! ([`crate::cmd::scan::probe`]). A `-tool` kind runs through the Bash lane with
//! the prompt as the tool description. A probe passes when the expected way
//! fires and no `must_not` way outranks it.
//!
//! The report is deterministic: rows keep the file's order and carry no timing.

use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::cmd::scan::probe::ProbeScan;
use crate::cmd::scan::scoring::{isolate, Isolation};

const DEFAULT_FILE: &str = "tests/probes/tree-sample.tsv";

/// One row of the probe file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    pub prompt: String,
    pub expected: String,
    pub kind: String,
    pub role: String,
    pub must_not: Vec<String>,
}

/// How a probe came out. `stage` is the scan stage that decided the expected way.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub probe: Probe,
    pub skipped: Option<String>,
    pub rank: Option<usize>,
    pub share: Option<f64>,
    pub margin: Option<f64>,
    pub peak: Option<f64>,
    pub confirm: Option<f64>,
    pub stage: String,
    pub fires: bool,
    pub sibling_over: Vec<String>,
    /// The other ways the probe fired, best first.
    pub also_fired: Vec<String>,
    pub boost_exercised: bool,
    pub late: bool,
}

impl Outcome {
    pub fn scored(&self) -> bool {
        self.skipped.is_none()
    }
    pub fn pass(&self) -> bool {
        self.scored() && self.fires && self.sibling_over.is_empty()
    }
    pub fn top1(&self) -> bool {
        self.rank == Some(1)
    }
}

/// Parse a probe file: a header line, then tab-separated rows.
pub fn parse(text: &str) -> Result<Vec<Probe>> {
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap_or("").split('\t').collect();
    if header.len() < 5 || header[..5] != ["prompt", "expected_way", "kind", "role", "must_not"] {
        bail!("probe file header must be prompt, expected_way, kind, role, must_not (tab-separated)");
    }
    let mut out = Vec::new();
    for (i, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 4 {
            bail!("probe row {} has {} fields, expected at least 4", i + 2, f.len());
        }
        out.push(Probe {
            prompt: f[0].to_string(),
            expected: f[1].to_string(),
            kind: f[2].to_string(),
            role: f[3].to_string(),
            must_not: f.get(4).map(|m| m.split(',').filter(|s| !s.is_empty()).map(str::to_string).collect()).unwrap_or_default(),
        });
    }
    Ok(out)
}

/// Whether the operator's config turns the way off (its domain or the way).
fn is_disabled(id: &str) -> bool {
    crate::session::domain_disabled(id.split('/').next().unwrap_or(id)) || crate::session::way_disabled(id)
}

/// Whether a kind belongs to the tool surface.
fn is_tool(kind: &str) -> bool {
    kind.ends_with("-tool")
}

/// Read one scan's result for `probe`.
pub fn evaluate(probe: &Probe, scan: &ProbeScan, disabled: impl Fn(&str) -> bool) -> Outcome {
    let position = scan.rows.iter().position(|r| r.id == probe.expected);
    let rank = position.map(|p| p + 1);
    let share = position.map(|p| scan.rows[p].score);
    let margin = share.map(|s| {
        let best_other = scan.rows.iter().filter(|r| r.id != probe.expected).map(|r| r.score).fold(f64::NEG_INFINITY, f64::max);
        if best_other.is_finite() { s - best_other } else { s }
    });
    let stage = scan.stages.get(&probe.expected).copied().unwrap_or("not-in-tree").to_string();
    let fires = stage == "fired";
    let rank_of = |id: &str| scan.rows.iter().position(|r| r.id == id);
    let mut sibling_over: Vec<String> = probe
        .must_not
        .iter()
        .filter(|m| match (rank_of(m), position) {
            (Some(m), Some(e)) => m < e,
            // The expected way has no row (a pattern-only way is unranked): a
            // sibling outranks it only when it did not fire.
            (Some(_), None) => !fires,
            _ => false,
        })
        .cloned()
        .collect();
    sibling_over.sort();
    // A way that cannot fire on this lane by design (scope, `when:`, a state
    // trigger with no regex, no matchable text) or that the operator's config
    // turned off is not a miss of the matcher: it is left out of the rates.
    let skipped = if stage == "not-in-tree" && disabled(&probe.expected) {
        Some("disabled")
    } else if matches!(stage.as_str(), "masked" | "state-trigger" | "not-embeddable") {
        Some("lane-ineligible")
    } else {
        None
    }
    .map(str::to_string);
    Outcome {
        probe: probe.clone(),
        skipped,
        rank,
        share,
        margin,
        peak: position.and_then(|p| scan.rows[p].peak),
        confirm: position.and_then(|p| scan.rows[p].confirm),
        also_fired: scan.fired.iter().filter(|(id, _)| *id != probe.expected).map(|(id, _)| id.clone()).collect(),
        fires,
        stage,
        sibling_over,
        boost_exercised: scan.boost_exercised,
        late: scan.late,
    }
}

/// Aggregate counts for a group of results.
#[derive(Default, Debug, PartialEq, Eq, Clone, Copy)]
pub struct Tally {
    pub scored: usize,
    pub passed: usize,
    pub top1: usize,
    pub fired: usize,
}

impl Tally {
    fn add(&mut self, r: &Outcome) {
        self.scored += 1;
        self.passed += usize::from(r.pass());
        self.top1 += usize::from(r.top1());
        self.fired += usize::from(r.fires);
    }
}

fn pct(n: usize, d: usize) -> String {
    if d == 0 { "-".to_string() } else { format!("{:.1}%", 100.0 * n as f64 / d as f64) }
}

/// Tallies overall, by role and by kind, over the scored results.
pub fn tally(results: &[Outcome]) -> (Tally, BTreeMap<String, Tally>, BTreeMap<String, Tally>) {
    let (mut all, mut roles, mut kinds) = (Tally::default(), BTreeMap::new(), BTreeMap::new());
    for r in results.iter().filter(|r| r.scored()) {
        all.add(r);
        roles.entry(r.probe.role.clone()).or_insert_with(Tally::default).add(r);
        kinds.entry(r.probe.kind.clone()).or_insert_with(Tally::default).add(r);
    }
    (all, roles, kinds)
}

fn opt(v: Option<f64>) -> String {
    v.map_or("-".to_string(), |v| format!("{v:.4}"))
}

fn rank_str(r: &Outcome) -> String {
    r.rank.map_or("-".to_string(), |n| n.to_string())
}

/// The scoring path: `late` (late interaction ran), `single` (the single-vector
/// fail-safe decided) or `bash` (the tool lane, always single-vector).
fn path_str(r: &Outcome) -> &'static str {
    if is_tool(&r.probe.kind) {
        "bash"
    } else if r.late {
        "late"
    } else {
        "single"
    }
}

fn stage_str(r: &Outcome) -> String {
    r.skipped.as_ref().map_or_else(|| r.stage.clone(), |why| format!("skipped: {why} ({})", r.stage))
}

pub fn run(
    file: Option<String>,
    ways_dir: Option<String>,
    corpus: Option<String>,
    project: Option<&str>,
    tsv: bool,
    body_rank: Option<crate::config::BodyRank>,
    unrelated: bool,
) -> Result<()> {
    let path = PathBuf::from(file.unwrap_or_else(|| DEFAULT_FILE.to_string()));
    let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let probes = parse(&text)?;

    let mut guard = None;
    if let (Some(ways), Some(corpus)) = (&ways_dir, &corpus) {
        let corpus = PathBuf::from(corpus);
        let artifacts = if corpus.is_dir() {
            corpus
        } else if corpus.file_name().is_some_and(|n| n == "ways-corpus-en.jsonl") {
            corpus.parent().map(Path::to_path_buf).unwrap_or_default()
        } else {
            bail!("--corpus names {}, which is not ways-corpus-en.jsonl; pass that file or its directory", corpus.display());
        };
        if !artifacts.join("ways-corpus-en.jsonl").is_file() {
            bail!("no ways-corpus-en.jsonl in {} (build it: ways corpus --ways-dir {ways} --output DIR)", artifacts.display());
        }
        let ways_dir = PathBuf::from(ways);
        if !ways_dir.is_dir() {
            bail!("{} is not a directory", ways_dir.display());
        }
        guard = Some(isolate(Isolation { ways_dir, artifacts }));
    }

    let project_dir = project.map(str::to_string).unwrap_or_else(crate::util::project_dir);
    // The scan reads its thresholds and toggles from the global config; so does the probe.
    let admission = crate::config::global().admission;
    let body_rank = body_rank.unwrap_or(crate::config::global().body_rank);
    if crate::paths::way_embed().is_none() || !crate::paths::corpus_dir().join(crate::paths::EN_MODEL).is_file() {
        bail!("embedding engine unavailable (way-embed or the MiniLM model is missing; run make setup)");
    }

    if unrelated {
        print_unrelated(&probes, &project_dir, admission, body_rank);
        drop(guard);
        return Ok(());
    }

    let mut results = Vec::with_capacity(probes.len());
    for probe in &probes {
        let scan = if is_tool(&probe.kind) {
            crate::cmd::scan::probe::bash(&probe.prompt, &project_dir)
        } else {
            crate::cmd::scan::probe::prompt(&probe.prompt, &project_dir, admission, body_rank)
        };
        results.push(evaluate(probe, &scan, is_disabled));
    }
    drop(guard);

    if tsv {
        print_tsv(&results);
    } else {
        print_table(&results);
    }
    print_summary(&results, admission.as_str(), body_rank, &project_dir);
    Ok(())
}

/// Run each row as an unrelated prompt: the way ranked first and its score
/// (summed share on the late path, calibrated probability on the single-vector
/// path), then the ways that fired. Ends with how many rows fired anything.
fn print_unrelated(probes: &[Probe], project_dir: &str, admission: crate::config::Admission, body_rank: crate::config::BodyRank) {
    println!("path\ttop_way\ttop_score\tfired\tprompt");
    let (mut fired_rows, mut late_rows) = (0usize, 0usize);
    for probe in probes {
        let scan = if is_tool(&probe.kind) {
            crate::cmd::scan::probe::bash(&probe.prompt, project_dir)
        } else {
            crate::cmd::scan::probe::prompt(&probe.prompt, project_dir, admission, body_rank)
        };
        let top = scan.rows.first();
        let fired: Vec<&str> = scan.fired.iter().map(|(id, _)| id.as_str()).collect();
        fired_rows += usize::from(!fired.is_empty());
        late_rows += usize::from(scan.late);
        println!(
            "{}\t{}\t{}\t{}\t{}",
            if is_tool(&probe.kind) { "bash" } else if scan.late { "late" } else { "single" },
            top.map_or("-", |r| r.id.as_str()),
            opt(top.map(|r| r.score)),
            if fired.is_empty() { "-".to_string() } else { fired.join(",") },
            probe.prompt,
        );
    }
    println!();
    let body_rank = if body_rank.is_on() { " · body rank: on" } else { "" };
    println!("unrelated: {} rows, {fired_rows} fired a way, {late_rows} on the late path · admission: {}{body_rank}", probes.len(), admission.as_str());
}

fn print_tsv(results: &[Outcome]) {
    println!("expected_way\trole\tkind\tpath\trank\tshare\tmargin\tstage\tfires\tsibling_over\tpass\tpeak\tconfirm\talso_fired\tprompt");
    for r in results {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            r.probe.expected,
            r.probe.role,
            r.probe.kind,
            path_str(r),
            rank_str(r),
            opt(r.share),
            r.margin.map_or("-".to_string(), |m| format!("{m:+.4}")),
            stage_str(r),
            r.fires,
            r.sibling_over.join(","),
            r.pass(),
            opt(r.peak),
            opt(r.confirm),
            r.also_fired.join(","),
            r.probe.prompt,
        );
    }
}

fn print_table(results: &[Outcome]) {
    println!(
        "{:<40}  {:<6}  {:<18}  {:<6}  {:>4}  {:>6}  {:>7}  {:<15}  {:<5}  {:<4}  sibling_over",
        "expected_way", "role", "kind", "path", "rank", "share", "margin", "stage", "fires", "pass"
    );
    println!("{}", "-".repeat(137));
    for r in results {
        println!(
            "{:<40}  {:<6}  {:<18}  {:<6}  {:>4}  {:>6}  {:>7}  {:<15}  {:<5}  {:<4}  {}",
            r.probe.expected,
            r.probe.role,
            r.probe.kind,
            path_str(r),
            rank_str(r),
            r.share.map_or("-".to_string(), |v| format!("{v:.3}")),
            r.margin.map_or("-".to_string(), |m| format!("{m:+.3}")),
            stage_str(r),
            r.fires,
            if r.scored() { r.pass().to_string() } else { "-".to_string() },
            r.sibling_over.join(","),
        );
    }
}

fn print_summary(results: &[Outcome], admission: &str, body_rank: crate::config::BodyRank, project_dir: &str) {
    let (all, roles, kinds) = tally(results);
    let skipped = results.iter().filter(|r| !r.scored()).count();
    println!();
    // Named only when on, so a run with the flag off prints what it always has.
    let body_rank = if body_rank.is_on() { " · body rank: on" } else { "" };
    println!("probes: {} total, {} scored, {skipped} skipped · admission: {admission}{body_rank} · project: {project_dir}", results.len(), all.scored);
    let off = crate::config::global().disabled_domains.join(",");
    println!("operator config: disabled domains: {}", if off.is_empty() { "none" } else { off.as_str() });
    println!("pass = the expected way fires and no must_not way outranks it; top-1 = the expected way ranks first");
    println!();
    println!("{:<22}  {:>6}  {:>6}  {:>8}  {:>6}  {:>8}  {:>6}  {:>8}", "group", "scored", "pass", "pass%", "top-1", "top-1%", "fires", "fires%");
    let line = |name: &str, t: &Tally| {
        println!(
            "{name:<22}  {:>6}  {:>6}  {:>8}  {:>6}  {:>8}  {:>6}  {:>8}",
            t.scored, t.passed, pct(t.passed, t.scored), t.top1, pct(t.top1, t.scored), t.fired, pct(t.fired, t.scored)
        );
    };
    line("overall", &all);
    for (k, t) in &roles {
        line(&format!("role={k}"), t);
    }
    for (k, t) in &kinds {
        line(&format!("kind={k}"), t);
    }
    let fallback = results.iter().filter(|r| r.scored() && !r.late && !is_tool(&r.probe.kind)).count();
    let late = results.iter().filter(|r| r.scored() && path_str(r) == "late").count();
    let bash = results.iter().filter(|r| r.scored() && path_str(r) == "bash").count();
    let boosted = results.iter().filter(|r| r.boost_exercised).count();
    println!();
    println!("path: {late} late interaction, {fallback} single-vector fallback (late interaction could not run), {bash} bash lane (scored probes)");
    println!("parent boost exercised by a parent fired in the same probe: {boosted} probes");
    println!("parent boost from an earlier turn's parent marker: not exercised (each probe is a fresh session)");
    for r in results.iter().filter(|r| !r.scored()) {
        println!("skipped: {} ({}): {} ({})", r.probe.expected, r.probe.kind, r.skipped.as_deref().unwrap_or(""), r.stage);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::scan::probe::ProbeRow;
    use std::collections::HashMap;

    fn scan(rows: &[(&str, f64)], stages: &[(&str, &'static str)]) -> ProbeScan {
        ProbeScan {
            rows: rows.iter().map(|(id, s)| ProbeRow { id: id.to_string(), score: *s, peak: None, confirm: None }).collect(),
            fired: Vec::new(),
            stages: stages.iter().map(|(k, v)| (k.to_string(), *v)).collect::<HashMap<_, _>>(),
            late: true,
            boost_exercised: false,
        }
    }

    fn probe(expected: &str, must_not: &[&str]) -> Probe {
        Probe {
            prompt: "p".into(),
            expected: expected.into(),
            kind: "direct".into(),
            role: "leaf".into(),
            must_not: must_not.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn parse_reads_header_rows_and_must_not() {
        let t = "prompt\texpected_way\tkind\trole\tmust_not\nhello there\ta/b\tdirect\tleaf\ta/c,a/d\nsolo\ta\tsituational\troot\t\n";
        let p = parse(t).unwrap();
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].must_not, vec!["a/c".to_string(), "a/d".to_string()]);
        assert!(p[1].must_not.is_empty());
        assert_eq!(p[1].role, "root");
        assert!(parse("wrong\theader\n").is_err());
    }

    #[test]
    fn a_fired_first_ranked_way_passes_with_its_margin() {
        let s = scan(&[("a/b", 0.5), ("a/c", 0.25)], &[("a/b", "fired"), ("a/c", "not-admitted")]);
        let r = evaluate(&probe("a/b", &["a/c"]), &s, |_| false);
        assert_eq!((r.rank, r.stage.as_str(), r.fires), (Some(1), "fired", true));
        assert!((r.margin.unwrap() - 0.25).abs() < 1e-9);
        assert!(r.sibling_over.is_empty());
        assert!(r.pass() && r.top1());
    }

    #[test]
    fn a_must_not_way_ahead_of_the_expected_way_fails_it_even_when_it_fires() {
        let s = scan(&[("a/c", 0.6), ("a/b", 0.3), ("a/d", 0.1)], &[("a/b", "fired"), ("a/c", "fired")]);
        let r = evaluate(&probe("a/b", &["a/c", "a/d"]), &s, |_| false);
        assert_eq!(r.rank, Some(2));
        assert_eq!(r.sibling_over, vec!["a/c".to_string()]);
        assert!(r.fires && !r.pass() && !r.top1());
        assert!(r.margin.unwrap() < 0.0);
    }

    #[test]
    fn a_way_that_does_not_fire_reports_its_stage_and_fails() {
        let s = scan(&[("a/b", 0.1)], &[("a/b", "not-admitted")]);
        let r = evaluate(&probe("a/b", &[]), &s, |_| false);
        assert_eq!((r.stage.as_str(), r.fires, r.pass()), ("not-admitted", false, false));
        assert!((r.margin.unwrap() - 0.1).abs() < 1e-9);
    }

    #[test]
    fn an_unknown_expected_way_is_not_in_tree_and_outranked_by_any_ranked_must_not() {
        let s = scan(&[("a/c", 0.4)], &[]);
        let r = evaluate(&probe("a/zzz", &["a/c", "a/q"]), &s, |_| false);
        assert_eq!((r.rank, r.stage.as_str()), (None, "not-in-tree"));
        assert_eq!(r.sibling_over, vec!["a/c".to_string()]);
    }

    #[test]
    fn a_way_that_cannot_fire_on_the_lane_is_skipped_and_leaves_the_rates() {
        let s = scan(&[("a/c", 0.4)], &[("a/b", "state-trigger"), ("a/c", "fired")]);
        let skipped = evaluate(&probe("a/b", &[]), &s, |_| false);
        assert_eq!(skipped.skipped.as_deref(), Some("lane-ineligible"));
        assert!(!skipped.scored() && !skipped.pass());
        let ok = evaluate(&probe("a/c", &[]), &s, |_| false);
        assert!(ok.scored());
        let (all, _, kinds) = tally(&[skipped, ok]);
        assert_eq!((all.scored, all.passed, all.top1), (1, 1, 1));
        assert_eq!(kinds["direct"].scored, 1);
    }

    #[test]
    fn a_trigger_way_with_a_regex_that_misses_is_a_failure_not_a_skip() {
        let s = scan(&[], &[("a/b", "regex-miss")]);
        let r = evaluate(&probe("a/b", &[]), &s, |_| false);
        assert!(r.scored() && !r.pass());
        assert_eq!(r.stage, "regex-miss");
    }

    #[test]
    fn a_pattern_only_way_that_fired_is_not_outranked_by_a_must_not_row() {
        let fired = scan(&[("a/c", 0.6)], &[("a/b", "fired"), ("a/c", "below-threshold")]);
        let r = evaluate(&probe("a/b", &["a/c"]), &fired, |_| false);
        assert!(r.rank.is_none() && r.sibling_over.is_empty() && r.pass());
        let missed = scan(&[("a/c", 0.6)], &[("a/b", "regex-miss")]);
        let r = evaluate(&probe("a/b", &["a/c"]), &missed, |_| false);
        assert_eq!(r.sibling_over, vec!["a/c".to_string()]);
    }

    #[test]
    fn an_unembeddable_way_is_skipped_on_the_lane() {
        let s = scan(&[], &[("a/b", "not-embeddable")]);
        let r = evaluate(&probe("a/b", &[]), &s, |_| false);
        assert_eq!(r.skipped.as_deref(), Some("lane-ineligible"));
    }

    #[test]
    fn a_way_the_operator_disabled_is_skipped_not_failed() {
        let s = scan(&[], &[]);
        let r = evaluate(&probe("a/b", &[]), &s, |id| id.starts_with("a/"));
        assert_eq!(r.skipped.as_deref(), Some("disabled"));
        let r = evaluate(&probe("zzz", &[]), &s, |_| false);
        assert!(r.scored(), "an unknown way that is not disabled stays a failure");
    }

    #[test]
    fn tally_groups_by_role_and_kind_and_leaves_skipped_rows_out() {
        let s = scan(&[("a/b", 0.5)], &[("a/b", "fired")]);
        let ok = evaluate(&probe("a/b", &[]), &s, |_| false);
        let mut bad = ok.clone();
        bad.fires = false;
        bad.probe.role = "root".into();
        let mut skipped = ok.clone();
        skipped.skipped = Some("tool lane".into());
        let (all, roles, kinds) = tally(&[ok, bad, skipped]);
        assert_eq!((all.scored, all.passed, all.top1), (2, 1, 2));
        assert_eq!(roles["leaf"].passed, 1);
        assert_eq!(roles["root"].passed, 0);
        assert_eq!(kinds["direct"].scored, 2);
    }

    /// Live path on a fixture tree: skips when the engine is absent.
    #[test]
    fn a_fixture_tree_probes_deterministically_through_the_real_scan() {
        let engine = crate::paths::corpus_dir();
        if crate::paths::way_embed().is_none() || !engine.join(crate::paths::EN_MODEL).is_file() {
            eprintln!("skip: way-embed or the MiniLM model is absent");
            return;
        }
        let root = std::env::temp_dir().join(format!("ways-probe-fixture-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let ways = root.join("ways");
        for (dir, desc, vocab, body, extra) in [
            ("garden", "watering and pruning tomato plants in a vegetable garden", "tomato garden watering pruning soil seedlings compost", "Water tomato plants at the base in the morning. Prune suckers from tomato plants weekly.", ""),
            ("proxy", "configuring an nginx reverse proxy with tls", "nginx proxy upstream tls certificate reverse load balancer", "Configure the nginx reverse proxy upstream block and the tls certificate paths.", ""),
            ("watcher", "watching the disk quota on shared hosts", "disk quota watcher hosts usage", "Check the disk quota before a large copy.", "trigger: session-start\npattern: \\bzzqqmarker\\b\n"),
        ] {
            std::fs::create_dir_all(ways.join(dir)).unwrap();
            std::fs::write(
                ways.join(dir).join(format!("{dir}.md")),
                format!("---\ndescription: {desc}\nvocabulary: {vocab}\nscope: agent\n{extra}---\n# {dir}\n\n{body}\n"),
            )
            .unwrap();
        }
        let out = root.join("corpus");
        if crate::cmd::corpus::run(Some(ways.to_string_lossy().into()), Some(out.to_string_lossy().into()), true, false, false).is_err() {
            eprintln!("skip: corpus build failed");
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        let _guard = isolate(Isolation { ways_dir: ways.clone(), artifacts: out });
        let p = Probe {
            prompt: "my tomato plants need watering every morning. the seedlings also need pruning and compost.".into(),
            expected: "garden".into(),
            kind: "direct".into(),
            role: "root".into(),
            must_not: vec!["proxy".into()],
        };
        let project = root.to_string_lossy().to_string();
        let run = || evaluate(&p, &crate::cmd::scan::probe::prompt(&p.prompt, &project, crate::config::Admission::Share, crate::config::BodyRank::Off), |_| false);
        let (a, b) = (run(), run());
        assert_eq!(a, b, "a probe carries no state between runs");
        assert_eq!(a.rank, Some(1), "{a:?}");
        assert!(a.sibling_over.is_empty(), "{a:?}");
        assert_eq!(a.stage, "fired", "{a:?}");
        assert!(!a.boost_exercised);
        assert!(a.late, "a two-sentence surface runs the late-interaction matcher: {a:?}");

        // A joined row (situational sentence, then direct sentence) takes the late path.
        let joined = Probe { prompt: "my tomato plants need watering every morning. prune the seedlings and add compost".into(), kind: "joined".into(), ..p.clone() };
        let j = evaluate(&joined, &crate::cmd::scan::probe::prompt(&joined.prompt, &project, crate::config::Admission::Share, crate::config::BodyRank::Off), |_| false);
        assert!(j.late && path_str(&j) == "late", "{j:?}");
        assert_eq!(j.stage, "fired", "{j:?}");

        // One sentence has nothing to chunk: the single-vector fail-safe decides.
        let one = evaluate(&p, &crate::cmd::scan::probe::prompt("my tomato plants need watering", &project, crate::config::Admission::Share, crate::config::BodyRank::Off), |_| false);
        assert!(!one.late && path_str(&one) == "single", "{one:?}");

        // The Bash lane, with the prompt as the tool description.
        let bash = |text: &str, expected: &str| {
            let bp = Probe { prompt: text.into(), expected: expected.into(), kind: "situational-tool".into(), role: "root".into(), must_not: Vec::new() };
            evaluate(&bp, &crate::cmd::scan::probe::bash(text, &project), |_| false)
        };
        let hit = bash("check zzqqmarker on the host", "watcher");
        assert_eq!((hit.stage.as_str(), hit.fires), ("fired", true), "{hit:?}");
        let miss = bash("check the host", "watcher");
        assert_eq!(miss.stage, "regex-miss", "{miss:?}");
        assert!(miss.scored() && !miss.pass());
        let _ = std::fs::remove_dir_all(&root);
    }
}
