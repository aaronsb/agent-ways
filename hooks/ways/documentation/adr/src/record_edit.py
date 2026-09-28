
# --- surgical frontmatter edits -----------------------------------------------------
#
# The record commands (consider, set, supersede, enact) change one field at a
# time. They rewrite only the lines of that field and leave every other byte of
# the file as it was: the other fields' YAML style, comments, line endings and
# the body. A new field is written in the tool's own style (_RecordDumper) at
# its place in V1_KEY_ORDER. A field keeps its layout where it can: a one-line
# field stays on one line, and a block list gains or loses item lines. Every
# edit is read back before it is returned, so a mismatch refuses the edit
# rather than writing something else.

_TOP_KEY = re.compile(r'^([A-Za-z_][A-Za-z0-9_-]*)\s*:(?:\s|$)')

class _Quoted(str):
    """A string written double-quoted, as the corpus writes `said` and a
    commit hash. It reads back as a plain str."""

_RecordDumper.add_representer(
    _Quoted, lambda dumper, value: dumper.represent_scalar('tag:yaml.org,2002:str', str(value), style='"'))

class _FlowList(list):
    """A list written on one line, as the corpus writes a considered
    entry's covers. It reads back as a plain list."""

_RecordDumper.add_representer(
    _FlowList, lambda dumper, value: dumper.represent_sequence('tag:yaml.org,2002:seq', list(value), flow_style=True))

def _prefer_double(value: str):
    """value double-quoted where YAML would single-quote it, since the
    corpus quotes with double quotes; plain where it needs no quotes."""
    return _Quoted(value) if _flow(value).startswith("'") else value

def _flow(value) -> str:
    return yaml.dump(value, Dumper=_RecordDumper, default_flow_style=True, sort_keys=False,
                     allow_unicode=True, width=100000).rstrip('\n').removesuffix('\n...').rstrip('\n')

def _block(key: str, value) -> list:
    text = yaml.dump({key: value}, Dumper=_RecordDumper, sort_keys=False, allow_unicode=True,
                     default_flow_style=False, width=100000)
    return text.rstrip('\n').split('\n')

def _plain(value):
    """value with _Quoted strings turned back into str, for comparison."""
    if isinstance(value, list):
        return [_plain(v) for v in value]
    if isinstance(value, dict):
        return {k: _plain(v) for k, v in value.items()}
    return str(value) if isinstance(value, _Quoted) else value

