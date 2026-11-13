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

# What an entry may hold besides its citations without saying anything:
# section markers such as D3 or §2.1, then punctuation and space.
_CITE_SECTION_MARK = re.compile(r"\bD\d+\b|§\s*\d+(?:\.\d+)*")
_CITE_PUNCT = re.compile(r"[\W_]+")
_CITE_MD_HEADING = re.compile(r"^ {0,3}(#{1,6})(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*$")
_CITE_MD_FENCE = re.compile(r"^ {0,3}(`{3,}|~{3,})")


class _CiteUnit:
    """One comment: a run of whole-line comments with the same marker, a
    block comment or docstring, or a comment trailing code. `refs` is the line
    of its `References:` line, or None; `entries` are its references entries
    as (first line, last line, text)."""
    __slots__ = ('lines', 'refs', 'entries')

    def __init__(self):
        self.lines, self.refs, self.entries = [], None, []


# --- comments and their references blocks ------------------------------------

def _cite_comment_text(text: str, seg) -> list:
    """The comment's text, one string per line, with its markers removed."""
    raw = text[seg.start:seg.end]
    if seg.marker != 'block':
        body = raw[len(seg.marker):]
        return [body.lstrip('#') if seg.marker == '#' else body]
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


def _cite_entries(numbered: list) -> list:
    """References entries from (line, text) pairs after a References line:
    an entry starts at a `- ` line and runs to the next one. A text of None
    ends the entry before it, as a Markdown blank line or heading does."""
    entries, open_entry = [], False
    for ln, body in numbered:
        if body is None:
            open_entry = False
            continue
        stripped = body.strip()
        if stripped == '-' or stripped.startswith('- '):
            entries.append([ln, ln, stripped[1:]])
            open_entry = True
        elif open_entry:
            entries[-1][1] = ln
            entries[-1][2] += ' ' + stripped
    return [tuple(e) for e in entries]


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
        unit.lines.extend((first + k, body) for k, body in enumerate(lines))
        seg.unit = unit
        end_line = first + len(lines) - 1
        last = (unit, seg.marker, end_line) if seg.marker != 'block' and whole_line else None
    for unit in units:
        for idx, (ln, body) in enumerate(unit.lines):
            if body.strip() == 'References:':
                unit.refs = ln
                unit.entries = _cite_entries(unit.lines[idx + 1:])
                break
    return units


def _cite_bare_entry(text: str) -> bool:
    """True when an entry holds only its citations, section markers and
    punctuation."""
    return not _CITE_PUNCT.sub('', _CITE_SECTION_MARK.sub('', CITE_RE.sub('', text)))


# --- per-file placement ----------------------------------------------------------

def _cite_is_test_path(name: str) -> bool:
    """Test code by its path: under a tests/ directory, or named as a test."""
    parts = name.split('/')
    base = parts[-1]
    return ('tests' in parts[:-1] or fnmatch.fnmatch(base, '*_test.*')
            or fnmatch.fnmatch(base, 'test_*.py') or fnmatch.fnmatch(base, '*.test.*'))


def _cite_cites(lines: list, ignored, first: int, last: int):
    """(line, match) for each citation on lines first..last (1-based)."""
    for ln in range(first, last + 1):
        if ln in ignored:
            continue
        for match in CITE_RE.finditer(lines[ln - 1]):
            yield ln, match


def _cite_bare_findings(units: list, lines: list, ignored) -> list:
    """An entry that names records and says nothing else."""
    findings = []
    for unit in units:
        for first, last, body in unit.entries:
            cites = list(_cite_cites(lines, ignored, first, last))
            if cites and _cite_bare_entry(body):
                ln, match = cites[0]
                findings.append((ln, match.start(), _PLACE_BARE.format(cited=match.group(0))))
    return findings


