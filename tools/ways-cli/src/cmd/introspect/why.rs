//! Why a way fired (ADR-154 §1, §2): the introspection model folded into
//! a per-way, per-channel index, and the detail the why-fired view shows
//! for one way, its body rendered from markdown.

use std::collections::HashMap;

use agent_tui::markdown;
use agent_tui::ratatui::style::{Modifier, Style};
use agent_tui::ratatui::text::{Line, Span};
use agent_tui::theme;
use ways_core::introspection::{JudgeVerdict, MatchCriteria, SessionIntrospection};

use crate::cmd::render;

/// One channel's "why it fired" for a way, aggregated from the model across the
/// way's fires *on that channel*. Matched spans are collected distinctly.
pub(crate) struct WhyEntry {
    pub(crate) way_path: Option<String>,
    trigger_channel: String,
    fire_score: Option<f64>,
    criteria: MatchCriteria,
    matched_spans: Vec<String>,
    /// The relevance judge's verdicts on this way's fires on the channel,
    /// in order, distinct.
    verdicts: Vec<JudgeVerdict>,
}

/// Index key: `(way_id, trigger_channel)`. A single way commonly fires on several
/// channels in one session (verified against the event log: e.g. `documentation`
/// fires bash + file + keyword + semantic). Each channel is its own coherent facet —
/// its own score and matched spans. Folding them into one `way_id` entry would let a
/// keyword span be shown under a semantic trigger, fabricating a semantic matched
/// term (the forbidden case, ADR-153). Keying by channel keeps each facet honest;
/// the drill-down looks up the channel the focused frame shows for that way. Still a
/// way_id-based join (no epoch alignment) per the ADR-154 §1 boundary — just at
/// channel granularity.
pub(crate) type WhyKey = (String, String);

pub(crate) type WhyIndex = HashMap<WhyKey, WhyEntry>;

/// Fold the model's per-turn fired-ways into a `(way_id, channel) → WhyEntry` index.
pub(crate) fn build_why_index(model: &SessionIntrospection) -> WhyIndex {
    let mut idx: WhyIndex = HashMap::new();
    for turn in &model.turns {
        for fw in &turn.fired_ways {
            let key = (fw.way_id.clone(), fw.trigger_channel.clone());
            let e = idx.entry(key).or_insert_with(|| WhyEntry {
                way_path: fw.way_path.clone(),
                trigger_channel: fw.trigger_channel.clone(),
                fire_score: fw.fire_score,
                criteria: fw.criteria.clone(),
                matched_spans: Vec::new(),
                verdicts: Vec::new(),
            });
            if let Some(v) = &fw.judge {
                if !e.verdicts.contains(v) {
                    e.verdicts.push(v.clone());
                }
            }
            if let Some(span) = fw.match_detail.as_ref().and_then(|m| m.matched_span.clone()) {
                if !e.matched_spans.contains(&span) {
                    e.matched_spans.push(span);
                }
            }
        }
    }
    idx
}

/// One verdict: the outcome, P(yes) against the threshold, and the call
/// that gave it. A way blocked with its ancestor shows the ancestor's.
fn verdict_line(v: &JudgeVerdict) -> Line<'static> {
    let (word, cmp) = match v.verdict.as_str() {
        "pass" => ("pass", "≥"),
        "block" => ("blocked", "<"),
        "would_block" => ("would block", "<"),
        other => (other, "vs"),
    };
    let style = if v.verdict == "pass" { Style::new() } else { theme::warn() };
    let mut line = vec![
        Span::styled(format!("  {word:<12}"), style),
        Span::raw(format!("P(yes) {:.2} {cmp} {:.2}", v.p_yes, v.threshold)),
    ];
    if let Some(a) = &v.ancestor {
        line.push(Span::raw(format!(" for {a}")));
    }
    line.push(Span::styled(format!("  {} · {} {} · {} ms", v.mode, v.engine, v.model, v.judge_ms), theme::muted()));
    Line::from(line)
}

/// Read a way file's body: everything after a leading `---`/`---` frontmatter
/// block. Line-based so a body that legitimately opens with a markdown list (`- …`)
/// or a `---` rule is preserved. A file with no opening fence, or an unterminated
/// fence, is returned whole (nothing is silently dropped).
pub(crate) fn read_way_body(path: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    match ways_core::frontmatter::split(&content) {
        Some((_, body)) => Some(body.to_string()),
        None => Some(content),
    }
}

fn bold() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

