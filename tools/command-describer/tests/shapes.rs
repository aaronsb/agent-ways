//! Real command shapes from Bash calls, each checked on its segments.

use command_describer::{parse, Class, Connector, Role, Segment};

/// `(connector, role, program, subcommand, class)` per segment.
type Row<'a> = (
    Connector,
    Role,
    Option<&'a str>,
    Option<&'a str>,
    Option<Class>,
);

fn rows(segs: &[Segment]) -> Vec<Row<'_>> {
    segs.iter()
        .map(|s| {
            (
                s.connector,
                s.role,
                s.program.as_deref(),
                s.subcommand.as_deref(),
                s.class,
            )
        })
        .collect()
}

/// Each operative segment's text, in order.
fn operative_texts(segs: &[Segment]) -> Vec<&str> {
    segs.iter()
        .filter(|s| s.is_operative())
        .map(|s| s.text.as_str())
        .collect()
}

fn programs(cmd: &str) -> Vec<String> {
    parse(cmd).into_iter().filter_map(|s| s.program).collect()
}

use Connector as C;
use Role as R;

#[test]
fn cd_then_make_test() {
    let segs = parse("cd tools && make test");
    assert_eq!(
        rows(&segs),
        vec![
            (C::Start, R::Navigation, Some("cd"), None, None),
            (
                C::And,
                R::Operative,
                Some("make"),
                Some("test"),
                Some(Class::BuildTest)
            ),
        ]
    );
    assert_eq!(operative_texts(&segs), ["make test"]);
}

#[test]
fn env_assignment_resolves_to_the_program() {
    let segs = parse("FOO=1 BAR='a b' cargo test -p ways-core");
    assert_eq!(
        rows(&segs),
        vec![(
            C::Start,
            R::Operative,
            Some("cargo"),
            Some("test"),
            Some(Class::BuildTest)
        )]
    );
    assert_eq!(segs[0].flags, ["-p"]);
    // The text keeps the prefix: it is the segment as written.
    assert_eq!(segs[0].text, "FOO=1 BAR='a b' cargo test -p ways-core");
}

#[test]
fn assignment_alone_is_navigation() {
    let segs = parse("X=1; export Y=2");
    assert_eq!(
        rows(&segs),
        vec![
            (C::Start, R::Navigation, None, None, None),
            (C::Semicolon, R::Navigation, Some("export"), None, None),
        ]
    );
}

#[test]
fn chained_git_and_gh_with_a_heredoc_commit_message() {
    let cmd = "git add -A && git commit -m \"$(cat <<'EOF'\nfeat: gh pr create && rm -rf /\nEOF\n)\" && git push -u origin feat/x && gh pr create --title \"x; y\" --body \"a && b\"";
    let segs = parse(cmd);
    assert_eq!(
        rows(&segs),
        vec![
            (
                C::Start,
                R::Operative,
                Some("git"),
                Some("add"),
                Some(Class::VcsWrite)
            ),
            (
                C::And,
                R::Operative,
                Some("git"),
                Some("commit"),
                Some(Class::VcsWrite)
            ),
            (
                C::CommandSubstitution,
                R::Operative,
                Some("cat"),
                None,
                Some(Class::ReadInspect)
            ),
            (
                C::And,
                R::Operative,
                Some("git"),
                Some("push"),
                Some(Class::Publish)
            ),
            (
                C::And,
                R::Operative,
                Some("gh"),
                Some("pr create"),
                Some(Class::Publish)
            ),
        ]
    );
    assert_eq!(segs[2].depth, 1);
    // The heredoc body is dropped: its `gh pr create` and `rm -rf /` are
    // neither segments nor in any segment's text.
    for s in &segs {
        assert!(!s.text.contains("rm -rf"), "{:?}", s.text);
        assert!(!s.text.contains("feat:"), "{:?}", s.text);
    }
    assert_eq!(
        segs[4].text,
        "gh pr create --title \"x; y\" --body \"a && b\""
    );
    assert_eq!(segs[4].flags, ["--title", "--body"]);
}

