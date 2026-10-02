//! ADR-116 `requires:` field helpers: infer permissions from a
//! macro.sh script, validate permission strings, and format/insert
//! the requires field back into frontmatter.
//!
//! Lives here rather than in `per_file.rs` because the shell
//! tokenizer and command-position recognition is substantial enough
//! that mixing it with the lint-rule dispatch would obscure both.

use std::path::Path;

/// Known external commands that map to Bash(cmd:*) permissions.
/// Shell builtins and control flow keywords are excluded.
const EXTERNAL_COMMANDS: &[&str] = &[
    "attend", "awk", "basename", "cargo", "cat", "chmod", "cp", "curl", "cut",
    "date", "df", "diff", "dirname", "du", "file", "find", "gh", "git", "grep",
    "head", "id", "jq", "ln", "ls", "make", "mkdir", "mv", "node", "npm",
    "pnpm", "ps", "python3", "realpath", "rg", "rm", "sed", "sha256sum",
    "sort", "stat", "tail", "tee", "touch", "tr", "tree", "uname", "uniq",
    "ways", "wc", "which", "xargs", "yarn",
];

/// Scan a macro.sh file and extract external commands → permission strings.
pub(super) fn scan_macro_requires(path: &Path) -> Vec<String> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let mut found: Vec<&str> = Vec::new();
    let mut heredoc_delim: Option<String> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip here-doc bodies — content between <<DELIM and DELIM
        if let Some(ref delim) = heredoc_delim {
            if trimmed == delim.as_str() {
                heredoc_delim = None;
            }
            continue;
        }
        // Detect here-doc start: cat <<EOF, cat <<'EOF', cat <<"EOF"
        if let Some(pos) = trimmed.find("<<") {
            let after = trimmed[pos + 2..].trim_start_matches(&['-', '~'][..]);
            let delim = after
                .trim_start_matches('\'').trim_start_matches('"')
                .split(|c: char| c.is_whitespace() || c == '\'' || c == '"')
                .next()
                .unwrap_or("");
            if !delim.is_empty() {
                heredoc_delim = Some(delim.to_string());
            }
        }

        // Skip comments and empty lines
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Extract commands from trap 'cmd args' — the quoted command string
        if trimmed.starts_with("trap ") {
            scan_trap_command(trimmed, EXTERNAL_COMMANDS, &mut found);
        }

        // Look for external commands: word boundaries in the line
        for cmd in EXTERNAL_COMMANDS {
            if line_uses_command(trimmed, cmd) && !found.contains(cmd) {
                found.push(cmd);
            }
        }
    }

    found.sort();

    let mut perms: Vec<String> = Vec::new();

    // Check if script needs Read access (reads files outside macro dir)
    let needs_read = found.iter().any(|c| {
        matches!(*c, "cat" | "head" | "tail" | "ls" | "file" | "stat" | "find" | "tree")
    });
    if needs_read {
        perms.push("Read".to_string());
    }

    for cmd in &found {
        perms.push(format!("Bash({cmd}:*)"));
    }

    perms
}

/// Check if a shell line uses a specific command (not just as a substring).
fn line_uses_command(line: &str, cmd: &str) -> bool {
    // Match command at line start, after pipe, after $(), after backtick,
    // after &&, after ||, after ;, or after assignment
    for token in shell_tokens(line) {
        if token == cmd {
            return true;
        }
    }
    false
}

