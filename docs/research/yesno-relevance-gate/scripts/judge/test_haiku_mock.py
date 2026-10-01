#!/usr/bin/env python3
"""Exercise the haiku backend against a mocked anthropic client. No network, no key.

Checks the request shape (model, temperature 0, small max_tokens, forced strict tool)
and the judgement -> p_yes mapping, including error paths.
"""
import json
import sys
from types import SimpleNamespace

import anthropic
import httpx

import judge


class FakeMessages:
    def __init__(self, replies):
        self.replies = list(replies)
        self.calls = []

    def create(self, **kw):
        self.calls.append(kw)
        r = self.replies.pop(0)
        if isinstance(r, Exception):
            raise r
        return r


def tool_reply(relevant, confidence):
    block = SimpleNamespace(type="tool_use", name="record_judgement",
                            input={"relevant": relevant, "confidence": confidence})
    return SimpleNamespace(content=[block], stop_reason="tool_use")


def main():
    req = httpx.Request("POST", "https://api.anthropic.com/v1/messages")
    rate = anthropic.RateLimitError("slow down", response=httpx.Response(429, request=req), body=None)
    msgs = FakeMessages([
        tool_reply(True, 0.9),
        tool_reply(False, 0.8),
        tool_reply(True, 1.7),  # clamped
        SimpleNamespace(content=[SimpleNamespace(type="text", text="yes")], stop_reason="end_turn"),
        rate,
    ])
    client = SimpleNamespace(messages=msgs)
    items = [{"id": f"i{k}", "summary": "database schema migrations",
              "turns": [{"role": "user", "text": "old"}, {"role": "assistant", "text": "x" * 900},
                        {"role": "user", "text": "split migration 0043 please"}]} for k in range(5)]
    args = judge.parse_args(["--backend", "haiku", "--in", "-", "--out", "-", "--turns", "2", "--max-turn-chars", "600"])
    rows = list(judge.run_haiku(items, args, client=client))

    got = [(r["p_yes"], r["answer"], r["error"] is None) for r in rows]
    want = [(0.9, "yes", True), (0.2, "no", True), (1.0, "yes", True), (None, None, False), (None, None, False)]
    assert all(g[1:] == w[1:] and (w[0] is None or abs(g[0] - w[0]) < 1e-9) for g, w in zip(got, want)), got

    c = msgs.calls[0]
    assert c["model"] == "claude-haiku-4-5-20251001" and c["extra_body"] == {"temperature": 0} and c["max_tokens"] <= 64
    assert c["tool_choice"] == {"type": "tool", "name": "record_judgement"} and c["tools"][0]["strict"] is True
    content = c["messages"][0]["content"]
    assert judge.INSTRUCTION in content and "Assistant: " + "x" * 600 + "..." in content and "old" not in content
    assert rows[4]["error"].startswith("rate_limited")
    for r in rows:
        print(json.dumps(r))
    print("haiku mock: OK", file=sys.stderr)


if __name__ == "__main__":
    main()
