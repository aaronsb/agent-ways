// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! The command (Bash) lane: `PreToolUse` on a shell command.

use anyhow::Result;
use command_describer::Segment;
use std::collections::HashSet;

use crate::cmd::show::ContextBudget;
use crate::session;

use super::candidates::{check_when, collect_checks, collect_with_off};
use super::decision::Surface;
use super::gate::{self, Trigger};
use super::judged;
use super::order::{order_hits, Hit};
use super::scoring::{batch_embed_score, capture_show_check, capture_show_way, EmbedScores};
use super::{
    capped_span, check_semantic_score, compile_trigger, discard, effective_thresholds, effective_thresholds_in_scan,
    emit_hook_context, has_shown_ancestor, lookbehind, reduce, regex_span, WayCandidate, BUDGET_COMMAND,
};

// ── Command scan ────────────────────────────────────────────────

pub fn command(
    cmd: &str,
    description: Option<&str>,
    session_id: &str,
    project: Option<&str>,
    transcript: Option<&str>,
) -> Result<()> {
    crate::cmd::show::set_firing_transcript(transcript);
    let project_dir = project
        .map(|s| s.to_string())
        .unwrap_or_else(crate::util::project_dir);

    session::bump_epoch(session_id);
    let scope = session::detect_scope(session_id);
    let (candidates, off) = collect_with_off(&project_dir);

    let mut context = String::new();
    let mut budget = ContextBudget::hook();

    // The ways' semantic match reads the call's operative verbs and its
    // description, never the agent's prose.
    //
    // References:
    // - ADR-188 item 3b: the matcher's text.
    let command = ShellCommand::parse(cmd);
    let reduced_for_embed = reduce::reduce_for_embed(&command.embed_surface(description), BUDGET_COMMAND);
    let embed_matches = batch_embed_score(&reduced_for_embed);

    let surface = CommandSurface { command: &command, description, session_id, scope: &scope, project_dir: &project_dir };
    let hits = command_hits(&surface, &candidates, &embed_matches, &HashSet::new());
    let checks = collect_checks(&project_dir);
    // A check's semantic score keeps the lookbehind: the assistant's prose
    // since the last human turn, the command and the description, cut to the
    // command budget, since heredoc bodies and payloads can run kilobytes.
    // The pass runs only when a check in scope asks for it.
    //
    // References:
    // - ADR-160 stage 1: the lookbehind.
    // - ADR-130: the embed input is capped.
    // - ADR-188 item 3e: the check-scoring pass keeps the lookbehind.
    let check_scores = std::cell::OnceCell::new();
    let check_embed = || {
        check_scores.get_or_init(|| {
            let lookbehind = lookbehind::intent(session_id);
            let query = format!("{} {} {}", lookbehind.as_deref().unwrap_or(""), cmd, description.unwrap_or(""));
            batch_embed_score(&reduce::reduce_for_embed(&query, BUDGET_COMMAND))
        })
    };
    let matched_checks = check_hits(&checks, &command, session_id, &scope, &project_dir, &check_embed);

    // The matcher's hits, then the checks, each marked by the read-only
    // refire check, go to the judge (scan/judged.rs) before any fire is
    // recorded.
    let scan = judged::Scan::begin(session_id);
    let by_id: std::collections::HashMap<&str, &WayCandidate> = candidates.iter().map(|w| (w.id.as_str(), w)).collect();
    let mut judge_candidates: Vec<judged::Candidate> = hits
        .iter()
        .filter_map(|hit| {
            let way = by_id.get(hit.id.as_str())?;
            Some(judged::Candidate {
                key: hit.id.clone(),
                description: way.description.clone(),
                trigger: way_trigger(way, &command, hit.payload.1.is_some()),
                channel: hit.payload.0.to_string(),
                p: hit.score,
                open: scan.open(&hit.id),
            })
        })
        .collect();
    judge_candidates.extend(matched_checks.iter().map(|(check, score)| judged::check_candidate(check, *score, "bash")));
    let lane = judged::Lane {
        surface: Surface::Command,
        hook_event: "PreToolUse".into(),
        scope: scope.clone(),
        project_dir: project_dir.clone(),
        session_id: session_id.to_string(),
        command: Some(judged::audit_command(cmd)),
    };

    let hit_of: std::collections::HashMap<&str, &BashHit> = hits.iter().map(|h| (h.id.as_str(), h)).collect();
    let mut shown: HashSet<String> = HashSet::new();
    let turns = || judged::tool_turns(session_id, &judged::bash_call(cmd, description, &command.segments, &judged::tldr_summaries()));
    judged::run(&lane, &scan, &judge_candidates, turns, gate::TOOL_WAIT, |c| {
        if let Some((check, score)) = judged::check_of(&c.key).and_then(|id| matched_checks.iter().find(|(w, _)| w.id == id)) {
            let out = capture_show_check(&check.id, session_id, "bash", *score, Some(&mut budget));
            let delivered = !out.is_empty();
            context.push_str(&out);
            return if delivered { "fired" } else { "held" };
        }
        let Some(hit) = hit_of.get(c.key.as_str()) else { return "error" };
        let (channel, span, needs_parent) = &hit.payload;
        if *needs_parent && !has_shown_ancestor(&hit.id, &shown) {
            return "withheld_for_parent";
        }
        // A regex hit carries its span and no surface; a semantic hit carries
        // the embedded surface and no span.
        let embedded = if span.is_some() { None } else { Some(reduced_for_embed.as_str()) };
        let out = capture_show_way(&hit.id, session_id, channel, hit.score, span.as_deref(), embedded, Some(&mut budget));
        if !out.body.is_empty() {
            shown.insert(hit.id.clone());
            context.push_str(&out.body);
        }
        out.outcome.as_str()
    });
    let fired: HashSet<String> = hits.iter().map(|h| h.id.clone()).collect();
    log_discarded(&surface, &candidates, &fired, &shown, &off, &embed_matches);

    if !context.is_empty() {
        emit_hook_context("PreToolUse", context.trim_end());
    }

    Ok(())
}

