"""Render the ways corpus into the MkDocs site under ways/.

Run by the mkdocs-gen-files plugin during `mkdocs build`. The ways live in
hooks/ways/, outside docs_dir, so each way file is read from there and written
as a virtual page; nothing is copied into docs/.

Layout: `{domain}/{way}/{way}.md` becomes `ways/{domain}/{way}/index.md`, so a
parent way is the landing page of the section holding its children. Other
markdown files in a way directory (`*.check.md`, think strategies) keep their
names. Relative links in a way body are resolved by links.py. Each page shows the way's frontmatter in a collapsed block above the
body; the body is rendered unchanged.
"""

import html
import re
import sys
from pathlib import Path

import mkdocs_gen_files
import yaml

sys.path.insert(0, str(Path(__file__).parent))
from links import REPO, ROOT, WAYS, fence_states, rewrite, way_page  # noqa: E402

OUT = Path("ways")

# Frontmatter fields shown first, in this order; the rest follow alphabetically.
FIELD_ORDER = [
    "description",
    "vocabulary",
    "pattern",
    "commands",
    "files",
    "trigger",
    "threshold",
    "path",
    "when",
    "refire",
    "macro",
    "scope",
]


def split_frontmatter(text):
    """Return (frontmatter dict, body) for a markdown file with optional YAML frontmatter."""
    m = re.match(r"---\n(.*?)^---[ \t]*$\n?", text, re.S | re.M)
    if not m:
        return {}, text
    try:
        meta = yaml.safe_load(m.group(1)) or {}
    except yaml.YAMLError:
        meta = {}
    body = text[m.end() :].lstrip("\n")
    return (meta if isinstance(meta, dict) else {}), body


def h1_index(lines):
    """Index of the first H1 outside code blocks, or None."""
    for i, (line, in_code) in enumerate(fence_states(lines)):
        if not in_code and line.startswith("# "):
            return i
    return None


def title_for(src, body):
    """The body's first H1, or a title derived from the file name."""
    lines = body.splitlines()
    i = h1_index(lines)
    if i is not None:
        return lines[i][2:].strip()
    return src.stem.replace(".check", " (check)").replace("-", " ").title()


def frontmatter_block(meta):
    """A collapsed admonition listing the way's frontmatter as a table.

    Values are emitted as escaped <code> elements; a pipe becomes &#124; so it
    neither splits the table row nor shows a backslash inside a regex.
    """
    if not meta:
        return ""
    keys = [k for k in FIELD_ORDER if k in meta]
    keys += sorted(k for k in meta if k not in FIELD_ORDER)
    rows = []
    for key in keys:
        value = meta[key]
        if isinstance(value, (list, dict)):
            value = yaml.safe_dump(value, default_flow_style=True, width=10_000).strip()
        cell = html.escape(str(value).replace("\n", " ")).replace("|", "&#124;")
        if key != "description":
            cell = f"<code>{cell}</code>"
        rows.append(f"    | `{key}` | {cell} |")
    table = "\n".join(["    | Field | Value |", "    |---|---|", *rows])
    return f'??? info "Frontmatter"\n\n{table}\n\n'


def render(src):
    """Build the page text for one way file."""
    meta, body = split_frontmatter(src.read_text(encoding="utf-8"))
    body = rewrite(body, src, way_page(src))
    title = title_for(src, body)
    rel = src.relative_to(ROOT)
    header = f"<small>Source: [`{rel}`]({REPO}/blob/main/{rel})</small>\n\n{frontmatter_block(meta)}"
    lines = body.splitlines()
    i = h1_index(lines)
    if i is not None:
        return "\n".join(lines[: i + 1] + ["", header] + lines[i + 1 :]) + "\n"
    return f"# {title}\n\n{header}{body}"


def domain_index(domains):
    """The ways/ landing page: one row per domain with its top-level ways."""
    lines = [
        "# Ways",
        "",
        "The ways corpus as shipped in `hooks/ways/`. Each page shows the way's "
        "trigger frontmatter (collapsed) and the guidance it injects when it fires.",
        "",
        "| Domain | Ways |",
        "|---|---|",
    ]
    for domain, ways in domains.items():
        links = ", ".join(f"[{w}]({domain}/{w}/index.md)" for w in ways) or "—"
        lines.append(f"| **{domain}** | {links} |")
    lines += ["", "Session-wide files: [core](core.md) is prepended to every session."]
    return "\n".join(lines) + "\n"


def main():
    domains = {}
    for src in sorted(WAYS.rglob("*.md")):
        if "__pycache__" in src.parts:
            continue
        dest = way_page(src)
        with mkdocs_gen_files.open(dest, "w") as f:
            f.write(render(src))
        mkdocs_gen_files.set_edit_path(dest, f"../{src.relative_to(ROOT)}")
        rel = src.relative_to(WAYS)
        if len(rel.parts) == 3 and rel.parts[1] == rel.stem:
            domains.setdefault(rel.parts[0], []).append(rel.parts[1])
    with mkdocs_gen_files.open(OUT / "index.md", "w") as f:
        f.write(domain_index(dict(sorted(domains.items()))))


main()
