#!/usr/bin/env python3
"""Print or check the `paths:` filter of each Rust build-<component>.yml.

A component's workflow must run when the component's crate changes, when any
workspace crate it depends on changes (directly or transitively, any
dependency kind, since the workflow also runs the crate's tests), when the
workspace manifest or lock changes, and when its workflow or the shared
reusable-build.yml changes. The crate set comes from `cargo metadata`, so the
filter follows the Cargo.toml files instead of being kept by hand.

Usage:
  scripts/workflow-paths.py            print the expected paths per component
  scripts/workflow-paths.py --check    exit 1 if a build-*.yml differs
"""
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TOOLS = os.path.join(ROOT, "tools")
WORKFLOWS = os.path.join(ROOT, ".github", "workflows")


def suite_bins():
    with open(os.path.join(TOOLS, "suite-bins")) as f:
        return [l.split()[0] for l in f if l[:1].isalpha()]


def metadata():
    out = subprocess.run(
        ["cargo", "metadata", "--manifest-path", os.path.join(TOOLS, "Cargo.toml"),
         "--format-version", "1", "--offline"],
        check=True, capture_output=True, text=True).stdout
    return json.loads(out)


def expected_paths(meta, component):
    members = set(meta["workspace_members"])
    by_id = {p["id"]: p for p in meta["packages"]}
    by_name = {by_id[m]["name"]: m for m in members}
    deps = {n["id"]: [d["pkg"] for d in n["deps"] if d["pkg"] in members]
            for n in meta["resolve"]["nodes"] if n["id"] in members}
    seen, todo = set(), [by_name[component]]
    while todo:
        pid = todo.pop()
        if pid in seen:
            continue
        seen.add(pid)
        todo.extend(deps.get(pid, []))
    root_dir = os.path.dirname(by_id[by_name[component]]["manifest_path"])
    dirs = sorted(os.path.relpath(os.path.dirname(by_id[p]["manifest_path"]), ROOT)
                  for p in seen if os.path.dirname(by_id[p]["manifest_path"]) != root_dir)
    own = os.path.relpath(root_dir, ROOT)
    return ([f"{own}/**"] + [f"{d}/**" for d in dirs]
            + ["tools/Cargo.toml", "tools/Cargo.lock",
               f".github/workflows/build-{component}.yml",
               ".github/workflows/reusable-build.yml"])


def block(paths):
    return "".join(f"      - '{p}'\n" for p in paths)


def current_blocks(text):
    """The two `paths:` lists (push, pull_request) of a caller, as text."""
    blocks, lines, i = [], text.splitlines(keepends=True), 0
    while i < len(lines):
        if lines[i].strip() == "paths:":
            j = i + 1
            while j < len(lines) and lines[j].startswith("      - "):
                j += 1
            blocks.append((i + 1, j))
            i = j
        else:
            i += 1
    return lines, blocks


def main():
    meta = metadata()
    check = "--check" in sys.argv
    write = "--write" in sys.argv
    bad = 0
    for comp in suite_bins():
        want = expected_paths(meta, comp)
        path = os.path.join(WORKFLOWS, f"build-{comp}.yml")
        lines, blocks = current_blocks(open(path).read())
        if not check and not write:
            print(f"{comp}:")
            print(block(want), end="")
            continue
        new = block(want)
        stale = [b for b in blocks if "".join(lines[b[0]:b[1]]) != new]
        if check and (stale or len(blocks) != 2):
            print(f"build-{comp}.yml: paths differ from cargo metadata; run scripts/workflow-paths.py --write")
            bad = 1
        if write and stale:
            for start, end in reversed(blocks):
                lines[start:end] = [new]
            open(path, "w").write("".join(lines))
            print(f"wrote build-{comp}.yml")
    sys.exit(bad)


if __name__ == "__main__":
    main()
