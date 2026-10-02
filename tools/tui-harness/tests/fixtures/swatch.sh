#!/usr/bin/env bash
# The tui-harness regression swatch: one row per thing the renderer must
# draw. Each row is a 10-column label, then the samples. tests/swatch.rs
# asserts on cells by label and offset, so keep the layout stable.
#
# Re-capture after a change (100x14):
#   th=tools/target/debug/tui-harness
#   $th launch swatch --cols 100 --rows 14 -- bash tools/tui-harness/tests/fixtures/swatch.sh
#   $th text swatch --ansi > tools/tui-harness/tests/fixtures/swatch.ansi
#   $th down swatch
# then re-bless the golden PNG (see tests/swatch.rs).

label() { printf '%-10s' "$1"; }
reset=$'\033[0m'

label attrs
printf '\033[1mbold%s \033[2mdim%s \033[3mitalic%s \033[4munderline%s \033[7mreverse%s \033[1;2mbold+dim%s\n' \
  "$reset" "$reset" "$reset" "$reset" "$reset" "$reset"

# Offset 2i: basic colour i. Offset 17 + 2(i-8): bright colour i.
label basic16
for i in 0 1 2 3 4 5 6 7; do printf '\033[4%dm  %s' "$i" "$reset"; done
printf ' '
for i in 0 1 2 3 4 5 6 7; do printf '\033[10%dm  %s' "$i" "$reset"; done
printf '\n'

# Offset k: palette index 16 + 5k, so offset 0 is 16, 36 is 196, 43 is 231.
label cube256
for i in $(seq 16 5 231); do printf '\033[48;5;%dm %s' "$i" "$reset"; done
printf '\n'

# Offset k: palette index 232 + k.
label grey
for i in $(seq 232 255); do printf '\033[48;5;%dm %s' "$i" "$reset"; done
printf '\n'

# Offset k: rgb(4k, 255-4k, 128).
label truecolor
for k in $(seq 0 63); do printf '\033[48;2;%d;%d;%dm %s' $((4 * k)) $((255 - 4 * k)) 128 "$reset"; done
printf '\n'

label fg/bg
printf '\033[38;5;208morange256%s \033[38;2;120;200;255mtruecolor%s \033[91mbright-red%s \033[33;44mfg-on-bg%s\n' \
  "$reset" "$reset" "$reset" "$reset"

# Offsets 0-3: red fg on blue bg, reversed. 5-6: default colours, reversed.
# 8-11: dim yellow, underlined, so the dimmed fg shows in the underline.
label rev/dim
printf '\033[31;44;7m    %s \033[7m  %s \033[2;4;33m    %s\n' "$reset" "$reset" "$reset"

label braille
printf '⠁⠃⠇⡇⣇⣧⣷⣿ ⣀⣤⣶⣿⠿⠛⠉ ⢸⣿⡇\n'

label box
printf '┌──┬──┐ ╭──╮ ═══ ║ ▁▂▃▄▅▆▇█ ░▒▓\n'
label ''
printf '└──┴──┘ ╰──╯     ║\n'

label icons
printf '\xee\x82\xa0 \xef\x84\x93 \xef\x87\x93 \xee\x9c\xa5 \xef\x90\x98 (nerd font)\n'

label wide
printf '中文字符 日本語 한국어 x end\n'

sleep "${SWATCH_HOLD:-600}"
