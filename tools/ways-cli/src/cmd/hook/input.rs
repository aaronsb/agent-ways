//! The hook payload Claude Code writes on stdin, read once into the request
//! each event needs. Pure: no filesystem, no environment beyond what the
//! caller passes, so each event's parse is pinned by a test.

use serde_json::Value;

/// The events `ways hook` serves. Each is one adapter script under
/// `hooks/ways/`, named by what it does rather than by Claude Code's event,
/// since some Claude Code events run more than one of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum HookEvent {
    /// UserPromptSubmit: match the prompt (check-prompt.sh).
    Prompt,
    /// SessionStart and UserPromptSubmit: state triggers (check-state.sh).
    State,
    /// PreToolUse Bash: match the command (check-bash-pre.sh).
    Command,
    /// PreToolUse Edit|Write: match the file (check-file-pre.sh).
    File,
    /// PreToolUse Task: match the delegation, stash for SubagentStart (check-task-pre.sh).
    Task,
    /// PostToolUse and PostToolUseFailure: run the postchecks (check-post.sh).
    PostTool,
    /// PostToolUse on `ways_read`: stamp the pull for the calling agent (check-pull.sh).
    Pull,
    /// PostToolUse: match queued operator messages (check-queued.sh).
    Queued,
    /// Stop: record the last response for the next prompt (check-response.sh).
    Stop,
    /// SubagentStart: inject the stashed ways (inject-subagent.sh).
    SubagentStart,
    /// SessionStart: clear this session's state (clear-markers.sh).
    SessionStart,
    /// PreToolUse TaskCreate: write the tasks-active marker (mark-tasks-active.sh). Nothing reads it today.
    TasksActive,
}

/// What every event reads from the payload.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Common {
    pub session: Option<String>,
    /// Set inside a subagent: its own id (the session id is the parent's).
    pub agent_id: Option<String>,
    /// `CLAUDE_PROJECT_DIR` when set, else the payload's `cwd`.
    pub project: Option<String>,
    pub transcript: Option<String>,
}

/// One event's work, with the values it needs. `Skip` when the payload lacks
/// what the event needs; the hook then does nothing.
#[derive(Debug, PartialEq, Eq)]
pub enum Request {
    Prompt { session: String, query: String },
    State { session: String, hook_event: String, query: Option<String> },
    Command { session: String, command: String, description: Option<String> },
    File { session: String, path: String },
    Task { session: String, query: String, team: Option<String>, subagent_type: Option<String> },
    PostTool { session: String, hook_event: String },
    Pull { session: String, id: String },
    Queued { session: String, transcript: String },
    Stop { session: String, transcript: String },
    SubagentStart { session: String },
    SessionStart { session: Option<String> },
    TasksActive { session: String },
    Skip,
}

/// The parsed payload. Malformed JSON reads as an empty payload.
pub struct HookInput {
    v: Value,
}

impl HookInput {
    pub fn parse(raw: &str) -> Self {
        Self { v: serde_json::from_str(raw).unwrap_or(Value::Null) }
    }