/// Simple shell tokenizer — extracts tokens in "command position".
/// Command position: first word of line, after |, &&, ||, ;, or inside $().
/// Also handles VAR=$(cmd ...) and trap 'cmd ...' patterns.
fn shell_tokens(line: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut expect_cmd = true; // first word is a command

    for word in line.split_whitespace() {
        // Handle VAR=$(cmd — split on $( inside the word
        let subshell_word = if let Some(pos) = word.find("$(") {
            let after = &word[pos + 2..];
            if !after.is_empty() {
                Some(after)
            } else {
                None // trailing $( — next word is the command
            }
        } else {
            None
        };

        // If we extracted a command from inside $(), process it
        if let Some(sw) = subshell_word {
            let clean = sw
                .trim_start_matches('\'')
                .trim_start_matches('"')
                .trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');
            if !clean.is_empty() && !clean.starts_with('-') {
                tokens.push(clean);
            }
        }

        // Detect if this word itself starts a subshell/substitution
        let starts_subshell = word.starts_with("$(")
            || word.starts_with('`')
            || word.starts_with('(');

        let is_cmd = expect_cmd || starts_subshell;

        // Strip leading shell syntax to get to the actual command
        let w = word
            .trim_start_matches("$(")
            .trim_start_matches('`')
            .trim_start_matches('(')
            .trim_start_matches('\'')
            .trim_start_matches('"');

        if is_cmd && !w.is_empty() && !w.starts_with('-') && !w.contains('=') {
            let clean = w.trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');
            if !clean.is_empty() && !tokens.contains(&clean) {
                tokens.push(clean);
            }
            expect_cmd = false;
        }

        // These signal the next word is a command
        if word.ends_with('|') || word == "&&" || word == "||" || word.ends_with(';')
            || word == "|" || word.ends_with("$(") || word.ends_with('`')
        {
            expect_cmd = true;
        }
    }
    tokens
}

/// Extract commands from trap 'cmd args' SIGNAL patterns.
/// The command string is inside single or double quotes as the second argument.
fn scan_trap_command<'a>(line: &'a str, external_commands: &[&'a str], found: &mut Vec<&'a str>) {
    let after_trap = line.strip_prefix("trap ").unwrap_or("").trim();
    let (quote_char, start) = if after_trap.starts_with('\'') {
        ('\'', 1)
    } else if after_trap.starts_with('"') {
        ('"', 1)
    } else {
        return;
    };

    if let Some(end) = after_trap[start..].find(quote_char) {
        let cmd_str = &after_trap[start..start + end];
        if let Some(first_word) = cmd_str.split_whitespace().next() {
            for cmd in external_commands {
                if first_word == *cmd && !found.contains(cmd) {
                    found.push(cmd);
                }
            }
        }
    }
}

/// Format requires list as YAML inline array for frontmatter.
pub(super) fn format_requires_yaml(reqs: &[String]) -> String {
    let items: Vec<String> = reqs.iter().map(|r| format!("\"{}\"", r)).collect();
    format!("requires: [{}]", items.join(", "))
}

/// Insert a requires: field into frontmatter, before the closing ---.
pub(super) fn insert_requires_field(content: &str, requires_line: &str) -> String {
    let mut lines: Vec<&str> = content.lines().collect();
    if let Some(idx) = ways_core::frontmatter::closing_fence_line(content) {
        lines.insert(idx, requires_line);
    }

    let mut result = lines.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// Parse requires: field from frontmatter as a list of strings.
/// Handles both inline array [a, b] and YAML list (- a) formats.
pub(super) fn extract_requires_list(fm: &str) -> Option<Vec<String>> {
    crate::cmd::permissions::extract_requires(fm)
}

/// Validate that a permission string is well-formed.
///
/// Intentionally stricter than `Permission::parse` — restricts tool names to
/// the four Claude Code tools (Read, Write, Edit, Bash) rather than accepting
/// any uppercase identifier. This catches typos like "Rad(foo)" that parse()
/// would accept as structurally valid but aren't real Claude Code permissions.
pub(super) fn is_valid_permission(perm: &str) -> bool {
    use agent_fmt::permissions::Permission;

    match Permission::parse(perm) {
        None => false,
        Some(Permission::Wildcard) => true,
        Some(Permission::Tool(ref t)) | Some(Permission::Scoped(ref t, _)) => {
            matches!(t.as_str(), "Read" | "Write" | "Edit" | "Bash")
        }
    }
}
