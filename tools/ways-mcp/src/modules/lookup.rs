//! The lookup tools of the `ways` module: `ways_search`, `ways_read` and
//! `ways_neighbors` (ADR-701 §5).
//!
//! Each tool runs `ways lookup <verb>` and returns the JSON object it prints.
//! The matcher, the body renderer, the disclosure stamps and the event log live
//! in the `ways` binary, which the hooks also run, so a pull stamps exactly what
//! an injection stamps and a search ranks exactly what a scan ranks. Nothing
//! here duplicates them, and no daemon is involved: the call is a child process
//! for its duration (ADR-501 item 1, ADR-187 item 5).

use crate::server::{Tool, ToolResult};
use crate::session::{self, Identity};
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const DEFAULT_TOP: u64 = 5;
const MAX_TOP: u64 = 20;

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "ways_search",
            description: "Search the ways for a query. Ranks the ways a prompt on this session would compete, \
                          best first, each with its id, route, description, cosine, share (its softmax share of \
                          the top eight) and margin (its cosine lead over the next candidate), and the matched \
                          body section once the body corpus exists. Disabled ways and ways out of scope for the \
                          session are not listed. Fires and stamps nothing; read a result with ways_read.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "What to look for, in plain words." },
                    "top_n": { "type": "integer", "minimum": 1, "maximum": MAX_TOP, "default": DEFAULT_TOP,
                               "description": "How many candidates to return." },
                },
                "required": ["query"],
                "additionalProperties": false,
            }),
        },
        Tool {
            name: "ways_read",
            description: "Read a way by id, as injection would deliver it. It always returns the way, including \
                          when the way was disclosed recently and injection would hold it back, and whatever \
                          its scope (a note says when the scope does not match this session). The tool changes \
                          nothing itself: a PostToolUse hook records the pull as a disclosure to the calling \
                          agent, so injection does not repeat the way on the next turn, and logs it with how \
                          far it sat inside the refire window. A way the operator disabled, or a project with \
                          ways switched off, is refused and the error names the switch.",
            input_schema: json!({
                "type": "object",
                "properties": { "id": { "type": "string", "description": "The way id, as ways_search returns it." } },
                "required": ["id"],
                "additionalProperties": false,
            }),
        },
        Tool {
            name: "ways_neighbors",
            description: "The ways around one way, each labelled with its kind: parent, child, see_also (a \
                          See Also entry in this way), see_also_from (a way whose See Also names this one) and \
                          semantic (the nearest ways by embedding, with cosine). A neighbour the operator \
                          disabled carries enabled: false, and one whose scope would not reach this session carries in_scope: false. \
                          Read-only.",
            input_schema: json!({
                "type": "object",
                "properties": { "id": { "type": "string", "description": "The way id, as ways_search returns it." } },
                "required": ["id"],
                "additionalProperties": false,
            }),
        },
    ]
}

/// Whether `tool` is one of the lookup tools.
pub fn handles(tool: &str) -> bool {
    matches!(tool, "ways_search" | "ways_read" | "ways_neighbors")
}

/// The version of the `ways lookup` JSON this server reads. A `ways` that
/// prints another is refused, so the two binaries can update apart.
const CONTRACT: u64 = 1;

/// How long a lookup may run before it is killed. A search embeds the query and
/// loads the corpus; the others read files.
fn deadline(tool: &str) -> Duration {
    Duration::from_secs(if tool == "ways_search" { 30 } else { 10 })
}

/// What a lookup call needs: the `ways` binary and the session it acts for.
pub struct Lookup {
    bin: PathBuf,
    session: Option<Identity>,
    /// Replaces the per-tool deadline; set from `WAYS_MCP_LOOKUP_TIMEOUT_MS`.
    timeout: Option<Duration>,
}

impl Lookup {
    /// Derived afresh for each call.
    pub fn current() -> Self {
        let timeout = std::env::var("WAYS_MCP_LOOKUP_TIMEOUT_MS").ok().and_then(|v| v.parse().ok()).map(Duration::from_millis);
        Self { bin: ways_bin(), session: session::identity(), timeout }
    }

    /// Runs the lookup tool `tool`; callers check [`handles`] first.
    pub fn call(&self, tool: &str, args: &Value) -> ToolResult {
        let verb: Result<Vec<String>, String> = match tool {
            "ways_search" => search_args(args),
            "ways_read" => id_args(args, "read"),
            "ways_neighbors" => id_args(args, "neighbors"),
            _ => Err(format!("not a lookup tool: {tool}")),
        };
        match verb {
            Ok(verb) => self.run(tool, &verb),
            Err(message) => failure(message),
        }
    }

