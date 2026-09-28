# ============================================================================
# Path rewriting for records that change folder (ADR-306 §6)
# ============================================================================
#
# A record's number is its identity and never changes. Under adr/v1 its
# folder decides its domain, so moving a record to another domain moves its
# file, and a domain renamed in place moves its folder. Either way every path
# to what moved is rewritten: markdown links, paths written from the repo
# root, and paths inside URLs. ADR-N citations are left alone: the number
# still names the same record.

# Frontmatter keys that record history and are never rewritten: an imported
# record keeps the path it was imported from.
RELOCATE_HISTORY_KEYS = ('imported',)

_PATH_WORD_RE = re.compile(r'[\w./-]+')
_FM_KEY_RE = re.compile(r'([A-Za-z_][\w-]*)\s*:')


class Relocation:
    """One simultaneous move: files (repo-relative old path -> new path),
    directories (old dir -> new dir), and optionally a domain key rename for
    catalog frontmatter (`domain: <key>`). `known` holds every tracked path
    and directory, so a relative path is rewritten only when it names one."""

    def __init__(self, files=None, dirs=None, known=None, domain=None):
        self.files = dict(files or {})
        self.dirs = dict(dirs or {})
        self.known = set(known or ())
        self.domain = domain  # (old key, new key) or None
        self.basenames = {posixpath.basename(p) for p in self.files}
        self._maps = ([(k.split('/'), v.split('/'), True) for k, v in self.files.items()]
                      + [(k.split('/'), v.split('/'), False) for k, v in self.dirs.items()])

    def target(self, rel: str) -> Optional[str]:
        """Where a repo-relative path is after the move, or None if it stays."""
        if rel in self.files:
            return self.files[rel]
        for old, new in self.dirs.items():
            if rel == old or rel.startswith(old + '/'):
                return new + rel[len(old):]
        return None

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
                moved = self.target(resolved)
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
        # A path written from the repo root, or inside a URL: the longest run
        # of its segments that ends a moved path. A directory needs two
        # segments (architecture/system), so a bare folder name in prose is
        # never taken for a path; a moved file matches on its name alone.
        parts = core.split('/')
        best = None
        for old, new, is_file in self._maps:
            ends = [len(parts)] if is_file else range(len(parts), 0, -1)
            for end in ends:
                for k in range(min(end, len(old)), 0, -1):
                    if k < 2 and not is_file:
                        break
                    if parts[end - k:end] == old[-k:]:
                        if best is None or k > best[0]:
                            best = (k, end, new)
                        break
        if best is None:
            return None
        k, end, new = best
        out = '/'.join(parts[:end - k] + (new[-k:] if len(new) >= k else new) + parts[end:])
        return (out + ('/' if slash else '')) if out != core else None

    def word(self, token: str, old_dir: str, new_dir: str) -> tuple:
        """One run of path characters rewritten; (text, count)."""
        stripped = token.rstrip('.')  # sentence punctuation is not the path
        if not stripped:
            return token, 0
        if '/' not in stripped and stripped not in self.basenames:
            # A bare sibling name is a path only when the file holding it
            # moved and the name resolves to a tracked file: the link must be
            # re-based even though its target stays put.
            sibling = posixpath.normpath(posixpath.join(old_dir, stripped))
            if old_dir == new_dir or sibling not in self.known:
                return token, 0
        new = self._path(stripped, old_dir, new_dir)
        if new is None:
            return token, 0
        return new + token[len(stripped):], 1

    def text(self, content: str, rel: str) -> tuple:
        """content of the file at rel with its paths rewritten; (text, count).
        History keys in frontmatter are left alone, and a frontmatter
        `domain:` follows a domain rename."""
        old_dir = posixpath.dirname(rel)
        moved = self.target(rel)
        new_dir = posixpath.dirname(moved) if moved else old_dir
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


def canonical_paths(text: str) -> str:
    """Every path in text cut to its last segment, so two versions of a body
    compare equal when only the folders their links run through differ."""
    def last(m):
        token = m.group(0)
        if '/' not in token:
            return token
        segments = [s for s in token.split('/') if s]
        return segments[-1] if segments else token
    return _PATH_WORD_RE.sub(last, text)
