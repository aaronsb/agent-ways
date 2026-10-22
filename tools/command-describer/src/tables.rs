//! The describer's tables, as data.
//!
//! Every list here is ported from the segmenter and class rules of the
//! Bash-surface study behind ADR-707. Later slices extend the describer by
//! editing these tables, not the parser. Additions made in the port, beyond
//! the study, are marked. The module is laid out by hand; rustfmt skips it
//! (see `lib.rs`).

use crate::Class;

/// Programs whose segment is navigation or setup, never operative.
/// `mkdir` is here because the study counted `mkdir -p` as setup.
pub const NAVIGATION: &[&str] = &[
    "cd", "pushd", "popd", "export", "set", "unset", "source", ".", "local", "declare", "alias",
    "trap", "shift", "echo", "printf", "true", "false", ":", "sleep", "wait", "read", "exit",
    "return", "test", "[", "[[", "umask", "shopt", "eval", "type", "hash", "ulimit", "mkdir",
];

/// Programs that are filters when a pipe feeds them. At the start of a call
/// (`cat file | …`) they are operative.
pub const FILTERS: &[&str] = &[
    "head", "tail", "cut", "sort", "uniq", "wc", "jq", "yq", "grep", "egrep", "fgrep", "rg", "awk",
    "gawk", "tr", "column", "nl", "fold", "paste", "rev", "fmt", "less", "more", "cat", "comm",
    "base64", "od", "xxd", "md5sum", "sha256sum", "sha1sum", "sed", "tac", "expand", "strings",
    "bat", "ugrep", "cmp", "diff", "zcat", "gzip", "gunzip", "iconv", "numfmt", "shuf", "split",
    "sponge",
];

/// Prefixes that run another program. The describer skips each, with its
/// flags, flag values, durations and assignments, to reach the real program.
/// `timeout` is folded in from the study's separate rule; `doas` and
/// `builtin` are added by the port.
pub const WRAPPERS: &[&str] = &[
    "sudo", "doas", "env", "nice", "time", "command", "builtin", "exec", "nohup", "xargs", "stdbuf",
    "ionice", "unbuffer", "timeout",
];

/// Shell keywords stripped from the front of a segment. `do` marks the
/// segment as a loop body.
pub const LEAD_KEYWORDS: &[&str] = &[
    "do", "then", "else", "elif", "if", "while", "until", "{", "!", "time",
];

/// Shell keywords that make the rest of a segment structure, not a command:
/// `for x in …`, `case … in`, `done`, `fi`, `esac`, a closing brace.
pub const SKIP_KEYWORDS: &[&str] = &[
    "for", "case", "select", "done", "fi", "esac", "}", "in",
];

/// Programs with subcommands, and how many leading subcommand words to keep.
/// A program listed with 0 has no subcommand but stops collecting words at
/// its first argument, as the listed programs do.
pub const SUBCOMMAND_DEPTH: &[(&str, usize)] = &[
    ("git", 1), ("gh", 2), ("cargo", 1), ("make", 1), ("docker", 2), ("podman", 2),
    ("systemctl", 1), ("tmux", 1), ("npm", 1), ("pnpm", 1), ("yarn", 1), ("pip", 1), ("pip3", 1),
    ("uv", 1), ("go", 1), ("kubectl", 1), ("ways", 2), ("adr", 1), ("doc", 1), ("attend", 2),
    ("attend-chat", 1), ("claude", 1), ("brew", 1), ("pacman", 0), ("yay", 0), ("apt", 1),
    ("rustup", 1), ("just", 1), ("glab", 2), ("journalctl", 0), ("gh-tasks", 1),
    ("ways-agent", 1), ("ways-audit", 1), ("ways-mcp", 1), ("mmaid", 1), ("busctl", 1),
    ("loginctl", 1), ("machinectl", 1), ("flatpak", 1), ("snap", 1), ("gcloud", 2), ("aws", 2),
];

