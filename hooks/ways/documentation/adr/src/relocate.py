# ============================================================================
# Path rewriting for records that change folder (ADR-306 §6)
# ============================================================================
#
# A record's number is its identity and never changes (ADR-310). Under adr/v1
# its folder decides its domain, so moving a record to another domain moves
# its file, and a domain renamed in place moves its folder. Either way every
# path to what moved is rewritten. A path is a relative link that resolves to
# what moved, a path written from the repo root that names it, or a URL into
# this repository at a branch that names it. Other URLs, a URL at a commit or
# a tag (a permalink), a path inside a fenced code block, a path written with
# backslashes, and words that only contain a folder's name are left alone.
# ADR-N citations are left alone too: the number still names the same record.
#
# The frozen check (rules_v1_integrity.py) reads references the same way:
# before it compares a frozen record with its accepted version, each
# reference that resolves to a record becomes that record's number, so a
# path rewritten after a move compares equal and a link repointed at another
# record does not.

# Frontmatter keys that record history and are never rewritten: an imported
# record keeps the path it was imported from.
RELOCATE_HISTORY_KEYS = ('imported',)

_PATH_WORD_RE = re.compile(r'[A-Za-z][\w+.-]*://[\w./~%-]+|[\w./-]+')
# host/owner/repo/blob|tree|raw/<ref>/<path>, and GitHub's raw host, where
# the ref follows the repo directly. A ref may hold slashes.
_REPO_URL_RE = re.compile(r'[A-Za-z][\w+.-]*://([^/]+)/([^/]+)/([^/]+)/(?:blob|tree|raw)/(.+)')
_RAW_URL_RE = re.compile(r'[A-Za-z][\w+.-]*://raw\.githubusercontent\.com/([^/]+)/([^/]+)/(.+)', re.IGNORECASE)
_COMMIT_RE = re.compile(r'[0-9a-f]{7,40}')
_FM_KEY_RE = re.compile(r'([A-Za-z_][\w-]*)\s*:')
_RECORD_FILE_RE = re.compile(r'ADR-(\d+)((?:\.\d+)?)-.*\.md')
_FENCE_RE = re.compile(r' {0,3}(`{3,}|~{3,})')


def record_number(rel: str) -> Optional[str]:
    """The number a record file's name carries (101, 101.1), or None when
    the name is not ADR-NNN-<slug>.md."""
    m = _RECORD_FILE_RE.fullmatch(posixpath.basename(rel))
    return f"{int(m.group(1))}{m.group(2)}" if m else None


def _repo_name(url: str) -> Optional[str]:
    """host/owner/repo, lowercase, from a remote URL or a written name."""
    m = re.fullmatch(r'(?:[A-Za-z][\w+.-]*://)?(?:[^@/]+@)?([^/:]+)(?::\d+)?[:/]([^/]+/[^/]+?)(?:\.git)?/?',
                     url.strip())
    return f"{m.group(1)}/{m.group(2)}".lower() if m else None


def project_repos(root: Path) -> set:
    """The names this repository goes by, host/owner/repo, lowercase:
    adr.yaml's `repository:` (a name or a list of names) when it is set,
    otherwise the origin remote. Empty without either."""
    configured = get_config().get('repository')
    if configured:
        values = [configured] if isinstance(configured, str) else configured if isinstance(configured, list) else []
        return {n for n in (_repo_name(str(v)) for v in values) if n}
    name = _repo_name((_git(['remote', 'get-url', 'origin'], root) or '').strip())
    return {name} if name else set()


class Repository:
    """This repository as URLs name it: its names, and its tags and branches,
    each read from git once when first needed. A URL into it at a branch
    names a path in the working tree. At a commit or a tag it is a
    permalink to a snapshot, and names nothing that moves."""

    def __init__(self, root: Path):
        self.root = root
        self.names = project_repos(root)
        self._tags = self._branches = None

    @property
    def tags(self) -> set:
        if self._tags is None:
            self._tags = set((_git(['tag', '-l'], self.root) or '').split())
        return self._tags

    @property
    def branches(self) -> set:
        """Local branches, and remote-tracking ones by their branch name."""
        if self._branches is None:
            out = _git(['for-each-ref', '--format=%(refname)', 'refs/heads', 'refs/remotes'], self.root) or ''
            names = set()
            for ref in out.split():
                if ref.startswith('refs/heads/'):
                    names.add(ref[len('refs/heads/'):])
                elif ref.startswith('refs/remotes/') and ref.count('/') >= 3:
                    names.add(ref.split('/', 3)[3])
            self._branches = names
        return self._branches

    def path(self, token: str) -> Optional[tuple]:
        """(where the path starts in token, the path from the repo root) for
        a URL into this repository at a branch; None otherwise. A ref with a
        slash is read as the longest leading run of segments that names a
        known branch; otherwise the ref is the first segment."""
        m = _RAW_URL_RE.fullmatch(token)
        if m:
            repo, rest, at = f"github.com/{m.group(1)}/{m.group(2)}", m.group(3), m.start(3)
        else:
            m = _REPO_URL_RE.fullmatch(token)
            if not m:
                return None
            repo, rest, at = '/'.join(m.group(1, 2, 3)), m.group(4), m.start(4)
        repo = repo.lower()
        if repo.endswith('.git'):
            repo = repo[:-4]
        if repo not in self.names:
            return None
        segments = rest.split('/')
        if len(segments) < 2:
            return None
        width = next((n for n in range(len(segments) - 1, 1, -1)
                      if '/'.join(segments[:n]) in self.branches), 1)
        ref = '/'.join(segments[:width])
        if width == 1 and ref not in self.branches and (_COMMIT_RE.fullmatch(ref) or ref in self.tags):
            return None
        path = '/'.join(segments[width:])
        return (at + len(ref) + 1, path) if path else None


