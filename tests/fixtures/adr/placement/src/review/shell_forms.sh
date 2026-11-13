#!/usr/bin/env bash
(( y = x << bits ))
# ADR-102 in a comment after arithmetic shifts
cat <<\EOF2
# a heredoc line that looks like a comment: ADR-101
EOF2
# ADR-102 in a comment after the heredoc