#[test]
fn heredoc_delimiter_closing_the_substitution_on_its_line() {
    let segs = parse("git commit -m \"$(cat <<'EOF'\nmsg; make deploy\nEOF)\" && git push");
    assert_eq!(
        programs("git commit -m \"$(cat <<'EOF'\nmsg; make deploy\nEOF)\" && git push"),
        ["git", "cat", "git"]
    );
    assert_eq!(segs[2].connector, C::And);
    assert_eq!(segs[2].subcommand.as_deref(), Some("push"));
}

#[test]
fn pipes_make_filters_after_the_first_command() {
    let segs = parse("git log --oneline | grep fix | head -5");
    assert_eq!(
        rows(&segs),
        vec![
            (
                C::Start,
                R::Operative,
                Some("git"),
                Some("log"),
                Some(Class::VcsRead)
            ),
            (C::Pipe, R::Filter, Some("grep"), None, None),
            (C::Pipe, R::Filter, Some("head"), None, None),
        ]
    );
    // A filter program at the start of the call is operative.
    let segs = parse("cat Cargo.toml | wc -l");
    assert_eq!(segs[0].role, R::Operative);
    assert_eq!(segs[0].class, Some(Class::ReadInspect));
    assert_eq!(segs[1].role, R::Filter);
}

#[test]
fn sed_in_place_after_a_pipe_is_not_a_filter() {
    let segs = parse("cat f | sed -i s/a/b/ g");
    assert_eq!(segs[1].role, R::Operative);
    assert_eq!(segs[1].class, Some(Class::FileWrite));
}

#[test]
fn pipe_with_stderr_and_a_non_filter_consumer() {
    let segs = parse("cargo test |& tee log");
    assert_eq!(segs[1].connector, C::Pipe);
    assert_eq!(
        (segs[1].role, segs[1].class),
        (R::Operative, Some(Class::FileWrite))
    );
}

#[test]
fn subshell_segments_are_nested() {
    let segs = parse("(cd tools && cargo build --release) && echo ok");
    assert_eq!(
        rows(&segs),
        vec![
            (C::Subshell, R::Navigation, Some("cd"), None, None),
            (
                C::And,
                R::Operative,
                Some("cargo"),
                Some("build"),
                Some(Class::BuildTest)
            ),
            (C::And, R::Navigation, Some("echo"), None, None),
        ]
    );
    assert_eq!(segs.iter().map(|s| s.depth).collect::<Vec<_>>(), [1, 1, 0]);
}

#[test]
fn for_loop_body_on_one_line() {
    let segs = parse("for f in *.rs; do rustfmt \"$f\"; cargo check; done");
    assert_eq!(
        rows(&segs),
        vec![
            (
                C::LoopBody,
                R::Operative,
                Some("rustfmt"),
                None,
                Some(Class::Other)
            ),
            (
                C::Semicolon,
                R::Operative,
                Some("cargo"),
                Some("check"),
                Some(Class::BuildTest)
            ),
        ]
    );
}

#[test]
fn for_loop_body_across_lines() {
    let segs = parse("for f in a b\ndo\n  echo $f\n  sed -i 's/x/y/' $f\ndone");
    assert_eq!(
        rows(&segs),
        vec![
            (C::LoopBody, R::Navigation, Some("echo"), None, None),
            (
                C::Newline,
                R::Operative,
                Some("sed"),
                None,
                Some(Class::FileWrite)
            ),
        ]
    );
}

#[test]
fn for_loop_over_a_substitution_keeps_the_substitution() {
    let segs = parse("for b in $(git branch --merged); do git branch -d $b; done");
    assert_eq!(
        rows(&segs),
        vec![
            (
                C::CommandSubstitution,
                R::Operative,
                Some("git"),
                Some("branch"),
                Some(Class::VcsRead)
            ),
            (
                C::LoopBody,
                R::Operative,
                Some("git"),
                Some("branch"),
                Some(Class::VcsWrite)
            ),
        ]
    );
}

#[test]
fn while_read_loop_with_input_redirect() {
    let segs =
        parse("while read -r line; do\n  gh issue comment \"$line\" --body x\ndone < ids.txt");
    assert_eq!(
        rows(&segs),
        vec![
            (C::Start, R::Navigation, Some("read"), None, None),
            (
                C::LoopBody,
                R::Operative,
                Some("gh"),
                Some("issue comment"),
                Some(Class::Publish)
            ),
        ]
    );
}