/// What fired a Bash-lane way, for the judge's switch: its `commands:`
/// regex, its `pattern:` on the description, or the embedding matcher.
fn way_trigger(way: &WayCandidate, command: &ShellCommand, regex: bool) -> Trigger {
    if !regex {
        Trigger::Semantic
    } else if way.commands.as_deref().is_some_and(|p| command.span(p).is_some()) {
        Trigger::Declared
    } else if way.pattern_strict {
        Trigger::Strict
    } else {
        Trigger::Keyword
    }
}

/// The checks that match a Bash call, with their match score: 3.0 for a
/// `commands:` regex, else the semantic score of a check that declares
/// `description` and `vocabulary`.
fn check_hits<'c, 'e>(
    checks: &'c [WayCandidate],
    command: &ShellCommand,
    session_id: &str,
    scope: &str,
    project_dir: &str,
    embed_matches: &dyn Fn() -> &'e EmbedScores,
) -> Vec<(&'c WayCandidate, f64)> {
    let mut out = Vec::new();
    for check in checks {
        if !session::scope_matches(&check.scope, scope) || !check_when(&check.when_project, &check.when_file_exists, project_dir) {
            continue;
        }
        let mut match_score: f64 = 0.0;
        if let Some(ref cmds_pattern) = check.commands {
            if command.span(cmds_pattern).is_some() {
                match_score = 3.0;
            }
        }
        if match_score == 0.0 && !check.description.is_empty() && !check.vocabulary.is_empty() {
            match_score = check_semantic_score(check, session_id, embed_matches());
        }
        if match_score > 0.0 {
            out.push((check, match_score));
        }
    }
    out
}

/// A Bash call parsed once into its ordered segments. A `commands:` regex
/// runs against each operative segment's own text, so `^gh` matches after
/// `git push &&`, and one program's arguments cannot satisfy a pattern
/// meant for another. It also runs against the text from the program word,
/// so `^gh` matches `GH_TOKEN=x gh …` and `timeout 60 gh …`. Heredoc
/// bodies, navigation (`cd`, `echo`, assignments) and filters after a pipe
/// are not matched. The semantic match reads the operative verbs
/// ([`ShellCommand::embed_surface`]), and the judge reads every segment
/// described.
///
/// References:
/// - ADR-188 item 3a: `commands:` per operative segment.
#[derive(Default)]
pub(super) struct ShellCommand {
    pub(super) segments: Vec<Segment>,
}

