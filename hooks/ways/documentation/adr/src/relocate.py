# ============================================================================
# Path rewriting for records that change folder (ADR-306 §6)
# ============================================================================
#
# A record's number is its identity and never changes. Under adr/v1 its
# folder decides its domain, so moving a record to another domain moves its
# file, and a domain renamed in place moves its folder. Either way every path
# to what moved is rewritten. A path is a relative link that resolves to what
# moved, a path written from the repo root that names it, or a URL into this
# repository's origin that names it. Other URLs, and words that only contain
# a folder's name, are left alone. ADR-N citations are left alone too: the
# number still names the same record.
#
# The frozen check (rules_v1_integrity.py) reads the same rewrite from git
# history: a frozen record may differ from its accepted version by exactly
# the rewrite of paths from where records were to where they are now.

# Frontmatter keys that record history and are never rewritten: an imported
# record keeps the path it was imported from.
RELOCATE_HISTORY_KEYS = ('imported',)

_PATH_WORD_RE = re.compile(r'[A-Za-z][\w+.-]*://[\w./~%-]+|[\w./-]+')
_REPO_URL_RE = re.compile(r'[A-Za-z][\w+.-]*://([^/]+)/([^/]+)/([^/]+)/(?:blob|tree|raw)/[^/]+/(.+)')
_FM_KEY_RE = re.compile(r'([A-Za-z_][\w-]*)\s*:')
_RECORD_FILE_RE = re.compile(r'ADR-\d+(?:\.\d+)?-.*\.md')


