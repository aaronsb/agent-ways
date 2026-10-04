"""Render the ways corpus into the MkDocs site under ways/.

Run by the mkdocs-gen-files plugin during `mkdocs build`. The ways live in
hooks/ways/, outside docs_dir, so each way file is read from there and written
as a virtual page; nothing is copied into docs/.

Layout: `{domain}/{way}/{way}.md` becomes `ways/{domain}/{way}/index.md`, so a
parent way is the landing page of the section holding its children. Other
markdown files in a way directory (`*.check.md`, think strategies) keep their
names. Relative links in a way body are resolved by links.py. ways/SUMMARY.md
gives literate-nav the corpus nav with readable labels, and a domain with no
domain-level way gets a generated landing page. Each page shows the way's frontmatter in a collapsed block above the
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


# Display names for the domain directories under hooks/ways/.
DOMAIN_LABELS = {
    "collaboration": "Collaboration",
    "data": "Data",
    "documentation": "Documentation",
    "ea": "Executive assistant",
    "itops": "IT operations",
    "meta": "Meta",
    "research": "Research",
    "softwaredev": "Software development",
    "workstation": "Workstation",
    "writing": "Writing",
}


def is_empty(src):
    """A placeholder file with no guidance in it, such as a lone `-`."""
    return len(split_frontmatter(src.read_text(encoding="utf-8"))[1].strip()) < 3


def way_label(src):
    """A way's nav label: its title without a trailing " Way"."""
    title = title_for(src, split_frontmatter(src.read_text(encoding="utf-8"))[1])
    return title[:-4] if title.endswith(" Way") else title


def description(src):
    """The way's frontmatter description, for the domain tables."""
    return str(split_frontmatter(src.read_text(encoding="utf-8"))[0].get("description", "")).replace("|", "&#124;")


def way_file(d):
    """The `{d}/{d}.md` file that makes directory d a way, or None."""
    f = d / f"{d.name}.md"
    return f if f.is_file() and not is_empty(f) else None


def has_pages(d):
    return any(not is_empty(f) for f in d.rglob("*.md") if "__pycache__" not in f.parts)


def nav_entries(d, indent):
    """literate-nav lines for the contents of directory d, relative to ways/."""
    pad = "    " * indent
    lines = []
    files = sorted(f for f in d.glob("*.md") if f != way_file(d) and not is_empty(f))
    for f in files:
        lines.append(f"{pad}* [{way_label(f)}]({way_page(f).relative_to('ways')})")
    subdirs = sorted((c for c in d.iterdir() if c.is_dir() and has_pages(c)), key=lambda c: c.name)
    ways = sorted((c for c in subdirs if way_file(c)), key=lambda c: way_label(way_file(c)).lower())
    for c in ways:
        lines.append(f"{pad}* [{way_label(way_file(c))}]({way_page(way_file(c)).relative_to('ways')})")
        lines += nav_entries(c, indent + 1)
    for c in (c for c in subdirs if not way_file(c)):
        lines.append(f"{pad}* {c.name.replace('-', ' ').capitalize()}")
        lines += nav_entries(c, indent + 1)
    return lines


def corpus_nav(domains):
    """SUMMARY.md for ways/: the landing page, each domain with its ways, then the session-wide files."""
    lines = ["* [The ways corpus](index.md)"]
    for d in domains:
        lines.append(f"* [{DOMAIN_LABELS.get(d.name, d.name.capitalize())}]({d.name}/index.md)")
        lines += nav_entries(d, 1)
    lines += ["* [Core (every session)](core.md)", "* [Skills catalog](skills-catalog.md)"]
    return "\n".join(lines) + "\n"


def domain_table(d):
    """Rows of `| way | description |` for the top-level ways in domain d."""
    rows = ["| Way | What it covers |", "|---|---|"]
    for c in sorted((c for c in d.iterdir() if c.is_dir() and way_file(c)), key=lambda c: way_label(way_file(c)).lower()):
        rows.append(f"| [{way_label(way_file(c))}]({c.name}/index.md) | {description(way_file(c))} |")
    return rows


def domain_index(d):
    """A landing page for a domain that has no domain-level way of its own."""
    label = DOMAIN_LABELS.get(d.name, d.name.capitalize())
    return "\n".join([f"# {label}", "", f"The ways in `hooks/ways/{d.name}/`.", "", *domain_table(d)]) + "\n"


def corpus_index(domains):
    """The ways/ landing page: every domain and its top-level ways."""
    lines = [
        "# The ways corpus",
        "",
        "Every way shipped in `hooks/ways/`, by domain. Each page shows the way's "
        "trigger frontmatter (collapsed) and the guidance it injects when it fires. "
        "[Core](core.md) is prepended to every session.",
        "",
        "| Domain | Ways |",
        "|---|---|",
    ]
    for d in domains:
        ways = sorted((c for c in d.iterdir() if c.is_dir() and way_file(c)), key=lambda c: way_label(way_file(c)).lower())
        links = ", ".join(f"[{way_label(way_file(c))}]({d.name}/{c.name}/index.md)" for c in ways)
        if not links and way_file(d):
            links = f"[{way_label(way_file(d))}]({d.name}/index.md)"
        label = DOMAIN_LABELS.get(d.name, d.name.capitalize())
        lines.append(f"| [**{label}**]({d.name}/index.md) | {links or '—'} |")
    return "\n".join(lines) + "\n"


def main():
    for src in sorted(WAYS.rglob("*.md")):
        if "__pycache__" in src.parts or is_empty(src):
            continue
        dest = way_page(src)
        with mkdocs_gen_files.open(dest, "w") as f:
            f.write(render(src))
        mkdocs_gen_files.set_edit_path(dest, f"../{src.relative_to(ROOT)}")
    domains = sorted((d for d in WAYS.iterdir() if d.is_dir() and has_pages(d)), key=lambda d: DOMAIN_LABELS.get(d.name, d.name))
    for d in domains:
        if not way_file(d):
            with mkdocs_gen_files.open(OUT / d.name / "index.md", "w") as f:
                f.write(domain_index(d))
    with mkdocs_gen_files.open(OUT / "index.md", "w") as f:
        f.write(corpus_index(domains))
    with mkdocs_gen_files.open(OUT / "SUMMARY.md", "w") as f:
        f.write(corpus_nav(domains))


main()
