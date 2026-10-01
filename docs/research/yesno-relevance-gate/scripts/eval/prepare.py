#!/usr/bin/env python3
"""Build the scoring units: one row per (item, way variant, context variant).

Writes units.jsonl (id, set, way_id, wv, cv, doc, query) and ways_meta.json
(route, ancestor descriptions, nodes lacking one).
"""
import json
import os
import re
from pathlib import Path

P = Path(__file__).resolve().parent
DATA = P.parent / "data"
ROOT = Path(os.path.expanduser("~/.claude/hooks/ways"))

DRAFTS = {
    "softwaredev": "Guidance for building software: code craft, delivery, environment, architecture and data.",
    "meta": "Guidance about how the agent works: knowledge, way authoring, delegation, trust and session practice.",
    "itops": "Guidance for operating systems and services: incidents, policy, proposals and runbooks.",
    "collaboration": "Guidance for working with other people: onboarding, handoffs and teams.",
    "workstation": "Guidance for setting up and maintaining a personal developer machine: shell, tools and packages.",
    "meta/attend": "Guidance for the attend awareness layer: sensors, peers and messaging.",
}


def frontmatter(path):
    if not path.exists():
        return {}
    m = re.match(r"---\n(.*?)\n---", path.read_text(), re.S)
    if not m:
        return {}
    out = {}
    for key in ("description", "vocabulary"):
        mm = re.search(rf"^{key}:\s*(.*)$", m.group(1), re.M)
        if mm:
            out[key] = mm.group(1).strip().strip('"').strip("'")
    return out


def node_desc(node):
    d = frontmatter(ROOT / node / (node.rsplit("/", 1)[-1] + ".md")).get("description")
    if d:
        return d, "corpus"
    if node in DRAFTS:
        return DRAFTS[node], "draft"
    return None, "missing"


def load(path):
    return [json.loads(l) for l in open(path) if l.strip()]


def render(turns):
    lines = []
    for t in turns:
        text = " ".join(t["text"].split())
        lines.append(f"{'User' if t['role'] == 'user' else 'Assistant'}: {text}")
    return "\n".join(lines)


def main():
    sets = {
        "s1": load(DATA / "eval.jsonl"),
        "s2": load(DATA / "random40.jsonl"),
        "s3": load(DATA / "session_unlabelled.jsonl"),
    }
    meta, missing = {}, {}
    for items in sets.values():
        for it in items:
            w = it["way_id"]
            if w in meta:
                continue
            parts = w.split("/")
            ancestors = ["/".join(parts[:i]) for i in range(1, len(parts))]
            anc = []
            for a in ancestors:
                d, src = node_desc(a)
                anc.append({"node": a, "desc": d, "src": src})
                if src != "corpus":
                    missing[a] = src
            fm = frontmatter(ROOT / w / (parts[-1] + ".md"))
            meta[w] = {"route": " › ".join(parts), "ancestors": anc,
                       "vocabulary": fm.get("vocabulary", ""), "corpus_desc": fm.get("description")}

    def doc(it, wv):
        m, s = meta[it["way_id"]], it["summary"]
        if wv == "A":
            return s
        if wv == "B":
            return f"{m['route']}\n{s}"
        lines = [m["route"]]
        for a in m["ancestors"]:
            name = a["node"].rsplit("/", 1)[-1]
            lines.append(f"{name}: {a['desc']}" if a["desc"] else f"{name}")
        lines.append(f"{it['way_id'].rsplit('/', 1)[-1]}: {s}")
        return "\n".join(lines)

    def query(it, cv):
        if cv == "T1":
            return render(it["turns"][-1:])
        if cv == "T4":
            return render(it["turns"])
        if cv == "TX":
            return render(it["turns_extended"])
        if cv == "TXG":
            return f"Session: {it['session_gist']}\n" + render(it["turns_extended"])
        if cv == "EX":
            return it["trigger_excerpt"]
        raise ValueError(cv)

    units = []
    for sname, items in sets.items():
        cvs = ["EX"] if sname == "s3" else ["T1", "T4", "TX", "TXG"]
        for it in items:
            for wv in ("A", "B", "C"):
                for cv in cvs:
                    units.append({"id": it["id"], "set": sname, "way_id": it["way_id"], "wv": wv, "cv": cv,
                                  "doc": doc(it, wv), "query": query(it, cv)})
    with open(P / "units.jsonl", "w") as f:
        for u in units:
            f.write(json.dumps(u) + "\n")
    json.dump({"ways": meta, "nodes_without_description": missing}, open(P / "ways_meta.json", "w"), indent=1)
    print(len(units), "units;", "nodes without corpus description:", missing)


if __name__ == "__main__":
    main()