#[test]
fn if_with_double_brackets() {
    let segs = parse("if [[ -f x && -d y ]]; then make build; else echo no; fi");
    assert_eq!(
        rows(&segs),
        vec![
            (C::Start, R::Navigation, None, None, None),
            (
                C::Semicolon,
                R::Operative,
                Some("make"),
                Some("build"),
                Some(Class::BuildTest)
            ),
            (C::Semicolon, R::Navigation, Some("echo"), None, None),
        ]
    );
    assert_eq!(segs[0].text, "[[ -f x && -d y ]]");
}

#[test]
fn case_arms_are_segments_and_patterns_are_not() {
    let segs = parse("case \"$1\" in start) systemctl start x;; stop) systemctl stop x;; esac");
    assert_eq!(
        programs("case \"$1\" in start) systemctl start x;; stop) systemctl stop x;; esac"),
        ["systemctl", "systemctl"]
    );
    assert_eq!(segs[0].subcommand.as_deref(), Some("start"));
    assert_eq!(segs[1].subcommand.as_deref(), Some("stop"));
    assert!(segs.iter().all(|s| s.class == Some(Class::Service)));
}

#[test]
fn case_across_lines_with_alternatives() {
    let cmd = "case \"$x\" in\n  start|restart) make run ;;\n  (stop) pkill -f server ;;\n  *) echo usage ;;\nesac";
    assert_eq!(programs(cmd), ["make", "pkill", "echo"]);
    // Outside a case, a `|` still pipes and a stray `)` keeps the segment.
    assert_eq!(programs("ls )"), ["ls"]);
}

#[test]
fn function_bodies_are_segments() {
    assert_eq!(programs("f() { cargo test; }"), ["cargo"]);
    assert_eq!(programs("function g { make lint; }"), ["make"]);
}

#[test]
fn quoted_separators_do_not_split() {
    let segs = parse("echo \"a;b|c && d\" && grep -E 'x|y' file.txt > out.txt");
    assert_eq!(segs.len(), 2);
    assert_eq!(segs[0].text, "echo \"a;b|c && d\"");
    // A redirect to a file makes a search a write.
    assert_eq!(
        (segs[1].program.as_deref(), segs[1].class),
        (Some("grep"), Some(Class::FileWrite))
    );
    let segs = parse("awk '{print $1; print $2}' f | sort");
    assert_eq!(segs.len(), 2);
    let segs = parse(r"echo a\;b; ls");
    assert_eq!(segs.len(), 2);
}

#[test]
fn redirect_to_dev_null_and_fd_duplication_write_nothing() {
    let segs = parse("ls 2>/dev/null; cat x 2>&1; cat y >/dev/null 2>&1");
    assert!(
        segs.iter().all(|s| s.class == Some(Class::ReadInspect)),
        "{:?}",
        rows(&segs)
    );
    let segs = parse("cat a b > joined.txt");
    assert_eq!(segs[0].class, Some(Class::FileWrite));
}

#[test]
fn redirects_to_streams_write_nothing() {
    for cmd in [
        "echo oops > /dev/stderr",
        "echo hi >/dev/stdout",
        "printf x > /dev/tty",
        "echo note 1>/dev/fd/2",
    ] {
        let segs = parse(cmd);
        assert_eq!(segs[0].role, R::Navigation, "{cmd}");
        assert_eq!(segs[0].class, None, "{cmd}");
    }
    assert_eq!(
        parse("cat x > /dev/stderr")[0].class,
        Some(Class::ReadInspect)
    );
    // A file under /dev that is not a stream still counts.
    assert_eq!(
        parse("echo x > /dev/fdisk.log")[0].class,
        Some(Class::FileWrite)
    );
}

#[test]
fn heredoc_to_a_file_then_another_command() {
    let cmd = "cat > /tmp/f.txt <<EOF\nline one; rm -rf /\nEOF\ncat /tmp/f.txt";
    let segs = parse(cmd);
    assert_eq!(
        rows(&segs),
        vec![
            (
                C::Start,
                R::Operative,
                Some("cat"),
                None,
                Some(Class::FileWrite)
            ),
            (
                C::Newline,
                R::Operative,
                Some("cat"),
                None,
                Some(Class::ReadInspect)
            ),
        ]
    );
    assert_eq!(segs[0].text, "cat > /tmp/f.txt <<EOF");
}

