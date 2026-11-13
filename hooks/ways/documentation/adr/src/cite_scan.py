# ============================================================================
# Citation placement: where each citation sits in a file
# ============================================================================
#
# `adr cite` reports a citation that sits where it should not. Text that stays
# with the repository explains itself in its own words and lists the records
# behind it in a references block at its end: a `References:` line that runs
# to the end of a code comment, or a `References` heading that runs to the
# next heading of the same or a higher level in a Markdown page. Text that
# ships with the built program (strings and code in a program source, and the
# paths adr.yaml lists in cite.shipped) names no record, because its reader
# cannot open one.
#
# The lexers in cite_lex mark the comments and strings in a file; a citation
# that falls in neither sits in code.

CITE_PLACEMENT_LEVELS = ('error', 'warn', 'off')
CITE_PROGRAM_EXTS = {'.rs', '.py', '.ts', '.tsx', '.js', '.mjs', '.go', '.sh'}

_PLACE_BODY = "{cited} sits in the comment body; move it to a References: block at the end of the comment"
_PLACE_SHIPPED = "{cited} is in text the program ships; say what it decides instead"
_PLACE_PAGE = "{cited} sits in the page body; move it to a References section"
_PLACE_BARE = "the references entry for {cited} says nothing about what the record decides here"
_PLACE_OUTSIDE = "{cited} sits in the References block outside an entry; start an entry with '- '"

# What an entry may hold besides its citations without saying anything:
# section markers such as D3 or §2.1, then punctuation and space.
_CITE_SECTION_MARK = re.compile(r"\bD\d+\b|§\s*\d+(?:\.\d+)*")
_CITE_PUNCT = re.compile(r"[\W_]+")
# An entry in a comment's References block starts with `- ` or `* `; in a
# Markdown References section with `-`, `*`, `+` or a number such as `1.`.
_CITE_ENTRY_COMMENT = re.compile(r"[-*](?:\s|$)")
_CITE_ENTRY_MD = re.compile(r"(?:[-*+]|\d+\.)(?:\s|$)")
_CITE_MD_HEADING = re.compile(r"^ {0,3}(#{1,6})(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*$")
_CITE_MD_SETEXT = re.compile(r"^ {0,3}(=+|-+)[ \t]*$")
_CITE_MD_FENCE = re.compile(r"^ {0,3}(`{3,}|~{3,})")
_CITE_SHEBANG_SHELL = re.compile(r"(?:ba|z|da|k)?sh")
_CITE_SHEBANG_PYTHON = re.compile(r"python[0-9.]*")
_CITE_RUST_TEST_MOD = re.compile(r"\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{")


class _CiteUnit:
    """One comment, or a Markdown References section. `refs` is the line of
    its `References:` line, or None; `entries` are its references entries as
    (first line, last line, text); `covered` holds the lines the entries
    span."""
    __slots__ = ('lines', 'refs', 'entries', 'covered')

    def __init__(self):
        self.lines, self.refs, self.entries, self.covered = [], None, [], set()

    def read_entries(self, numbered: list, entry_re) -> None:
        """Read the entries from (line, text) pairs in the block. An entry
        starts at a line entry_re matches and runs to the next one; a text of
        None ends the entry before it, as a Markdown blank line or heading
        does."""
        entries, open_entry = [], False
        for ln, body in numbered:
            if body is None:
                open_entry = False
                continue
            stripped = body.strip()
            m = entry_re.match(stripped)
            if m:
                entries.append([ln, ln, stripped[m.end():]])
                open_entry = True
            elif open_entry:
                entries[-1][1] = ln
                entries[-1][2] += ' ' + stripped
        self.entries = [tuple(e) for e in entries]
        self.covered = {ln for first, last, _ in self.entries for ln in range(first, last + 1)}


# --- comments and their references blocks ------------------------------------

def _cite_comment_text(text: str, seg) -> list:
    """The comment's text, one string per line, with its markers removed."""
    raw = text[seg.start:seg.end]
    if seg.marker != 'block':
        body = raw[len(seg.marker):]
        return [body.lstrip(seg.marker[0]) if seg.marker in ('#', '//') else body]
    lines = raw.split('\n')
    out = []
    for idx, line in enumerate(lines):
        line = line.rstrip('\r')
        if idx == 0:
            line = re.sub(r'^(/\*[*!]?|[rRbBuUfF]{0,2}("""|\'\'\'))', '', line)
        else:
            line = line.lstrip()
            if line.startswith('*') and not line.startswith('*/'):
                line = line[1:]
        if idx == len(lines) - 1:
            line = re.sub(r'(\*/|"""|\'\'\')\s*$', '', line)
        out.append(line)
    return out