impl ShellCommand {
    pub(super) fn parse(cmd: &str) -> Self {
        ShellCommand { segments: command_describer::parse(cmd) }
    }

    fn operative(&self) -> impl Iterator<Item = &Segment> {
        self.segments.iter().filter(|s| s.is_operative())
    }

    /// The first match of a `commands:` regex in any operative segment,
    /// capped for the event log like [`regex_span`]; `None` for no match or
    /// an invalid pattern.
    pub(super) fn span(&self, pattern: &str) -> Option<String> {
        let re = compile_trigger(pattern)?;
        self.operative()
            .find_map(|s| re.find(&s.text).or_else(|| re.find(s.program_text())))
            .map(|m| capped_span(m.as_str()))
    }

    /// The text the semantic match embeds: each operative segment's verb
    /// (program, subcommand and flags, with arguments and paths dropped), in
    /// order, then the tool call's description. The agent's prose is not
    /// part of it: the surface with prose, command and description was on
    /// topic in 3.4% of its fires, and verbs plus the description fired 473
    /// times on 200 calls where that surface fired 1,201, on topic in 7.6%.
    ///
    /// References:
    /// - ADR-188 item 3b: the matcher's text is the verbs and the description.
    /// - ADR-707 studies 1 and 2: today's surface, and surface C.
    pub(super) fn embed_surface(&self, description: Option<&str>) -> String {
        let verbs: Vec<String> = self.operative().map(Segment::verb).filter(|v| !v.is_empty()).collect();
        let description = description.map(str::trim).filter(|d| !d.is_empty());
        verbs.iter().map(String::as_str).chain(description).collect::<Vec<_>>().join(" ")
    }
}

/// What the Bash lane matches a command against.
pub(super) struct CommandSurface<'a> {
    pub(super) command: &'a ShellCommand,
    pub(super) description: Option<&'a str>,
    pub(super) session_id: &'a str,
    pub(super) scope: &'a str,
    pub(super) project_dir: &'a str,
}