#[test]
fn heredoc_variants() {
    // `<<-` strips leading tabs from the delimiter line.
    assert_eq!(
        programs("cat <<-EOF | grep x\n\tbody; rm y\n\tEOF\necho done"),
        ["cat", "grep", "echo"]
    );
    // Two heredocs on one line: both bodies dropped, in order.
    assert_eq!(
        programs("cat <<A <<\"B\"\na; rm a\nA\nb; rm b\nB\nrm x"),
        ["cat", "rm"]
    );
    // A here-string is not a heredoc.
    assert_eq!(programs("grep x <<< \"$s\"\nls"), ["grep", "ls"]);
    // An unterminated heredoc runs to the end.
    assert_eq!(programs("cat <<EOF\nrm -rf /\n"), ["cat"]);
}

#[test]
fn wrappers_resolve_to_the_real_program() {
    let segs = parse("sudo -u root systemctl restart nginx; timeout 30 cargo test 2>&1 | tail -20");
    assert_eq!(
        rows(&segs),
        vec![
            (
                C::Start,
                R::Operative,
                Some("systemctl"),
                Some("restart"),
                Some(Class::Service)
            ),
            (
                C::Semicolon,
                R::Operative,
                Some("cargo"),
                Some("test"),
                Some(Class::BuildTest)
            ),
            (C::Pipe, R::Filter, Some("tail"), None, None),
        ]
    );
    assert_eq!(programs("timeout -s KILL 10s make check"), ["make"]);
    assert_eq!(
        programs("env RUST_LOG=debug cargo run --bin ways -- scan"),
        ["cargo"]
    );
    assert_eq!(
        programs("nohup nice -n 5 ./server --port 8080 &"),
        ["server"]
    );
    assert_eq!(programs("ls | xargs -I {} rm {}"), ["ls", "rm"]);
    assert_eq!(programs("time cargo build"), ["cargo"]);
    assert_eq!(programs("/usr/bin/git status"), ["git"]);
}

#[test]
fn background_connector() {
    let segs = parse("./server & sleep 1; curl localhost:8080");
    assert_eq!(
        segs.iter().map(|s| s.connector).collect::<Vec<_>>(),
        [C::Start, C::Background, C::Semicolon]
    );
}

#[test]
fn command_and_process_substitutions_are_segments() {
    let segs = parse("VERSION=`git describe --tags` && echo $VERSION");
    assert_eq!(
        rows(&segs),
        vec![
            (C::Start, R::Navigation, None, None, None),
            (
                C::CommandSubstitution,
                R::Operative,
                Some("git"),
                Some("describe"),
                Some(Class::VcsRead)
            ),
            (C::And, R::Navigation, Some("echo"), None, None),
        ]
    );
    let segs = parse("diff <(sort a) <(sort b)");
    assert_eq!(
        programs("diff <(sort a) <(sort b)"),
        ["diff", "sort", "sort"]
    );
    assert_eq!(segs[1].connector, C::CommandSubstitution);
    // A substitution inside double quotes is parsed too.
    assert_eq!(
        programs("echo \"head: $(git rev-parse HEAD)\""),
        ["echo", "git"]
    );
    // Arithmetic and parameter expansion are not commands.
    assert_eq!(programs("echo $((1+2)) ${HOME:-/x} $'a\\'b'"), ["echo"]);
}

#[test]
fn comments_are_dropped() {
    assert_eq!(
        programs("python3 -m pytest -q tests/ # run tests && make deploy"),
        ["python3"]
    );
    assert_eq!(programs("# a note\nmake lint"), ["make"]);
    assert_eq!(programs("git log --format='%h #%s' -5"), ["git"]);
    assert_eq!(programs("echo a#b"), ["echo"]);
    assert!(parse("   # only a comment").is_empty());
}

#[test]
fn newlines_after_an_operator_continue_the_list() {
    let segs = parse("cargo +nightly fmt --all &&\n  git status");
    assert_eq!(segs[1].connector, C::And);
    assert_eq!(segs[0].subcommand.as_deref(), Some("fmt"));
    let segs = parse("cargo test \\\n  --workspace");
    assert_eq!(segs.len(), 1);
    assert_eq!(segs[0].flags, ["--workspace"]);
    let segs = parse("make\nmake test");
    assert_eq!(segs[1].connector, C::Newline);
}

