#!/usr/bin/env python3
"""check-bash-bound.py: a guard hook on the Bash tool (ADR-181).

Wired under hooks.PreToolUse with matcher Bash and no `|| true`. The host
passes {"tool_name": "Bash", "tool_input": {"command": "...",
"run_in_background": bool, ...}} on stdin. A command in one of four refused
classes is stopped: the reason and the accepted form go to stderr and the
hook exits 2, which blocks the call and hands the model the reason.

The script exits only 0 or 2. Every failure path (not Bash, no command,
unparseable stdin, an exception) exits 0 with one line on stderr. A bug here
degrades to no guard and never blocks everything.

Adapted from the bound-hook in llopresto87/Cypress (MIT), narrowed to what
the Claude Code harness leaves open: the harness already bounds every
foreground command by timeout and offers run_in_background, so builds and
installs are not refused here.
"""

import json
import re
import sys

# Refused classes. Each entry is (name, match, exempt, kind).
#   match   regex tried at a command-word position, past any path prefix
#   exempt  regex tried from the same position; a match means no hit
#   kind    "kill"        refused outright; repair is a pid-based kill
#           "prompt"      refused; repair is the non-interactive flag
#           "foreground"  refused unless run in the background or under a
#                         timeout prefix; repair is run_in_background
#           "pipe"        refused outright; repair is download, read, run
REFUSED = [
    ("pkill", r"pkill\b", None, "kill"),
    ("killall", r"killall\b", None, "kill"),
    ("kill by pattern", r"kill\s+-",
     r"kill\s+-\S+\s+(?:\d+|%\d+|\$\(\s*cat\s+[^)]*pid[^)]*\)|`\s*cat\s+[^`]*pid[^`]*`|\$\{?[A-Za-z_][A-Za-z0-9_]*\}?)\s*(?:$|[;&|])",
     "kill"),
    ("sudo without -n", r"sudo\b", r"sudo\s+-n\b", "prompt"),
    ("ssh without BatchMode", r"ssh\s", r"ssh\b[^;|&]*-o\s*BatchMode=yes", "prompt"),
    ("docker exec -it", r"docker\s+exec\b[^;|&]*\s-\w*i\w*t\w*\b", None, "prompt"),
    ("apt without -y", r"apt(?:-get)?\s+(?:install|remove|purge|upgrade|dist-upgrade|full-upgrade)\b",
     r"apt(?:-get)?\b[^;|&]*\s(?:-y|--yes|--assume-yes)\b", "prompt"),
    ("pacman without --noconfirm", r"pacman\s+-S\w*\b", r"pacman\b[^;|&]*--noconfirm\b", "prompt"),
    ("yay without --noconfirm", r"yay\b", r"yay\b[^;|&]*--noconfirm\b", "prompt"),
    ("journalctl -f", r"journalctl\b[^;|&]*\s-f\b", None, "foreground"),
    ("tail -f", r"tail\b[^;|&]*\s-[a-zA-Z]*f\b", None, "foreground"),
    ("docker run attached", r"docker\s+run\b",
     r"docker\s+run\b[^;|&]*\s(?:-d\b|--detach\b|--rm\b[^;|&]*\s-d\b)", "foreground"),
    ("pipe to shell", r"(?:bash|sh|zsh|dash)(?:\s+-\S+)*\s*$", None, "pipe"),
]

SEPARATOR = re.compile(r"\|\||&&|\$\(|[;\n|&(){}`]")

# Words that stand in front of the real command. Seeing `timeout` marks the
# position as bounded for the foreground class.
PREFIX_WORD = re.compile(
    r"""(?:
        [A-Za-z_][A-Za-z0-9_]*=(?:"[^"]*"|'[^']*'|\S*)
      | timeout(?:\s+--preserve-status)?(?:\s+-k\s*[0-9.]+[smhd]?)?(?:\s+-s\s*\S+)?\s+[0-9.]+[smhd]?
      | nice(?:\s+-n\s*-?\d+)?
      | ionice(?:\s+-c\s*\d+)?
      | env | command | exec | setsid | nohup | time | then | else | do
    )(?=\s)""",
    re.X,
)
PATH_PREFIX = re.compile(r"(?:[A-Za-z0-9_.~${}+-]*/)+")
QUOTED = re.compile(r'"(?:[^"\\]|\\.)*"|\'[^\']*\'')
WRAPPER = re.compile(r"\b(?:bash|sh|zsh|dash)\s+(?:-[A-Za-z]+\s+)*-c\s+|\beval\s+")
BACKGROUND = re.compile(r"(?<![>&])&(?!&)")


