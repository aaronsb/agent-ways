#!/usr/bin/env bash
# Runs the store check.
#
# References:
# - ADR-101: the check runs before every write.

count=${#items[@]}  # ADR-102 in a trailing comment
echo "store check per ADR-101"
cat <<EOM
A heredoc body ships too: ADR-102
EOM
cat <<-'QUOTED'
	ADR-101 in a quoted, tab-stripped heredoc
	QUOTED
echo 'single #quoted ADR-102'
# ADR-101 in a comment after the heredocs, out of place.
