#!/usr/bin/env bash
# Research way macro — show available skills for investigation tasks
#
# Research is mostly greenfield (no official Anthropic skills),
# but some knowledge-work plugins are relevant.

# `ways` exports the enabled plugin ids, one per line (name@marketplace).
installed="${WAYS_ENABLED_PLUGINS:-}"

echo ""
echo "## Skills for Research"
echo ""
echo "| Task | Skill | Installed | Install command |"
echo "|------|-------|-----------|-----------------|"

for entry in \
  "Research synthesis|research-synthesis|knowledge-work-plugins" \
  "User research|user-research|knowledge-work-plugins" \
  "Data exploration|explore-data|knowledge-work-plugins" \
  "Statistical analysis|statistical-analysis|knowledge-work-plugins"; do

  IFS='|' read -r task skill marketplace <<< "$entry"
  if echo "$installed" | grep -q "$skill"; then
    status="yes"
  else
    status="no"
  fi
  echo "| ${task} | \`${skill}\` | ${status} | \`claude plugin install ${skill}@${marketplace}\` |"
done

echo ""
echo "Most research uses built-in tools (WebSearch, WebFetch, Read, Grep). Skills add structured workflows on top."