def _fenced(lines: list, start: int = 0) -> set:
    """Indexes of the lines from start on that sit in a fenced code block,
    fence lines included."""
    inside, fence = set(), None
    for i in range(start, len(lines)):
        m = _FENCE_RE.match(lines[i])
        if fence is None:
            if m:
                fence = m.group(1)
                inside.add(i)
        else:
            inside.add(i)
            if m and m.group(1)[0] == fence[0] and len(m.group(1)) >= len(fence) \
                    and not lines[i][m.end():].strip():
                fence = None
    return inside


def _sub_paths(line: str, swap) -> str:
    """line with each run of path characters replaced by swap(token), or
    left alone when it touches a backslash: a Windows path is not a
    path this tool resolves."""
    def one(m):
        before = m.string[m.start() - 1:m.start()]
        after = m.string[m.end():m.end() + 1]
        if before == '\\' or after == '\\':
            return m.group(0)
        return swap(m.group(0))
    return _PATH_WORD_RE.sub(one, line)


class Relocation:
    """One simultaneous move: files (repo-relative old path -> new path),
    directories (old dir -> new dir), and optionally a domain key rename for
    catalog frontmatter (`domain: <key>`). `known` holds tracked paths and
    directories: a path is rewritten only when it names one, or names a
    moved file. `repo` (a Repository) says which URLs point into this
    repository at a branch; such a URL is rewritten like a path from the
    root."""

    def __init__(self, files=None, dirs=None, known=None, domain=None, repo=None):
        self.files = dict(files or {})
        self.dirs = dict(dirs or {})
        self.known = set(known or ())
        self.domain = domain  # (old key, new key) or None
        self.repo = repo
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
        """A URL into this repository at a branch, with the path it names
        rewritten."""
        found = self.repo.path(token) if self.repo else None
        if found is None:
            return None
        at, path = found
        slash = path.endswith('/')
        new = self._rooted(path.rstrip('/'))
        if new is None:
            return None
        return token[:at] + new + ('/' if slash else '')

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

    def text(self, content: str, rel: str) -> tuple:
        """content of the file at rel with its paths rewritten; (text, count).
        History keys in frontmatter are left alone, a frontmatter `domain:`
        follows a domain rename, and in a Markdown file a fenced code block
        is left as written: it quotes text, such as a command, rather than
        linking to a file."""
        moved = self.target(rel)
        old_dir = posixpath.dirname(rel)
        new_dir = posixpath.dirname(moved) if moved else old_dir
        lines = content.split('\n')
        fm_end = None
        if lines and lines[0].rstrip('\r') == '---':
            fm_end = next((i for i in range(1, len(lines)) if lines[i].rstrip('\r') == '---'), None)
        fenced = _fenced(lines, fm_end + 1 if fm_end is not None else 0) if rel.lower().endswith('.md') else set()
        total, key, out = 0, None, []

        def swap(token):
            nonlocal total
            new, n = self.word(token, old_dir, new_dir)
            total += n
            return new

        for i, line in enumerate(lines):
            if i in fenced:
                out.append(line)
                continue
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
            out.append(_sub_paths(line, swap))
        return '\n'.join(out), total


def former_paths() -> dict:
    """adr.yaml's `former_paths:`, {repo-relative path: record number}: the
    paths of files that became records without carrying a number before,
    such as design notes brought into the corpus. Malformed entries are
    left out; lint reports them."""
    configured = get_config().get('former_paths')
    paths = {}
    if not isinstance(configured, dict):
        return paths
    for record, values in configured.items():
        m = re.fullmatch(r'(?:ADR-)?(\d+)((?:\.\d+)?)', str(record))
        values = [values] if isinstance(values, str) else values if isinstance(values, list) else []
        if m:
            for value in values:
                if isinstance(value, str) and value.strip():
                    paths[posixpath.normpath(value.strip().lstrip('/'))] = f"{int(m.group(1))}{m.group(2)}"
    return paths


class RecordRefs:
    """References read as the record they resolve to (ADR-310: a record's
    number is its identity). `records` maps repo-relative paths to record
    numbers; a reference resolves when it names one of them as a relative
    path from the file that holds it, a path from the repo root, or a URL
    into this repository at a branch. Each becomes a token carrying only the
    number, so the same record named from another folder reads the same.
    Everything else, fenced code included, is left as written."""

    def __init__(self, records: dict, repo: Optional['Repository']):
        self.records = records
        self.repo = repo

    def _word(self, token: str, holder_dir: str) -> str:
        stripped = token.rstrip('.')
        candidates = []
        if '://' in stripped:
            found = self.repo.path(stripped) if self.repo else None
            if found:
                candidates.append(found[1].rstrip('/').lstrip('/'))
        elif stripped:
            if not stripped.startswith('/'):
                candidates.append(posixpath.normpath(posixpath.join(holder_dir, stripped)))
            rest = stripped.lstrip('/')
            if rest and posixpath.normpath(rest) == rest:
                candidates.append(rest)
        for path in candidates:
            if path in self.records:
                return f"\0ADR-{self.records[path]}\0" + token[len(stripped):]
        return token

    def text(self, content: str, rel: str) -> str:
        """content, the body of the record at rel, with its references read."""
        holder = posixpath.dirname(rel)
        lines = content.split('\n')
        fenced = _fenced(lines)
        return '\n'.join(line if i in fenced else _sub_paths(line, lambda t: self._word(t, holder))
                         for i, line in enumerate(lines))

    def value(self, value, rel: str):
        """A parsed frontmatter value of the record at rel, with the
        references in its strings read."""
        if isinstance(value, str):
            holder = posixpath.dirname(rel)
            return _sub_paths(value, lambda t: self._word(t, holder))
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