def _cite_units(text: str, segs: list, line_starts: list) -> list:
    """Group comment segments into comments and find each one's references
    block."""
    units, last = [], None  # last: (unit, marker, line) of a whole-line comment
    for seg in segs:
        if seg.kind != 'comment':
            continue
        first = bisect.bisect_right(line_starts, seg.start)
        whole_line = not text[line_starts[first - 1]:seg.start].strip()
        lines = _cite_comment_text(text, seg)
        if (seg.marker != 'block' and whole_line and last is not None
                and last[1] == seg.marker and last[2] == first - 1):
            unit = last[0]
        else:
            unit = _CiteUnit()
            units.append(unit)
        unit.lines.extend((first + k, body.rstrip('\r')) for k, body in enumerate(lines))
        seg.unit = unit
        end_line = first + len(lines) - 1
        last = (unit, seg.marker, end_line) if seg.marker != 'block' and whole_line else None
    for unit in units:
        for idx, (ln, body) in enumerate(unit.lines):
            if body.strip() == 'References:':
                unit.refs = ln
                unit.read_entries(unit.lines[idx + 1:], _CITE_ENTRY_COMMENT)
                break
    return units


def _cite_bare_entry(text: str) -> bool:
    """True when an entry holds only its citations, section markers and
    punctuation."""
    return not _CITE_PUNCT.sub('', _CITE_SECTION_MARK.sub('', CITE_RE.sub('', text)))


def _cite_bare_findings(units: list, lines: list, ignored, skip=lambda ln: False) -> list:
    """An entry that names records and says nothing else. `skip` leaves out
    entries the shipped rule already reports."""
    findings = []
    for unit in units:
        for first, last, body in unit.entries:
            cites = list(_cite_cites(lines, ignored, first, last))
            if cites and not skip(cites[0][0]) and _cite_bare_entry(body):
                ln, match = cites[0]
                findings.append((ln, match.start(), _PLACE_BARE.format(cited=match.group(0))))
    return findings


# --- per-file placement ----------------------------------------------------------

def _cite_file_kind(name: str, text: str):
    """The extension whose rules a file is read by: its own, or for a file
    with none, `.sh` or `.py` by its shebang. None for a file not read."""
    suffix = Path(name).suffix
    if suffix:
        return suffix if suffix in CITE_EXTS else None
    first = text.split('\n', 1)[0]
    if not first.startswith('#!'):
        return None
    words = first[2:].split()
    if words and Path(words[0]).name == 'env':
        words = [w for w in words[1:] if not w.startswith('-')]
    prog = Path(words[0]).name if words else ''
    if _CITE_SHEBANG_SHELL.fullmatch(prog):
        return '.sh'
    if _CITE_SHEBANG_PYTHON.fullmatch(prog):
        return '.py'
    return None


# File names that mark test code.
_CITE_TEST_NAMES = ('*_test.*', 'test_*.py', '*.test.*', 'tests.rs', '*_tests.rs', 'test-*.*', '*-test.*')


def _cite_is_test_path(name: str) -> bool:
    """Test code by its path: under a tests/ directory, or named as a test."""
    parts = name.split('/')
    return 'tests' in parts[:-1] or any(fnmatch.fnmatch(parts[-1], p) for p in _CITE_TEST_NAMES)


def _cite_code_offsets(text: str, segs: list, start: int):
    """Offsets from start that sit outside every comment and string."""
    k, i, n = bisect.bisect_right([s.start for s in segs], start) - 1, start, len(text)
    k = max(k, 0)
    while i < n:
        while k < len(segs) and segs[k].end <= i:
            k += 1
        if k < len(segs) and segs[k].start <= i < segs[k].end:
            i = segs[k].end
            continue
        yield i
        i += 1


def _cite_rust_test_lines(text: str, lines: list, segs: list, line_starts: list) -> set:
    """Lines of the inline test modules in a Rust file: a `#[cfg(test)]`
    whose next item, past further attributes and comments, is `mod name {`,
    from the attribute to the module's closing brace (or the end of the
    file). A `#[cfg(test)]` on any other item marks nothing."""
    test = set()
    for idx, line in enumerate(lines):
        if line.strip() != '#[cfg(test)]':
            continue
        nxt = idx + 1
        while nxt < len(lines) and (not lines[nxt].strip() or lines[nxt].strip().startswith(('#[', '//'))):
            nxt += 1
        if nxt >= len(lines) or not _CITE_RUST_TEST_MOD.match(lines[nxt]):
            continue
        brace = line_starts[nxt] + lines[nxt].index('{')
        depth, end_line = 0, len(lines)
        for offset in _cite_code_offsets(text, segs, brace):
            if text[offset] == '{':
                depth += 1
            elif text[offset] == '}':
                depth -= 1
                if depth == 0:
                    end_line = bisect.bisect_right(line_starts, offset)
                    break
        test.update(range(idx + 1, end_line + 1))
    return test