def unwrap(cmd, depth=0):
    """Replace `bash -c "..."` and `eval "..."` with their payload."""
    if depth > 4:
        return cmd
    out, pos, changed = [], 0, False
    while True:
        m = WRAPPER.search(cmd, pos)
        if not m:
            out.append(cmd[pos:])
            break
        q = QUOTED.match(cmd, m.end())
        if not q:
            out.append(cmd[pos:m.end()])
            pos = m.end()
            continue
        body = q.group(0)[1:-1]
        if q.group(0)[0] == '"':
            body = re.sub(r"\\(.)", r"\1", body)
        out.append(cmd[pos:m.start()])
        out.append(body)
        pos = q.end()
        changed = True
    new = "".join(out)
    return unwrap(new, depth + 1) if changed else new


def masked(cmd):
    """Quoted strings blanked to spaces so separators are read outside quotes."""
    return QUOTED.sub(lambda m: " " * len(m.group(0)), cmd)


def command_positions(cmd):
    """Offsets where a command word starts, with whether a timeout prefix is in scope."""
    starts = [0] + [m.end() for m in SEPARATOR.finditer(cmd)]
    found = {}
    for start in starts:
        pos, bounded = start, False
        while True:
            while pos < len(cmd) and cmd[pos].isspace():
                pos += 1
            found[pos] = found.get(pos, False) or bounded
            m = PREFIX_WORD.match(cmd, pos)
            if not m or m.end() == pos:
                break
            if cmd[pos:m.end()].startswith("timeout"):
                bounded = True
            pos = m.end()
    return sorted(found.items())


def detached(cmd):
    """The setsid/nohup form, accepted for compatibility with other harnesses."""
    return ("setsid" in cmd or "nohup" in cmd) and bool(BACKGROUND.search(cmd))


def first_hit(cmd, background):
    text = masked(cmd)
    for pos, bounded in command_positions(text):
        offsets = [pos]
        p = PATH_PREFIX.match(text, pos)
        if p and p.end() > pos:
            offsets.append(p.end())
        for off in offsets:
            tail = text[off:]
            for name, match, exempt, kind in REFUSED:
                if not re.match(match, tail):
                    continue
                if kind == "pipe" and not text[:pos].rstrip().endswith("|"):
                    continue
                if exempt and re.match(exempt, tail):
                    continue
                if kind == "foreground" and (bounded or background):
                    continue
                return off, name, kind
    return None


def segment_of(cmd, offset):
    text = masked(cmd)
    start = 0
    for m in SEPARATOR.finditer(text):
        if m.end() <= offset:
            start = m.end()
    end = len(cmd)
    for m in SEPARATOR.finditer(text):
        if m.start() > offset:
            end = m.start()
            break
    return cmd[start:end].strip()


def reason(cmd, offset, name, kind):
    seg = segment_of(cmd, offset) or cmd.strip()
    head = "[check-bash-bound] BLOCKED: `%s` (%s)." % (seg, name)
    if kind == "kill":
        body = ("A process is stopped by its recorded or literal pid, never by a pattern "
                "that can match this shell, a subagent, or an operator process.\n"
                "  kill -TERM <pid>            (from a pidfile: kill -TERM $(cat job.pid))\n"
                "  pgrep -f <pattern>          to find the pid first, then confirm it is yours")
    elif kind == "prompt":
        body = ("This command sits on a prompt until the timeout and then fails. Give it "
                "its non-interactive flag:\n"
                "  sudo -n ...   ssh -o BatchMode=yes ...   docker exec (no -it) ...   "
                "apt -y ...   pacman --noconfirm ...")
    elif kind == "foreground":
        body = ("This command never returns in the foreground and burns the whole timeout. "
                "Run it in the background:\n"
                "  Bash tool with run_in_background: true, then read its output file\n"
                "  or:  timeout 30 %s" % seg)
    else:
        body = ("Piping a download into a shell runs whatever arrived. Download to a file, "
                "read it, then run the file:\n"
                "  curl -fsSL <url> -o install.sh && sed -n 1,80p install.sh && bash install.sh")
    return head + "\n" + body


def main():
    try:
        payload = json.loads(sys.stdin.read())
    except Exception:
        return 0
    if not isinstance(payload, dict) or payload.get("tool_name") != "Bash":
        return 0
    tool_input = payload.get("tool_input") or {}
    cmd = tool_input.get("command")
    if not isinstance(cmd, str) or not cmd.strip():
        return 0
    background = bool(tool_input.get("run_in_background")) or detached(cmd)
    hit = first_hit(unwrap(cmd), background)
    if hit is None:
        return 0
    sys.stderr.write(reason(cmd, *hit) + "\n")
    return 2


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:  # a guard bug must never brick a session
        sys.stderr.write("[check-bash-bound] internal error, allowing: %s\n" % exc)
        sys.exit(0)