/// The detail for one way: its trigger, resolved `MatchCriteria`, the matched
/// spans (or an honest note when there's no recoverable term), and the way's
/// `body` as a reader sees it, rendered from markdown to `width`. `None` entry
/// means the frame's way has no model record. Built while drawing: the styles
/// are the frame's palette.
pub(crate) fn detail_lines(way_id: &str, entry: Option<&WhyEntry>, body: Option<&str>, width: u16) -> Vec<Line<'static>> {
    let mut out: Vec<Line<'static>> = vec![Line::styled(way_id.to_string(), bold())];
    let Some(e) = entry else {
        out.push(Line::styled("no fire record in the model for this way", theme::muted()));
        return out;
    };
    if let Some(p) = &e.way_path {
        out.push(Line::styled(p.clone(), theme::muted()));
    }
    out.push(Line::raw(""));

    let channel = render::format_trigger(&e.trigger_channel);
    let mut trigger = vec![Span::styled("Trigger  ", bold()), Span::raw(channel)];
    if let Some(s) = e.fire_score {
        trigger.push(Span::styled(format!("  (score {s:.2})"), theme::muted()));
    }
    out.push(Line::from(trigger));

    out.push(Line::styled("Criteria", bold()));
    let c = &e.criteria;
    let mut wrote = false;
    for (label, val) in [
        ("pattern", &c.pattern),
        ("commands", &c.commands),
        ("files", &c.files),
        ("trigger", &c.trigger),
        ("vocabulary", &c.vocabulary),
        ("scope", &c.scope),
    ] {
        if let Some(v) = val {
            out.push(Line::from(vec![Span::styled(format!("  {label}: "), theme::muted()), Span::raw(v.clone())]));
            wrote = true;
        }
    }
    if !wrote {
        out.push(Line::styled("  (none recorded)", theme::muted()));
    }

    if !e.verdicts.is_empty() {
        out.push(Line::raw(""));
        out.push(Line::styled("Judge", bold()));
        for v in &e.verdicts {
            out.push(verdict_line(v));
        }
    }

    out.push(Line::raw(""));
    out.push(Line::styled("Matched", bold()));
    if e.trigger_channel == "judge" {
        out.push(Line::styled("  matched, then kept out by the relevance judge: nothing was injected", theme::muted()));
    } else if e.matched_spans.is_empty() {
        let note = if e.trigger_channel.starts_with("semantic") {
            "  semantic fire — matched by embedding; no recoverable term"
        } else {
            "  no span recorded (fired before matched-span enrichment)"
        };
        out.push(Line::styled(note, theme::muted()));
    } else {
        for span in &e.matched_spans {
            out.push(Line::styled(format!("  “{span}”"), theme::accent()));
        }
    }

    if let Some(body) = body {
        out.push(Line::raw(""));
        out.push(Line::styled("── way ─────────────", theme::muted()));
        out.extend(markdown::render(body, width));
    }
    out
}

// Drill-down "why" folding + rendering (the join-honesty-critical part).
#[cfg(test)]
mod tests {
    use super::*;
    use ways_core::introspection::{
        FiredWay, IntrospectionSummary, JoinConfidence, MatchCriteria, MatchDetail,
        SessionIntrospection, Turn,
    };

    fn fired(way: &str, channel: &str, span: Option<&str>, score: Option<f64>) -> FiredWay {
        FiredWay {
            way_id: way.into(),
            trigger_channel: channel.into(),
            gated: false,
            suppressed: None,
            redisclosed: false,
            fire_score: score,
            way_path: None,
            criteria: MatchCriteria { pattern: Some("p".into()), ..Default::default() },
            match_detail: span.map(|s| MatchDetail {
                matched_span: Some(s.into()),
                confidence: JoinConfidence::Keyed,
            }),
            judge: None,
        }
    }

    fn model(turns_ways: Vec<Vec<FiredWay>>) -> SessionIntrospection {
        let turns = turns_ways
            .into_iter()
            .map(|fired_ways| Turn {
                epoch: 1,
                token_position: 0,
                ts: "2026-01-01T00:00:00Z".into(),
                transcript_uuid: None,
                join_confidence: JoinConfidence::Heuristic,
                fired_ways,
            })
            .collect();
        SessionIntrospection {
            id: "s".into(),
            project: "/p".into(),
            window_k: 200,
            summary: IntrospectionSummary::default(),
            turns,
        }
    }

