#!/usr/bin/env bash
# Writing way macro — show available skills for content creation
#
# Skills are not required to be installed. This tells Claude what exists
# and how to invoke it. The user installs on first use.

# Check which writing-relevant skills are installed
# `ways` exports the enabled plugin ids, one per line (name@marketplace).
installed="${WAYS_ENABLED_PLUGINS:-}"

echo ""
echo "## Skills for Writing"
echo ""
echo "These official Anthropic skills handle file creation. Reference by name — if not installed, tell the user how to install."
echo ""
echo "| Task | Skill | Installed | Install command |"
echo "|------|-------|-----------|-----------------|"

# Check each skill
for entry in \
  "Structured doc co-authoring|doc-coauthoring|anthropic-agent-skills" \
  "Slide deck / presentation|pptx|anthropic-agent-skills" \
  "Word document output|docx|anthropic-agent-skills" \
  "PDF creation|pdf|anthropic-agent-skills" \
  "Status reports, newsletters|status-report|knowledge-work-plugins" \
  "Process documentation|process-doc|knowledge-work-plugins"; do

  IFS='|' read -r task skill marketplace <<< "$entry"
  if echo "$installed" | grep -q "$skill"; then
    status="yes"
  else
    status="no"
  fi
  echo "| ${task} | \`${skill}\` | ${status} | \`claude plugin install ${skill}@${marketplace}\` |"
done

echo ""
echo "Invoke installed skills naturally: \"Use the pptx skill to create a presentation about X.\""
echo "For uninstalled skills, tell the user: \"This task would benefit from the \`<skill>\` skill. Install with \`claude plugin install <skill>@<marketplace>\`.\""
