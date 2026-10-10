#!/bin/bash
# usage: jac.sh <worktree>
mkdir -p /tmp/kgtopo-a3601/mainx && tar -xf /tmp/kgtopo-a3601/main-ways.tar -C /tmp/kgtopo-a3601/mainx
for d in collaboration data documentation ea evaluative itops meta softwaredev workstation; do
  for L in main br; do
    if [ "$L" = main ]; then R=/tmp/kgtopo-a3601/mainx/hooks/ways; else R="$1/hooks/ways"; fi
    COLUMNS=400 ways author tree "$R/$d" --jaccard 2>&1 | grep -oE "[0-9]\.[0-9]+[[:space:]]*$" \
      | awk -v L="$L" -v d="$d" '{v=$1+0; c++; if (v>=0.15) n++; if (v>m) m=v} END {printf "%s %s pairs:%d >=0.15:%d max:%.3f\n", d, L, c, n, m}'
  done
done
