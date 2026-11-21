// SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
// SPDX-License-Identifier: AGPL-3.0-or-later
// From agent-ways: https://github.com/aaronsb/agent-ways

//! scan/lookbehind.rs — transcript lookbehind for the tool-use channel (ADR-160
//! stage 1: contextualize the surface).
//!
//! A bare shell command is an *action*, not a statement of intent, so it matches
//! ways poorly on its own. The reasoning that led to the tool call sits just
//! above it in the transcript — the assistant's prose since the last human turn.
//! This module extracts that prose so the command surface can be embedded as
//! intent, not just the literal artifact.
//!
//! **Fail-safe.** Every path returns `None` (the caller falls back to
//! command-only) when there is no transcript, no usable prose, or anything
//! unparseable — lookbehind can only *add* intent context, never break matching.
//!
//! **Cheap on the hot path.** Bash is the highest-volume scan surface. The
//! intent prose comes from the transcript's tail, since the current turn lives
//! at the end. The judge's request can sit far back, so its lookup reads back
//! a page at a time and parses only the lines that can hold one; a 10 MB
//! subagent transcript costs some 9 ms warm, and a miss over the whole cap
//! some 55 ms.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;

/// Only the tail is needed — the current turn is at the end of the transcript.
/// A turn with more than this much assistant prose + tool traffic before the
/// command is vanishingly rare; if the human boundary falls outside the tail we
/// simply return the prose we found (still useful) or None.
const TAIL_BYTES: u64 = 64 * 1024;

/// The page the request lookup reads back from the transcript's end.
const REQUEST_PAGE_BYTES: u64 = 256 * 1024;

/// How far back the request lookup reads before it gives up. A transcript
/// larger than this still yields a subagent's brief, its first line, through
/// [`first_line_request`].
const REQUEST_SCAN_CAP: u64 = 32 * 1024 * 1024;

/// The longest first line [`first_line_request`] reads.
const FIRST_LINE_CAP: u64 = 1024 * 1024;

/// The calling agent's prose since its last human turn, or `None`.
///
/// A subagent's hook carries the parent's session id, so a session-id lookup
/// would embed the lead's narration into the subagent's surface. See
/// [`intent_in`].
pub(crate) fn intent(session_id: &str) -> Option<String> {
    intent_in(
        &ways_core::paths::claude_dir(),
        crate::cmd::show::firing_transcript(),
        &crate::util::project_dir(),
        session_id,
        &crate::session::current_agent(),
    )
}

/// The calling agent's request for the relevance judge: its last human
/// turn, a subagent's brief for a subagent that had no other. The agent's
/// narration is left out, because it let off-topic ways pass when measured,
/// and the operator chose the request and the call. Read from that agent's
/// own transcript, as [`intent`] reads it, but back past its tail
/// ([`last_human_in_file`]); empty when there is none.
///
/// References:
/// - ADR-707: the measurement of narration as judge input.
/// - ADR-712 §4: the operator's choice of the request and the call.
pub(crate) fn recent_turns(session_id: &str) -> Vec<ways_agent_core::judge::Turn> {
    recent_turns_in(
        &ways_core::paths::claude_dir(),
        crate::cmd::show::firing_transcript(),
        &crate::util::project_dir(),
        session_id,
        &crate::session::current_agent(),
    )
}

fn recent_turns_in(
    claude: &claude_sessions::ClaudeDir,
    hook_transcript: Option<&str>,
    project_dir: &str,
    session_id: &str,
    agent: &str,
) -> Vec<ways_agent_core::judge::Turn> {
    use ways_agent_core::judge::{Role, Turn};
    crate::session::transcript_in(claude, hook_transcript, project_dir, session_id, agent)
        .and_then(|t| last_human_in_file(&t, REQUEST_PAGE_BYTES, REQUEST_SCAN_CAP).or_else(|| first_line_request(&t, FIRST_LINE_CAP)))
        .map(|text| Turn { role: Role::User, text })
        .into_iter()
        .collect()
}