    /// A non-empty string at a JSON pointer.
    fn text(&self, pointer: &str) -> Option<String> {
        self.v.pointer(pointer).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string)
    }

    fn flag(&self, pointer: &str) -> bool {
        self.v.pointer(pointer).and_then(Value::as_bool).unwrap_or(false)
    }

    pub fn common(&self, env_project: Option<String>) -> Common {
        Common {
            session: self.text("/session_id"),
            // Names marker files and state directories, so stored by its key.
            agent_id: self.text("/agent_id").map(|a| crate::session::agent_key(&a)),
            project: env_project.filter(|p| !p.is_empty()).or_else(|| self.text("/cwd")),
            transcript: self.text("/transcript_path"),
        }
    }

    /// The request `event` makes of this payload.
    pub fn request(&self, event: HookEvent) -> Request {
        // Every event builds paths under the sessions root from the id, so
        // only a plain id is used; anything else is treated as absent.
        let Some(session) = self.text("/session_id").filter(|s| crate::session::is_plain_session_id(s)) else {
            // Clearing with no session id logs the start and clears nothing.
            return match event {
                HookEvent::SessionStart => Request::SessionStart { session: None },
                _ => Request::Skip,
            };
        };
        let lower = |p: &str| self.text(p).map(|s| s.to_lowercase());
        match event {
            HookEvent::Prompt => Request::Prompt { session, query: lower("/prompt").unwrap_or_default() },
            HookEvent::State => Request::State {
                session,
                hook_event: self.text("/hook_event_name").unwrap_or_else(|| "SessionStart".into()),
                // Only a harness-envelope test reads it, so it is passed whole.
                query: self.text("/prompt"),
            },
            HookEvent::Command => Request::Command {
                session,
                command: self.text("/tool_input/command").unwrap_or_default(),
                description: lower("/tool_input/description"),
            },
            HookEvent::File => match self.text("/tool_input/file_path") {
                Some(path) => Request::File { session, path },
                None => Request::Skip,
            },
            HookEvent::Task => match lower("/tool_input/prompt") {
                Some(query) => Request::Task {
                    session,
                    query,
                    team: self.text("/tool_input/team_name"),
                    subagent_type: self.text("/tool_input/subagent_type"),
                },
                None => Request::Skip,
            },
            HookEvent::PostTool => Request::PostTool {
                session,
                hook_event: self.text("/hook_event_name").unwrap_or_else(|| "PostToolUse".into()),
            },
            HookEvent::Pull => match self.text("/tool_input/id") {
                Some(id) => Request::Pull { session, id },
                None => Request::Skip,
            },
            HookEvent::Queued => match self.text("/transcript_path") {
                Some(transcript) => Request::Queued { session, transcript },
                None => Request::Skip,
            },
            HookEvent::Stop => match self.text("/transcript_path") {
                // A Stop the hook itself continued must not loop.
                Some(transcript) if !self.flag("/stop_hook_active") => Request::Stop { session, transcript },
                _ => Request::Skip,
            },
            HookEvent::SubagentStart => Request::SubagentStart { session },
            HookEvent::SessionStart => Request::SessionStart { session: Some(session) },
            HookEvent::TasksActive => Request::TasksActive { session },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use HookEvent as E;

    fn req(event: HookEvent, raw: &str) -> Request {
        HookInput::parse(raw).request(event)
    }

    #[test]
    fn common_fields_and_the_project_default() {
        let raw = r#"{"session_id":"s1","agent_id":"a1","cwd":"/srv/cwd","transcript_path":"/t.jsonl"}"#;
        let input = HookInput::parse(raw);
        assert_eq!(
            input.common(None),
            Common {
                session: Some("s1".into()),
                agent_id: Some("a1".into()),
                project: Some("/srv/cwd".into()),
                transcript: Some("/t.jsonl".into()),
            }
        );
        // CLAUDE_PROJECT_DIR wins over cwd; an empty one does not.
        assert_eq!(input.common(Some("/srv/env".into())).project.as_deref(), Some("/srv/env"));
        assert_eq!(input.common(Some(String::new())).project.as_deref(), Some("/srv/cwd"));
        assert_eq!(HookInput::parse("not json").common(None), Common::default());
        // The agent id names marker files: one that is not plain, is over-long,
        // or is the literal `main` is kept as a hashed key, never as main.
        let key = |id: &str| {
            HookInput::parse(&format!(r#"{{"session_id":"s1","agent_id":"{id}"}}"#)).common(None).agent_id.unwrap()
        };
        let long = "a".repeat(65);
        for odd in ["/../", "main", long.as_str()] {
            let k = key(odd);
            assert!(k.starts_with('h') && k.len() == 17 && k != "main", "{odd} -> {k}");
            assert_eq!(crate::session::agent_key(&k), k, "the key is stable");
        }
        assert_ne!(key("main"), key("/../"));
        assert_eq!(key(&"a".repeat(64)), "a".repeat(64));
    }

    #[test]
    fn prompt_is_lowercased_whole() {
        let raw = r#"{"session_id":"s","prompt":"-Spawn A Thing\nÉtat"}"#;
        assert_eq!(req(E::Prompt, raw), Request::Prompt { session: "s".into(), query: "-spawn a thing\nétat".into() });
    }

    #[test]
    fn state_carries_the_event_and_the_prompt() {
        let raw = r#"{"session_id":"s","hook_event_name":"UserPromptSubmit","prompt":"  <Task-Notification> x"}"#;
        assert_eq!(
            req(E::State, raw),
            Request::State {
                session: "s".into(),
                hook_event: "UserPromptSubmit".into(),
                query: Some("  <Task-Notification> x".into()),
            }
        );
        assert_eq!(
            req(E::State, r#"{"session_id":"s"}"#),
            Request::State { session: "s".into(), hook_event: "SessionStart".into(), query: None }
        );
    }

    #[test]
    fn command_keeps_the_command_and_lowercases_the_description() {
        let raw = r#"{"session_id":"s","tool_input":{"command":"--Git Commit","description":"Commit It"}}"#;
        assert_eq!(
            req(E::Command, raw),
            Request::Command { session: "s".into(), command: "--Git Commit".into(), description: Some("commit it".into()) }
        );
    }

    #[test]
    fn file_needs_a_path() {
        let raw = r#"{"session_id":"s","tool_input":{"file_path":"/srv/p/README.md"}}"#;
        assert_eq!(req(E::File, raw), Request::File { session: "s".into(), path: "/srv/p/README.md".into() });
        assert_eq!(req(E::File, r#"{"session_id":"s","tool_input":{}}"#), Request::Skip);
    }

    #[test]
    fn task_needs_a_prompt() {
        let raw = r#"{"session_id":"s","tool_input":{"prompt":"Review This","team_name":"red","subagent_type":"code-reviewer"}}"#;
        assert_eq!(
            req(E::Task, raw),
            Request::Task {
                session: "s".into(),
                query: "review this".into(),
                team: Some("red".into()),
                subagent_type: Some("code-reviewer".into()),
            }
        );
        assert_eq!(req(E::Task, r#"{"session_id":"s","tool_input":{}}"#), Request::Skip);
    }

    #[test]
    fn post_tool_names_its_event() {
        let raw = r#"{"session_id":"s","hook_event_name":"PostToolUseFailure","tool_name":"Bash"}"#;
        assert_eq!(req(E::PostTool, raw), Request::PostTool { session: "s".into(), hook_event: "PostToolUseFailure".into() });
        assert_eq!(
            req(E::PostTool, r#"{"session_id":"s"}"#),
            Request::PostTool { session: "s".into(), hook_event: "PostToolUse".into() }
        );
    }

    #[test]
    fn pull_needs_the_way_id_the_tool_was_given() {
        let raw = r#"{"session_id":"s","tool_input":{"id":"d/w"}}"#;
        assert_eq!(req(E::Pull, raw), Request::Pull { session: "s".into(), id: "d/w".into() });
        assert_eq!(req(E::Pull, r#"{"session_id":"s","tool_input":{}}"#), Request::Skip);
    }

    #[test]
    fn queued_needs_a_transcript() {
        let raw = r#"{"session_id":"s","transcript_path":"/t.jsonl"}"#;
        assert_eq!(req(E::Queued, raw), Request::Queued { session: "s".into(), transcript: "/t.jsonl".into() });
        assert_eq!(req(E::Queued, r#"{"session_id":"s"}"#), Request::Skip);
    }

    #[test]
    fn stop_needs_a_transcript_and_never_loops() {
        let raw = r#"{"session_id":"s","transcript_path":"/t.jsonl","stop_hook_active":false}"#;
        assert_eq!(req(E::Stop, raw), Request::Stop { session: "s".into(), transcript: "/t.jsonl".into() });
        let active = r#"{"session_id":"s","transcript_path":"/t.jsonl","stop_hook_active":true}"#;
        assert_eq!(req(E::Stop, active), Request::Skip);
    }

    #[test]
    fn session_events() {
        assert_eq!(req(E::SubagentStart, r#"{"session_id":"s"}"#), Request::SubagentStart { session: "s".into() });
        assert_eq!(req(E::TasksActive, r#"{"session_id":"s"}"#), Request::TasksActive { session: "s".into() });
        assert_eq!(req(E::SessionStart, r#"{"session_id":"s"}"#), Request::SessionStart { session: Some("s".into()) });
        assert_eq!(req(E::SessionStart, r#"{"cwd":"/p"}"#), Request::SessionStart { session: None });
    }

    #[test]
    fn no_session_no_work() {
        for e in [E::Prompt, E::State, E::Command, E::File, E::Task, E::PostTool, E::Pull, E::Queued, E::Stop, E::SubagentStart, E::TasksActive] {
            assert_eq!(req(e, r#"{"prompt":"x","transcript_path":"/t"}"#), Request::Skip, "{e:?}");
            // An id that would leave the sessions root is no id.
            let escaping = r#"{"session_id":"../victim","prompt":"x","transcript_path":"/t"}"#;
            assert_eq!(req(e, escaping), Request::Skip, "{e:?}");
        }
        assert_eq!(req(E::SessionStart, r#"{"session_id":"../x"}"#), Request::SessionStart { session: None });
    }
}
