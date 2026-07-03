//! Why-fired drill-down (ADR-154 §1, §2): fold the introspection model into a
//! per-way/per-channel index and render the two-panel detail view.

use std::collections::HashMap;
use std::fmt::Write;

use crate::cmd::compositor::{self, Panel};
use crate::cmd::render;
use ways_core::introspection::{MatchCriteria, SessionIntrospection};

use super::layout::{header_lines, render_status_bar};
use super::model::{ActiveWay, Player};

/// One channel's "why it fired" for a way, aggregated from the model across the
/// way's fires *on that channel*. Matched spans are collected distinctly.
#[cfg(feature = "tui")]
pub(super) struct WhyEntry {
    way_path: Option<String>,
    trigger_channel: String,
    fire_score: Option<f64>,
    criteria: MatchCriteria,
    matched_spans: Vec<String>,
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
#[cfg(feature = "tui")]
pub(super) type WhyKey = (String, String);

#[cfg(feature = "tui")]
pub(super) type WhyIndex = HashMap<WhyKey, WhyEntry>;

/// Fold the model's per-turn fired-ways into a `(way_id, channel) → WhyEntry` index.
#[cfg(feature = "tui")]
fn build_why_index(model: &SessionIntrospection) -> WhyIndex {
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
            });
            if let Some(span) = fw.match_detail.as_ref().and_then(|m| m.matched_span.clone()) {
                if !e.matched_spans.contains(&span) {
                    e.matched_spans.push(span);
                }
            }
        }
    }
    idx
}

/// Read a way file's body: everything after a leading `---`/`---` frontmatter
/// block. Line-based so a body that legitimately opens with a markdown list (`- …`)
/// or a `---` rule is preserved. A file with no opening fence, or an unterminated
/// fence, is returned whole (nothing is silently dropped).
#[cfg(feature = "tui")]
fn read_way_body(path: &str) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut lines = content.lines();
    if lines.next() != Some("---") {
        return Some(content); // no opening fence — treat all as body
    }
    let mut in_frontmatter = true;
    let mut body = String::new();
    for line in lines {
        if in_frontmatter {
            if line == "---" {
                in_frontmatter = false; // consume the closing fence line
            }
            continue;
        }
        body.push_str(line);
        body.push('\n');
    }
    // Unterminated frontmatter (no closing fence) → don't drop the whole file.
    if in_frontmatter {
        return Some(content);
    }
    Some(body)
}

/// Render the detail panel for one way: its trigger, resolved `MatchCriteria`, the
/// matched spans (or an honest note when there's no recoverable term), and the way
/// body a human would read. `None` entry means the frame's way has no model record.
#[cfg(feature = "tui")]
fn render_why_detail(way_id: &str, entry: Option<&WhyEntry>) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "\x1b[1m{way_id}\x1b[0m");
    let Some(e) = entry else {
        let _ = writeln!(out, "\x1b[2mno fire record in the model for this way\x1b[0m");
        return out;
    };
    if let Some(p) = &e.way_path {
        let _ = writeln!(out, "\x1b[2m{p}\x1b[0m");
    }
    let _ = writeln!(out);

    let channel = render::format_trigger(&e.trigger_channel);
    match e.fire_score {
        Some(s) => {
            let _ = writeln!(out, "\x1b[1mTrigger\x1b[0m  {channel}  \x1b[2m(score {s:.2})\x1b[0m");
        }
        None => {
            let _ = writeln!(out, "\x1b[1mTrigger\x1b[0m  {channel}");
        }
    }

    let _ = writeln!(out, "\x1b[1mCriteria\x1b[0m");
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
            let _ = writeln!(out, "  \x1b[2m{label}:\x1b[0m {v}");
            wrote = true;
        }
    }
    if let Some(t) = c.embed_threshold {
        let _ = writeln!(out, "  \x1b[2membed_threshold:\x1b[0m {t}");
        wrote = true;
    }
    if !wrote {
        let _ = writeln!(out, "  \x1b[2m(none recorded)\x1b[0m");
    }

    let _ = writeln!(out);
    let _ = writeln!(out, "\x1b[1mMatched\x1b[0m");
    if e.matched_spans.is_empty() {
        if e.trigger_channel.starts_with("semantic") {
            let _ = writeln!(out, "  \x1b[2msemantic fire — matched by embedding; no recoverable term\x1b[0m");
        } else {
            let _ = writeln!(out, "  \x1b[2mno span recorded (fired before matched-span enrichment)\x1b[0m");
        }
    } else {
        for span in &e.matched_spans {
            let _ = writeln!(out, "  \x1b[0;36m“{span}”\x1b[0m");
        }
    }

    if let Some(body) = e.way_path.as_deref().and_then(read_way_body) {
        let _ = writeln!(out);
        let _ = writeln!(out, "\x1b[2m── way ─────────────\x1b[0m");
        for line in body.lines() {
            let _ = writeln!(out, "{line}");
        }
    }
    out
}

