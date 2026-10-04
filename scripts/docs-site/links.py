"""Resolve relative markdown links for the published site.

Pages in docs/ and the generated ways pages link to files by their position in
the repository. On the site, a target in docs/ or hooks/ways/ maps to its page,
and any other repository path (../README.md, ../governance/, a script) maps to
its GitHub URL. Used by gen_ways.py for way pages and, as an MkDocs hook, for
the pages in docs/. The hook also drops the generated SUMMARY.md once
literate-nav has built the nav from it, so it is not published as a page.
"""

import os
import re
from pathlib import Path, PurePosixPath

from mkdocs.plugins import event_priority

ROOT = Path(__file__).resolve().parents[2]
DOCS = ROOT / "docs"
WAYS = ROOT / "hooks" / "ways"
REPO = "https://github.com/aaronsb/agent-ways"

LINK = re.compile(r"(\]\()([^)\s]+)(\))")
FENCE = re.compile(r"^ {0,3}(`{3,}|~{3,})(.*)$")
INLINE_CODE = re.compile(r"(`+).+?\1")
EXCLUDED = {"adr.yaml"}


def way_page(path):
    """The site path of a way file: `{d}/{w}/{w}.md` becomes `ways/{d}/{w}/index.md`."""
    rel = path.relative_to(WAYS)
    if rel.parent.name == rel.stem:
        return PurePosixPath("ways", *rel.parent.parts, "index.md")
    return PurePosixPath("ways", *rel.parts)


def site_path(target):
    """The docs-relative site path for a repository file, or None if it is not published."""
    rel = target.relative_to(ROOT)
    if target.name in EXCLUDED or "scripts" in rel.parts or target.suffix == ".py":
        return None
    if target.is_dir():
        for index in ("index.md", "README.md", f"{target.name}.md"):
            if (target / index).is_file():
                return site_path(target / index)
        return None
    if target.is_relative_to(DOCS):
        return PurePosixPath(target.relative_to(DOCS).as_posix())
    if target.is_relative_to(WAYS) and target.suffix == ".md":
        return way_page(target)
    return None


def resolve(href, source, page):
    """Rewrite one link href found in `source` (a real file) for the page at `page` (site path)."""
    if re.match(r"^[a-z][a-z0-9+.-]*:|^#", href, re.I):
        return href
    path, _, fragment = href.partition("#")
    anchor = f"#{fragment}" if fragment else ""
    target = (ROOT / path.lstrip("/")) if path.startswith("/") else (source.parent / path)
    target = Path(os.path.normpath(target))
    if not target.exists() or not target.is_relative_to(ROOT):
        return href
    published = site_path(target)
    if published is not None:
        return os.path.relpath(published, page.parent).replace(os.sep, "/") + anchor
    kind = "tree" if target.is_dir() else "blob"
    return f"{REPO}/{kind}/main/{target.relative_to(ROOT).as_posix()}{anchor}"


def fence_states(lines):
    """Yield (line, in_code) for each line, tracking fenced code blocks.

    A fence closes only on the character that opened it, repeated at least as
    many times, with nothing after it; fence lines count as code.
    """
    opener = None
    for line in lines:
        m = FENCE.match(line.rstrip("\n"))
        if opener is None and m:
            opener = m.group(1)
            yield line, True
        elif opener is not None:
            if m and m.group(1)[0] == opener[0] and len(m.group(1)) >= len(opener) and not m.group(2).strip():
                opener = None
            yield line, True
        else:
            yield line, False


def rewrite(markdown, source, page):
    """Rewrite every relative link in `markdown` outside code blocks and code spans."""

    def sub_links(text):
        return LINK.sub(lambda m: m.group(1) + resolve(m.group(2), source, page) + m.group(3), text)

    out = []
    for line, in_code in fence_states(markdown.splitlines(keepends=True)):
        if not in_code:
            pieces, pos = [], 0
            for span in INLINE_CODE.finditer(line):
                pieces += [sub_links(line[pos : span.start()]), span.group(0)]
                pos = span.end()
            line = "".join(pieces) + sub_links(line[pos:])
        out.append(line)
    return "".join(out)


@event_priority(-100)
def on_nav(nav, config, files):
    """MkDocs hook: after literate-nav has read SUMMARY.md, stop it rendering as a page."""
    summary = files.get_file_from_path("SUMMARY.md")
    if summary is not None:
        files.remove(summary)
    return nav


def on_page_markdown(markdown, page, config, files):
    """MkDocs hook: rewrite links on pages that come from docs/."""
    if page.file.generated_by:
        return markdown
    source = DOCS / page.file.src_uri
    return rewrite(markdown, source, PurePosixPath(page.file.src_uri))
