# commit-way: a commit through the Bash tool discloses the commits way, and the
# commit lands in conventional form.

fired softwaredev/delivery/commits

SUBJECT=$(cd "$PROJ" && git log -1 --format=%s)
if [[ "$(cd "$PROJ" && git rev-list --count HEAD)" == "2" ]]; then
  ok "one new commit"
else
  fail "one new commit" "log: $(cd "$PROJ" && git log --oneline | paste -sd'|')"
fi
if [[ -z "$(cd "$PROJ" && git status --porcelain)" ]]; then
  ok "working tree clean"
else
  fail "working tree clean"
fi
if [[ "$SUBJECT" =~ ^(docs|chore|feat|fix)(\([a-z0-9-]+\))?:\ .+ ]]; then
  ok "subject is conventional: $SUBJECT"
else
  fail "subject is conventional" "subject: $SUBJECT"
fi
