#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2025-2026 Aaron Bockelie
# SPDX-FileCopyrightText: 2026 Jonathan Muse
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# Builds a link graph from git-tracked markdown files.
# Finds all internal doc links, resolves paths, identifies dead ends,
# orphans, and broken links. Outputs Mermaid diagram or JSON.
#
# Usage: doc-graph.sh [--json] [--mermaid] [--stats] [--all] [--docs-only]
#   --mermaid    Output Mermaid diagram (default)
#   --json       Output JSON adjacency list
#   --stats      Output dead ends, orphans, broken links
#   --all        All outputs
#   --docs-only  Human-navigable docs only (exclude machine-layer files)
#
# Operates on git-tracked .md files in the repository root.

set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || echo "$PWD")"
cd "$REPO_ROOT"

# Parse flags
OUTPUT_MERMAID=false
OUTPUT_JSON=false
OUTPUT_STATS=false
DOCS_ONLY=false

if [[ $# -eq 0 ]]; then
    OUTPUT_MERMAID=true
fi

while [[ $# -gt 0 ]]; do
    case "$1" in
        --mermaid)   OUTPUT_MERMAID=true ;;
        --json)      OUTPUT_JSON=true ;;
        --stats)     OUTPUT_STATS=true ;;
        --all)       OUTPUT_MERMAID=true; OUTPUT_JSON=true; OUTPUT_STATS=true ;;
        --docs-only) DOCS_ONLY=true ;;
        -h|--help)
            sed -n '/^# Builds a link graph/,/^# Operates on/s/^# //p' "$0"
            exit 0
            ;;
        *) echo "Unknown flag: $1" >&2; exit 1 ;;
    esac
    shift
done

# Collect git-tracked markdown files
if $DOCS_ONLY; then
    # Human-navigable docs only:
    #   Root community files, docs/**, governance/README.md
    # Excludes machine-layer files consumed by systems, not humans:
    #   hooks/ways/** (injected context), agents/** (agent defs),
    #   commands/** (slash commands), skills/** (skill defs),
    #   .github/** (templates), tests/** (test fixtures)
    mapfile -t MD_FILES < <(git ls-files '*.md' | grep -E '^(README|CONTRIBUTING|CODE_OF_CONDUCT|SECURITY|CLAUDE)\.md$|^docs/|^governance/' | sort)
else
    mapfile -t MD_FILES < <(git ls-files '*.md' | sort)
fi

# Build adjacency list: source -> [targets]
# Also track: all files seen, all link targets, broken links
declare -A OUTGOING    # file -> space-separated list of resolved targets
declare -A INCOMING    # file -> space-separated list of sources linking to it
declare -A BROKEN      # "source -> target" -> original link text
declare -A ALL_FILES   # all tracked md files

for f in "${MD_FILES[@]}"; do
    ALL_FILES["$f"]=1
    OUTGOING["$f"]=""
done

