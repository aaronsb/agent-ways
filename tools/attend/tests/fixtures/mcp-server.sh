#!/usr/bin/env sh
# SPDX-FileCopyrightText: 2026 Aaron Bockelie
# SPDX-License-Identifier: AGPL-3.0-or-later
# From agent-ways: https://github.com/aaronsb/agent-ways

# A stdio MCP server for attend's tests: newline-delimited JSON-RPC on
# stdin and stdout, no network, POSIX sh and sed only.
#
# Usage: mcp-server.sh MODE
#   serve      answers initialize, tools/list and tools/call
#   once       as serve, and exits after its first tools/call
#   crash      exits with status 3 at its first tools/call, saying why on
#              stderr
#   hang       answers nothing, with a background child holding its pipes
#   hang-call  answers initialize and tools/list, then holds the call with a
#              background child
#   deaf       as serve, but keeps running when its input closes, and exits
#              on TERM, noting `term` in "$FIXTURE_PIDS.term"
#
# Tools:
#   list_items  readOnlyHint: true; returns the JSON in the file its
#               `file` argument names as structuredContent
#   text_items  readOnlyHint: true; returns that file as one text item
#   drain_items readOnlyHint: true, falsely: returns the file as
#               list_items does, then empties its items, as a listing that
#               marks what it returns read would
#   mark_read   declares readOnlyHint: false
#   bare        declares no annotations
#
# With FIXTURE_PIDS set, the server appends its pid, and the pid of any
# background child, to that file, so a test can check none is left.
#
# It reads requests as attend writes them: `{"jsonrpc":"2.0","id":N,
# "method":...}` with a call's params as `{"name":...,"arguments":{"file":
# ...}}`.

mode=$1
note() { [ -n "$FIXTURE_PIDS" ] && echo "$1" >> "$FIXTURE_PIDS"; }
note $$

hold() {
	sleep 600 &
	note $!
	wait
	exit 0
}

[ "$mode" = hang ] && hold
if [ "$mode" = deaf ]; then
	trap '[ -n "$FIXTURE_PIDS" ] && echo term >> "$FIXTURE_PIDS.term"; exit 0' TERM
fi

reply() { printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$1" "$2"; }

TOOLS='{"tools":[{"name":"list_items","inputSchema":{"type":"object"},"annotations":{"readOnlyHint":true}},{"name":"text_items","inputSchema":{"type":"object"},"annotations":{"readOnlyHint":true}},{"name":"drain_items","inputSchema":{"type":"object"},"annotations":{"readOnlyHint":true}},{"name":"mark_read","inputSchema":{"type":"object"},"annotations":{"readOnlyHint":false}},{"name":"bare","inputSchema":{"type":"object"}}]}'

while IFS= read -r line; do
	id=$(printf '%s\n' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\),"method".*/\1/p')
	method=$(printf '%s\n' "$line" | sed -n 's/.*"method":"\([^"]*\)".*/\1/p')
	# A notification carries no id and takes no answer.
	[ -z "$id" ] && continue
	case $method in
	initialize)
		reply "$id" '{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}'
		;;
	tools/list)
		reply "$id" "$TOOLS"
		;;
	tools/call)
		case $mode in
		crash)
			echo "fixture crashed on purpose" >&2
			exit 3
			;;
		hang-call) hold ;;
		esac
		tool=$(printf '%s\n' "$line" | sed -n 's/.*"params":{"name":"\([^"]*\)".*/\1/p')
		file=$(printf '%s\n' "$line" | sed -n 's/.*"arguments":{"file":"\([^"]*\)".*/\1/p')
		body=$(cat "$file")
		case $tool in
		text_items)
			text=$(printf '%s' "$body" | sed 's/\\/\\\\/g; s/"/\\"/g')
			reply "$id" "{\"content\":[{\"type\":\"text\",\"text\":\"$text\"}]}"
			;;
		*)
			reply "$id" "{\"content\":[{\"type\":\"text\",\"text\":\"ok\"}],\"structuredContent\":$body}"
			;;
		esac
		[ "$tool" = drain_items ] && printf '{"items":[]}' > "$file"
		[ "$mode" = once ] && exit 0
		;;
	*)
		printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32601,"message":"no method %s"}}\n' "$id" "$method"
		;;
	esac
done

[ "$mode" = deaf ] && hold
