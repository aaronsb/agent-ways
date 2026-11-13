# ============================================================================
# Citation placement: lexers that mark comments and strings
# ============================================================================
#
# One small lexer per language the placement check reads. Each returns the
# file's comments and strings as segments, by character offset; text in no
# segment is code. They are line based and approximate: a regex literal, a
# quote inside a shell command substitution or a multi-line YAML string can
# be misread, and a fixture is the place to pin a case that matters.

_CITE_IDENT = re.compile(r"[A-Za-z0-9_]+")
_CITE_HEREDOC = re.compile(r"<<(-?)[ \t]*(['\"]?)([A-Za-z_][A-Za-z0-9_]*)\2")
_CITE_YAML_BLOCK = re.compile(r"(?::|^\s*-|^)\s*[|>][-+1-9]*\s*$")


class _CiteSeg:
    """A comment or string in a file, by character offset. `marker` is a
    line comment's marker (`//`, `///`, `#`, `--`), or 'block' for a block
    comment or a Python docstring."""
    __slots__ = ('kind', 'start', 'end', 'marker', 'unit')

    def __init__(self, kind, start, end, marker=None):
        self.kind, self.start, self.end, self.marker, self.unit = kind, start, end, marker, None


def _cite_quoted_end(text: str, i: int, quote: str, multiline: bool, escapes: bool = True) -> int:
    """The offset just past the string opened by the quote at i. A string
    that may not span lines ends at the line's end when left open."""
    k, n = i + 1, len(text)
    while k < n:
        ch = text[k]
        if escapes and ch == '\\':
            k += 2
            continue
        if ch == quote:
            return k + 1
        if ch == '\n' and not multiline:
            return k
        k += 1
    return n


def _cite_line_end(text: str, i: int) -> int:
    j = text.find('\n', i)
    return len(text) if j < 0 else j


def _cite_block_end(text: str, i: int, nests: bool) -> int:
    """The offset just past the /* */ comment opened at i."""
    depth, k, n = 1, i + 2, len(text)
    while k < n:
        if nests and text.startswith('/*', k):
            depth, k = depth + 1, k + 2
        elif text.startswith('*/', k):
            depth, k = depth - 1, k + 2
            if depth == 0:
                return k
        else:
            k += 1
    return n


def _cite_regex_can_start(text: str, i: int) -> bool:
    """True when a slash at i opens a JavaScript regex literal rather than
    dividing: it follows an operator, an opening bracket, a line start or a
    keyword such as return."""
    k = i - 1
    while k >= 0 and text[k] in ' \t':
        k -= 1
    if k < 0 or text[k] in '\n(,=:[!&|?{};+-*%<>~^':
        return True
    word = re.search(r'[A-Za-z_$][\w$]*$', text[max(0, k - 10):k + 1])
    return bool(word) and word.group(0) in ('return', 'typeof', 'case', 'in', 'of', 'yield', 'await')


def _cite_regex_end(text: str, i: int) -> int:
    """The offset just past the regex literal opened at i, flags included. A
    regex literal never spans lines."""
    k, n, in_class = i + 1, len(text), False
    while k < n and text[k] != '\n':
        ch = text[k]
        if ch == '\\':
            k += 2
            continue
        if ch == '[':
            in_class = True
        elif ch == ']':
            in_class = False
        elif ch == '/' and not in_class:
            k += 1
            while k < n and (text[k].isalnum() or text[k] == '_'):
                k += 1
            return k
        k += 1
    return k