    fn run(&self, tool: &str, verb: &[String]) -> ToolResult {
        let mut cmd = Command::new(&self.bin);
        cmd.arg("lookup");
        if let Some(project) = self.session.as_ref().and_then(|s| s.project.as_deref()) {
            cmd.args(["--project", project]);
        }
        cmd.arg(&verb[0]);
        // The session scopes a search, a read's scope note and neighbours' scope marks.
        if let Some(s) = &self.session {
            cmd.args(["--session", &s.id]);
        }
        // Everything after `--` is positional, so a query or id that begins with a dash is not a flag.
        cmd.args(&verb[1..]);

        let limit = self.timeout.unwrap_or_else(|| deadline(tool));
        let out = match run_bounded(cmd, limit) {
            Ok(o) => o,
            Err(Bounded::Spawn(e)) => return failure(format!("could not run {}: {e}", self.bin.display())),
            Err(Bounded::TimedOut) => return failure(format!("the ways lookup timed out after {}s", limit.as_secs_f64())),
        };
        let Ok(Value::Object(mut content)) = serde_json::from_slice::<Value>(&out.stdout) else {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return failure(format!("the ways lookup printed no result (exit {:?}): {}", out.code, stderr.trim()));
        };
        match content.remove("contract").and_then(|c| c.as_u64()) {
            Some(CONTRACT) => {}
            other => {
                let said = other.map_or("none".to_string(), |c| c.to_string());
                return failure(format!("the ways lookup speaks contract {said}, this server expects {CONTRACT}: update ways and ways-mcp together"));
            }
        }
        let is_error = !out.success || content.contains_key("error");
        if !is_error && tool != "ways_neighbors" {
            content.insert("session".into(), self.session.as_ref().map_or(Value::Null, |s| json!(s.id)));
        }
        ToolResult { content, is_error }
    }
}

struct Finished {
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    code: Option<i32>,
    success: bool,
}

enum Bounded {
    Spawn(std::io::Error),
    TimedOut,
}

/// Run `cmd` and collect its output, killing and reaping it at `limit`. Both
/// pipes are drained on their own threads so a large result cannot block the
/// child on a full pipe. After a kill the readers are left to finish by
/// themselves: a grandchild may still hold a pipe open, and the call must not
/// wait for it.
fn run_bounded(mut cmd: Command, limit: Duration) -> Result<Finished, Bounded> {
    use std::io::Read;
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(Bounded::Spawn)?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut p) = pipe {
                let _ = p.read_to_end(&mut buf);
            }
            let _ = tx.send(buf);
        });
        rx
    };
    let out_rx = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err_rx = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= limit => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Bounded::TimedOut);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => return Err(Bounded::Spawn(e)),
        }
    };
    // The child has exited, so its pipes close and the readers finish.
    let stdout = out_rx.recv().unwrap_or_default();
    let stderr = err_rx.recv().unwrap_or_default();
    Ok(Finished { stdout, stderr, code: status.code(), success: status.success() })
}

/// `ways` next to this binary as it was launched (the install links both into
/// one bin directory; `current_exe` would name the link's target), else the one
/// on `PATH`. `WAYS_BIN` overrides both.
fn ways_bin() -> PathBuf {
    if let Some(p) = std::env::var_os("WAYS_BIN").filter(|p| !p.is_empty()) {
        return p.into();
    }
    let name = if cfg!(windows) { "ways.exe" } else { "ways" };
    sibling_of(std::env::args_os().next().as_deref(), name).unwrap_or_else(|| name.into())
}

/// `name` in the directory `argv0` was launched from, when it names one and the
/// file is there. A bare command name says nothing about where it lives.
fn sibling_of(argv0: Option<&std::ffi::OsStr>, name: &str) -> Option<PathBuf> {
    let dir = std::path::Path::new(argv0?).parent().filter(|d| !d.as_os_str().is_empty())?;
    Some(dir.join(name)).filter(|p| p.is_file())
}

fn failure(message: String) -> ToolResult {
    let mut content = Map::new();
    content.insert("error".into(), json!(message));
    ToolResult { content, is_error: true }
}

fn string_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    match args.get(key).and_then(Value::as_str).map(str::trim) {
        Some(s) if !s.is_empty() => Ok(s),
        _ => Err(format!("`{key}` is required: a non-empty string")),
    }
}