/// The request one transcript line holds, if it holds one ([`request_text`]).
fn human_text(line: &[u8]) -> Option<String> {
    request_text(&serde_json::from_slice::<Value>(line.trim_ascii()).ok()?)
}

/// The request in the transcript's first line, read up to `cap` bytes. A
/// subagent's brief is that line, so this finds it when the backward read
/// stops at its cap first.
fn first_line_request(path: &Path, cap: u64) -> Option<String> {
    let mut buf = Vec::new();
    std::fs::File::open(path).ok()?.take(cap).read_to_end(&mut buf).ok()?;
    let nl = buf.iter().position(|b| *b == b'\n')?;
    human_text(&buf[..nl])
}

/// The newest request in the transcript at `path` ([`request_text`]), read
/// back from its end a page at a time, at most `cap` bytes.
///
/// The request is often far from the end. A subagent's brief is the first
/// line of its transcript, and Claude Code writes some 300 KB of setup
/// records (instructions, the skill listing, prompt snapshots) after it
/// before the agent's first tool call. A fixed tail never reaches it. Only
/// a line that names the `user` role is parsed, so the pages of assistant
/// turns and most attachments cost a byte search.
fn last_human_in_file(path: &Path, page: u64, cap: u64) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let floor = len.saturating_sub(cap);
    let mut end = len;
    // The start of a line that runs on into the page already read.
    let mut carry: Vec<u8> = Vec::new();
    while end > floor {
        // At the cap, read one byte more: a newline there means the line
        // that starts at `floor` is whole.
        let start = match end.saturating_sub(page).max(floor) {
            s if s == floor && floor > 0 => s - 1,
            s => s,
        };
        let mut buf = vec![0u8; (end - start) as usize];
        f.seek(SeekFrom::Start(start)).ok()?;
        f.read_exact(&mut buf).ok()?;
        buf.extend_from_slice(&carry);
        // Before the file's start, the page's first line may begin on an
        // earlier page: keep it for the next read.
        let body = if start == 0 {
            carry = Vec::new();
            &buf[..]
        } else {
            match buf.iter().position(|b| *b == b'\n') {
                Some(nl) => {
                    carry = buf[..nl].to_vec();
                    &buf[nl + 1..]
                }
                None => {
                    carry = buf;
                    end = start;
                    continue;
                }
            }
        };
        let found = body
            .split(|b| *b == b'\n')
            .rev()
            .filter(|line| memmem(line, b"\"user\"") || memmem(line, b"\"queued_command\""))
            .find_map(human_text);
        if found.is_some() {
            return found;
        }
        end = start;
    }
    None
}

fn memmem(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// [`intent`] against an explicit config dir, hook transcript and agent. Read
/// from that agent's own transcript ([`crate::session::transcript_in`]); a
/// subagent with no transcript of its own gets no lookbehind.
fn intent_in(
    claude: &claude_sessions::ClaudeDir,
    hook_transcript: Option<&str>,
    project_dir: &str,
    session_id: &str,
    agent: &str,
) -> Option<String> {
    let transcript = crate::session::transcript_in(claude, hook_transcript, project_dir, session_id, agent)?;
    assistant_prose_since_last_human(&read_tail(&transcript, TAIL_BYTES)?)
}

/// Read up to `max_bytes` from the end of `path`, lossily decoded (a tail that
/// splits a UTF-8 boundary or a JSON line is fine — partial lines just fail to
/// parse and are skipped).
fn read_tail(path: &Path, max_bytes: u64) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(max_bytes);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Walk the transcript backward collecting assistant text blocks until a
/// request ([`request_text`]) ends the walk. `tool_result` records (also role `user`) are
/// part of the current turn's tool loop and are skipped, so multi-tool turns
/// still resolve to the human turn that started them. Returns the collected
/// prose in chronological order, or `None` if there is none.
pub(crate) fn assistant_prose_since_last_human(transcript: &str) -> Option<String> {
    let mut prose: Vec<String> = Vec::new(); // reverse-chronological while building
    for line in transcript.lines().rev() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue; // a partial first line (tail split) or non-JSON — skip
        };
        let content = v.get("message").and_then(|m| m.get("content"));
        match turn_role(&v) {
            Some("assistant") => collect_text_blocks(content, &mut prose),
            _ if request_text(&v).is_some() => break,
            _ => {} // tool_result user records, notifications, meta — skip
        }
    }
    if prose.is_empty() {
        return None;
    }
    prose.reverse();
    let joined: String = prose.join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
    (!joined.is_empty()).then_some(joined)
}