class FrontmatterEdit:
    """One record's file, split into its frontmatter lines and the rest."""

    def __init__(self, raw: bytes):
        text = raw.decode('utf-8')
        lines = text.split('\n')
        if not lines or lines[0].rstrip('\r').strip() != '---':
            raise ValueError("the file has no YAML frontmatter")
        end = next((i for i, line in enumerate(lines[1:], 1) if line.strip() == '---'), None)
        if end is None:
            raise ValueError("the frontmatter has no closing ---")
        self.crlf = lines[0].endswith('\r')
        self.head = lines[:1]
        self.tail = lines[end:]
        self.lines = [line[:-1] if self.crlf and line.endswith('\r') else line for line in lines[1:end]]
        self.fields = self._load()

    def _load(self) -> dict:
        data = yaml.safe_load('\n'.join(self.lines)) or {}
        if not isinstance(data, dict):
            raise ValueError("the frontmatter is not a mapping of fields")
        return data

    def bytes(self) -> bytes:
        body = [line + '\r' for line in self.lines] if self.crlf else list(self.lines)
        return '\n'.join(self.head + body + self.tail).encode('utf-8')

    def _blocks(self) -> list:
        """(key, first line, last line) of each top-level field. A field's
        block ends at its last indented line: a comment or blank line at
        column 0 before the next key stays outside it."""
        blocks = []
        for i, line in enumerate(self.lines):
            match = _TOP_KEY.match(line)
            if match:
                blocks.append([match.group(1), i, i])
            elif blocks and line[:1] in (' ', '\t') and line.strip():
                blocks[-1][2] = i
            elif blocks and line.strip() and not line.startswith('#'):
                blocks[-1][2] = i  # a continuation at column 0, such as a flow list's rest
        return [tuple(b) for b in blocks]

    def _block(self, key: str) -> Optional[tuple]:
        found = [b for b in self._blocks() if b[0] == key]
        if len(found) > 1:
            raise ValueError(f"'{key}' appears {len(found)} times in the frontmatter")
        return found[0] if found else None

    def _replace(self, start: int, end: int, new_lines: list) -> None:
        self.lines[start:end + 1] = new_lines

    def _insert_at(self, key: str) -> int:
        """The line a new field goes in at: after the nearest field before it in
        V1_KEY_ORDER, else before the nearest field after it, else at the end."""
        blocks = self._blocks()
        if key in V1_KEY_ORDER:
            rank = V1_KEY_ORDER.index(key)
            before = [b for b in blocks if b[0] in V1_KEY_ORDER and V1_KEY_ORDER.index(b[0]) < rank]
            if before:
                return max(b[2] for b in before) + 1
            after = [b for b in blocks if b[0] in V1_KEY_ORDER and V1_KEY_ORDER.index(b[0]) > rank]
            if after:
                return min(b[1] for b in after)
        return blocks[-1][2] + 1 if blocks else len(self.lines)

    def _one_line(self, key: str, value, old: str) -> str:
        """key: value on one line, keeping a trailing comment of the old line."""
        text = _flow(value) if isinstance(value, (list, dict)) else _block(key, value)[0][len(key) + 2:]
        comment = re.search(r'\s+#[^\'"\]]*$', old)
        return f"{key}: {text}" + (comment.group(0) if comment else '')

    def _render(self, key: str, value, block: Optional[tuple]) -> list:
        """The lines for key: value. An existing one-line field stays on one
        line; a new field, or one written as a block, takes the tool's style."""
        if block is not None and block[1] == block[2]:
            one = self._one_line(key, value, self.lines[block[1]])
            if not (isinstance(value, list) and value and any(isinstance(v, dict) for v in value)):
                return [one]
        lines = _block(key, value)
        if block is not None and len(lines) > 1:
            lines = [lines[0]] + self._reindent(lines[1:], block)
        return lines

    def _item_indent(self, block: tuple) -> Optional[str]:
        for line in self.lines[block[1] + 1:block[2] + 1]:
            match = re.match(r'^(\s*)- ', line) or re.match(r'^(\s*)-$', line)
            if match:
                return match.group(1)
        return None

    def _reindent(self, item_lines: list, block: tuple) -> list:
        """Item lines rendered with the tool's two-space indent, moved to the
        indent the field's existing items use."""
        indent = self._item_indent(block)
        if indent is None or indent == '  ':
            return item_lines
        return [indent + line[2:] if line.startswith('  ') else line for line in item_lines]

    def _commit(self, expected: dict) -> None:
        if _plain(self._load()) != _plain(expected):
            raise ValueError("the edited frontmatter does not read back as intended")
        self.fields = self._load()

    # --- operations -------------------------------------------------------------

    def set(self, key: str, value) -> None:
        expected = dict(self.fields)
        expected[key] = value
        block = self._block(key)
        if block is None:
            at = self._insert_at(key)
            self.lines[at:at] = _block(key, value)
        else:
            self._replace(block[1], block[2], self._render(key, value, block))
        self._commit(expected)

    def append(self, key: str, items: list) -> None:
        """Append items to a list field, creating it when absent. A scalar
        becomes a list holding it first."""
        current = self.fields.get(key)
        before = [] if current in (None, '') else (list(current) if isinstance(current, list) else [current])
        value = before + list(items)
        block = self._block(key)
        indent = self._item_indent(block) if block is not None else None
        if block is None or indent is None or not isinstance(current, list) or not current:
            self.set(key, value)
            return
        expected = dict(self.fields)
        expected[key] = value
        new = self._reindent(_block(key, list(items))[1:], block)
        self.lines[block[2] + 1:block[2] + 1] = new
        self._commit(expected)

    def remove(self, key: str, items: list) -> list:
        """Remove each item from a list field. Returns the items not found."""
        current = self.fields.get(key)
        entries = current if isinstance(current, list) else ([] if current is None else [current])
        wanted = [str(_plain(i)) for i in items]
        missing = [i for i, w in zip(items, wanted) if w not in [str(e) for e in entries]]
        if missing:
            return missing
        value = [e for e in entries if str(e) not in wanted]
        block = self._block(key)
        indent = self._item_indent(block) if block is not None else None
        if indent is None or not value:
            self.set(key, value)
            return []
        expected = dict(self.fields)
        expected[key] = value
        starts = [i for i in range(block[1] + 1, block[2] + 1)
                  if re.match(re.escape(indent) + r'-(\s|$)', self.lines[i])]
        chunks = [(s, (starts[n + 1] - 1) if n + 1 < len(starts) else block[2]) for n, s in enumerate(starts)]
        for start, end in reversed(chunks):
            text = '\n'.join(line[len(indent):] for line in self.lines[start:end + 1])
            loaded = yaml.safe_load(text)
            if isinstance(loaded, list) and len(loaded) == 1 and str(loaded[0]) in wanted:
                del self.lines[start:end + 1]
        self._commit(expected)
        return []