def _cite_place_markdown(lines: list, ignored, shipped: bool) -> list:
    """A Markdown page cites in its frontmatter or under a References
    heading; a shipped page cites only in its frontmatter."""
    front_end = 0
    if lines and lines[0].rstrip('\r') == '---':
        for idx in range(1, len(lines)):
            if lines[idx].rstrip('\r') in ('---', '...'):
                front_end = idx + 1
                break
    findings, fence, ref_level = [], None, None
    unit = _CiteUnit()
    for idx in range(front_end, len(lines)):
        ln, line = idx + 1, lines[idx].rstrip('\r')
        in_refs = ref_level is not None
        if fence is not None:
            m = _CITE_MD_FENCE.match(line)
            if m and m.group(1)[0] == fence[0] and len(m.group(1)) >= len(fence) and not line.strip(' `~'):
                fence = None
        elif _CITE_MD_FENCE.match(line):
            fence = _CITE_MD_FENCE.match(line).group(1)
        else:
            m = _CITE_MD_HEADING.match(line)
            if m:
                level, heading = len(m.group(1)), (m.group(2) or '').strip()
                if ref_level is not None and level <= ref_level:
                    ref_level = None
                if heading == 'References' and ref_level is None:
                    ref_level = level
                in_refs = ref_level is not None and heading != 'References'
                if in_refs:
                    unit.lines.append((ln, None))
                    continue
        if in_refs:
            unit.lines.append((ln, line if line.strip() else None))
        if ln in ignored:
            continue
        for match in CITE_RE.finditer(line):
            if shipped:
                findings.append((ln, match.start(), _PLACE_SHIPPED.format(cited=match.group(0))))
            elif not in_refs:
                findings.append((ln, match.start(), _PLACE_PAGE.format(cited=match.group(0))))
    if not shipped:
        unit.entries = _cite_entries(unit.lines)
        findings += _cite_bare_findings([unit], lines, ignored)
    return findings


def _cite_placement(name: str, text: str, lines: list, ignored, shipped: bool) -> list:
    """Placement findings for one file, as (line, column, message).

    A citation in a comment follows the comment rule: it belongs in the
    comment's references block, whose entries each say what the record
    decides. In a program source, a citation in a string or in code reaches
    the built program, unless the code is test code; in a shipped path, every
    citation in a program source does. Values in data files are left alone.
    """
    suffix = Path(name).suffix
    if suffix == '.md':
        return _cite_place_markdown(lines, ignored, shipped)
    lexer = _CITE_LEXERS.get(suffix)
    if lexer is None:
        return []
    program = suffix in CITE_PROGRAM_EXTS
    segs = lexer(text)
    line_starts = [0]
    for line in lines[:-1]:
        line_starts.append(line_starts[-1] + len(line) + 1)
    units = _cite_units(text, segs, line_starts)
    starts = [seg.start for seg in segs]

    test_from = None
    if _cite_is_test_path(name):
        test_from = 1
    elif suffix == '.rs':
        test_from = next((k + 2 for k, line in enumerate(lines) if line.strip() == '#[cfg(test)]'), None)
    ships = program and shipped and test_from != 1

    findings = []
    for ln, match in _cite_cites(lines, ignored, 1, len(lines)):
        offset = line_starts[ln - 1] + match.start()
        k = bisect.bisect_right(starts, offset) - 1
        seg = segs[k] if k >= 0 and segs[k].end > offset else None
        cited = match.group(0)
        if seg is not None and seg.kind == 'comment':
            if ships:
                findings.append((ln, match.start(), _PLACE_SHIPPED.format(cited=cited)))
            elif seg.unit.refs is None or ln <= seg.unit.refs:
                findings.append((ln, match.start(), _PLACE_BODY.format(cited=cited)))
        elif program and (test_from is None or ln < test_from):
            findings.append((ln, match.start(), _PLACE_SHIPPED.format(cited=cited)))
    if not ships:
        findings += _cite_bare_findings(units, lines, ignored)
    return findings


# --- lines changed since a ref ----------------------------------------------------

def _cite_changed_lines(root: Path, ref: str):
    """The lines added or changed since the merge base of ref and HEAD, as
    {path: set of lines, or True for a whole new file}: commits since the base,
    staged and unstaged edits, and untracked files. None when the merge base
    cannot be found."""
    base = _git(['merge-base', ref, 'HEAD'], root)
    if not base or not base.strip():
        return None
    out = _git(['diff', '-U0', '--no-color', '--no-ext-diff', '-M', '--src-prefix=a/', '--dst-prefix=b/',
                base.strip(), '--'], root)
    if out is None:
        return None
    changed, current = {}, None
    for line in out.split('\n'):
        if line.startswith('+++ '):
            path = line[4:].strip('"')
            current = path[2:] if path.startswith('b/') else None
        elif line.startswith('@@') and current is not None:
            m = re.match(r'@@ -\S+ \+(\d+)(?:,(\d+))? @@', line)
            if m:
                start, count = int(m.group(1)), int(m.group(2) or 1)
                changed.setdefault(current, set()).update(range(start, start + count))
    untracked = _git(['ls-files', '--others', '--exclude-standard', '-z'], root) or ''
    for path in untracked.split('\0'):
        if path:
            changed[path] = True
    return changed