/// Render the why-fired drill-down: the current frame's fired ways on the left,
/// the selected way's model-derived detail on the right, composed with the
/// micro-compositor (ADR-154 §2). The model index is built lazily on first entry.
#[cfg(feature = "tui")]
pub(super) fn render_why(player: &mut Player) -> String {
    if player.why_index.is_none() {
        let model = SessionIntrospection::from_session(
            &player.session_id,
            &player.project_name,
            player.context_window_k,
        );
        player.why_index = Some(build_why_index(&model));
    }

    let ways_len = player.frames[player.current].ways.len();
    let sel = player.why_selected.min(ways_len.saturating_sub(1));
    player.why_selected = sel;

    let total_w = player.term_width as usize;
    // Shared chrome is header (4) + footer (2) = 6 rows; the body fills the rest of
    // the drawable height (`term_height − 1`, since fit_to_terminal drops the last
    // cell to avoid a scroll), so 4 + body_h + 2 == term_height − 1.
    let body_h = (player.term_height as usize).saturating_sub(7).max(3);
    let gap = 2;
    let left_w = (total_w / 3).clamp(16, 40).min(total_w.saturating_sub(gap + 12).max(16));
    let right_w = total_w.saturating_sub(left_w + gap).max(12);

    // Build both panels while borrowing the index + frame, then drop those borrows
    // before the scroll write-backs below.
    let (left, right) = {
        let idx = player.why_index.as_ref().unwrap();
        let frame = &player.frames[player.current];

        // Look up the model facet for the channel this frame shows the way on, so a
        // multi-channel way's detail is coherent (a keyword span never surfaces under
        // a semantic trigger).
        let facet = |w: &ActiveWay| idx.get(&(w.id.clone(), w.trigger.clone()));

        // Prefix each row with the epoch the way fired in, so a row cross-references
        // straight back to the Timeline's Epoch column. Right-align to the widest
        // epoch in the frame so the way ids stay aligned.
        let epoch_w = frame
            .ways
            .iter()
            .map(|w| w.epoch_fired)
            .max()
            .unwrap_or(0)
            .to_string()
            .len();

        // A 2-space left margin aligns this list with the Timeline's Way column (and
        // `ways list`), so the lists don't shift horizontally when toggling views.
        let mut left_lines: Vec<String> = Vec::with_capacity(frame.ways.len());
        for (i, w) in frame.ways.iter().enumerate() {
            // A filled bullet marks a way the model has a fire record for on this channel.
            let bullet = if facet(w).is_some() { "•" } else { "·" };
            if i == sel {
                // Margin stays plain; only the content is reversed (as write_way_row does),
                // so the highlight bar starts at the Way column, not in the margin.
                let raw = format!("{bullet} e{:>ew$} {}", w.epoch_fired, w.id, ew = epoch_w);
                left_lines.push(format!(
                    "  \x1b[7m{}\x1b[0m",
                    compositor::fit_visible(&raw, left_w.saturating_sub(2))
                ));
            } else {
                // Dim the epoch tag so the way id stays prominent.
                left_lines.push(format!(
                    "  {bullet} \x1b[2me{:>ew$}\x1b[0m {}",
                    w.epoch_fired,
                    w.id,
                    ew = epoch_w
                ));
            }
        }
        if left_lines.is_empty() {
            left_lines.push("  \x1b[2m(no ways in this frame)\x1b[0m".to_string());
        }

        let detail = frame
            .ways
            .get(sel)
            .map(|w| render_why_detail(&w.id, facet(w)))
            .unwrap_or_else(|| "\x1b[2mno ways fired in this frame\x1b[0m".to_string());

        (
            Panel::from_lines(left_lines).fixed_width(left_w),
            Panel::from_text(&detail).fixed_width(right_w),
        )
    };

    let detail_scroll = player.why_detail_scroll.min(right.max_scroll(body_h));
    player.why_detail_scroll = detail_scroll;
    let left_scroll = sel
        .saturating_sub(body_h.saturating_sub(1))
        .min(left.max_scroll(body_h));

    let composited = compositor::hjoin2(
        &left.viewport(left_scroll, body_h),
        &right.viewport(detail_scroll, body_h),
        gap,
    );

    // Shared footer (same 2 rows as the Timeline).
    let mut nav_buf = String::new();
    render_status_bar(&mut nav_buf, player);
    let nav: Vec<String> = nav_buf.lines().map(str::to_string).collect();

    // Shared header, with the drill-down's own column labels + rule so the two
    // panels are named the way the Timeline's columns are.
    let labels = format!(
        "\x1b[1m{}\x1b[0m{}\x1b[1mWhy it fired\x1b[0m",
        compositor::pad_visible("  Way · epoch", left_w),
        " ".repeat(gap),
    );
    let rule = format!(
        "\x1b[2m{}\x1b[0m",
        "─".repeat((left_w + gap + right_w).min(total_w))
    );
    let header = header_lines(player, vec![labels, rule]);

    // header (4) + body (body_h) + footer (2) = exactly the drawable height, so the
    // chrome lines up row-for-row with the Timeline view when toggling. On a very
    // short terminal (where body_h hits its floor) drop from the top so the footer
    // stays visible, mirroring compose_screen.
    let drawable = (player.term_height as usize).saturating_sub(1).max(4);
    let mut lines = header;
    lines.extend(composited);
    lines.extend(nav);
    if lines.len() > drawable {
        lines.drain(0..lines.len() - drawable);
    }
    lines.join("\n")
}