/// A Bash-lane hit; the payload is (channel, matched span, fired only on
/// this scan's parent boost).
type BashHit = Hit<(&'static str, Option<String>, bool)>;

/// The Bash lane's way hits for one command, in admission order: the command
/// and description regexes, then the semantic lane on the tool description.
/// `fired_in_scan` holds ways already fired in this scan, whose children they
/// boost. Shared by the scan and the probe scorer.
pub(super) fn command_hits(
    surface: &CommandSurface<'_>,
    candidates: &[WayCandidate],
    embed_matches: &EmbedScores,
    fired_in_scan: &HashSet<String>,
) -> Vec<BashHit> {
    let CommandSurface { command, description, session_id, scope, project_dir } = *surface;
    // Way matching: commands regex + pattern regex + semantic (ADR-155 §4).
    // Hits are collected, then admitted in a fixed order (scan/order.rs).
    // Payload: (channel, matched span, fired only on this scan's parent boost).
    let mut hits: Vec<BashHit> = Vec::new();
    let mut fired_ids: HashSet<String> = fired_in_scan.clone();
    for way in candidates {
        if !session::scope_matches(&way.scope, scope) {
            continue;
        }
        if !check_when(&way.when_project, &way.when_file_exists, project_dir) {
            continue;
        }

        // Commands regex first, per operative segment, then the description
        // pattern — capture the span of whichever matched (ADR-153 §3), with
        // the pattern that matched it.
        let matched = way
            .commands
            .as_deref()
            .and_then(|p| command.span(p).map(|s| (p, s)))
            .or_else(|| match (description, way.pattern.as_deref()) {
                // Pattern compiles case-insensitively (ADR-157), so match the
                // description in its original case for a truer captured span.
                (Some(desc), Some(pat)) => regex_span(pat, desc).map(|s| (pat, s)),
                _ => None,
            });

        if let Some((pat, span)) = matched {
            fired_ids.insert(way.id.clone());
            hits.push(Hit::explicit(&way.id, pat, &span, ("bash", Some(span.clone()), false)));
            continue;
        }

        // Semantic lane at the bash surface (ADR-155 §4): the tool
        // `description` is Claude's own natural-language statement of intent,
        // scored with the same per-way thresholds as the prompt surface,
        // against embeddings this event already computed for checks. State-
        // triggered ways are excluded, mirroring the task surface — their
        // trigger is a condition, not a topic. No near-miss logging here:
        // bash events are the highest-volume surface, and the tuning stream
        // (ADR-134) is fed by the prompt/task surfaces.
        if way.trigger.is_some() {
            continue;
        }
        let (t, scan_boost) = effective_thresholds_in_scan(way, session_id, &fired_ids);
        let prob_en = embed_matches.prob_en(&way.corpus_id, way.embeddable());
        let prob_multi = embed_matches.prob_multi(&way.corpus_id, way.embeddable());
        // `semantic:` prefix keeps every consumer that special-cases semantic
        // channels (drill-down's "no recoverable term" note, span handling)
        // treating this lane correctly.
        let fired = if prob_en.is_some_and(|p| p >= t.semantic) {
            Some(("semantic:bash:en", prob_en))
        } else if prob_multi.is_some_and(|p| p >= t.semantic) {
            Some(("semantic:bash:multi", prob_multi))
        } else {
            None
        };
        if let Some((channel, score)) = fired {
            // Fired only on this scan's parent boost: shown only with that parent.
            let base = effective_thresholds(way, session_id).semantic;
            let needs_parent = scan_boost && !score.is_some_and(|p| p >= base);
            fired_ids.insert(way.id.clone());
            hits.push(Hit::scored(&way.id, score, (channel, None, needs_parent)));
        }
    }
    order_hits(&mut hits);
    hits
}

/// The Bash lane's shadow pass: [`command_hits`] over each switched-off way
/// alone, beside the enabled ways the scan `fired` so a child of one keeps its
/// in-scan boost, each hit logged as a discarded fire. A hit that needed that
/// boost counts only when an ancestor was `shown`, as delivery requires.
/// Switched-off ways never lift each other. A semantic hit carries its scores
/// beside the enabled ways that compete on this lane.
pub(super) fn log_discarded(
    surface: &CommandSurface<'_>,
    candidates: &[WayCandidate],
    fired: &HashSet<String>,
    shown: &HashSet<String>,
    off: &discard::Switched,
    embed_matches: &EmbedScores,
) {
    if off.ways.is_empty() {
        return;
    }
    let enabled: std::collections::HashMap<&str, &str> = candidates
        .iter()
        .filter(|w| w.embeddable() && w.trigger.is_none() && session::scope_matches(&w.scope, surface.scope))
        .filter(|w| check_when(&w.when_project, &w.when_file_exists, surface.project_dir))
        .map(|w| (w.corpus_id.as_str(), w.id.as_str()))
        .collect();
    let at = discard::LaneLog { lane: "command", scope: surface.scope, session: surface.session_id, project: surface.project_dir };
    for way in &off.ways {
        for hit in command_hits(surface, std::slice::from_ref(way), embed_matches, fired) {
            let (trigger, span, needs_parent) = &hit.payload;
            if *needs_parent && !has_shown_ancestor(&hit.id, shown) {
                continue;
            }
            let scores = match span {
                Some(_) => None,
                None => discard::single_scores(embed_matches, &enabled, way),
            };
            discard::log(off, &at, &discard::Hit { way: &hit.id, trigger, span: span.as_deref(), scores });
        }
    }
}

#[cfg(test)]
mod shell_command_tests {
    //! ADR-188 item 3a: a `commands:` regex runs per operative segment.
    use super::ShellCommand;

    fn span(pattern: &str, cmd: &str) -> Option<String> {
        ShellCommand::parse(cmd).span(pattern)
    }

    #[test]
    fn an_anchored_pattern_matches_after_a_connector() {
        assert_eq!(span(r"^gh\ |^gh$", "git push && gh pr create --fill").as_deref(), Some("gh "));
        assert_eq!(span(r"^git\ commit", "cd repo && git add -A && git commit -m x").as_deref(), Some("git commit"));
        assert_eq!(span(r"^ssh\ ", "ls; ssh host uptime").as_deref(), Some("ssh "));
    }

    #[test]
    fn one_programs_arguments_do_not_satisfy_another_programs_pattern() {
        // The raw string matches each of these; no operative segment does.
        assert_eq!(span(r"\battend\b", "cd ~/plugins/attend && ls"), None);
        assert_eq!(span(r"\bmake\b", r#"echo "make it stick" && ls"#), None);
        assert_eq!(span(r"git\ push", "git log | grep 'git push'"), None);
        assert_eq!(span(r"git\ diff.*\.md", "git diff --stat; cat notes.md"), None);
        assert_eq!(span(r"gh-tasks", "F=hooks/issues/gh-tasks; head -1 $F"), None);
        // The program's own segment still matches.
        assert_eq!(span(r"\bmake\b", r#"echo "build" && make test"#).as_deref(), Some("make"));
    }

    #[test]
    fn a_pattern_matches_from_the_program_word() {
        assert_eq!(span(r"^gh\ ", "GH_TOKEN=x gh pr create").as_deref(), Some("gh "));
        assert_eq!(span(r"^gh\ ", "cd x && timeout 580 gh pr checks 29 --watch").as_deref(), Some("gh "));
        assert_eq!(span(r"^cargo", "TMPDIR=/tmp/t cargo test -p ways").as_deref(), Some("cargo"));
        // The segment text, wrapper included, is still matched.
        assert_eq!(span(r"\bsudo\b", "sudo -n true && yay -S foo").as_deref(), Some("sudo"));
    }

    #[test]
    fn an_anchored_pattern_matches_inside_eval_and_a_shell_command_string() {
        for cmd in [
            "eval \"git push --force\"",
            "eval git push",
            "bash -c \"git push origin main\"",
            "sh -c 'cd /tmp && git push'",
            "bash -lc 'git push'",
            "sudo sh -c 'git push'",
            "env FOO=1 bash -c \"git push\"",
            "bash -c \"sh -c 'git push'\"",
        ] {
            assert_eq!(span(r"^git\ push", cmd).as_deref(), Some("git push"), "{cmd}");
        }
        for cmd in ["bash deploy.sh", "eval", "bash", "bash <<EOF\ngit push\nEOF"] {
            assert_eq!(span(r"^git\ push", cmd), None, "{cmd}");
        }
    }

    #[test]
    fn heredoc_text_does_not_match() {
        let body = "cat > notes.md <<'EOF'\nthen git commit and make test\nEOF\nls";
        assert_eq!(span(r"git\ commit", body), None);
        assert_eq!(span(r"\bmake\b", body), None);
        let pr = "gh pr create --title t --body \"$(cat <<'EOF'\nRan make test and git push.\nEOF\n)\"";
        assert_eq!(span(r"\bmake\b", pr), None);
        assert_eq!(span(r"git\ push", pr), None);
        assert_eq!(span(r"gh\ pr\ create", pr).as_deref(), Some("gh pr create"));
    }

    /// bounded-execution's pattern as shipped, per segment: flag alternatives
    /// match combined flags, and word alternatives keep their boundary.
    #[test]
    fn bounded_execution_pattern_matches_its_commands() {
        let way = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../hooks/ways/softwaredev/environment/bounded-execution/bounded-execution.md"
        );
        let text = std::fs::read_to_string(way).unwrap();
        let pattern = text.lines().find_map(|l| l.strip_prefix("commands: ")).unwrap();
        let fires = [
            "pacman -Syu --noconfirm",
            "pacman -S foo",
            "tail -f app.log",
            "tail -fn20 app.log",
            "tail -n 20 -f app.log",
            "tail -F app.log",
            "tail -100f app.log",
            "tail --follow=name x.log",
            "kill -9 123",
            "kill -- -123",
            "ssh -o BatchMode=yes host uptime",
            "ssh host uptime",
            "sudo -n true",
            "cd x && make test",
            "docker run --rm img",
            "tmux kill-session -t work",
        ];
        for cmd in fires {
            assert!(span(pattern, cmd).is_some(), "should fire: {cmd}");
        }
        let silent = [
            "pacman -Ss foo",
            "tail -n 5 app.log",
            "tail -100 app.log",
            "kill 123",
            "ssh-add -l",
            "ssh-keygen -t ed25519",
            "cmake ..",
            "npm installer",
            "echo 'tail -f app.log'",
        ];
        for cmd in silent {
            assert_eq!(span(pattern, cmd), None, "should not fire: {cmd}");
        }
    }

    /// The semantic match reads each operative segment's program,
    /// subcommand and flags, then the description. Arguments, paths,
    /// navigation and filters are dropped, and the agent's prose, which the
    /// lane read before, has no way in.
    ///
    /// References:
    /// - ADR-188 item 3b: the matcher's text.
    #[test]
    fn the_semantic_surface_is_the_operative_verbs_and_the_description() {
        let surface = |cmd: &str, desc: Option<&str>| ShellCommand::parse(cmd).embed_surface(desc);
        let s = surface("git branch -D foo", Some("Delete the merged branch"));
        assert_eq!(s, "git branch -D Delete the merged branch");
        assert!(!s.contains("foo"), "no argument: {s}");
        assert_eq!(surface("cd repo && cargo test -p ways | tail -5", Some("Run the tests")), "cargo test -p Run the tests");
        assert_eq!(surface("git show abc --stat", None), "git show --stat");
        assert_eq!(surface("cd /tmp", Some("  Go to tmp ")), "Go to tmp");
        assert_eq!(ShellCommand::default().embed_surface(Some("Probe text")), "Probe text");
        assert_eq!(surface("", None), "");
    }

    #[test]
    fn no_segment_or_a_bad_pattern_is_no_match() {
        assert_eq!(span(r"gh", ""), None);
        assert_eq!(span(r"(unclosed", "gh pr list"), None);
        assert_eq!(ShellCommand::default().span(r".*"), None);
    }
}

#[cfg(test)]
mod judge_trigger_tests {
    //! #1287: the Bash lane's candidates reach the judge as the kind that
    //! fired them, a semantic hit included.
    use super::*;
    use crate::cmd::scan::eligibility_tests::way;
    use ways_core::calibration::{Calibration, ModelCalibration};

    const CAL: ModelCalibration = ModelCalibration { a: 10.0, b: -2.5, auc: 1.0, n: 0 };

    #[test]
    fn each_bash_hit_is_judged_as_the_kind_that_fired_it() {
        let mut regex = way("cmd/way", "agent", None);
        regex.commands = Some(r"^git\ commit".into());
        let mut keyword = way("kw/way", "agent", None);
        keyword.pattern = Some("parser".into());
        let sem = way("sem/way", "agent", None);
        let cands = [regex, keyword, sem];
        let scores = EmbedScores {
            en: Some(vec![("sem/way".to_string(), 0.9), ("cmd/way".to_string(), 0.0), ("kw/way".to_string(), 0.0)]),
            multi: None,
            calibration: Calibration { en: Some(CAL), multi: None },
        };
        let command = ShellCommand::parse("git commit -m fix");
        let surface = CommandSurface {
            command: &command,
            description: Some("fix the parser"),
            session_id: "judge-trigger-test",
            scope: "agent",
            project_dir: "/p",
        };
        let hits = command_hits(&surface, &cands, &scores, &HashSet::new());
        let kinds: std::collections::HashMap<&str, Trigger> = hits
            .iter()
            .map(|h| (h.id.as_str(), way_trigger(cands.iter().find(|w| w.id == h.id).unwrap(), &command, h.payload.1.is_some())))
            .collect();
        assert_eq!(kinds.get("sem/way"), Some(&Trigger::Semantic), "{kinds:?}");
        assert_eq!(kinds.get("cmd/way"), Some(&Trigger::Declared));
        assert_eq!(kinds.get("kw/way"), Some(&Trigger::Keyword));
        assert!(kinds.values().all(|k| gate::JUDGED.judges(*k)), "the switch sends every one");
    }
}