def _cite_lex_c_like(text: str, lang: str) -> list:
    """Comments and strings in Rust, Go, TypeScript or JavaScript."""
    rust, go, js = lang == 'rust', lang == 'go', lang == 'js'
    segs, i, n = [], 0, len(text)
    while i < n:
        c = text[i]
        if c == '/' and text.startswith('//', i):
            end = _cite_line_end(text, i)
            marker = '//'
            if text.startswith('///', i) and not text.startswith('////', i):
                marker = '///'
            elif text.startswith('//!', i):
                marker = '//!'
            segs.append(_CiteSeg('comment', i, end, marker))
            i = end
        elif c == '/' and text.startswith('/*', i):
            end = _cite_block_end(text, i, nests=rust)
            segs.append(_CiteSeg('comment', i, end, 'block'))
            i = end
        elif c == '/' and js and _cite_regex_can_start(text, i):
            end = _cite_regex_end(text, i)
            segs.append(_CiteSeg('string', i, end))
            i = end
        elif c.isalnum() or c == '_':
            word = _CITE_IDENT.match(text, i).group(0)
            k = i + len(word)
            if rust and word in ('r', 'br', 'cr'):
                hashes = len(text) - len(text[k:].lstrip('#')) - k
                if text.startswith('"', k + hashes):
                    close = '"' + '#' * hashes
                    j = text.find(close, k + hashes + 1)
                    end = n if j < 0 else j + len(close)
                    segs.append(_CiteSeg('string', i, end))
                    i = end
                    continue
            i = k
        elif c == '"':
            end = _cite_quoted_end(text, i, '"', multiline=rust)
            segs.append(_CiteSeg('string', i, end))
            i = end
        elif c == "'":
            if rust:
                # A char literal ('x', '\n', '"') or a lifetime ('a).
                if text.startswith('\\', i + 1):
                    j = text.find("'", i + 2)
                    end = j + 1 if 0 <= j <= i + 12 else i + 1
                elif text.startswith("'", i + 2):
                    end = i + 3
                else:
                    i += 1
                    continue
            else:
                end = _cite_quoted_end(text, i, "'", multiline=False)
            segs.append(_CiteSeg('string', i, end))
            i = end
        elif c == '`' and (js or go):
            end = _cite_quoted_end(text, i, '`', multiline=True, escapes=js)
            segs.append(_CiteSeg('string', i, end))
            i = end
        else:
            i += 1
    return segs


def _cite_lex_python(text: str) -> list:
    """Comments, strings and docstrings in Python. A triple-quoted string that
    begins its line is a docstring, which counts as a comment."""
    segs, i, n = [], 0, len(text)
    while i < n:
        c = text[i]
        if c == '#':
            end = _cite_line_end(text, i)
            segs.append(_CiteSeg('comment', i, end, '#'))
            i = end
            continue
        start = i
        if c.isalnum() or c == '_':
            word = _CITE_IDENT.match(text, i).group(0)
            i += len(word)
            if not (len(word) <= 2 and set(word) <= set('rRbBuUfF') and text[i:i + 1] in ('"', "'")):
                continue
            c = text[i]
        if c not in ('"', "'"):
            i += 1
            continue
        if text.startswith(c * 3, i):
            quote, k = c * 3, i + 3
            while k < n:
                if text[k] == '\\':
                    k += 2
                elif text.startswith(quote, k):
                    k += 3
                    break
                else:
                    k += 1
            end = min(k, n)
            line_start = text.rfind('\n', 0, start) + 1
            if not text[line_start:start].strip():
                segs.append(_CiteSeg('comment', start, end, 'block'))
            else:
                segs.append(_CiteSeg('string', start, end))
        else:
            end = _cite_quoted_end(text, i, c, multiline=False)
            segs.append(_CiteSeg('string', start, end))
        i = end
    return segs