fn search_args(args: &Value) -> Result<Vec<String>, String> {
    let query = string_arg(args, "query")?;
    let top = match args.get("top_n") {
        None | Some(Value::Null) => DEFAULT_TOP,
        Some(v) => v.as_u64().filter(|n| (1..=MAX_TOP).contains(n)).ok_or_else(|| format!("`top_n` must be an integer from 1 to {MAX_TOP}"))?,
    };
    Ok(vec!["search".into(), "--top".into(), top.to_string(), "--".into(), query.into()])
}

fn id_args(args: &Value, verb: &str) -> Result<Vec<String>, String> {
    let id = string_arg(args, "id")?;
    Ok(vec![verb.into(), "--".into(), id.into()])
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A stand-in `ways` that prints its arguments as a JSON object, or the
    /// given text, and exits with `code`.
    fn stub(name: &str, body: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ways-mcp-stub-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ways");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    const ECHO_ARGV: &str = r#"printf '{"contract":1,"argv":['; sep=""; for a in "$@"; do printf '%s"%s"' "$sep" "$a"; sep=","; done; printf ']}\n'"#;

    fn lookup(bin: PathBuf, session: Option<(&str, Option<&str>)>) -> Lookup {
        Lookup { bin, timeout: None, session: session.map(|(id, project)| Identity { id: id.into(), project: project.map(Into::into) }) }
    }

    /// Runs a tool, retrying when the kernel reports the freshly written stub
    /// busy: a parallel test's fork can hold its write descriptor for a moment.
    fn call(l: &Lookup, tool: &str, args: &Value) -> ToolResult {
        for _ in 0..20 {
            let r = l.call(tool, args);
            if !r.content.get("error").and_then(Value::as_str).is_some_and(|e| e.contains("Text file busy")) {
                return r;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        l.call(tool, args)
    }

    fn argv(r: &ToolResult) -> Vec<String> {
        r.content["argv"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect()
    }

    #[test]
    fn search_passes_the_session_project_and_a_bounded_top_n() {
        let l = lookup(stub("search", ECHO_ARGV), Some(("sess-1", Some("/work/proj"))));
        let r = call(&l, "ways_search", &json!({ "query": "write a unit test", "top_n": 3 }));
        assert!(!r.is_error);
        assert_eq!(
            argv(&r),
            ["lookup", "--project", "/work/proj", "search", "--session", "sess-1", "--top", "3", "--", "write a unit test"]
        );
        assert_eq!(r.content["session"], "sess-1");
    }

    #[test]
    fn read_names_the_way_after_the_separator_so_a_dash_is_not_a_flag() {
        let l = lookup(stub("read", ECHO_ARGV), Some(("sess-2", None)));
        let r = call(&l, "ways_read", &json!({ "id": "--session" }));
        assert_eq!(argv(&r), ["lookup", "read", "--session", "sess-2", "--", "--session"]);
    }

    #[test]
    fn neighbors_carries_the_session_to_mark_out_of_scope_ways() {
        let l = lookup(stub("neighbors", ECHO_ARGV), Some(("sess-3", Some("/p"))));
        let r = call(&l, "ways_neighbors", &json!({ "id": "d/code" }));
        assert_eq!(argv(&r), ["lookup", "--project", "/p", "neighbors", "--session", "sess-3", "--", "d/code"]);
        assert!(r.content.get("contract").is_none(), "the contract is checked, not passed on");
    }

    #[test]
    fn without_a_session_the_tools_act_unscoped_and_report_it() {
        let l = lookup(stub("nosession", ECHO_ARGV), None);
        let r = call(&l, "ways_read", &json!({ "id": "d/w" }));
        assert_eq!(argv(&r), ["lookup", "read", "--", "d/w"]);
        assert_eq!(r.content["session"], Value::Null);
    }

    #[test]
    fn a_ways_error_object_is_an_error_result_with_its_message() {
        let l = lookup(stub("error", r#"printf '{"contract":1,"error":"way d/w is disabled: the project toggle `d/*: false`"}\n'; exit 1"#), Some(("s", None)));
        let r = call(&l, "ways_read", &json!({ "id": "d/w" }));
        assert!(r.is_error);
        assert!(r.content["error"].as_str().unwrap().contains("d/*: false"));
    }

    #[test]
    fn a_ways_that_prints_nothing_usable_is_an_error_with_its_stderr() {
        let l = lookup(stub("garbage", "echo oops >&2; exit 3"), None);
        let r = call(&l, "ways_search", &json!({ "query": "x" }));
        assert!(r.is_error);
        let e = r.content["error"].as_str().unwrap();
        assert!(e.contains("oops") && e.contains("exit Some(3)"), "{e}");
    }

    #[test]
    fn a_missing_ways_binary_is_an_error_naming_it() {
        let l = lookup("/nonexistent/ways".into(), None);
        let r = call(&l, "ways_neighbors", &json!({ "id": "d/w" }));
        assert!(r.is_error);
        assert!(r.content["error"].as_str().unwrap().contains("/nonexistent/ways"));
    }

    #[test]
    fn bad_arguments_are_refused_before_ways_runs() {
        let l = lookup("/nonexistent/ways".into(), None);
        for (tool, args) in [
            ("ways_search", json!({})),
            ("ways_search", json!({ "query": "  " })),
            ("ways_search", json!({ "query": "x", "top_n": 0 })),
            ("ways_search", json!({ "query": "x", "top_n": 21 })),
            ("ways_search", json!({ "query": "x", "top_n": "3" })),
            ("ways_read", json!({})),
            ("ways_neighbors", json!({ "id": 7 })),
        ] {
            let r = call(&l, tool, &args);
            assert!(r.is_error, "{tool} {args}");
            assert!(!r.content["error"].as_str().unwrap().contains("could not run"), "{tool} {args}: ways was run");
        }
    }

    #[test]
    fn a_ways_that_hangs_is_killed_at_the_deadline_and_the_call_returns() {
        let mut l = lookup(stub("hang", "exec sleep 30"), None);
        l.timeout = Some(std::time::Duration::from_millis(300));
        let started = std::time::Instant::now();
        let r = call(&l, "ways_read", &json!({ "id": "d/w" }));
        assert!(started.elapsed() < std::time::Duration::from_secs(5), "returned after {:?}", started.elapsed());
        assert!(r.is_error);
        assert_eq!(r.content["error"], "the ways lookup timed out after 0.3s");
    }

    #[test]
    fn the_deadline_is_longer_for_a_search_than_for_a_read() {
        assert_eq!(deadline("ways_search"), std::time::Duration::from_secs(30));
        assert_eq!(deadline("ways_read"), std::time::Duration::from_secs(10));
        assert_eq!(deadline("ways_neighbors"), std::time::Duration::from_secs(10));
    }

    #[test]
    fn output_larger_than_a_pipe_buffer_is_read_while_the_child_runs() {
        let l = lookup(stub("big", r#"printf '{"contract":1,"body":"'; head -c 300000 /dev/zero | tr '\0' x; printf '"}\n'"#), None);
        let r = call(&l, "ways_read", &json!({ "id": "d/w" }));
        assert!(!r.is_error, "{:?}", r.content.get("error"));
        assert_eq!(r.content["body"].as_str().unwrap().len(), 300_000);
    }

    #[test]
    fn a_ways_that_speaks_another_contract_is_refused() {
        for body in [r#"printf '{"contract":2,"candidates":[]}\n'"#, r#"printf '{"candidates":[]}\n'"#] {
            let l = lookup(stub("contract", body), None);
            let r = call(&l, "ways_search", &json!({ "query": "x" }));
            assert!(r.is_error);
            let e = r.content["error"].as_str().unwrap();
            assert!(e.contains("contract") && e.contains('1'), "{e}");
        }
    }

    #[test]
    fn ways_is_found_beside_the_launch_path_not_beside_the_resolved_target() {
        let base = std::env::temp_dir().join(format!("ways-mcp-sibling-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (bin, data) = (base.join("bin"), base.join("data"));
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("ways-mcp"), "").unwrap();
        std::fs::write(bin.join("ways"), "").unwrap();
        std::os::unix::fs::symlink(data.join("ways-mcp"), bin.join("ways-mcp")).unwrap();
        // argv[0] is the projected link; its target's directory holds no `ways`.
        assert_eq!(sibling_of(Some(bin.join("ways-mcp").as_os_str()), "ways"), Some(bin.join("ways")));
        assert_eq!(sibling_of(Some(std::ffi::OsStr::new("ways-mcp")), "ways"), None, "a bare name says nothing about where it lives");
        assert_eq!(sibling_of(Some(data.join("ways-mcp").as_os_str()), "ways"), None);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn only_the_three_lookup_tools_are_claimed() {
        assert!(handles("ways_search") && handles("ways_read") && handles("ways_neighbors"));
        assert!(!handles("ways_status"));
    }

    #[test]
    fn the_three_tools_are_listed_with_required_arguments() {
        let t = tools();
        assert_eq!(t.iter().map(|t| t.name).collect::<Vec<_>>(), ["ways_search", "ways_read", "ways_neighbors"]);
        assert_eq!(t[0].input_schema["required"], json!(["query"]));
        assert_eq!(t[1].input_schema["required"], json!(["id"]));
        assert_eq!(t[2].input_schema["required"], json!(["id"]));
    }
}
