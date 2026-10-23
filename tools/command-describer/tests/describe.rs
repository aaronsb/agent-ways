//! Descriptions and the ordered described block, against the shipped tldr
//! extract (`catalog/commands/tldr/summaries.tsv`, tldr-pages v2.3). The
//! expected summaries are that release's text; a new pin may change them.

use std::path::Path;
use std::sync::OnceLock;

use command_describer::{describe, described_block, parse, Summaries};

fn data() -> &'static Summaries {
    static DATA: OnceLock<Summaries> = OnceLock::new();
    DATA.get_or_init(|| {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/commands/tldr/summaries.tsv");
        Summaries::load(&path).expect("the shipped tldr extract")
    })
}

fn block(description: &str, command: &str) -> String {
    described_block(description, &parse(command), data())
}

#[test]
fn the_extract_loads() {
    assert!(data().len() > 5000, "{} pages", data().len());
}

#[test]
fn program_summaries() {
    let d = data();
    assert_eq!(d.lookup("ls", None), Some("List directory contents"));
    assert_eq!(
        d.lookup("grep", None),
        Some("Find patterns in files using regular expressions")
    );
    assert_eq!(
        d.lookup("make", None),
        Some("Task runner for targets described in Makefile")
    );
    assert_eq!(
        d.lookup("git", None),
        Some("Distributed version control system")
    );
}

#[test]
fn subcommand_summaries() {
    let d = data();
    assert_eq!(
        d.lookup("git", Some("push")),
        Some("Push commits to a remote repository")
    );
    assert_eq!(
        d.lookup("gh", Some("pr create")),
        Some("Manage GitHub pull requests")
    );
    assert_eq!(
        d.lookup("docker", Some("run")),
        Some("Run a command in a new Docker container")
    );
    assert_eq!(
        d.lookup("cargo", Some("test")),
        Some("Execute the unit and integration tests of a Rust package")
    );
    assert_eq!(d.lookup("npm", Some("run")), Some("Run a script"));
}

#[test]
fn a_missing_subcommand_page_falls_back_to_the_program() {
    // No make-test page: the make page answers.
    assert_eq!(
        data().lookup("make", Some("test")),
        Some("Task runner for targets described in Makefile")
    );
    // No git-stash-pop page: git-stash answers.
    assert_eq!(
        data().lookup("git", Some("stash pop")),
        Some("Stash local Git changes in a temporary area")
    );
}

#[test]
fn an_alias_page_takes_its_targets_summary() {
    // aria2 is "an alias of `aria2c`" in tldr.
    assert_eq!(data().lookup("aria2", None), data().lookup("aria2c", None));
    assert!(data().lookup("aria2", None).is_some());
}

#[test]
fn an_unknown_program_gives_none() {
    assert_eq!(data().lookup("no-such-program-xyz", None), None);
    assert_eq!(data().lookup("no-such-program-xyz", Some("push")), None);
}

#[test]
fn describe_labels_every_segment_and_summarizes_operatives_and_filters() {
    let segs = parse("cd tools && myprog --x | grep y");
    let got: Vec<(String, Option<&str>)> = describe(&segs, data())
        .into_iter()
        .map(|d| (d.label, d.summary))
        .collect();
    assert_eq!(
        got,
        vec![
            ("cd".to_string(), None),
            ("myprog".to_string(), None),
            (
                "grep".to_string(),
                Some("Find patterns in files using regular expressions")
            ),
        ]
    );
}

#[test]
fn block_keeps_a_pipeline_on_one_line() {
    assert_eq!(
        block("Find fix commits", "git log --oneline | grep fix | head"),
        "About to run: Find fix commits\n\
         git log (Show a history of commits) | grep (Find patterns in files using regular expressions) | head (Output the first part of files)"
    );
}

#[test]
fn block_splits_a_chain_and_drops_navigation() {
    let cmd = "cd tools && git add -A && git commit -m 'x' && git push -u origin HEAD && gh pr create --title t";
    assert_eq!(
        block("Commit, push and open a PR", cmd),
        "About to run: Commit, push and open a PR\n\
         git add (Adds changed files to the index)\n\
         git commit (Commit files to the repository)\n\
         git push (Push commits to a remote repository)\n\
         gh pr create (Manage GitHub pull requests)"
    );
}

#[test]
fn block_drops_a_heredoc_body_and_gives_the_substitution_its_own_line() {
    let cmd = "gh pr create --title t --body \"$(cat <<'EOF'\nbody | with && symbols\nEOF\n)\"";
    assert_eq!(
        block("Open a PR", cmd),
        "About to run: Open a PR\n\
         gh pr create (Manage GitHub pull requests)\n\
         cat (Print and concatenate files)"
    );
}

#[test]
fn block_gives_a_loop_body_its_own_line() {
    assert_eq!(
        block(
            "Count lines",
            "for f in *.rs; do wc -l \"$f\"; done && echo done"
        ),
        "About to run: Count lines\nwc (Count lines, words, and bytes)"
    );
    assert_eq!(
        block("Show titles", "while read l; do gh issue view $l | jq .title; done < ids.txt"),
        "About to run: Show titles\n\
         gh issue view (Manage GitHub issues) | jq (A JSON processor that uses a domain-specific language (DSL))"
    );
}