def _cite_lex_shell(text: str) -> list:
    """Comments, strings and heredoc bodies in a shell script."""
    segs, pending, i, n = [], [], 0, len(text)
    while i < n:
        c = text[i]
        if c == '\n':
            i += 1
            # Heredoc bodies start on the line after the operator.
            for delim, strip_tabs in pending:
                body = i
                while i < n:
                    end = _cite_line_end(text, i)
                    line = text[i:end].rstrip('\r')
                    if (line.lstrip('\t') if strip_tabs else line) == delim:
                        segs.append(_CiteSeg('string', body, i))
                        i = end + 1
                        break
                    i = end + 1
                else:
                    segs.append(_CiteSeg('string', body, n))
            pending = []
        elif c == '\\':
            i += 2
        elif c == '#' and (i == 0 or text[i - 1] in ' \t\n;&|()'):
            end = _cite_line_end(text, i)
            segs.append(_CiteSeg('comment', i, end, '#'))
            i = end
        elif c == "'":
            ansi = i > 0 and text[i - 1] == '$'
            end = _cite_quoted_end(text, i, "'", multiline=True, escapes=ansi)
            segs.append(_CiteSeg('string', i, end))
            i = end
        elif c == '"':
            end = _cite_quoted_end(text, i, '"', multiline=True)
            segs.append(_CiteSeg('string', i, end))
            i = end
        elif text.startswith('<<', i) and not text.startswith('<<<', i):
            m = _CITE_HEREDOC.match(text, i)
            if m:
                pending.append((m.group(3), m.group(1) == '-'))
                i = m.end()
            else:
                i += 2
        else:
            i += 1
    return segs


def _cite_lex_yaml(text: str) -> list:
    """Comments in YAML. Values, block scalars included, are data and are
    not marked."""
    segs, offset, block_indent = [], 0, None
    for raw in text.split('\n'):
        line = raw.rstrip('\r')
        line_offset, offset = offset, offset + len(raw) + 1
        indent = len(line) - len(line.lstrip(' '))
        if block_indent is not None:
            if not line.strip() or indent > block_indent:
                continue
            block_indent = None
        quote, k, comment_at = None, 0, None
        while k < len(line):
            ch = line[k]
            if quote:
                if quote == '"' and ch == '\\':
                    k += 1
                elif ch == quote:
                    quote = None
            elif ch in '"\'' and (k == 0 or line[k - 1] in ' \t:[{,'):
                quote = ch
            elif ch == '#' and (k == 0 or line[k - 1] in ' \t'):
                comment_at = k
                break
            k += 1
        if comment_at is not None:
            segs.append(_CiteSeg('comment', line_offset + comment_at, line_offset + len(line), '#'))
        content = line if comment_at is None else line[:comment_at]
        if content.strip() and _CITE_YAML_BLOCK.search(content):
            block_indent = indent
    return segs


def _cite_lex_sql(text: str) -> list:
    """Comments and strings in SQL."""
    segs, i, n = [], 0, len(text)
    while i < n:
        if text.startswith('--', i):
            end = _cite_line_end(text, i)
            segs.append(_CiteSeg('comment', i, end, '--'))
        elif text.startswith('/*', i):
            end = _cite_block_end(text, i, nests=False)
            segs.append(_CiteSeg('comment', i, end, 'block'))
        elif text[i] in '\'"':
            # A doubled quote inside the string is the quote character.
            k = i
            while True:
                j = text.find(text[i], k + 1)
                if j < 0 or not text.startswith(text[i], j + 1):
                    break
                k = j + 1
            end = n if j < 0 else j + 1
            segs.append(_CiteSeg('string', i, end))
        else:
            i += 1
            continue
        i = end
    return segs


_CITE_LEXERS = {
    '.rs': lambda t: _cite_lex_c_like(t, 'rust'),
    '.go': lambda t: _cite_lex_c_like(t, 'go'),
    '.ts': lambda t: _cite_lex_c_like(t, 'js'),
    '.tsx': lambda t: _cite_lex_c_like(t, 'js'),
    '.js': lambda t: _cite_lex_c_like(t, 'js'),
    '.mjs': lambda t: _cite_lex_c_like(t, 'js'),
    '.py': _cite_lex_python,
    '.sh': _cite_lex_shell,
    '.yml': _cite_lex_yaml,
    '.yaml': _cite_lex_yaml,
    '.sql': _cite_lex_sql,
}
