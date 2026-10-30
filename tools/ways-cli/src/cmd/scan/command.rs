//! The command (Bash) lane: `PreToolUse` on a shell command.

use anyhow::Result;
use command_describer::Segment;
use std::collections::HashSet;

use crate::cmd::show::ContextBudget;
use crate::session;

use super::candidates::{check_when, collect_checks, collect_with_off};
use super::discard;
use super::order::{order_hits, Hit};
use super::scoring::{batch_embed_score, capture_show_check, capture_show_way, EmbedScores};
use super::{
    capped_span, check_semantic_score, compile_trigger, effective_thresholds, effective_thresholds_in_scan,
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

    // One embed pass for the whole surface (ways and checks share it).
    // ADR-130: cap embed input. Heredoc bodies (gh pr create --body
    // "$(cat <<EOF…)"), curl -d JSON payloads, and similar argument-
    // body bash commands can run kilobytes long. The regex matchers
    // below see every operative segment in full; only the embed query is
    // reduced.
    // ADR-160 stage 1 (contextualize): a bare command is an action, not intent.
    // Prepend the assistant's prose since the last human turn — the reasoning that
    // led to this tool call — so the semantic lane matches on intent. Fail-safe:
    // None → command + tool description only (unchanged). The reducer trims the
    // combined surface to the command budget by sentence salience.
    let lookbehind = lookbehind::intent(session_id);
    let query_for_embed = format!(
        "{} {} {}",
        lookbehind.as_deref().unwrap_or(""),
        cmd,
        description.unwrap_or("")
    );
    let reduced_for_embed = reduce::reduce_for_embed(&query_for_embed, BUDGET_COMMAND);
    let embed_matches = batch_embed_score(&reduced_for_embed);

    let command = ShellCommand::parse(cmd);
    let surface = CommandSurface { command: &command, description, session_id, scope: &scope, project_dir: &project_dir };
    let hits = command_hits(&surface, &candidates, &embed_matches);
    log_discarded(&surface, &candidates, &off, &embed_matches);
    let mut shown: HashSet<String> = HashSet::new();
    for hit in &hits {
        let (channel, span, needs_parent) = &hit.payload;
        if *needs_parent && !has_shown_ancestor(&hit.id, &shown) {
            continue;
        }
        // A regex hit carries its span and no surface; a semantic hit carries
        // the embedded surface and no span.
        let surface = if span.is_some() { None } else { Some(reduced_for_embed.as_str()) };
        let out = capture_show_way(
            &hit.id,
            session_id,
            channel,
            hit.score,
            span.as_deref(),
            surface,
            Some(&mut budget),
        )
        .body;
        if !out.is_empty() {
            shown.insert(hit.id.clone());
            context.push_str(&out);
        }
    }

    // Check matching: commands regex + semantic scoring.
    let checks = collect_checks(&project_dir);

    for check in &checks {
        if !session::scope_matches(&check.scope, &scope) {
            continue;
        }
        if !check_when(&check.when_project, &check.when_file_exists, &project_dir) {
            continue;
        }

        let mut match_score: f64 = 0.0;

        if let Some(ref cmds_pattern) = check.commands {
            if command.span(cmds_pattern).is_some() {
                match_score = 3.0;
            }
        }

        if match_score == 0.0 && !check.description.is_empty() && !check.vocabulary.is_empty() {
            match_score = check_semantic_score(check, session_id, &embed_matches);
        }

        if match_score > 0.0 {
            let out = capture_show_check(&check.id, session_id, "bash", match_score, Some(&mut budget));
            if !out.is_empty() {
                context.push_str(&out);
            }
        }
    }

    if !context.is_empty() {
        emit_hook_context("PreToolUse", context.trim_end());
    }

    Ok(())
}

/// A Bash call as the `commands:` regexes read it (ADR-188 item 3a): its
/// operative segments, parsed once per call. A regex runs against each
/// segment's own text, so `^gh` matches after `git push &&`, and one
/// program's arguments cannot satisfy a pattern meant for another. It also
/// runs against the text from the program word, so `^gh` matches
/// `GH_TOKEN=x gh …` and `timeout 60 gh …`. Heredoc bodies, navigation
/// (`cd`, `echo`, assignments) and filters after a pipe are not matched.
#[derive(Default)]
pub(super) struct ShellCommand {
    operative: Vec<Segment>,
}

impl ShellCommand {
    pub(super) fn parse(cmd: &str) -> Self {
        let operative = command_describer::parse(cmd).into_iter().filter(Segment::is_operative).collect();
        ShellCommand { operative }
    }

    /// The first match of a `commands:` regex in any operative segment,
    /// capped for the event log like [`regex_span`]; `None` for no match or
    /// an invalid pattern.
    pub(super) fn span(&self, pattern: &str) -> Option<String> {
        let re = compile_trigger(pattern)?;
        self.operative
            .iter()
            .find_map(|s| re.find(&s.text).or_else(|| re.find(s.program_text())))
            .map(|m| capped_span(m.as_str()))
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

/// The Bash lane's way hits for one command, in admission order: the command
/// and description regexes, then the semantic lane on the tool description.
/// Shared by the scan and the probe scorer.
pub(super) fn command_hits(
    surface: &CommandSurface<'_>,
    candidates: &[WayCandidate],
    embed_matches: &EmbedScores,
) -> Vec<Hit<(&'static str, Option<String>, bool)>> {
    let CommandSurface { command, description, session_id, scope, project_dir } = *surface;
    // Way matching: commands regex + pattern regex + semantic (ADR-155 §4).
    // Hits are collected, then admitted in a fixed order (scan/order.rs).
    // Payload: (channel, matched span, fired only on this scan's parent boost).
    let mut hits: Vec<Hit<(&'static str, Option<String>, bool)>> = Vec::new();
    let mut fired_ids: HashSet<String> = HashSet::new();
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

/// The Bash lane's shadow pass: [`command_hits`] over the switched-off ways
/// alone, each hit logged as a discarded fire. A hit that needed a
/// switched-off parent's boost is dropped, since switched-off ways never lift
/// each other. A semantic hit carries its scores beside the enabled ways that
/// compete on this lane.
pub(super) fn log_discarded(surface: &CommandSurface<'_>, candidates: &[WayCandidate], off: &discard::Switched, embed_matches: &EmbedScores) {
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
    for hit in command_hits(surface, &off.ways, embed_matches) {
        let (trigger, span, needs_parent) = &hit.payload;
        if *needs_parent {
            continue;
        }
        let scores = match span {
            Some(_) => None,
            None => off.ways.iter().find(|w| w.id == hit.id).and_then(|w| discard::single_scores(embed_matches, &enabled, w)),
        };
        discard::log(off, &at, &discard::Hit { way: &hit.id, trigger, span: span.as_deref(), scores });
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

    #[test]
    fn no_segment_or_a_bad_pattern_is_no_match() {
        assert_eq!(span(r"gh", ""), None);
        assert_eq!(span(r"(unclosed", "gh pr list"), None);
        assert_eq!(ShellCommand::default().span(r".*"), None);
    }
}