#[test]
fn per_segment_text_lets_an_anchored_regex_match_after_a_connector() {
    // ADR-188 item 3a: `^gh` misses on the raw string, matches the segment.
    let cmd = "git push && gh pr create --fill";
    let segs = parse(cmd);
    let texts = operative_texts(&segs);
    assert_eq!(texts, ["git push", "gh pr create --fill"]);
    assert!(!cmd.starts_with("gh"));
    assert!(texts[1].starts_with("gh "));
}

#[test]
fn program_text_starts_at_the_program_word() {
    let text = |cmd: &str| -> Vec<String> {
        parse(cmd)
            .iter()
            .map(|s| s.program_text().to_string())
            .collect()
    };
    assert_eq!(
        text("GH_TOKEN=x gh pr create --fill"),
        ["gh pr create --fill"]
    );
    assert_eq!(
        text("sudo -u root pacman -S foo && timeout 30s gh pr checks 3"),
        ["pacman -S foo", "gh pr checks 3"]
    );
    assert_eq!(text("git status"), ["git status"]);
    // No program: the whole text.
    assert_eq!(text("FOO=1"), ["FOO=1"]);
    // A heredoc body cut before the program word keeps the offset in `text`.
    let segs = parse("cat <<EOF $(\nbody\nEOF\nA=1 git push)");
    let push = segs
        .iter()
        .find(|s| s.program.as_deref() == Some("git"))
        .unwrap();
    assert_eq!(push.program_text(), "git push");
}

#[test]
fn a_navigation_program_behind_a_wrapper_is_operative() {
    let segs = parse("sudo -n true && yay -S --noconfirm foo");
    assert_eq!(segs[0].program.as_deref(), Some("true"));
    assert_eq!(segs[0].role, R::Operative);
    assert_eq!(operative_texts(&segs)[0], "sudo -n true");
    // An assignment is not a wrapper.
    assert_eq!(parse("FOO=1 echo hi")[0].role, R::Navigation);
}

#[test]
fn time_and_builtin_leave_a_navigation_program_navigation() {
    for cmd in [
        "time echo hi",
        "builtin cd /tmp",
        "/usr/bin/time -p echo hi",
    ] {
        let segs = parse(cmd);
        assert_eq!(segs[0].role, R::Navigation, "{cmd}");
    }
    assert_eq!(parse("builtin cd /tmp")[0].program.as_deref(), Some("cd"));
    assert_eq!(parse("sudo -n true")[0].role, R::Operative);
    // They still resolve to an operative program.
    assert_eq!(parse("time cargo build")[0].role, R::Operative);
}