/// `git` subcommands that take a second subcommand word: `git stash pop`,
/// `git worktree add`, `git remote add`, `git submodule update`.
pub const GIT_SECOND_WORD: &[&str] = &["stash", "worktree", "remote", "submodule"];

/// Flags that take the next word as their value, per program. The value is
/// not read as a subcommand or as the program (`sudo -u root cmd`,
/// `git -C dir status`, `timeout -s KILL 10 cmd`). Added by the port: the
/// study read such values as words.
pub const VALUE_FLAGS: &[(&str, &[&str])] = &[
    ("sudo", &["-u", "-g", "-C", "-D", "-h", "-p", "-r", "-t", "-U", "-T", "--user", "--group", "--chdir"]),
    ("doas", &["-u", "-C"]),
    ("env", &["-u", "-C", "-S", "--unset", "--chdir", "--split-string"]),
    ("nice", &["-n", "--adjustment"]),
    ("ionice", &["-c", "-n", "-p", "-P", "-u", "--class", "--classdata"]),
    ("stdbuf", &["-i", "-o", "-e", "--input", "--output", "--error"]),
    ("xargs", &["-I", "-n", "-P", "-L", "-d", "-a", "-E", "-s", "--max-args", "--max-procs", "--delimiter", "--arg-file"]),
    ("time", &["-f", "-o", "--format", "--output"]),
    ("timeout", &["-s", "-k", "--signal", "--kill-after"]),
    ("git", &["-C", "-c"]),
    ("gh", &["-R", "--repo"]),
    ("make", &["-C", "-f", "-I", "-o", "-W", "--directory", "--file", "--makefile"]),
    ("cargo", &["-p", "--package", "--manifest-path", "--target", "--bin", "--example", "--test", "--features", "-F", "-Z"]),
    ("docker", &["-H", "--host", "--context", "-f", "--file", "-p", "--project-name"]),
    ("podman", &["-H", "--host", "--context", "-f", "--file", "-p", "--project-name"]),
    ("kubectl", &["-n", "--namespace", "--context", "--kubeconfig"]),
    ("npm", &["--prefix", "-w", "--workspace"]),
    ("pnpm", &["-C", "--dir", "--filter", "-F"]),
    ("tmux", &["-L", "-S", "-f"]),
    ("systemctl", &["-H", "-M", "--host", "--machine"]),
];

/// The input to a class rule: one segment from its program on.
pub struct ClassInput<'a> {
    /// The program's base name.
    pub program: &'a str,
    /// Every word after the program, flags included, redirections excluded.
    pub tokens: &'a [&'a str],
    /// The positional words after the program: tokens not starting with
    /// `-`, less the values of [`VALUE_FLAGS`].
    pub words: &'a [&'a str],
    /// The segment redirects output to a file other than `/dev/null`.
    pub redirect: bool,
}

impl ClassInput<'_> {
    fn word(&self, i: usize) -> &str {
        self.words.get(i).copied().unwrap_or("")
    }
}