    fn detail(way: &str, entry: Option<&WhyEntry>) -> String {
        agent_tui::theme::set(agent_tui::theme::Palette::default());
        detail_lines(way, entry, None, 80)
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn key(way: &str, channel: &str) -> WhyKey {
        (way.to_string(), channel.to_string())
    }

    #[test]
    fn why_index_keys_by_channel_and_dedups_spans() {
        let m = model(vec![
            vec![fired("d/a", "keyword", Some("commit"), None)],
            vec![
                fired("d/a", "keyword", Some("commit"), None), // dup span, same channel
                fired("d/a", "keyword", Some("stage"), None),  // new span
            ],
        ]);
        let idx = build_why_index(&m);
        let e = idx.get(&key("d/a", "keyword")).expect("keyed by (way, channel)");
        assert_eq!(e.matched_spans, vec!["commit", "stage"], "deduped, first-seen order");
    }

    #[test]
    fn multichannel_way_keeps_each_facet_honest() {
        // The real hole (verified common in the event log): a way fires BOTH
        // semantically and by keyword in one session. The semantic facet must never
        // borrow the keyword fire's span (that would fabricate a semantic term).
        let idx = build_why_index(&model(vec![
            vec![fired("d/doc", "semantic:embedding:en", None, Some(0.71))], // semantic first
            vec![fired("d/doc", "keyword", Some("diataxis"), None)],         // keyword later
        ]));

        let sem = detail("d/doc", idx.get(&key("d/doc", "semantic:embedding:en")));
        assert!(sem.contains("no recoverable term"), "semantic facet stays term-free: {sem}");
        assert!(!sem.contains("diataxis"), "keyword span must NOT appear under semantic");
        assert!(sem.contains("score 0.71"));

        let kw = detail("d/doc", idx.get(&key("d/doc", "keyword")));
        assert!(kw.contains("diataxis"), "keyword facet shows its own real span");
    }

    #[test]
    fn detail_labels_semantic_and_missing_spans_honestly() {
        // Semantic fire → names the embedding + score, never a fabricated term.
        let sem = build_why_index(&model(vec![vec![fired(
            "d/s", "semantic:embedding:en", None, Some(0.73),
        )]]));
        let out = detail("d/s", sem.get(&key("d/s", "semantic:embedding:en")));
        assert!(out.contains("no recoverable term"), "semantic honesty: {out}");
        assert!(out.contains("score 0.73"));

        // Keyword fire with a span → shows the quoted span.
        let kw = build_why_index(&model(vec![vec![fired(
            "d/k", "keyword", Some("threat model"), None,
        )]]));
        assert!(detail("d/k", kw.get(&key("d/k", "keyword"))).contains("threat model"));

        // Keyword fire, no span (pre-enrichment) → says so, invents nothing.
        let none = build_why_index(&model(vec![vec![fired("d/n", "keyword", None, None)]]));
        assert!(detail("d/n", none.get(&key("d/n", "keyword"))).contains("no span recorded"));
    }

    fn verdict(v: &str, p: f64, ancestor: Option<&str>) -> JudgeVerdict {
        JudgeVerdict {
            verdict: v.into(),
            p_yes: p,
            threshold: 0.3,
            mode: "enforce".into(),
            engine: "anthropic".into(),
            model: "claude-haiku-4-5".into(),
            judge_ms: 700,
            reason: ancestor.map(|_| "ancestor".into()),
            ancestor: ancestor.map(str::to_string),
        }
    }

    /// The Judge section lists each verdict against the threshold; a way
    /// the judge blocked says it was kept out, and one blocked with its
    /// ancestor names the ancestor whose P(yes) it shows.
    #[test]
    fn detail_shows_the_judges_verdicts_and_what_was_kept_out() {
        let passed = FiredWay { judge: Some(verdict("pass", 0.91, None)), ..fired("d/a", "keyword", Some("commit"), None) };
        let blocked = FiredWay { suppressed: Some("judge".into()), judge: Some(verdict("block", 0.05, None)), ..fired("d/b", "judge", None, None) };
        let child = FiredWay { suppressed: Some("judge".into()), judge: Some(verdict("block", 0.05, Some("d/b"))), ..fired("d/b/c", "judge", None, None) };
        let idx = build_why_index(&model(vec![vec![passed, blocked, child]]));

        let a = detail("d/a", idx.get(&key("d/a", "keyword")));
        assert!(a.contains("Judge\n  pass        P(yes) 0.91 ≥ 0.30  enforce · anthropic claude-haiku-4-5 · 700 ms"), "{a}");
        assert!(a.contains("“commit”") && !a.contains("kept out"), "{a}");

        let b = detail("d/b", idx.get(&key("d/b", "judge")));
        assert!(b.contains("  blocked     P(yes) 0.05 < 0.30  enforce"), "{b}");
        assert!(b.contains("matched, then kept out by the relevance judge: nothing was injected"), "{b}");

        let c = detail("d/b/c", idx.get(&key("d/b/c", "judge")));
        assert!(c.contains("  blocked     P(yes) 0.05 < 0.30 for d/b  enforce"), "{c}");
    }

    #[test]
    fn detail_handles_way_with_no_model_record() {
        assert!(detail("d/x", None).contains("no fire record"));
    }

    #[test]
    fn read_way_body_preserves_leading_dashes_and_handles_edges() {
        let base = std::env::temp_dir().join(format!("ways-body-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&base);
        let write_read = |name: &str, content: &str| {
            let p = base.join(name);
            std::fs::write(&p, content).unwrap();
            read_way_body(p.to_str().unwrap()).unwrap()
        };
        // A body opening with a markdown list keeps its leading dashes.
        assert_eq!(
            write_read("list.md", "---\ndescription: d\n---\n- one\n- two\n"),
            "- one\n- two\n"
        );
        // Empty frontmatter → body is everything after the closing fence.
        assert_eq!(write_read("empty.md", "---\n---\nbody line\n"), "body line\n");
        // No opening fence → the whole file is the body.
        assert_eq!(write_read("nofm.md", "just text\nmore\n"), "just text\nmore\n");
        let _ = std::fs::remove_dir_all(&base);
    }
}
