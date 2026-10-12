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
    "head", "id", "jq", "ln", "ls", "make", "mkdir", "mv", "node", "npm", "paste",
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
    let mut quote_ctx = vec![Ctx::Code];

    for line in content.lines() {
        let trimmed = line.trim();

        // Skip here-doc bodies — content between <<DELIM and DELIM
        if let Some(ref delim) = heredoc_delim {
            if trimmed == delim.as_str() {
                heredoc_delim = None;
            }
            continue;
        }
        // Quoted text is data: `echo "a && touch b"` runs no `touch`. The
        // masked line keeps byte positions, so the delimiter below is read
        // from the original.
        let masked = mask_line(trimmed, &mut quote_ctx);

        // Detect here-doc start: cat <<EOF, cat <<'EOF', cat <<"EOF".
        // A `<<<` here-string has no body and is not one.
        if let Some(pos) = heredoc_start(&masked) {
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
            if line_uses_command(&masked, cmd) && !found.contains(cmd) {
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

/// Where [`mask_line`] stands at a byte: in code, inside a `$( )` or
/// backtick substitution, or inside a quoted string.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Ctx {
    Code,
    Subst,
    Backtick,
    Single,
    /// `$'...'`, where a backslash escapes the next byte.
    AnsiC,
    Double,
}

/// [`mask_line`] for one line on its own.
#[cfg(test)]
fn mask_quoted(line: &str) -> String {
    mask_line(line, &mut vec![Ctx::Code])
}

/// `line` with the literal text inside quotes replaced by `x`, byte for byte:
/// single-quoted text whole, double-quoted text except a `$( )` or backtick
/// substitution inside it, whose commands do run, and a `#` comment's text.
/// The quote marks stay, so a quoted string reads as one word that is never
/// a command name. `stack` carries the context from the previous line, so a
/// string opened there (a multi-line awk program) is still a string here.
fn mask_line(line: &str, stack: &mut Vec<Ctx>) -> String {
    let bytes = line.as_bytes();
    let mut out = bytes.to_vec();
    if stack.is_empty() {
        stack.push(Ctx::Code);
    }
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let top = *stack.last().unwrap_or(&Ctx::Code);
        match top {
            Ctx::Single => {
                if b == b'\'' {
                    stack.pop();
                } else {
                    out[i] = b'x';
                }
            }
            Ctx::AnsiC => match b {
                b'\'' => {
                    stack.pop();
                }
                b'\\' => {
                    out[i] = b'x';
                    if i + 1 < bytes.len() {
                        out[i + 1] = b'x';
                    }
                    i += 1;
                }
                _ => out[i] = b'x',
            },
            Ctx::Double => match b {
                b'"' => {
                    stack.pop();
                }
                b'\\' => {
                    out[i] = b'x';
                    if i + 1 < bytes.len() {
                        out[i + 1] = b'x';
                    }
                    i += 1;
                }
                b'$' if bytes.get(i + 1) == Some(&b'(') => {
                    stack.push(Ctx::Subst);
                    i += 1;
                }
                b'`' => stack.push(Ctx::Backtick),
                _ => out[i] = b'x',
            },
            Ctx::Code | Ctx::Subst | Ctx::Backtick => match b {
                b'\\' => i += 1,
                // A comment runs to the end of the line: `#` that starts a
                // word, not the one in `$#` or `${#x}`.
                b'#' if i == 0 || bytes[i - 1].is_ascii_whitespace() || bytes[i - 1] == b';' => {
                    out[i + 1..].fill(b'x');
                    break;
                }
                b'$' if bytes.get(i + 1) == Some(&b'\'') => {
                    stack.push(Ctx::AnsiC);
                    i += 1;
                }
                b'\'' => stack.push(Ctx::Single),
                b'"' => stack.push(Ctx::Double),
                // `$(` and the process substitutions `<(` and `>(`
                b'$' | b'<' | b'>' if bytes.get(i + 1) == Some(&b'(') => {
                    stack.push(Ctx::Subst);
                    i += 1;
                }
                b')' if top == Ctx::Subst => {
                    stack.pop();
                }
                b'`' if top == Ctx::Backtick => {
                    stack.pop();
                }
                b'`' => stack.push(Ctx::Backtick),
                _ => {}
            },
        }
        i += 1;
    }
    // A substitution does not span lines in the macros this reads; a quote
    // does. Drop any unclosed substitution so only quotes carry over.
    while matches!(stack.last(), Some(Ctx::Subst | Ctx::Backtick)) {
        stack.pop();
    }
    // Every byte of a multi-byte character shares one context, so each is
    // replaced or none is, and the result stays valid UTF-8.
    String::from_utf8_lossy(&out).into_owned()
}

/// Where the first here-doc operator `<<` in `line` starts, skipping each
/// `<<<` here-string, which carries its text on the same line.
fn heredoc_start(line: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(i) = line[from..].find("<<").map(|i| from + i) {
        let run = line[i..].bytes().take_while(|&b| b == b'<').count();
        if run == 2 {
            return Some(i);
        }
        from = i + run;
    }
    None
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

/// Commands that run the next word as a command: `timeout 2 gh`, `env X=1
/// jq`, `nice -n 5 make`, `nohup cmd`, `command cmd` (not `command -v cmd`,
/// which only looks it up).
const WRAPPERS: &[&str] = &["timeout", "env", "nice", "nohup", "command", "exec"];

/// Simple shell tokenizer — extracts tokens in "command position".
/// Command position: first word of line, after |, &&, ||, ;, `{`, `then`,
/// `do`, `else`, after a wrapper (see [`WRAPPERS`]), or inside $() or
/// backticks.
/// Also handles VAR=$(cmd ...) and trap 'cmd ...' patterns.
fn shell_tokens(line: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut expect_cmd = true; // first word is a command
    // The wrapper whose command comes next. Its options, assignments and a
    // duration or count come first.
    let mut wrapper: Option<&str> = None;

    for word in line.split_whitespace() {
        if let Some(wr) = wrapper {
            if wr == "command" && matches!(word, "-v" | "-V") {
                // `command -v x` looks `x` up; nothing runs.
                wrapper = None;
                expect_cmd = false;
                continue;
            }
            let is_arg = word.starts_with('-')
                || word.contains('=')
                || word.trim_end_matches(['s', 'm', 'h', 'd']).parse::<f64>().is_ok();
            if is_arg {
                continue;
            }
            wrapper = None;
        }

        // Handle VAR=$(cmd or "text `cmd — a substitution that opens
        // inside the word
        let opens = [
            word.find("$(").map(|p| p + 2),
            word.find("<(").map(|p| p + 2),
            word.find(">(").map(|p| p + 2),
            word.char_indices().skip(1).find(|&(_, c)| c == '`').map(|(p, _)| p + 1),
        ];
        let subshell_word = opens.into_iter().flatten().min().and_then(|pos| {
            let after = &word[pos..];
            // A trailing $( or ` — the next word is the command
            (!after.is_empty()).then_some(after)
        });

        // If we extracted a command from inside $(), process it
        if let Some(sw) = subshell_word {
            let clean = sw
                .trim_start_matches('\'')
                .trim_start_matches('"')
                .trim_end_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != '-');
            if !clean.is_empty() && !clean.starts_with('-') {
                tokens.push(clean);
                if WRAPPERS.contains(&clean) {
                    wrapper = Some(clean);
                    expect_cmd = true;
                    continue;
                }
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
            if WRAPPERS.contains(&clean) {
                wrapper = Some(clean);
                continue;
            }
            expect_cmd = false;
        }

        // These signal the next word is a command
        if word.ends_with('|') || word == "&&" || word == "||" || word.ends_with(';')
            || word == "|" || word.ends_with("$(") || word.ends_with('`')
            || matches!(word, "{" | "then" | "do" | "else")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(name: &str, src: &str) -> Vec<String> {
        let dir = std::env::temp_dir().join(format!("ways-requires-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("macro.sh");
        std::fs::write(&path, src).unwrap();
        let got = scan_macro_requires(&path);
        let _ = std::fs::remove_dir_all(&dir);
        got
    }

    #[test]
    fn a_here_string_is_not_a_heredoc() {
        let got = scan("herestring", "f() { grep -o x <<<\"$1\" | head -1; }\nX=$(attend status)\necho \"$X\" | sed 1d\n");
        assert_eq!(got, ["Read", "Bash(attend:*)", "Bash(grep:*)", "Bash(head:*)", "Bash(sed:*)"]);
    }

    #[test]
    fn a_heredoc_body_is_skipped() {
        let got = scan("heredoc", "cat <<'EOF'\ngit status\nEOF\nwc -l x\n");
        assert_eq!(got, ["Read", "Bash(cat:*)", "Bash(wc:*)"]);
    }

    #[test]
    fn quoted_text_names_no_command_but_a_substitution_in_it_does() {
        // The adr macro's decline hint (adr/macro.sh) once gave a false `touch`.
        let got = scan("quoted", "echo \"To decline: \\`mkdir -p .claude && touch .claude/no-adr\\`\"\nX=\"$(attend status | grep x)\"\nawk '{ print $1 } && sort' f | wc -l\necho 'then git'\n");
        assert_eq!(got, ["Bash(attend:*)", "Bash(awk:*)", "Bash(grep:*)", "Bash(wc:*)"]);
        assert_eq!(mask_quoted("a \"b c\" 'd' e"), "a \"xxx\" 'x' e");
        assert_eq!(mask_quoted("cat <<'EOF'").len(), "cat <<'EOF'".len());
    }

    #[test]
    fn a_quote_opened_on_an_earlier_line_carries_over() {
        // The makefile way's macro: an awk program spans lines, and the
        // line that closes it pipes on to tr and sort.
        let got = scan(
            "multiline",
            "TARGETS=$(awk '\n  /^[a-z]+:/ { print $1; then git }\n' \"$MAKEFILE\" 2>/dev/null | tr ' ' '\\n' | sort -u | tr '\\n' ' ')\n",
        );
        assert_eq!(got, ["Bash(awk:*)", "Bash(sort:*)", "Bash(tr:*)"]);
        // A comment's apostrophe opens no quote.
        let got = scan("comment", "# don't stop here\ngrep x f # and then git\nwc -l f\n");
        assert_eq!(got, ["Bash(grep:*)", "Bash(wc:*)"]);
    }

    #[test]
    fn a_backtick_inside_double_quotes_runs() {
        let got = scan("backtick", "echo \"tick: `uname -r`\"\n");
        assert_eq!(got, ["Bash(uname:*)"]);
    }

    #[test]
    fn a_wrapper_runs_the_next_command() {
        let got = scan(
            "wrappers",
            "timeout 2 gh api user &\nX=$(timeout 1.5 jq . f)\nenv A=1 B=2 git status\nnice -n 5 make\ncommand -v sed >/dev/null || exit 0\ndone < <(file -b x)\n",
        );
        assert_eq!(got, ["Read", "Bash(file:*)", "Bash(gh:*)", "Bash(git:*)", "Bash(jq:*)", "Bash(make:*)"]);
    }

    #[test]
    fn a_heredoc_after_a_here_string_on_one_line_is_found() {
        assert_eq!(heredoc_start("x <<<y; cat <<EOF"), Some(12));
        assert_eq!(heredoc_start("x <<<\"$1\""), None);
    }
}