/// A condition on a [`ClassInput`].
pub enum Cond {
    Always,
    /// The first positional word is one of these (`""` means none).
    W0(&'static [&'static str]),
    /// The second positional word is one of these (`""` means none).
    W1(&'static [&'static str]),
    /// Some token equals one of these.
    Has(&'static [&'static str]),
    /// Some token starts with one of these.
    HasPrefix(&'static [&'static str]),
    /// Output is redirected to a file.
    Redirect,
    /// More than this many positional words.
    WordsOver(usize),
    Not(&'static Cond),
    All(&'static [Cond]),
    Any(&'static [Cond]),
    /// A rule too specific for the combinators.
    Pred(fn(&ClassInput) -> bool),
}

impl Cond {
    pub fn eval(&self, c: &ClassInput) -> bool {
        match self {
            Cond::Always => true,
            Cond::W0(set) => set.contains(&c.word(0)),
            Cond::W1(set) => set.contains(&c.word(1)),
            Cond::Has(set) => c.tokens.iter().any(|t| set.contains(t)),
            Cond::HasPrefix(set) => c.tokens.iter().any(|t| set.iter().any(|p| t.starts_with(p))),
            Cond::Redirect => c.redirect,
            Cond::WordsOver(n) => c.words.len() > *n,
            Cond::Not(inner) => !inner.eval(c),
            Cond::All(cs) => cs.iter().all(|x| x.eval(c)),
            Cond::Any(cs) => cs.iter().any(|x| x.eval(c)),
            Cond::Pred(f) => f(c),
        }
    }
}

/// One class rule: for these programs (empty: any program), when the
/// condition holds, the segment has this class.
pub struct ClassRule {
    pub programs: &'static [&'static str],
    pub when: Cond,
    pub class: Class,
}

/// `pacman -S…` (not `-Ss`/`-Si`), `-R…` or `-U…`: the study's
/// `^-(S[^si]*|R\w*|U\w*)$`.
fn pacman_writes(c: &ClassInput) -> bool {
    c.tokens.iter().any(|t| {
        let Some(rest) = t.strip_prefix('-') else { return false };
        let mut chars = rest.chars();
        let word_tail = |s: &str| s.chars().all(|ch| ch.is_alphanumeric() || ch == '_');
        match chars.next() {
            Some('S') => !chars.as_str().contains(['s', 'i']),
            Some('R') | Some('U') => word_tail(chars.as_str()),
            _ => false,
        }
    })
}

const SERVICE: &[&str] = &[
    "systemctl", "kill", "pkill", "killall", "tmux", "screen", "docker", "podman", "launchctl",
    "nohup", "setsid", "journalctl", "loginctl", "busctl", "machinectl", "ps", "pgrep", "lsof",
    "fuser", "ss", "systemd-run", "kmscon", "swaymsg", "hyprctl", "qemu-system-x86_64", "virsh",
    "reboot", "shutdown",
];

const PACKAGE: &[&str] = &[
    "pacman", "yay", "paru", "apt", "apt-get", "brew", "pip", "pip3", "pipx", "npm", "pnpm",
    "yarn", "uv", "rustup", "flatpak", "snap", "dnf", "makepkg", "go",
];

const BUILD_TEST: &[&str] = &[
    "cargo", "make", "pytest", "ctest", "cmake", "ninja", "meson", "gcc", "cc", "clang", "rustc",
    "tsc", "jest", "vitest", "bats", "shellcheck", "clippy-driver", "just", "bun", "deno", "node",
    "mvn", "gradle", "dotnet", "msbuild", "zig", "luac", "busted", "tox", "nox", "mypy", "ruff",
    "eslint", "prettier", "bash-test",
];

const SEARCH: &[&str] = &[
    "grep", "rg", "ag", "ack", "fd", "find", "locate", "ugrep", "fzf", "git-grep",
];

const FILE_WRITE: &[&str] = &[
    "tee", "cp", "mv", "rm", "mkdir", "touch", "chmod", "chown", "ln", "install", "rsync", "tar",
    "unzip", "patch", "truncate", "dd", "rmdir", "trash", "shred",
];

const READ_INSPECT: &[&str] = &[
    "cat", "head", "tail", "less", "more", "bat", "ls", "lsd", "eza", "tree", "wc", "awk", "sed",
    "jq", "yq", "stat", "file", "diff", "cmp", "readlink", "realpath", "du", "df", "which", "type",
    "whereis", "nl", "sort", "uniq", "cut", "tr", "column", "od", "xxd", "hexdump", "strings",
    "md5sum", "sha256sum", "date", "env", "printenv", "uname", "id", "whoami", "hostname", "free",
    "uptime", "lscpu", "lsblk", "ip", "pwd", "basename", "dirname", "fc-list", "fc-match",
    "identify", "pdfinfo", "ldd", "nm", "objdump", "readelf", "base64", "comm", "paste", "tac",
    "rev", "fold", "fmt", "expand", "look", "getent", "locale", "lspci", "lsusb", "dmesg",
    "ldconfig", "pkg-config", "man", "tldr", "git-log", "zcat", "gzip",
];

/// The class rules, in order; the first that matches decides, and a
/// segment no rule matches is [`Class::Other`].
pub static CLASS_RULES: &[ClassRule] = &[
    // gh: writes to the forge publish, the rest reads it.
    ClassRule {
        programs: &["gh"],
        when: Cond::All(&[
            Cond::W0(&["pr", "issue", "release", "repo", "label"]),
            Cond::W1(&["create", "merge", "comment", "edit", "close", "reopen", "review", "ready", "delete", "lock", "develop"]),
        ]),
        class: Class::Publish,
    },
    ClassRule {
        programs: &["gh"],
        when: Cond::All(&[
            Cond::W0(&["api"]),
            Cond::Any(&[
                Cond::All(&[Cond::Has(&["-X", "--method"]), Cond::Has(&["POST", "PATCH", "PUT", "DELETE"])]),
                Cond::Has(&["-f", "-F", "--field", "--raw-field"]),
            ]),
        ]),
        class: Class::Publish,
    },
    ClassRule {
        programs: &["gh"],
        when: Cond::All(&[Cond::W0(&["workflow", "run"]), Cond::W1(&["run", "rerun", "cancel"])]),
        class: Class::Publish,
    },
    ClassRule { programs: &["gh"], when: Cond::Always, class: Class::VcsRead },
    // git
    ClassRule { programs: &["git"], when: Cond::W0(&["push"]), class: Class::Publish },
    ClassRule {
        programs: &["git"],
        when: Cond::Any(&[
            Cond::All(&[Cond::W0(&["worktree"]), Cond::W1(&["list"])]),
            Cond::All(&[Cond::W0(&["stash"]), Cond::W1(&["list", "show"])]),
            Cond::All(&[Cond::W0(&["remote"]), Cond::W1(&["", "get-url", "show"])]),
            Cond::All(&[
                Cond::W0(&["config"]),
                Cond::Not(&Cond::Any(&[Cond::Has(&["--add", "--unset", "--global"]), Cond::WordsOver(2)])),
            ]),
        ]),
        class: Class::VcsRead,
    },
    ClassRule {
        programs: &["git"],
        when: Cond::W0(&[
            "commit", "checkout", "switch", "merge", "rebase", "reset", "stash", "tag", "add", "rm",
            "mv", "worktree", "cherry-pick", "revert", "pull", "restore", "clean", "am", "apply",
            "init", "clone", "config", "submodule", "remote", "gc", "prune",
        ]),
        class: Class::VcsWrite,
    },
    ClassRule {
        programs: &["git"],
        when: Cond::All(&[Cond::W0(&["branch"]), Cond::Has(&["-d", "-D", "-m", "-M", "--delete", "-f"])]),
        class: Class::VcsWrite,
    },
    ClassRule { programs: &["git"], when: Cond::W0(&["grep"]), class: Class::Search },
    ClassRule { programs: &["git"], when: Cond::Always, class: Class::VcsRead },
    // services and processes: the read-only ones inspect.
    ClassRule {
        programs: &["ps", "pgrep", "lsof", "ss", "journalctl", "fuser"],
        when: Cond::Always,
        class: Class::ReadInspect,
    },
    ClassRule {
        programs: &["systemctl"],
        when: Cond::W0(&["status", "show", "list-units", "is-active", "is-enabled", "cat", "list-timers"]),
        class: Class::ReadInspect,
    },
    ClassRule {
        programs: &["tmux"],
        when: Cond::W0(&["ls", "list-sessions", "list-windows", "capture-pane", "display-message", "has-session", "list-panes"]),
        class: Class::ReadInspect,
    },
    ClassRule {
        programs: &["docker", "podman"],
        when: Cond::W0(&["ps", "images", "logs", "inspect"]),
        class: Class::ReadInspect,
    },
    ClassRule { programs: SERVICE, when: Cond::Always, class: Class::Service },
    // packages: package managers that also run builds and queries.
    ClassRule {
        programs: &["npm", "pnpm", "yarn"],
        when: Cond::W0(&["test", "run", "build", "exec", "start", "lint"]),
        class: Class::BuildTest,
    },
    ClassRule {
        programs: &["go"],
        when: Cond::W0(&["test", "build", "run", "vet"]),
        class: Class::BuildTest,
    },
    ClassRule {
        programs: &["go"],
        when: Cond::Not(&Cond::W0(&["install", "get", "mod"])),
        class: Class::ReadInspect,
    },
    ClassRule {
        programs: &["pacman", "yay", "paru"],
        when: Cond::Not(&Cond::Pred(pacman_writes)),
        class: Class::ReadInspect,
    },
    ClassRule {
        programs: &["pip", "pip3", "npm", "brew", "apt", "uv"],
        when: Cond::W0(&["list", "show", "search", "info", "view", "outdated", "freeze"]),
        class: Class::ReadInspect,
    },
    ClassRule { programs: PACKAGE, when: Cond::Always, class: Class::Package },
    ClassRule {
        programs: &["cargo"],
        when: Cond::W0(&["install", "add", "update", "remove", "uninstall"]),
        class: Class::Package,
    },
    // build and test
    ClassRule {
        programs: &["node"],
        when: Cond::Not(&Cond::Has(&["--test"])),
        class: Class::Other,
    },
    ClassRule { programs: BUILD_TEST, when: Cond::Always, class: Class::BuildTest },
    // search; find -delete writes.
    ClassRule { programs: &["find"], when: Cond::Has(&["-delete"]), class: Class::FileWrite },
    ClassRule { programs: SEARCH, when: Cond::Always, class: Class::Search },
    // file writes
    ClassRule {
        programs: &["sed"],
        when: Cond::HasPrefix(&["-i", "--in-place"]),
        class: Class::FileWrite,
    },
    ClassRule {
        programs: &["tar"],
        when: Cond::Has(&["-t", "-tf", "-tzf", "tf", "--list"]),
        class: Class::ReadInspect,
    },
    ClassRule { programs: FILE_WRITE, when: Cond::Always, class: Class::FileWrite },
    ClassRule { programs: &[], when: Cond::Redirect, class: Class::FileWrite },
    ClassRule { programs: READ_INSPECT, when: Cond::Always, class: Class::ReadInspect },
    // this project's own tools
    ClassRule {
        programs: &["adr"],
        when: Cond::W0(&["new", "set", "accept", "archive", "rename", "consider", "index"]),
        class: Class::FileWrite,
    },
    ClassRule { programs: &["ways"], when: Cond::W0(&["corpus"]), class: Class::BuildTest },
    ClassRule { programs: &["ways"], when: Cond::W0(&["manage", "init"]), class: Class::Package },
    ClassRule {
        programs: &["attend"],
        when: Cond::All(&[Cond::W0(&["message", "channel"]), Cond::W1(&["send", "reply", "join", "add"])]),
        class: Class::Publish,
    },
];

/// How many subcommand words `program` keeps, or `None` when it has no
/// subcommands.
pub fn subcommand_depth(program: &str) -> Option<usize> {
    SUBCOMMAND_DEPTH.iter().find(|(p, _)| *p == program).map(|(_, n)| *n)
}

/// Whether `flag` (without any `=value`) takes the next word as its value
/// for `program`.
pub fn takes_value(program: &str, flag: &str) -> bool {
    VALUE_FLAGS
        .iter()
        .find(|(p, _)| *p == program)
        .is_some_and(|(_, flags)| flags.contains(&flag))
}

/// The class of an operative segment: the first matching rule's class.
pub fn classify(input: &ClassInput) -> Class {
    CLASS_RULES
        .iter()
        .find(|r| (r.programs.is_empty() || r.programs.contains(&input.program)) && r.when.eval(input))
        .map_or(Class::Other, |r| r.class)
}