class Relocation:
    """One simultaneous move: files (repo-relative old path -> new path),
    directories (old dir -> new dir), and optionally a domain key rename for
    catalog frontmatter (`domain: <key>`). `known` holds tracked paths and
    directories: a path is rewritten only when it names one, or names a
    moved file. `repos` holds this repository's `host/owner/repo`, lowercase,
    so a URL into it is rewritten like a path from the root."""

    def __init__(self, files=None, dirs=None, known=None, domain=None, repos=()):
        self.files = dict(files or {})
        self.dirs = dict(dirs or {})
        self.known = set(known or ())
        self.domain = domain  # (old key, new key) or None
        self.repos = set(repos)
        self.basenames = {posixpath.basename(p) for p in self.files}

    def target(self, rel: str) -> Optional[str]:
        """Where a repo-relative path is after the move, or None if it stays."""
        if rel in self.files:
            return self.files[rel]
        for old, new in self.dirs.items():
            if rel == old or rel.startswith(old + '/'):
                return new + rel[len(old):]
        return None

    def _moved(self, rel: str) -> Optional[str]:
        """target(rel) when rel names a real path that moved: a moved file, or
        a path under a moved folder that is tracked before or after the move."""
        if rel in self.files:
            return self.files[rel]
        moved = self.target(rel)
        if moved is not None and (rel in self.known or moved in self.known):
            return moved
        return None

    def _rooted(self, core: str) -> Optional[str]:
        """core read as a path from the repo root, where it names something
        that moved; None otherwise."""
        lead = '/' if core.startswith('/') else ''
        rest = core[len(lead):]
        if not rest or posixpath.normpath(rest) != rest:
            return None
        moved = self._moved(rest)
        return lead + moved if moved is not None else None

    def _path(self, token: str, old_dir: str, new_dir: str) -> Optional[str]:
        """token rewritten as a path to something that moved, or None."""
        slash = token.endswith('/') and len(token) > 1
        core = token.rstrip('/') if slash else token
        if not core:
            return None
        # Relative to the file that holds it, resolved from where that file
        # was. A file that moves re-bases its links to paths that stay.
        if not core.startswith('/'):
            resolved = posixpath.normpath(posixpath.join(old_dir, core))
            if not resolved.startswith('..'):
                moved = self._moved(resolved)
                if moved is not None or (old_dir != new_dir and resolved in self.known):
                    dest = moved or resolved
                    if posixpath.normpath(posixpath.join(new_dir, core)) == dest:
                        return None
                    # Keep the link's shape where it still resolves: rename
                    # only the segments that moved (../system/X -> ../platform/X).
                    segments, walked = [], old_dir
                    for segment in core.split('/'):
                        walked = posixpath.normpath(posixpath.join(walked, segment)) if segment else walked
                        after = self.target(walked) if segment not in ('', '.', '..') else None
                        segments.append(posixpath.basename(after) if after else segment)
                    new = '/'.join(segments)
                    if posixpath.normpath(posixpath.join(new_dir, new)) != dest:
                        new = posixpath.relpath(dest, new_dir or '.')
                    if core.startswith('./') and not new.startswith('.'):
                        new = './' + new
                    return new + ('/' if slash else '')
                if resolved in self.known:
                    return None  # a real path that stays
        # A path written from the repo root names what moved in full.
        new = self._rooted(core)
        return (new + ('/' if slash else '')) if new is not None and new != core else None

    def _url(self, token: str) -> Optional[str]:
        """A URL into this repository, with the path it names rewritten."""
        m = _REPO_URL_RE.fullmatch(token)
        repo = '/'.join(m.group(1, 2, 3)).lower() if m else ''
        if repo.endswith('.git'):
            repo = repo[:-4]
        if not m or repo not in self.repos:
            return None
        path = m.group(4)
        slash = path.endswith('/')
        new = self._rooted(path.rstrip('/'))
        if new is None:
            return None
        return token[:m.start(4)] + new + ('/' if slash else '')

    def word(self, token: str, old_dir: str, new_dir: str) -> tuple:
        """One run of path characters rewritten; (text, count)."""
        stripped = token.rstrip('.')  # sentence punctuation is not the path
        if not stripped:
            return token, 0
        if '://' in stripped:
            new = self._url(stripped)
        else:
            if '/' not in stripped and stripped not in self.basenames:
                # A bare sibling name is a path only when the file holding it
                # moved and the name resolves to a tracked file: the link must
                # be re-based even though its target stays put.
                sibling = posixpath.normpath(posixpath.join(old_dir, stripped))
                if old_dir == new_dir or sibling not in self.known:
                    return token, 0
            new = self._path(stripped, old_dir, new_dir)
        if new is None:
            return token, 0
        return new + token[len(stripped):], 1

    def _dirs(self, rel: str) -> tuple:
        """(the folder of the file at rel, its folder after the move)"""
        moved = self.target(rel)
        return posixpath.dirname(rel), posixpath.dirname(moved) if moved else posixpath.dirname(rel)

    def text(self, content: str, rel: str) -> tuple:
        """content of the file at rel with its paths rewritten; (text, count).
        History keys in frontmatter are left alone, and a frontmatter
        `domain:` follows a domain rename."""
        old_dir, new_dir = self._dirs(rel)
        lines = content.split('\n')
        fm_end = None
        if lines and lines[0].rstrip('\r') == '---':
            fm_end = next((i for i in range(1, len(lines)) if lines[i].rstrip('\r') == '---'), None)
        total, key, out = 0, None, []

        def swap(m):
            nonlocal total
            new, n = self.word(m.group(0), old_dir, new_dir)
            total += n
            return new

        for i, line in enumerate(lines):
            if fm_end is not None and 0 < i < fm_end:
                m = _FM_KEY_RE.match(line)
                if m:
                    key = m.group(1)
                if key in RELOCATE_HISTORY_KEYS:
                    out.append(line)
                    continue
                if self.domain and m and key == 'domain':
                    dm = re.fullmatch(r'(domain:\s*)([\w-]+)(\s*(?:#.*)?)', line.rstrip('\r'))
                    if dm and dm.group(2) == self.domain[0]:
                        out.append(dm.group(1) + self.domain[1] + dm.group(3)
                                   + ('\r' if line.endswith('\r') else ''))
                        total += 1
                        continue
            out.append(_PATH_WORD_RE.sub(swap, line))
        return '\n'.join(out), total

    def value(self, value, rel: str):
        """A parsed frontmatter value of the file at rel, with the paths in
        its strings rewritten as text() rewrites them."""
        old_dir, new_dir = self._dirs(rel)
        if isinstance(value, str):
            return _PATH_WORD_RE.sub(lambda m: self.word(m.group(0), old_dir, new_dir)[0], value)
        if isinstance(value, list):
            return [self.value(v, rel) for v in value]
        if isinstance(value, dict):
            return {k: self.value(v, rel) for k, v in value.items()}
        return value