def _cite_place_markdown(lines: list, ignored, shipped: bool) -> list:
    """A Markdown page cites in its frontmatter or in an entry under a
    References heading; a shipped page cites only in its frontmatter."""
    lines = list(lines)
    if lines and lines[0].startswith('﻿'):
        lines[0] = lines[0][1:]
    front_end = 0
    if lines and lines[0].rstrip('\r') == '---':
        for idx in range(1, len(lines)):
            if lines[idx].rstrip('\r') in ('---', '...'):
                front_end = idx + 1
                break

    # First pass: which lines sit in a References section, and its entries.
    in_refs, unit = set(), _CiteUnit()
    fence, ref_level, underline = None, None, None
    for idx in range(front_end, len(lines)):
        ln, line = idx + 1, lines[idx].rstrip('\r')
        heading = None
        if idx == underline:
            continue  # a setext underline belongs to the heading above it
        if fence is not None:
            m = _CITE_MD_FENCE.match(line)
            if m and m.group(1)[0] == fence[0] and len(m.group(1)) >= len(fence) and not line.strip(' `~'):
                fence = None
        elif _CITE_MD_FENCE.match(line):
            fence = _CITE_MD_FENCE.match(line).group(1)
        else:
            m = _CITE_MD_HEADING.match(line)
            if m:
                heading = (len(m.group(1)), (m.group(2) or '').strip())
            elif (line.strip() and idx + 1 < len(lines) and not _CITE_ENTRY_MD.match(line.strip())
                    and (idx == front_end or not lines[idx - 1].strip())):
                setext = _CITE_MD_SETEXT.match(lines[idx + 1].rstrip('\r'))
                if setext:
                    heading = (1 if setext.group(1)[0] == '=' else 2, line.strip())
                    underline = idx + 1
        if heading is not None:
            level, title = heading
            if ref_level is not None and level <= ref_level:
                ref_level = None
            if ref_level is None:
                if title == 'References':
                    ref_level = level
                continue
            in_refs.add(ln)
            unit.lines.append((ln, None))  # a deeper heading stays in the section
            continue
        if ref_level is not None:
            in_refs.add(ln)
            unit.lines.append((ln, line if line.strip() else None))
    unit.read_entries(unit.lines, _CITE_ENTRY_MD)

    findings = []
    for ln, match in _cite_cites(lines, ignored, front_end + 1, len(lines)):
        cited = match.group(0)
        if shipped:
            findings.append((ln, match.start(), _PLACE_SHIPPED.format(cited=cited)))
        elif ln not in in_refs:
            findings.append((ln, match.start(), _PLACE_PAGE.format(cited=cited)))
        elif ln not in unit.covered:
            findings.append((ln, match.start(), _PLACE_OUTSIDE.format(cited=cited)))
    if not shipped:
        findings += _cite_bare_findings([unit], lines, ignored)
    return findings


