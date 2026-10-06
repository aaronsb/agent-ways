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
use std::process::Command;

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
                          when the way was disclosed recently and injection would hold it back (out_of_band is \
                          true then, with the epochs since its last disclosure). It records the disclosure for \
                          this session, so injection does not repeat the way on the next turn, and logs the pull. \
                          A way the operator disabled is refused and the error names the toggle.",
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
                          disabled carries enabled: false. Read-only.",
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

/// What a lookup call needs: the `ways` binary and the session it acts for.
pub struct Lookup {
    bin: PathBuf,
    session: Option<Identity>,
}

impl Lookup {
    /// Derived afresh for each call.
    pub fn current() -> Self {
        Self { bin: ways_bin(), session: session::identity() }
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
        // The session scopes a search and is stamped by a read; neighbours need none.
        if tool != "ways_neighbors" {
            if let Some(s) = &self.session {
                cmd.args(["--session", &s.id]);
            }
        }
        // Everything after `--` is positional, so a query or id that begins with a dash is not a flag.
        cmd.args(&verb[1..]);

        let out = match cmd.output() {
            Ok(o) => o,
            Err(e) => return failure(format!("could not run {}: {e}", self.bin.display())),
        };
        let Ok(Value::Object(mut content)) = serde_json::from_slice::<Value>(&out.stdout) else {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return failure(format!("ways lookup printed no result (exit {:?}): {}", out.status.code(), stderr.trim()));
        };
        let is_error = !out.status.success() || content.contains_key("error");
        if !is_error && tool != "ways_neighbors" {
            content.insert("session".into(), self.session.as_ref().map_or(Value::Null, |s| json!(s.id)));
        }
        ToolResult { content, is_error }
    }
}

/// `ways` next to this binary (the install links both into one bin directory),
/// else the one on `PATH`. `WAYS_BIN` overrides both.
fn ways_bin() -> PathBuf {
    if let Some(p) = std::env::var_os("WAYS_BIN").filter(|p| !p.is_empty()) {
        return p.into();
    }
    let name = if cfg!(windows) { "ways.exe" } else { "ways" };
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(name)))
        .filter(|p| p.is_file())
        .unwrap_or_else(|| name.into())
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

    const ECHO_ARGV: &str = r#"printf '{"argv":['; sep=""; for a in "$@"; do printf '%s"%s"' "$sep" "$a"; sep=","; done; printf ']}\n'"#;

    fn lookup(bin: PathBuf, session: Option<(&str, Option<&str>)>) -> Lookup {
        Lookup { bin, session: session.map(|(id, project)| Identity { id: id.into(), project: project.map(Into::into) }) }
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
    fn neighbors_needs_no_session() {
        let l = lookup(stub("neighbors", ECHO_ARGV), Some(("sess-3", Some("/p"))));
        let r = call(&l, "ways_neighbors", &json!({ "id": "d/code" }));
        assert_eq!(argv(&r), ["lookup", "--project", "/p", "neighbors", "--", "d/code"]);
        assert!(r.content.get("session").is_none());
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
        let l = lookup(stub("error", r#"printf '{"error":"way d/w is disabled: the project toggle `d/*: false`"}\n'; exit 1"#), Some(("s", None)));
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