def tracked_paths(root: Path) -> tuple:
    """(every file git tracks, sorted; those files and their folders)"""
    out = _git(['ls-files', '-z'], root)
    tracked = sorted(n for n in (out or '').split('\0') if n)
    known = set(tracked)
    for name in tracked:
        parent = posixpath.dirname(name)
        while parent and parent not in known:
            known.add(parent)
            parent = posixpath.dirname(parent)
    return tracked, known


def origin_repos(root: Path) -> set:
    """{host/owner/repo} of the origin remote, lowercase; empty without one."""
    url = (_git(['remote', 'get-url', 'origin'], root) or '').strip()
    m = re.fullmatch(r'(?:[A-Za-z][\w+.-]*://)?(?:[^@/]+@)?([^/:]+)(?::\d+)?[:/]([^/]+/[^/]+?)(?:\.git)?/?', url)
    return {f"{m.group(1)}/{m.group(2)}".lower()} if m else set()


def _renames(output: Optional[str]) -> list:
    """[(old, new)] from `--name-status -z` output, in its order."""
    tokens = [t.lstrip('\n') for t in (output or '').split('\0')]
    pairs, i = [], 0
    while i < len(tokens):
        if re.fullmatch(r'R\d*', tokens[i]) and i + 2 < len(tokens):
            pairs.append((tokens[i + 1], tokens[i + 2]))
            i += 3
        else:
            i += 1
    return pairs


_HISTORY_RELOCATIONS = {}

def history_relocation(root: Path) -> Relocation:
    """Every earlier path of a record file, mapped to where the file is now,
    read from the renames in HEAD's history and those staged since (git mv
    stages a rename). A folder that no longer exists maps to a new folder
    when every record renamed out of it in one commit went there. Rename
    detection only: a copy is a new file."""
    key = str(root)
    if key in _HISTORY_RELOCATIONS:
        return _HISTORY_RELOCATIONS[key]
    tracked, known = tracked_paths(root)
    log = _git(['log', 'HEAD', '-M', '--diff-filter=R', '--name-status', '-z', '--format=%x01'], root) or ''
    commits = [_renames(chunk) for chunk in reversed(log.split('\x01'))]
    commits.append(_renames(_git(['diff', 'HEAD', '-M', '--diff-filter=R', '--name-status', '-z'], root)))
    where, dirs = {}, {}
    for renames in commits:
        went = {}
        for old, new in renames:
            for earlier, now in where.items():
                if now == old:
                    where[earlier] = new
            where[old] = new
            if _RECORD_FILE_RE.fullmatch(posixpath.basename(new)) \
                    and posixpath.basename(old) == posixpath.basename(new):
                went.setdefault(posixpath.dirname(old), set()).add(posixpath.dirname(new))
        for old_dir, new_dirs in went.items():
            if len(new_dirs) == 1 and old_dir not in known:
                dirs[old_dir] = next(iter(new_dirs))
    # A folder renamed twice maps to where it went last.
    for old_dir in dirs:
        seen, new_dir = {old_dir}, dirs[old_dir]
        while new_dir in dirs and new_dir not in seen:
            seen.add(new_dir)
            new_dir = dirs[new_dir]
        dirs[old_dir] = new_dir
    files = {old: new for old, new in where.items()
             if old != new and _RECORD_FILE_RE.fullmatch(posixpath.basename(new))}
    relocation = Relocation(files=files, dirs=dirs, known=known, repos=origin_repos(root))
    _HISTORY_RELOCATIONS[key] = relocation
    return relocation