def _cite_placement(kind: str, name: str, text: str, lines: list, ignored, shipped: bool) -> list:
    """Placement findings for one file, as (line, column, message). `kind`
    is the extension the file is read by (see _cite_file_kind).

    A citation in a comment follows the comment rule: it belongs in an entry
    of the comment's References block, and each entry says what the record
    decides. In a program source, a citation in a string or in code reaches
    the built program. In a shipped path every citation is out of place, data
    files included. Test code is held to the comment rule only, ahead of the
    shipped rule. Values in data files outside shipped paths are left alone.
    """
    test_path = _cite_is_test_path(name)
    if kind == '.md':
        return _cite_place_markdown(lines, ignored, shipped and not test_path)
    lexer = _CITE_LEXERS.get(kind)
    program = kind in CITE_PROGRAM_EXTS
    segs = lexer(text) if lexer else []
    line_starts = [0]
    for line in lines[:-1]:
        line_starts.append(line_starts[-1] + len(line) + 1)
    units = _cite_units(text, segs, line_starts)
    starts = [seg.start for seg in segs]
    test_lines = set()
    if not test_path and kind == '.rs' and '#[cfg(test)]' in text:
        test_lines = _cite_rust_test_lines(text, lines, segs, line_starts)

    def in_test(ln: int) -> bool:
        return test_path or ln in test_lines

    def ships(ln: int) -> bool:
        return shipped and not in_test(ln)

    findings = []
    for ln, match in _cite_cites(lines, ignored, 1, len(lines)):
        offset = line_starts[ln - 1] + match.start()
        k = bisect.bisect_right(starts, offset) - 1
        seg = segs[k] if k >= 0 and segs[k].end > offset else None
        cited = match.group(0)
        if ships(ln):
            findings.append((ln, match.start(), _PLACE_SHIPPED.format(cited=cited)))
        elif seg is not None and seg.kind == 'comment':
            unit = seg.unit
            if unit.refs is None or ln <= unit.refs:
                findings.append((ln, match.start(), _PLACE_BODY.format(cited=cited)))
            elif ln not in unit.covered:
                findings.append((ln, match.start(), _PLACE_OUTSIDE.format(cited=cited)))
        elif program and not in_test(ln):
            findings.append((ln, match.start(), _PLACE_SHIPPED.format(cited=cited)))
    findings += _cite_bare_findings(units, lines, ignored, skip=ships)
    return findings


def _cite_cites(lines: list, ignored, first: int, last: int):
    """(line, match) for each citation on lines first..last (1-based)."""
    for ln in range(first, last + 1):
        if ln in ignored:
            continue
        for match in CITE_RE.finditer(lines[ln - 1]):
            yield ln, match


# --- lines changed since a ref ----------------------------------------------------

def _cite_git_path(raw: str):
    """A path from a diff's `+++ ` line: git's C-style quoting undone, the
    trailing tab git adds after a path with a space removed, and the `b/`
    prefix stripped. None for /dev/null."""
    raw = raw.rstrip('\t')
    if raw.startswith('"') and raw.endswith('"'):
        out, k, body = bytearray(), 0, raw[1:-1]
        escapes = {'n': 10, 't': 9, '"': 34, '\\': 92, 'a': 7, 'b': 8, 'f': 12, 'r': 13, 'v': 11}
        while k < len(body):
            ch = body[k]
            if ch == '\\' and k + 1 < len(body):
                nxt = body[k + 1]
                if nxt in '01234567' and re.match(r'[0-7]{3}', body[k + 1:k + 4]):
                    out.append(int(body[k + 1:k + 4], 8))
                    k += 4
                    continue
                out.append(escapes.get(nxt, ord(nxt)))
                k += 2
                continue
            out.extend(ch.encode('utf-8'))
            k += 1
        raw = out.decode('utf-8', errors='replace')
    return raw[2:] if raw.startswith('b/') else None


def _cite_changed_lines(root: Path, ref: str) -> tuple:
    """(changed, problem). `changed` maps each path to the lines added or
    changed since the merge base of ref and HEAD, or True for a whole new
    file: commits since the base, staged and unstaged edits, and untracked
    files. When git cannot say, `changed` is None and `problem` says why."""
    if _git(['rev-parse', '--verify', '--quiet', f'{ref}^{{commit}}'], root) is None:
        return None, f"--since {ref}: no such commit"
    base = _git(['merge-base', ref, 'HEAD'], root)
    if not base or not base.strip():
        return None, (f"--since {ref}: {ref} and HEAD have no merge base "
                      f"(a shallow clone may stop before it; fetch more history)")
    out = _git(['diff', '-U0', '--no-color', '--no-ext-diff', '-M', '--src-prefix=a/', '--dst-prefix=b/',
                base.strip(), '--'], root)
    if out is None:
        return None, f"--since {ref}: git diff against the merge base failed"
    untracked = _git(['ls-files', '--others', '--exclude-standard', '-z'], root)
    if untracked is None:
        return None, f"--since {ref}: git ls-files failed to list untracked files"
    changed, current = {}, None
    for line in out.split('\n'):
        if line.startswith('+++ '):
            current = _cite_git_path(line[4:])
        elif line.startswith('@@') and current is not None:
            m = re.match(r'@@ -\S+ \+(\d+)(?:,(\d+))? @@', line)
            if m:
                start, count = int(m.group(1)), int(m.group(2) or 1)
                changed.setdefault(current, set()).update(range(start, start + count))
    for path in untracked.split('\0'):
        if path:
            changed[path] = True
    return changed, None