/// Push an assistant record's text blocks onto `out` (reverse order, so the
/// caller's final reverse yields chronological prose). Handles both a plain
/// string content and the block-array form.
fn collect_text_blocks(content: Option<&Value>, out: &mut Vec<String>) {
    match content {
        Some(Value::String(s)) => {
            if !s.trim().is_empty() {
                out.push(s.clone());
            }
        }
        Some(Value::Array(blocks)) => {
            for b in blocks.iter().rev() {
                if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                    if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                        if !t.trim().is_empty() {
                            out.push(t.to_string());
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// The text of a record that asks the agent for something: what a person
/// typed, a subagent's brief, or a lead's message to a running subagent.
///
/// Claude Code marks most user records with an `origin`, and the origins
/// [`asks`] names count, as a user record or as a `queued_command`
/// attachment. A lead's mid-run message comes either way and is marked
/// `isMeta`; a prompt typed while a turn runs is an attachment with
/// `humanTurn`. Every other origin (`task-notification`, `plugin`, `peer`)
/// is something arriving, not a request.
/// A record with no origin, such as a subagent's brief, counts unless it is
/// one of Claude Code's own: `isMeta` reminders and skill bodies, a
/// compaction summary, or a fixed-form echo ([`HARNESS_PREFIXES`]).
fn request_text(v: &Value) -> Option<String> {
    if v.get("type").and_then(Value::as_str) == Some("attachment") {
        let a = v.get("attachment")?;
        let asked = a.get("type").and_then(Value::as_str) == Some("queued_command")
            && (a.get("humanTurn").and_then(Value::as_bool) == Some(true)
                || a.get("origin").and_then(|o| o.get("kind")).and_then(Value::as_str).is_some_and(asks));
        return asked.then(|| joined_text(a.get("prompt"))).flatten();
    }
    if turn_role(v) != Some("user") {
        return None;
    }
    let content = v.get("message").and_then(|m| m.get("content"));
    if !is_human_message(content) {
        return None;
    }
    match v.get("origin").and_then(|o| o.get("kind")).and_then(Value::as_str) {
        Some(kind) if asks(kind) => {}
        Some(_) => return None,
        None => {
            let flag = |k: &str| v.get(k).and_then(Value::as_bool) == Some(true);
            if flag("isMeta") || flag("isCompactSummary") {
                return None;
            }
        }
    }
    let text = joined_text(content)?;
    let harness = v.get("origin").is_none() && HARNESS_PREFIXES.iter().any(|p| text.starts_with(p));
    (!harness).then_some(text)
}

/// Whether a record of this origin asks the agent for something: a typed
/// prompt (`human`), a lead's message (`coordinator`), or the goal condition
/// a `/goal` run continues on (`auto-continuation`, "Goal set: …").
fn asks(kind: &str) -> bool {
    matches!(kind, "human" | "coordinator" | "auto-continuation")
}

/// The openings of user records Claude Code writes itself, with no origin:
/// slash-command and shell echoes, an interrupt, and relayed messages.
const HARNESS_PREFIXES: &[&str] = &[
    "<command-name>",
    "<command-message>",
    "<local-command-",
    "<bash-input>",
    "<bash-stdout>",
    "<bash-stderr>",
    "<task-notification>",
    "[Request interrupted",
    "Another Claude session sent a message",
];

/// A message content's text blocks, joined with whitespace collapsed.
fn joined_text(content: Option<&Value>) -> Option<String> {
    let mut text = Vec::new();
    collect_text_blocks(content, &mut text);
    text.reverse();
    let joined = text.join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
    (!joined.is_empty()).then_some(joined)
}

/// The role of a conversation turn: a `user` or `assistant` record whose
/// message has that role. Claude Code also writes `api-request-blob`
/// records, copies of the composed request whose messages carry `user`,
/// `assistant` and `system` roles (the CLAUDE.md context among them); they
/// are not turns.
fn turn_role(v: &Value) -> Option<&str> {
    let role = v.get("message")?.get("role")?.as_str()?;
    (v.get("type").and_then(|t| t.as_str()) == Some(role)).then_some(role)
}

/// A genuine human turn vs a `tool_result` carrier (both are role `user`). A real
/// prompt is a plain string or has a text block and no `tool_result` block.
fn is_human_message(content: Option<&Value>) -> bool {
    match content {
        Some(Value::String(_)) => true,
        Some(Value::Array(blocks)) => {
            let block_type = |b: &Value| b.get("type").and_then(|t| t.as_str()).map(str::to_string);
            let has_tool_result = blocks.iter().any(|b| block_type(b).as_deref() == Some("tool_result"));
            let has_text = blocks.iter().any(|b| block_type(b).as_deref() == Some("text"));
            has_text && !has_tool_result
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal transcript records in the Claude Code JSONL shape.
    fn user_text(t: &str) -> String {
        format!(r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"text","text":"{t}"}}]}}}}"#)
    }
    fn assistant_text(t: &str) -> String {
        format!(r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"{t}"}}]}}}}"#)
    }
    fn tool_result(t: &str) -> String {
        format!(r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","content":"{t}"}}]}}}}"#)
    }

    #[test]
    fn collects_assistant_prose_since_last_human() {
        let t = [
            user_text("older question"),
            assistant_text("older answer"),
            user_text("let us commit the fix"),
            assistant_text("I will stage the change and commit it."),
            tool_result("staged"),
            assistant_text("Now committing."),
        ]
        .join("\n");
        let prose = assistant_prose_since_last_human(&t).unwrap();
        // Spans the tool_result within the turn, stops at the human message,
        // excludes the earlier turn's "older answer".
        assert!(prose.contains("I will stage the change"), "{prose}");
        assert!(prose.contains("Now committing"), "{prose}");
        assert!(!prose.contains("older answer"), "{prose}");
        assert!(!prose.contains("let us commit"), "prose must not include the human turn: {prose}");
    }

    #[test]
    fn none_when_no_assistant_prose() {
        let t = user_text("just a question with no assistant turn yet");
        assert!(assistant_prose_since_last_human(&t).is_none());
    }

    /// A subagent's Bash surface is its own reasoning. The hook reports the
    /// parent's session id (and, per Claude Code's hooks reference, the
    /// parent's `transcript_path`), so a session-id lookup alone reads the
    /// lead's narration into the subagent's surface.
    #[test]
    fn a_subagent_reads_its_own_prose_never_the_leads() {
        let root = std::env::temp_dir().join(format!("ways-lookbehind-sub-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let claude = claude_sessions::ClaudeDir::at(root.join(".claude"));
        let project = "/srv/p";
        let dir = root.join(".claude/projects/-srv-p");
        let subagents = dir.join("sess/subagents");
        std::fs::create_dir_all(subagents.join("workflows/wf_1")).unwrap();
        let turn = |human: &str, prose: &str| format!("{}\n{}\n", user_text(human), assistant_text(prose));
        std::fs::write(dir.join("sess.jsonl"), turn("ship it", "lead narration about merging the branch")).unwrap();
        std::fs::write(subagents.join("agent-atask.jsonl"), turn("brief", "subagent reasoning about the failing test")).unwrap();
        std::fs::write(subagents.join("workflows/wf_1/agent-awf.jsonl"), turn("brief", "workflow agent reasoning about the index")).unwrap();
        let parent = dir.join("sess.jsonl");

        let main = intent_in(&claude, None, project, "sess", crate::session::MAIN_AGENT).unwrap();
        assert!(main.contains("lead narration"), "{main}");
        for hook_transcript in [None, parent.to_str()] {
            let sub = intent_in(&claude, hook_transcript, project, "sess", "atask").unwrap_or_default();
            assert!(sub.contains("subagent reasoning"), "{sub}");
            assert!(!sub.contains("lead narration"), "{sub}");
            let wf = intent_in(&claude, hook_transcript, project, "sess", "awf").unwrap_or_default();
            assert!(wf.contains("workflow agent reasoning"), "{wf}");
            assert!(!wf.contains("lead narration"), "{wf}");
            // A subagent with no transcript of its own gets no lookbehind, not the lead's.
            assert_eq!(intent_in(&claude, hook_transcript, project, "sess", "agone"), None);
        }
        std::fs::remove_dir_all(&root).ok();
    }

    /// The judge reads the calling agent's own last human turn, a subagent's
    /// brief, never the lead's, and none of the agent's narration.
    #[test]
    fn recent_turns_are_the_calling_agents_own() {
        let root = std::env::temp_dir().join(format!("ways-lookbehind-turns-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let claude = claude_sessions::ClaudeDir::at(root.join(".claude"));
        let dir = root.join(".claude/projects/-srv-p");
        std::fs::create_dir_all(dir.join("sess/subagents")).unwrap();
        let turn = |human: &str, prose: &str| format!("{}\n{}\n{}\n", user_text(human), assistant_text(prose), tool_result("ok"));
        std::fs::write(dir.join("sess.jsonl"), turn("ship it", "lead narration")).unwrap();
        std::fs::write(dir.join("sess/subagents/agent-atask.jsonl"), turn("the brief", "subagent reasoning")).unwrap();
        let texts = |agent: &str| recent_turns_in(&claude, None, "/srv/p", "sess", agent).into_iter().map(|t| (t.role, t.text)).collect::<Vec<_>>();
        use ways_agent_core::judge::Role;
        assert_eq!(texts("atask"), [(Role::User, "the brief".to_string())]);
        assert_eq!(texts(crate::session::MAIN_AGENT), [(Role::User, "ship it".to_string())]);
        assert!(texts("agone").is_empty());
        std::fs::remove_dir_all(&root).ok();
    }

    fn meta_reminder(t: &str) -> String {
        format!(r#"{{"type":"user","isMeta":true,"message":{{"role":"user","content":"{t}"}}}}"#)
    }
    fn setup_record(bytes: usize) -> String {
        format!(r#"{{"type":"attachment","attachment":{{"type":"instructions","content":"{}"}}}}"#, "x".repeat(bytes))
    }

    /// A subagent transcript as Claude Code writes it: the brief, a meta
    /// reminder, then setup records far past any fixed tail before the
    /// agent's first tool call.
    fn subagent_transcript(brief: &str, later: &[String]) -> String {
        let mut lines = vec![user_text(brief), meta_reminder("Other agents active in this session")];
        lines.extend((0..12).map(|_| setup_record(30 * 1024)));
        lines.push(assistant_text("Reading the brief file."));
        lines.push(tool_result("brief contents"));
        lines.extend(later.iter().cloned());
        lines.join("\n") + "\n"
    }

    /// The judge reads a subagent's brief though 360 KB of setup records
    /// sit between the brief and the call.
    #[test]
    fn the_brief_is_found_past_the_setup_records() {
        let root = std::env::temp_dir().join(format!("ways-lookbehind-deep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let claude = claude_sessions::ClaudeDir::at(root.join(".claude"));
        let dir = root.join(".claude/projects/-srv-p/sess/subagents");
        std::fs::create_dir_all(&dir).unwrap();
        let t = subagent_transcript("review the parser change", &[]);
        assert!(t.len() as u64 > 4 * TAIL_BYTES, "the fixture must reach past the old tail");
        std::fs::write(dir.join("agent-atask.jsonl"), &t).unwrap();
        std::fs::write(root.join(".claude/projects/-srv-p/sess.jsonl"), user_text("lead prompt") + "\n").unwrap();
        let texts = recent_turns_in(&claude, None, "/srv/p", "sess", "atask").into_iter().map(|t| t.text).collect::<Vec<_>>();
        assert_eq!(texts, ["review the parser change"]);
        std::fs::remove_dir_all(&root).ok();
    }

    /// A message the lead sends mid-run is the newer request.
    #[test]
    fn a_later_lead_message_is_the_request() {
        let path = std::env::temp_dir().join(format!("ways-lookbehind-later-{}.jsonl", std::process::id()));
        let t = subagent_transcript("the brief", &[user_text("now approve the fonts"), assistant_text("ok"), tool_result("done")]);
        std::fs::write(&path, &t).unwrap();
        assert_eq!(last_human_in_file(&path, REQUEST_PAGE_BYTES, REQUEST_SCAN_CAP).as_deref(), Some("now approve the fonts"));
        std::fs::remove_file(&path).ok();
    }

    /// Claude Code's reminders are not the request, in the lookup or in the
    /// prose walk.
    #[test]
    fn a_meta_reminder_is_not_human() {
        let t = [user_text("the brief"), meta_reminder("Other agents active"), assistant_text("working")].join("\n");
        let path = std::env::temp_dir().join(format!("ways-lookbehind-meta-{}.jsonl", std::process::id()));
        std::fs::write(&path, &t).unwrap();
        assert_eq!(last_human_in_file(&path, 64, 1 << 20).as_deref(), Some("the brief"));
        std::fs::remove_file(&path).ok();
        let prose = assistant_prose_since_last_human(&[user_text("q"), assistant_text("before"), meta_reminder("r"), assistant_text("after")].join("\n")).unwrap();
        assert!(prose.contains("before") && prose.contains("after"), "{prose}");
    }

    /// A copy of the composed request is not a turn, in the lookup or in the
    /// prose walk.
    #[test]
    fn a_request_blob_is_not_a_turn() {
        let blob = |role: &str, t: &str| format!(r#"{{"type":"api-request-blob","message":{{"role":"{role}","content":[{{"type":"text","text":"{t}"}}]}}}}"#);
        let t = [user_text("the brief"), blob("user", "CLAUDE.md context"), blob("assistant", "copied narration"), assistant_text("own narration")].join("\n");
        let path = std::env::temp_dir().join(format!("ways-lookbehind-blob-{}.jsonl", std::process::id()));
        std::fs::write(&path, &t).unwrap();
        assert_eq!(last_human_in_file(&path, 64, 1 << 20).as_deref(), Some("the brief"));
        std::fs::remove_file(&path).ok();
        let prose = assistant_prose_since_last_human(&t).unwrap();
        assert_eq!(prose, "own narration");
    }

    fn origin_user(kind: &str, meta: bool, t: &str) -> String {
        format!(r#"{{"type":"user","isMeta":{meta},"origin":{{"kind":"{kind}"}},"message":{{"role":"user","content":"{t}"}}}}"#)
    }

    /// A lead's message to a running subagent is the newest request, though
    /// Claude Code writes it `isMeta`, and it ends the prose walk.
    #[test]
    fn a_coordinator_message_is_the_request() {
        let t = [
            user_text("fix issue 1287"),
            assistant_text("phase one narration"),
            origin_user("coordinator", true, "The coordinator sent a message while you were working: fix the review findings"),
            assistant_text("phase two narration"),
        ]
        .join("\n");
        let path = std::env::temp_dir().join(format!("ways-lookbehind-coord-{}.jsonl", std::process::id()));
        std::fs::write(&path, &t).unwrap();
        let found = last_human_in_file(&path, 64, 1 << 20).unwrap();
        assert!(found.contains("fix the review findings"), "{found}");
        std::fs::remove_file(&path).ok();
        assert_eq!(assistant_prose_since_last_human(&t).unwrap(), "phase two narration");
    }

    /// What arrives without being asked for is not the request: notifications,
    /// plugin and peer messages, a compaction summary, Claude Code's echoes.
    #[test]
    fn arrivals_and_echoes_are_not_the_request() {
        let compact = r#"{"type":"user","isCompactSummary":true,"message":{"role":"user","content":"This session is being continued"}}"#.to_string();
        let arrivals = [
            origin_user("task-notification", false, "<task-notification>done</task-notification>"),
            origin_user("plugin", false, "The attend plugin sent a message: keepwarm"),
            origin_user("peer", true, "Another Claude session sent a message"),
            compact,
            user_text("<command-name>/compact</command-name>"),
            user_text("<local-command-stdout>Compacted</local-command-stdout>"),
            user_text("<bash-stdout>ok</bash-stdout>"),
            user_text("[Request interrupted by user]"),
            user_text("Another Claude session sent a message: report"),
        ];
        let t = std::iter::once(origin_user("human", false, "typed prompt")).chain(arrivals.iter().cloned()).collect::<Vec<_>>().join("\n");
        let path = std::env::temp_dir().join(format!("ways-lookbehind-arrivals-{}.jsonl", std::process::id()));
        std::fs::write(&path, &t).unwrap();
        assert_eq!(last_human_in_file(&path, 64, 1 << 20).as_deref(), Some("typed prompt"));
        std::fs::remove_file(&path).ok();
    }

    /// A prompt typed while a turn runs is a queued attachment, and it is
    /// the newest request.
    #[test]
    fn a_queued_prompt_is_the_request() {
        let queued = r#"{"type":"attachment","attachment":{"type":"queued_command","humanTurn":true,"origin":{"kind":"human"},"prompt":"also check the cap"}}"#;
        let peer = r#"{"type":"attachment","attachment":{"type":"queued_command","origin":{"kind":"peer"},"prompt":"a peer's note"}}"#;
        let t = [origin_user("human", false, "first prompt"), assistant_text("working"), queued.to_string(), peer.to_string()].join("\n");
        let path = std::env::temp_dir().join(format!("ways-lookbehind-queued-{}.jsonl", std::process::id()));
        std::fs::write(&path, &t).unwrap();
        assert_eq!(last_human_in_file(&path, 64, 1 << 20).as_deref(), Some("also check the cap"));
        std::fs::remove_file(&path).ok();
    }

    /// A lead's message that arrives as a queued attachment, and the goal a
    /// `/goal` run continues on, are each the newest request.
    #[test]
    fn a_queued_lead_message_and_a_goal_are_requests() {
        let lead = r#"{"type":"attachment","attachment":{"type":"queued_command","origin":{"kind":"coordinator"},"isMeta":true,"prompt":"Another item for this round"}}"#;
        let goal = r#"{"type":"attachment","attachment":{"type":"queued_command","origin":{"kind":"auto-continuation"},"prompt":"Goal set: finish the fixes"}}"#;
        let path = std::env::temp_dir().join(format!("ways-lookbehind-qlead-{}.jsonl", std::process::id()));
        for (rec, want) in [(lead, "Another item for this round"), (goal, "Goal set: finish the fixes")] {
            let t = [user_text("the brief"), assistant_text("working"), rec.to_string(), assistant_text("more")].join("\n");
            std::fs::write(&path, &t).unwrap();
            assert_eq!(last_human_in_file(&path, 64, 1 << 20).as_deref(), Some(want));
            assert_eq!(assistant_prose_since_last_human(&t).unwrap(), "more");
        }
        std::fs::remove_file(&path).ok();
    }

    /// A brief further back than the cap is still found on the first line.
    #[test]
    fn a_brief_past_the_cap_is_read_from_the_first_line() {
        let root = std::env::temp_dir().join(format!("ways-lookbehind-head-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let claude = claude_sessions::ClaudeDir::at(root.join(".claude"));
        let dir = root.join(".claude/projects/-srv-p/sess/subagents");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(root.join(".claude/projects/-srv-p/sess.jsonl"), user_text("lead prompt") + "\n").unwrap();
        let t = subagent_transcript("the long-running brief", &[]);
        let path = dir.join("agent-atask.jsonl");
        std::fs::write(&path, &t).unwrap();
        assert_eq!(last_human_in_file(&path, REQUEST_PAGE_BYTES, 64 * 1024), None, "the cap must stop the backward read");
        assert_eq!(first_line_request(&path, FIRST_LINE_CAP).as_deref(), Some("the long-running brief"));
        assert_eq!(first_line_request(&path, 8), None, "a first line longer than its cap is not read");
        let texts = recent_turns_in(&claude, None, "/srv/p", "sess", "atask").into_iter().map(|t| t.text).collect::<Vec<_>>();
        assert_eq!(texts, ["the long-running brief"]);
        std::fs::remove_dir_all(&root).ok();
    }

    /// A whole line that starts exactly at the cap is read.
    #[test]
    fn a_line_starting_at_the_cap_is_whole() {
        let tail = [user_text("at the edge"), assistant_text("end")].join("\n");
        let t = format!("{}\n{tail}", setup_record(100));
        let path = std::env::temp_dir().join(format!("ways-lookbehind-edge-{}.jsonl", std::process::id()));
        std::fs::write(&path, &t).unwrap();
        for page in [1, 16, 4096] {
            assert_eq!(last_human_in_file(&path, page, tail.len() as u64).as_deref(), Some("at the edge"), "page {page}");
            assert_eq!(last_human_in_file(&path, page, tail.len() as u64 - 1), None, "page {page}");
        }
        std::fs::remove_file(&path).ok();
    }

    /// Paging gives the same answer as reading the whole file, whatever the
    /// page size, including pages shorter than a line.
    #[test]
    fn paging_matches_a_whole_file_read() {
        let t = [
            user_text("first"),
            assistant_text(&"a".repeat(300)),
            user_text("second request"),
            tool_result(&"r".repeat(500)),
            meta_reminder("reminder"),
            assistant_text("end"),
        ]
        .join("\n");
        let path = std::env::temp_dir().join(format!("ways-lookbehind-pages-{}.jsonl", std::process::id()));
        for body in [t.clone(), t.clone() + "\n"] {
            std::fs::write(&path, &body).unwrap();
            for page in [1, 7, 64, 199, 1000, 1 << 20] {
                assert_eq!(last_human_in_file(&path, page, 1 << 20).as_deref(), Some("second request"), "page {page}");
            }
        }
        std::fs::remove_file(&path).ok();
    }

    /// The lookup stops at its cap: a request further back is not found.
    #[test]
    fn the_cap_bounds_the_read() {
        let t = [user_text("far back"), setup_record(4096), assistant_text("end")].join("\n");
        let path = std::env::temp_dir().join(format!("ways-lookbehind-cap-{}.jsonl", std::process::id()));
        std::fs::write(&path, &t).unwrap();
        assert_eq!(last_human_in_file(&path, 512, 2048), None);
        assert_eq!(last_human_in_file(&path, 512, 1 << 20).as_deref(), Some("far back"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn partial_leading_line_is_skipped() {
        // A tail that starts mid-line (unparseable) must not poison the result.
        let t = format!("nk\":\"garbage partial line\"}}\n{}", assistant_text("real intent prose here"));
        let prose = assistant_prose_since_last_human(&t).unwrap();
        assert_eq!(prose, "real intent prose here");
    }
}