#[test]
fn a_quoted_closing_bracket_does_not_close_a_test() {
    let segs = parse(r#"[[ $x == "]]" && -f y ]] && make test"#);
    assert_eq!(
        segs.iter()
            .map(|s| s.program.as_deref())
            .collect::<Vec<_>>(),
        [None, Some("make")]
    );
    assert_eq!(segs[1].connector, C::And);
    // An unquoted `]]` still closes it.
    assert_eq!(programs("[[ -f y ]] && make test"), ["make"]);
}

#[test]
fn a_subshell_after_a_leading_keyword_is_parsed() {
    assert_eq!(programs("time (python3 -c pass)"), ["python3"]);
    assert_eq!(programs("! (git push) && echo no"), ["git", "echo"]);
    assert_eq!(
        programs("if (make test); then echo ok; fi"),
        ["make", "echo"]
    );
}

#[test]
fn loop_control_is_navigation() {
    let segs = parse("for f in a b; do [ -f $f ] && continue; break; done");
    assert!(segs.iter().all(|s| s.role == R::Navigation), "{segs:?}");
}

#[test]
fn operative_texts_skip_navigation_and_filters() {
    let segs = parse("cd x && git log | grep y | head; echo done");
    assert_eq!(operative_texts(&segs), ["git log"]);
}

#[test]
fn spans_slice_the_original_command() {
    let cmd = "FOO=1 cargo test && (cd a; make) | tee log";
    for s in parse(cmd) {
        assert_eq!(&cmd[s.span.clone()], s.text, "{s:?}");
    }
}

#[test]
fn verb_drops_arguments() {
    let segs = parse("gh pr create --title \"x y\" --body-file /tmp/b.md");
    assert_eq!(segs[0].verb(), "gh pr create --title --body-file");
    let segs = parse("grep -rn needle src/");
    assert_eq!(segs[0].verb(), "grep -rn");
}

#[test]
fn malformed_input_never_fails() {
    assert!(parse("").is_empty());
    assert!(parse("   \n\t ").is_empty());
    assert_eq!(programs("echo \"unterminated && rm x"), ["echo"]);
    assert_eq!(programs("echo 'unterminated && rm x"), ["echo"]);
    assert_eq!(programs("(cd x && make"), ["cd", "make"]);
    assert_eq!(programs("echo $(git status"), ["echo", "git"]);
    assert_eq!(programs("ls )"), ["ls"]);
    assert_eq!(programs("&& ; | ls"), ["ls"]);
    let _ = parse("\\");
    let _ = parse("$");
    let _ = parse("<<");
    let _ = parse("cat <<");
    let _ = parse("$(");
    let _ = parse("`");
    let _ = parse("[[ (");
}

#[test]
fn deep_nesting_is_bounded() {
    let deep = format!("{}ls{}", "( ".repeat(200), " )".repeat(200));
    let segs = parse(&deep);
    assert!(segs.len() <= 1);
    let deep = format!("{}ls{}", "$(echo ".repeat(200), ")".repeat(200));
    let _ = parse(&deep);
    let shallow = format!("{}ls{}", "( ".repeat(10), " )".repeat(10));
    let segs = parse(&shallow);
    assert_eq!(segs.len(), 1);
    assert_eq!(segs[0].depth, 10);
}

#[test]
fn non_ascii_text_is_kept_whole() {
    let segs = parse("echo \"héllo — ✓\" && git commit -m 'fix: ü'");
    assert_eq!(segs[1].text, "git commit -m 'fix: ü'");
}

#[test]
fn class_order_and_action_classes() {
    assert_eq!(Class::ALL.iter().filter(|c| c.is_action()).count(), 6);
    assert!(Class::Publish.is_action() && Class::FileWrite.is_action());
    assert!(!Class::VcsRead.is_action() && !Class::Other.is_action());
    let mut sorted = Class::ALL;
    sorted.sort();
    assert_eq!(sorted, Class::ALL);
}

#[test]
fn every_subshell_after_a_prefix_is_parsed() {
    // Review finding 1: each shape the reviewer listed.
    let segs = parse("for d in */; do (cd \"$d\" && git push); done");
    assert_eq!(
        rows(&segs),
        vec![
            (C::Subshell, R::Navigation, Some("cd"), None, None),
            (
                C::And,
                R::Operative,
                Some("git"),
                Some("push"),
                Some(Class::Publish)
            ),
        ]
    );
    assert_eq!(
        programs("if (grep -q x f); then make; fi"),
        ["grep", "make"]
    );
    assert_eq!(programs("! (rm x)"), ["rm"]);
    assert_eq!(programs("{ (rm y); }"), ["rm"]);
    assert_eq!(programs("if true\nthen\n(rm a)\nfi"), ["true", "rm"]);
    let segs = parse("foo() ( rm -rf x )");
    assert_eq!(
        rows(&segs),
        vec![(
            C::Subshell,
            R::Operative,
            Some("rm"),
            None,
            Some(Class::FileWrite)
        )]
    );
    // `time (…)` leaves no `time` segment behind.
    assert_eq!(parse("time (python3 -c pass)").len(), 1);
    // `if ((…))` is still arithmetic, not a subshell.
    assert_eq!(programs("if ((x > 1)); then make; fi"), ["make"]);
}

#[test]
fn a_case_pattern_paren_does_not_close_a_substitution() {
    // Review finding 2.
    let segs = parse("echo $(case x in a) rm z;; esac) && ls");
    assert_eq!(
        programs("echo $(case x in a) rm z;; esac) && ls"),
        ["echo", "rm", "ls"]
    );
    assert_eq!((segs[1].depth, segs[1].text.as_str()), (1, "rm z"));
    assert_eq!((segs[2].connector, segs[2].depth), (C::And, 0));
    assert_eq!(programs("(case x in a) rm z;; esac) && ls"), ["rm", "ls"]);
}

#[test]
fn a_case_pattern_with_a_leading_paren() {
    // Review finding 3.
    let segs = parse("case x in (a) rm -rf /;; (b) ls;; esac");
    assert_eq!(
        programs("case x in (a) rm -rf /;; (b) ls;; esac"),
        ["rm", "ls"]
    );
    assert_eq!(segs[0].text, "rm -rf /");
    assert_eq!(segs[0].class, Some(Class::FileWrite));
}

#[test]
fn dollar_quotes_and_braces_inside_double_quotes_are_literal() {
    // Review finding 4: neither `'` opens a quote that swallows the rest.
    let segs = parse("echo \"cost $'5\" && rm -rf x");
    assert_eq!(programs("echo \"cost $'5\" && rm -rf x"), ["echo", "rm"]);
    assert_eq!(segs[0].text, "echo \"cost $'5\"");
    assert_eq!(programs("echo \"${x:-it's}\" ; rm w"), ["echo", "rm"]);
    assert_eq!(programs("echo \"say $\\\"hi\\\"\" && ls"), ["echo", "ls"]);
    // Outside double quotes, $'…' is still an ANSI-C string.
    assert_eq!(programs("echo $'a; rm x' && ls"), ["echo", "ls"]);
}

/// The operative `git push` segment of a parse: its depth and text.
fn git_push(segs: &[Segment]) -> Option<(usize, &str)> {
    segs.iter()
        .find(|s| {
            s.is_operative()
                && s.program.as_deref() == Some("git")
                && s.subcommand.as_deref() == Some("push")
        })
        .map(|s| (s.depth, s.text.as_str()))
}

#[test]
fn eval_arguments_are_parsed_as_a_command() {
    let segs = parse("eval \"git push --force\"");
    assert_eq!(
        rows(&segs),
        vec![
            (C::Start, R::Navigation, Some("eval"), None, None),
            (
                C::CommandString,
                R::Operative,
                Some("git"),
                Some("push"),
                Some(Class::Publish)
            ),
        ]
    );
    assert_eq!(git_push(&segs), Some((1, "git push --force")));
    assert_eq!(segs[1].flags, ["--force"]);
    // Unquoted arguments are joined with spaces, as bash joins them.
    assert_eq!(git_push(&parse("eval git push")), Some((1, "git push")));
    assert_eq!(
        git_push(&parse("eval git 'push' origin")),
        Some((1, "git push origin"))
    );
    // The string is a list of its own: each command in it is a segment.
    let segs = parse("cd /x && eval 'make build; git push'");
    assert_eq!(operative_texts(&segs), ["make build", "git push"]);
}

#[test]
fn eval_alone_stays_navigation() {
    let segs = parse("eval");
    assert_eq!(
        rows(&segs),
        vec![(C::Start, R::Navigation, Some("eval"), None, None)]
    );
}

#[test]
fn a_shell_command_string_is_parsed_as_a_command() {
    let segs = parse("bash -c \"git push origin main\"");
    assert_eq!(
        rows(&segs),
        vec![
            (
                C::Start,
                R::Operative,
                Some("bash"),
                None,
                Some(Class::Other)
            ),
            (
                C::CommandString,
                R::Operative,
                Some("git"),
                Some("push"),
                Some(Class::Publish)
            ),
        ]
    );
    assert_eq!(git_push(&segs), Some((1, "git push origin main")));

    let segs = parse("sh -c 'cd /tmp && rm -rf build'");
    assert_eq!(
        rows(&segs),
        vec![
            (C::Start, R::Operative, Some("sh"), None, Some(Class::Other)),
            (C::CommandString, R::Navigation, Some("cd"), None, None),
            (
                C::And,
                R::Operative,
                Some("rm"),
                None,
                Some(Class::FileWrite)
            ),
        ]
    );
    assert_eq!(segs.iter().map(|s| s.depth).collect::<Vec<_>>(), [0, 1, 1]);
}

#[test]
fn a_shell_c_flag_in_a_cluster_or_behind_a_wrapper() {
    let segs = parse("bash -lc \"make test\"");
    assert_eq!(
        operative_texts(&segs),
        ["bash -lc \"make test\"", "make test"]
    );
    assert_eq!(segs[1].class, Some(Class::BuildTest));
    for cmd in [
        "bash -ec 'git push'",
        "sh -xc 'git push'",
        "zsh -c 'git push'",
        "dash -c 'git push'",
        "ksh -c 'git push'",
        "/bin/bash -c 'git push'",
        "bash -euo pipefail -c 'git push'",
        "bash --norc -c 'git push'",
        "sudo sh -c 'git push'",
        "sudo -u deploy bash -lc 'git push'",
        "env FOO=1 bash -c \"git push\"",
        "nohup bash -c 'git push' &",
        "ls | xargs -I {} sh -c 'git push'",
    ] {
        assert_eq!(git_push(&parse(cmd)), Some((1, "git push")), "{cmd}");
    }
    let segs = parse("sudo sh -c 'git push'");
    assert_eq!(
        (segs[0].program.as_deref(), segs[1].program.as_deref()),
        (Some("sh"), Some("git"))
    );
}

#[test]
fn nested_shell_strings_nest_and_terminate() {
    let segs = parse("bash -c \"sh -c 'git push'\"");
    assert_eq!(
        segs.iter()
            .map(|s| (s.depth, s.program.as_deref()))
            .collect::<Vec<_>>(),
        [(0, Some("bash")), (1, Some("sh")), (2, Some("git"))]
    );
    assert_eq!(git_push(&segs), Some((2, "git push")));
    // Escaped quotes inside a double-quoted string.
    let segs = parse(r#"bash -c "bash -c \"git push\"""#);
    assert_eq!(git_push(&segs), Some((2, "git push")));
    // Past the depth cap the string is not parsed, and the parse ends.
    let deep = format!("{}git push", "eval ".repeat(200));
    let segs = parse(&deep);
    assert!(segs.len() <= 32, "{}", segs.len());
    assert!(segs.iter().all(|s| s.depth < 32));
    assert_eq!(git_push(&segs), None);
    let shallow = format!("{}git push", "eval ".repeat(10));
    assert_eq!(git_push(&parse(&shallow)), Some((10, "git push")));
}

#[test]
fn a_shell_without_a_command_string_is_unchanged() {
    for cmd in [
        "bash deploy.sh",
        "bash",
        "sh -s",
        "bash -x deploy.sh -c 'git push'",
        "bash <<EOF\ngit push\nEOF",
        "echo bash -c 'git push'",
    ] {
        let segs = parse(cmd);
        assert_eq!(segs.len(), 1, "{cmd}: {segs:?}");
        assert_eq!(git_push(&segs), None, "{cmd}");
    }
    assert_eq!(programs("bash deploy.sh && git status"), ["bash", "git"]);
}

#[test]
fn a_substitution_in_a_command_string_is_one_segment() {
    // The outer lexer reads `$( … )` in a double-quoted word; the string's
    // own parse reads it again. It is described once.
    let segs = parse("bash -c \"echo $(git push)\"");
    let pushes = segs.iter().filter(|s| s.text == "git push").count();
    assert_eq!(pushes, 1, "{segs:?}");
    assert_eq!(git_push(&segs), Some((2, "git push")));
    let segs = parse("eval \"$(ssh-agent -s)\"");
    let agents = segs
        .iter()
        .filter(|s| s.program.as_deref() == Some("ssh-agent"))
        .count();
    assert_eq!(agents, 1, "{segs:?}");
}

#[test]
fn command_string_spans_slice_the_original_command() {
    for cmd in [
        "bash -c 'git push origin' && ls",
        "eval \"git push\"",
        "sudo sh -c 'cd /tmp && rm -rf build'",
        "bash -c \"sh -c 'git push'\"",
    ] {
        for s in parse(cmd) {
            assert_eq!(&cmd[s.span.clone()], s.text, "{cmd}: {s:?}");
        }
    }
    // An escaped string cannot be sliced: the span covers its word.
    let cmd = r#"bash -c "echo \"hi\"" && ls"#;
    let segs = parse(cmd);
    assert_eq!(segs[1].text, "echo \"hi\"");
    assert_eq!(&cmd[segs[1].span.clone()], r#""echo \"hi\"""#);
}
