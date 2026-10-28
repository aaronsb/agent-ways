#!/usr/bin/env bash
# Fixture for the tui-harness drive test: print coloured text, wait for one
# key, then read one line and echo it back, then hold the pane open. Every
# wait is bounded, so a killed test run leaves nothing behind for long.
# The row ends in green-ground spaces out to the right edge (40 columns).
printf '\033[1;31mRED\033[0m plain \033[44m    \033[0m\033[42m%26s\033[0m\n' ''

printf 'press a key\n'
IFS= read -r -s -t 60 -n 1 key
printf 'got:%s\n' "$key"
printf 'type a line\n'
IFS= read -r -s -t 60 line
printf 'line:[%s]\n' "$line"
sleep 60