// Drill-down "why" folding + rendering (the join-honesty-critical part).
#[cfg(all(test, feature = "tui"))]
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
            fire_score: score,
            way_path: None,
            criteria: MatchCriteria { pattern: Some("p".into()), ..Default::default() },
            match_detail: span.map(|s| MatchDetail {
                matched_span: Some(s.into()),
                confidence: JoinConfidence::Keyed,
            }),
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

        let sem = render_why_detail("d/doc", idx.get(&key("d/doc", "semantic:embedding:en")));
        assert!(sem.contains("no recoverable term"), "semantic facet stays term-free: {sem}");
        assert!(!sem.contains("diataxis"), "keyword span must NOT appear under semantic");
        assert!(sem.contains("score 0.71"));

        let kw = render_why_detail("d/doc", idx.get(&key("d/doc", "keyword")));
        assert!(kw.contains("diataxis"), "keyword facet shows its own real span");
    }

    #[test]
    fn detail_labels_semantic_and_missing_spans_honestly() {
        // Semantic fire → names the embedding + score, never a fabricated term.
        let sem = build_why_index(&model(vec![vec![fired(
            "d/s", "semantic:embedding:en", None, Some(0.73),
        )]]));
        let out = render_why_detail("d/s", sem.get(&key("d/s", "semantic:embedding:en")));
        assert!(out.contains("no recoverable term"), "semantic honesty: {out}");
        assert!(out.contains("score 0.73"));

        // Keyword fire with a span → shows the quoted span.
        let kw = build_why_index(&model(vec![vec![fired(
            "d/k", "keyword", Some("threat model"), None,
        )]]));
        assert!(render_why_detail("d/k", kw.get(&key("d/k", "keyword"))).contains("threat model"));

        // Keyword fire, no span (pre-enrichment) → says so, invents nothing.
        let none = build_why_index(&model(vec![vec![fired("d/n", "keyword", None, None)]]));
        assert!(render_why_detail("d/n", none.get(&key("d/n", "keyword"))).contains("no span recorded"));
    }

    #[test]
    fn detail_handles_way_with_no_model_record() {
        assert!(render_why_detail("d/x", None).contains("no fire record"));
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