for src in "${MD_FILES[@]}"; do
    src_dir="$(dirname "$src")"

    # Extract markdown links: [text](path) — skip URLs, anchors, images
    # Captures the path portion of [text](path)
    while IFS= read -r raw_link; do
        # Skip external URLs
        [[ "$raw_link" =~ ^https?:// ]] && continue
        [[ "$raw_link" =~ ^mailto: ]] && continue
        [[ "$raw_link" =~ ^#  ]] && continue

        # Strip anchor fragments and query strings
        link="${raw_link%%#*}"
        link="${link%%\?*}"

        # Skip empty after stripping
        [[ -z "$link" ]] && continue

        # Skip non-markdown links (images, scripts, binaries)
        # But include links to directories (which might have README.md)
        if [[ "$link" == *.md ]]; then
            : # markdown file, proceed
        elif [[ -d "$src_dir/$link" ]] && [[ -f "$src_dir/$link/README.md" ]]; then
            link="$link/README.md"
        else
            continue
        fi

        # Resolve relative path
        resolved="$(cd "$REPO_ROOT" && realpath --relative-to=. "$src_dir/$link" 2>/dev/null || echo "")"

        [[ -z "$resolved" ]] && continue

        # Check if target exists
        if [[ -f "$REPO_ROOT/$resolved" ]]; then
            # In docs-only mode, only track links within the file set
            if $DOCS_ONLY && [[ -z "${ALL_FILES[$resolved]:-}" ]]; then
                continue
            fi
            OUTGOING["$src"]+=" $resolved"
            INCOMING["$resolved"]+=" $src"
        else
            BROKEN["$src -> $resolved"]="$raw_link"
        fi
    # Extract markdown link targets — `[text](target)` → `target`. Uses portable
    # grep -oE (BSD grep has no -P/PCRE) + sed to strip the `](` … `)` wrapper.
    done < <(grep -oE '\]\([^)]+\)' "$REPO_ROOT/$src" 2>/dev/null | sed -E 's/^\]\(//; s/\)$//' || true)
done

# Deduplicate adjacency lists
declare -A OUTGOING_DEDUP
declare -A INCOMING_DEDUP

for f in "${MD_FILES[@]}"; do
    if [[ -n "${OUTGOING[$f]:-}" ]]; then
        OUTGOING_DEDUP["$f"]="$(echo "${OUTGOING[$f]}" | tr ' ' '\n' | sort -u | tr '\n' ' ' | sed 's/^ *//;s/ *$//')"
    else
        OUTGOING_DEDUP["$f"]=""
    fi
done

for f in "${MD_FILES[@]}"; do
    if [[ -n "${INCOMING[$f]:-}" ]]; then
        INCOMING_DEDUP["$f"]="$(echo "${INCOMING[$f]}" | tr ' ' '\n' | sort -u | tr '\n' ' ' | sed 's/^ *//;s/ *$//')"
    else
        INCOMING_DEDUP["$f"]=""
    fi
done

# Classify files
declare -a DEAD_ENDS=()   # files with no outgoing doc links
declare -a ORPHANS=()     # files with no incoming doc links
declare -a HUB_FILES=()   # files with 3+ outgoing links

for f in "${MD_FILES[@]}"; do
    out_count=0
    if [[ -n "${OUTGOING_DEDUP[$f]:-}" ]]; then
        out_count=$(echo "${OUTGOING_DEDUP[$f]}" | wc -w)
    fi

    in_count=0
    if [[ -n "${INCOMING_DEDUP[$f]:-}" ]]; then
        in_count=$(echo "${INCOMING_DEDUP[$f]}" | wc -w)
    fi

    if [[ $out_count -eq 0 ]]; then
        DEAD_ENDS+=("$f")
    fi

    if [[ $in_count -eq 0 ]]; then
        ORPHANS+=("$f")
    fi

    if [[ $out_count -ge 3 ]]; then
        HUB_FILES+=("$f")
    fi
done

# --- Output: Mermaid ---
if $OUTPUT_MERMAID; then
    echo "graph LR"

    # Create node IDs from file paths (replace special chars)
    node_id() {
        echo "$1" | sed 's/[\/\.\-]/_/g' | sed 's/^_//'
    }

    # Classify nodes by type
    for f in "${MD_FILES[@]}"; do
        nid="$(node_id "$f")"
        label="$(basename "$f" .md)"
        dir="$(dirname "$f")"

        # Use different shapes for different roles
        is_dead_end=false
        is_orphan=false
        for d in "${DEAD_ENDS[@]}"; do [[ "$d" == "$f" ]] && is_dead_end=true && break; done
        for o in "${ORPHANS[@]}"; do [[ "$o" == "$f" ]] && is_orphan=true && break; done

        if $is_dead_end && $is_orphan; then
            # Isolated: no links in or out
            echo "    ${nid}[/\"${label} (${dir})\"/]"
        elif $is_dead_end; then
            # Dead end: receives links but links nowhere
            echo "    ${nid}[\"${label} (${dir}) DEAD END\"]"
        elif $is_orphan; then
            # Entry point: links out but nothing links to it
            echo "    ${nid}((\"${label} (${dir})\"))"
        else
            echo "    ${nid}[\"${label} (${dir})\"]"
        fi
    done

    echo ""

    # Edges
    for src in "${MD_FILES[@]}"; do
        if [[ -n "${OUTGOING_DEDUP[$src]:-}" ]]; then
            for tgt in ${OUTGOING_DEDUP[$src]}; do
                src_id="$(node_id "$src")"
                tgt_id="$(node_id "$tgt")"
                echo "    ${src_id} --> ${tgt_id}"
            done
        fi
    done

    echo ""

    # Style dead ends red, orphans blue
    dead_ids=()
    orphan_ids=()
    isolated_ids=()
    for d in "${DEAD_ENDS[@]}"; do
        is_also_orphan=false
        for o in "${ORPHANS[@]}"; do [[ "$o" == "$d" ]] && is_also_orphan=true && break; done
        if $is_also_orphan; then
            isolated_ids+=("$(node_id "$d")")
        else
            dead_ids+=("$(node_id "$d")")
        fi
    done
    for o in "${ORPHANS[@]}"; do
        is_also_dead=false
        for d in "${DEAD_ENDS[@]}"; do [[ "$o" == "$d" ]] && is_also_dead=true && break; done
        if ! $is_also_dead; then
            orphan_ids+=("$(node_id "$o")")
        fi
    done

    if [[ ${#dead_ids[@]} -gt 0 ]]; then
        echo "    style $(IFS=,; echo "${dead_ids[*]}") fill:#C2572A,color:#FFFFFF"
    fi
    if [[ ${#orphan_ids[@]} -gt 0 ]]; then
        echo "    style $(IFS=,; echo "${orphan_ids[*]}") fill:#2D7D9A,color:#FFFFFF"
    fi
    if [[ ${#isolated_ids[@]} -gt 0 ]]; then
        echo "    style $(IFS=,; echo "${isolated_ids[*]}") fill:#7B2D8E,color:#FFFFFF"
    fi
fi

# --- Output: Stats ---
if $OUTPUT_STATS; then
    total=${#MD_FILES[@]}
    dead=${#DEAD_ENDS[@]}
    orphan=${#ORPHANS[@]}
    broken_count=0
    for _ in "${!BROKEN[@]}"; do ((broken_count++)); done 2>/dev/null || true

    # Count edges
    edge_count=0
    for f in "${MD_FILES[@]}"; do
        if [[ -n "${OUTGOING_DEDUP[$f]:-}" ]]; then
            edge_count=$((edge_count + $(echo "${OUTGOING_DEDUP[$f]}" | wc -w)))
        fi
    done

    echo ""
    echo "=== Documentation Link Graph ==="
    echo ""
    echo "Files:        $total"
    echo "Links:        $edge_count"
    echo "Dead ends:    $dead  (receive links but link nowhere)"
    echo "Orphans:      $orphan  (link out but nothing links to them)"
    echo "Broken links: $broken_count"
    echo ""

    if [[ $dead -gt 0 ]]; then
        echo "--- Dead Ends (no outgoing doc links) ---"
        for d in "${DEAD_ENDS[@]}"; do
            in_count=0
            [[ -n "${INCOMING_DEDUP[$d]:-}" ]] && in_count=$(echo "${INCOMING_DEDUP[$d]}" | wc -w)
            echo "  $d  (linked from $in_count files)"
        done
        echo ""
    fi

    if [[ $orphan -gt 0 ]]; then
        echo "--- Orphans (no incoming links) ---"
        for o in "${ORPHANS[@]}"; do
            out_count=0
            [[ -n "${OUTGOING_DEDUP[$o]:-}" ]] && out_count=$(echo "${OUTGOING_DEDUP[$o]}" | wc -w)
            echo "  $o  (links to $out_count files)"
        done
        echo ""
    fi

    if [[ $broken_count -gt 0 ]]; then
        echo "--- Broken Links ---"
        for key in "${!BROKEN[@]}"; do
            echo "  $key  (original: ${BROKEN[$key]})"
        done 2>/dev/null || true
        echo ""
    fi

    if [[ ${#HUB_FILES[@]} -gt 0 ]]; then
        echo "--- Hub Files (3+ outgoing links) ---"
        for h in "${HUB_FILES[@]}"; do
            out_count=$(echo "${OUTGOING_DEDUP[$h]}" | wc -w)
            echo "  $h  ($out_count outgoing)"
        done
        echo ""
    fi
fi

# --- Output: JSON ---
if $OUTPUT_JSON; then
    echo "{"
    echo '  "files": ['

    first_file=true
    for f in "${MD_FILES[@]}"; do
        $first_file || echo ","
        first_file=false

        out_json="[]"
        if [[ -n "${OUTGOING_DEDUP[$f]:-}" ]]; then
            out_json="[$(echo "${OUTGOING_DEDUP[$f]}" | tr ' ' '\n' | sed 's/.*/"&"/' | paste -sd, -)]"
        fi

        in_json="[]"
        if [[ -n "${INCOMING_DEDUP[$f]:-}" ]]; then
            in_json="[$(echo "${INCOMING_DEDUP[$f]}" | tr ' ' '\n' | sed 's/.*/"&"/' | paste -sd, -)]"
        fi

        is_dead="false"
        is_orphan="false"
        for d in "${DEAD_ENDS[@]}"; do [[ "$d" == "$f" ]] && is_dead="true" && break; done
        for o in "${ORPHANS[@]}"; do [[ "$o" == "$f" ]] && is_orphan="true" && break; done

        printf '    {"file": "%s", "links_to": %s, "linked_from": %s, "dead_end": %s, "orphan": %s}' \
            "$f" "$out_json" "$in_json" "$is_dead" "$is_orphan"
    done

    echo ""
    echo "  ],"

    # Broken links
    echo '  "broken_links": ['
    first_broken=true
    for key in "${!BROKEN[@]}"; do
        $first_broken || echo ","
        first_broken=false
        src="${key%% ->*}"
        tgt="${key##*-> }"
        printf '    {"source": "%s", "target": "%s", "original": "%s"}' \
            "$src" "$tgt" "${BROKEN[$key]}"
    done 2>/dev/null || true
    echo ""
    echo "  ]"
    echo "}"
fi