#[test]
fn block_splits_at_semicolon_newline_and_subshell() {
    assert_eq!(
        block(
            "Test then look",
            "(cd tools && make test); ls -la\ngit status"
        ),
        "About to run: Test then look\n\
         make test (Task runner for targets described in Makefile)\n\
         ls (List directory contents)\n\
         git status (Show the changes to files in a Git repository)"
    );
}

#[test]
fn block_keeps_or_inline_and_drops_its_navigation() {
    assert_eq!(
        block("Test", "cargo test || cargo build"),
        "About to run: Test\n\
         cargo test (Execute the unit and integration tests of a Rust package) || cargo build (Compile a local package and all of its dependencies)"
    );
    assert_eq!(
        block("Test", "cargo test || echo failed"),
        "About to run: Test\ncargo test (Execute the unit and integration tests of a Rust package)"
    );
}

#[test]
fn block_shows_an_unknown_program_by_name_alone() {
    assert_eq!(
        block("  Run   the\ntool ", "myprog --flag && git status"),
        "About to run: Run the tool\n\
         myprog\n\
         git status (Show the changes to files in a Git repository)"
    );
}

#[test]
fn block_with_only_navigation_is_the_head_line() {
    assert_eq!(block("Move", "cd /tmp && export A=1"), "About to run: Move");
    assert_eq!(block("", ""), "About to run: ");
}

#[test]
fn block_shows_the_commands_of_a_command_string() {
    assert_eq!(
        block("Force push", "eval \"git push --force\""),
        "About to run: Force push\ngit push (Push commits to a remote repository)"
    );
    let shell = block("Push", "bash -c 'git push origin main'");
    let lines: Vec<&str> = shell.lines().collect();
    assert_eq!(lines.len(), 3, "{shell}");
    assert!(lines[1].starts_with("bash"), "{shell}");
    assert_eq!(lines[2], "git push (Push commits to a remote repository)");
}

#[test]
fn block_joins_a_pipe_to_the_segment_at_its_own_depth() {
    let grep = "grep (Find patterns in files using regular expressions)";
    // The pipe reads the shell's output, not the last command in its string.
    let shell = block("Search", "bash -c 'git status; git log' | grep x");
    let lines: Vec<&str> = shell.lines().collect();
    assert_eq!(lines.len(), 4, "{shell}");
    assert!(lines[1].starts_with("bash"), "{shell}");
    assert!(lines[1].ends_with(&format!(" | {grep}")), "{shell}");
    assert!(lines[2].starts_with("git status"), "{shell}");
    assert!(lines[3].starts_with("git log"), "{shell}");
    // The same for a substitution.
    assert_eq!(
        block("Search", "cat $(ls) | grep x"),
        format!(
            "About to run: Search\n\
             cat (Print and concatenate files) | {grep}\n\
             ls (List directory contents)"
        )
    );
}

#[test]
fn block_joins_a_pipe_after_a_subshell_to_its_last_line() {
    let log = "git log (Show a history of commits)";
    let grep = "grep (Find patterns in files using regular expressions)";
    let push = "git push (Push commits to a remote repository)";
    let make = "make (Task runner for targets described in Makefile)";
    assert_eq!(
        block("Search", "(cd x && git log) | grep y"),
        format!("About to run: Search\n{log} | {grep}")
    );
    let shell = block("Push", "bash -c 'make'; (cd x && git push) | grep y");
    let lines: Vec<&str> = shell.lines().collect();
    assert_eq!(lines.len(), 4, "{shell}");
    assert!(lines[1].starts_with("bash"), "{shell}");
    assert_eq!(lines[2..], [make.to_string(), format!("{push} | {grep}")]);
}

#[test]
fn block_keeps_plain_pipelines_as_they_were() {
    let log = "git log (Show a history of commits)";
    let grep = "grep (Find patterns in files using regular expressions)";
    let head = "head (Output the first part of files)";
    let make = "make (Task runner for targets described in Makefile)";
    let ls = "ls (List directory contents)";
    for (cmd, lines) in [
        (
            "git log | grep x | head",
            vec![format!("{log} | {grep} | {head}")],
        ),
        ("make || ls", vec![format!("{make} || {ls}")]),
        (
            "make && git log | grep x",
            vec![make.to_string(), format!("{log} | {grep}")],
        ),
        (
            "git log | grep x && make || ls",
            vec![format!("{log} | {grep}"), format!("{make} || {ls}")],
        ),
        (
            "{ git log; ls; } | grep x",
            vec![log.to_string(), format!("{ls} | {grep}")],
        ),
        (
            "for f in x; do git log; done | grep x",
            vec![format!("{log} | {grep}")],
        ),
    ] {
        let want = std::iter::once("About to run: Run".to_string())
            .chain(lines)
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(block("Run", cmd), want, "{cmd}");
    }
}
