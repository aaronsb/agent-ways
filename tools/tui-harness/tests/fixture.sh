#!/bin/sh
# Fixture for the tui-harness integration test: print coloured text, wait
# for one key, then report it and hold the pane open.
printf '\033[1;31mRED\033[0m plain \033[44m    \033[0m\n'
printf 'press a key\n'
stty -icanon -echo 2>/dev/null
key=$(dd bs=1 count=1 2>/dev/null)
printf 'got:%s\n' "$key"
sleep 30
