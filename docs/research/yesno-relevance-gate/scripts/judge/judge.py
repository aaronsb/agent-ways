#!/usr/bin/env python3
"""Yes/no relevance judge: is this way relevant to the last turns of the conversation?

Backends:
  local  Qwen3-Reranker-0.6B. P(yes) = softmax over the "yes"/"no" logits at the
         next position. Engine `llamacpp` (default) drives llama-server --reranking;
         engine `transformers` is the CPU fp32 reference.
  haiku  claude-haiku-4-5-20251001 with a forced structured tool call
         {"relevant": bool, "confidence": float}; p_yes = confidence if relevant
         else 1 - confidence.

Input JSONL:  {"id", "summary", "turns": [{"role": "user"|"assistant", "text"}, ...]}  (oldest -> newest)
Output JSONL: {"id", "backend", "p_yes", "answer", "ms", "error"}
"""

import argparse
import json
import os
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent

# The instruction shared by every backend, selected with --instruction. Vary it here.
INSTRUCTIONS = {
    "default": "Decide whether this guidance is relevant to what the conversation is doing "
               "in its most recent turns.",
    "alt": "Is this guidance useful for what the conversation is currently working on?",
}
INSTRUCTION = INSTRUCTIONS["default"]

# Qwen3-Reranker chat frame, verbatim from the model card.
QWEN_SYSTEM = (
    'Judge whether the Document meets the requirements based on the Query and the '
    'Instruct provided. Note that the answer can only be "yes" or "no".'
)
QWEN_PREFIX = f"<|im_start|>system\n{QWEN_SYSTEM}<|im_end|>\n<|im_start|>user\n"
QWEN_SUFFIX = "<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n"

DEFAULT_HF = HERE / "hf" / "Qwen3-Reranker-0.6B"
DEFAULT_GGUF = HERE / "gguf" / "qwen3-reranker-0.6b-f16-raw.gguf"
DEFAULT_SERVER_BIN = HERE / "build-cpu" / "bin" / "llama-server"

HAIKU_MODEL = "claude-haiku-4-5-20251001"
KEY_FILE = Path.home() / ".config" / "agent-ways" / "anthropic-api-key"


# ---------------------------------------------------------------- rendering

def render_turns(turns, n_turns=None, max_chars=None):
    """Turns as 'User: ... / Assistant: ...' lines, last n_turns kept, each capped."""
    if n_turns:
        turns = turns[-n_turns:]
    lines = []
    for t in turns:
        text = " ".join(t["text"].split())
        if max_chars and len(text) > max_chars:
            text = text[:max_chars].rstrip() + "..."
        speaker = "User" if t["role"] == "user" else "Assistant"
        lines.append(f"{speaker}: {text}")
    return "\n".join(lines)


def render_parts(item, n_turns, max_chars, instruction="default"):
    """The shared pieces every backend renders from."""
    return {
        "instruction": INSTRUCTIONS[instruction],
        "summary": item["summary"],
        "turns": render_turns(item["turns"], n_turns, max_chars),
    }


def render_qwen(parts, orientation):
    """Full Qwen3-Reranker prompt. orientation 'turns-query': Query=turns, Document=summary."""
    if orientation == "turns-query":
        query, doc = parts["turns"], parts["summary"]
    else:
        query, doc = parts["summary"], parts["turns"]
    body = f"<Instruct>: {parts['instruction']}\n<Query>: {query}\n<Document>: {doc}"
    return QWEN_PREFIX + body + QWEN_SUFFIX


def render_plain(parts):
    """Prompt for a chat model."""
    return (
        f"{parts['instruction']}\n\n"
        f"<guidance>\n{parts['summary']}\n</guidance>\n\n"
        f"<conversation>\n{parts['turns']}\n</conversation>"
    )


def result(item_id, backend, p_yes, ms, error=None):
    return {
        "id": item_id,
        "backend": backend,
        "p_yes": p_yes,
        "answer": None if p_yes is None else ("yes" if p_yes >= 0.5 else "no"),
        "ms": round(ms, 3),
        "error": error,
    }


def group_by_turns(items, n_turns, max_chars):
    """Consecutive items sharing the same rendered turns form one batch (one prompt's candidates)."""
    groups, key = [], None
    for it in items:
        k = render_turns(it["turns"], n_turns, max_chars)
        if k != key:
            groups.append([])
            key = k
        groups[-1].append(it)
    return groups


# ---------------------------------------------------------------- local: transformers

class TransformersJudge:
    def __init__(self, model_dir, threads=None):
        import torch
        from transformers import AutoModelForCausalLM, AutoTokenizer
        if threads:
            torch.set_num_threads(threads)
        self.torch = torch
        t0 = time.perf_counter()
        self.tok = AutoTokenizer.from_pretrained(model_dir, padding_side="left")
        self.model = AutoModelForCausalLM.from_pretrained(model_dir, dtype=torch.float32).eval()
        self.load_ms = (time.perf_counter() - t0) * 1000
        self.yes_id = self.tok.convert_tokens_to_ids("yes")
        self.no_id = self.tok.convert_tokens_to_ids("no")

    def p_yes(self, prompts):
        torch = self.torch
        enc = self.tok(prompts, padding=True, add_special_tokens=False, return_tensors="pt")
        with torch.no_grad():
            logits = self.model(**enc).logits[:, -1, :]
        pair = torch.stack([logits[:, self.yes_id], logits[:, self.no_id]], dim=1)
        return torch.softmax(pair.float(), dim=1)[:, 0].tolist()


# ---------------------------------------------------------------- local: llama.cpp

class LlamaCppJudge:
    """Talks to llama-server --reranking on a GGUF whose rerank template is '{query}{document}'.

    The prompt is rendered here and sent as the document with an empty query, so the
    instruction stays in INSTRUCTION. The server's rank pooling for Qwen3 returns
    softmax([yes, no])[0] as relevance_score.
    """

    def __init__(self, url=None, gguf=DEFAULT_GGUF, server_bin=DEFAULT_SERVER_BIN,
                 threads=None, ngl=0, port=18333):
        self.proc = None
        t0 = time.perf_counter()
        if url is None:
            url = f"http://127.0.0.1:{port}"
            cmd = [str(server_bin), "-m", str(gguf), "--reranking", "--port", str(port),
                   "-c", "8192", "-b", "8192", "-ub", "8192", "-np", "1", "-ngl", str(ngl)]
            if threads:
                cmd += ["-t", str(threads)]
            self.proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            self._wait_ready(url)
        self.url = url
        self.load_ms = (time.perf_counter() - t0) * 1000

    def _wait_ready(self, url, timeout=120):
        deadline = time.time() + timeout
        while time.time() < deadline:
            if self.proc.poll() is not None:
                raise RuntimeError(f"llama-server exited with {self.proc.returncode}")
            try:
                with urllib.request.urlopen(url + "/health", timeout=1) as r:
                    if r.status == 200:
                        return
            except Exception:
                pass
            time.sleep(0.05)
        raise RuntimeError("llama-server did not become ready")

    def p_yes(self, prompts):
        body = json.dumps({"query": "", "documents": prompts}).encode()
        req = urllib.request.Request(self.url + "/rerank", data=body,
                                     headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=120) as r:
            out = json.load(r)
        scores = [None] * len(prompts)
        for res in out["results"]:
            scores[res["index"]] = res["relevance_score"]
        return scores

    def close(self):
        if self.proc:
            self.proc.terminate()
            self.proc.wait()


def run_local(items, args):
    if args.engine == "transformers":
        judge = TransformersJudge(args.hf_model, args.threads)
    else:
        judge = LlamaCppJudge(args.server_url, args.gguf, args.server_bin, args.threads, args.ngl)
    print(f"[judge] {args.engine} load_ms={judge.load_ms:.1f}", file=sys.stderr)
    try:
        groups = group_by_turns(items, args.turns, args.max_turn_chars) if args.batch \
            else [[it] for it in items]
        for group in groups:
            prompts = [render_qwen(render_parts(it, args.turns, args.max_turn_chars, args.instruction),
                                   args.orientation)
                       for it in group]
            t0 = time.perf_counter()
            try:
                scores, err = judge.p_yes(prompts), None
            except Exception as e:  # one failed request must not drop the batch silently
                scores, err = [None] * len(group), f"{type(e).__name__}: {e}"
            ms = (time.perf_counter() - t0) * 1000 / len(group)
            for it, s in zip(group, scores):
                yield result(it["id"], f"local-{args.engine}", s, ms, err)
    finally:
        if isinstance(judge, LlamaCppJudge):
            judge.close()


# ---------------------------------------------------------------- haiku

HAIKU_SYSTEM = (
    "You are a relevance gate for a coding assistant's guidance system. You are given "
    "one piece of guidance and the most recent turns of a conversation. Call the "
    "record_judgement tool exactly once. `relevant` is your yes/no answer; `confidence` "
    "is your probability, from 0 to 1, that this answer is correct."
)

JUDGE_TOOL = {
    "name": "record_judgement",
    "description": "Record whether the guidance is relevant to the recent conversation.",
    "strict": True,
    "input_schema": {
        "type": "object",
        "properties": {
            "relevant": {"type": "boolean"},
            "confidence": {"type": "number"},
        },
        "required": ["relevant", "confidence"],
        "additionalProperties": False,
    },
}


def read_api_key(key_file=None):
    """ANTHROPIC_API_KEY, then --api-key-file, then KEY_FILE."""
    key = os.environ.get("ANTHROPIC_API_KEY")
    if key and key.strip():
        return key.strip()
    for path in (key_file, KEY_FILE):
        if path and Path(path).exists():
            return Path(path).read_text().strip()
    return None


def make_haiku_client(key_file=None):
    import anthropic
    key = read_api_key(key_file)
    if not key:
        raise SystemExit(f"no API key: set ANTHROPIC_API_KEY, pass --api-key-file, or write {KEY_FILE}")
    return anthropic.Anthropic(api_key=key)


def p_yes_from_judgement(judgement):
    conf = min(1.0, max(0.0, float(judgement["confidence"])))
    return conf if judgement["relevant"] else 1.0 - conf


def haiku_one(client, prompt):
    """Returns (p_yes, error)."""
    import anthropic
    try:
        resp = client.messages.create(
            model=HAIKU_MODEL,
            max_tokens=64,
            # SDK 1.x dropped the temperature kwarg; Haiku 4.5 still honours it.
            extra_body={"temperature": 0},
            system=HAIKU_SYSTEM,
            tools=[JUDGE_TOOL],
            tool_choice={"type": "tool", "name": JUDGE_TOOL["name"]},
            messages=[{"role": "user", "content": prompt}],
        )
    except anthropic.RateLimitError as e:
        return None, f"rate_limited: {e.message}"
    except anthropic.APIStatusError as e:
        return None, f"api_status_{e.status_code}: {e.message}"
    except anthropic.APIConnectionError as e:
        return None, f"connection: {e}"
    block = next((b for b in resp.content if b.type == "tool_use"), None)
    if block is None:
        return None, f"no tool_use block (stop_reason={resp.stop_reason})"
    try:
        return p_yes_from_judgement(block.input), None
    except (KeyError, TypeError, ValueError) as e:
        return None, f"bad judgement {block.input!r}: {e}"


def run_haiku(items, args, client=None):
    """Items are judged `--concurrency` at a time; ms is each call's own round trip."""
    from concurrent.futures import ThreadPoolExecutor
    client = client or make_haiku_client(args.api_key_file)

    def one(it):
        prompt = render_plain(render_parts(it, args.turns, args.max_turn_chars, args.instruction))
        t0 = time.perf_counter()
        p, err = haiku_one(client, prompt)
        return result(it["id"], "haiku", p, (time.perf_counter() - t0) * 1000, err)

    with ThreadPoolExecutor(max_workers=max(1, args.concurrency)) as pool:
        yield from pool.map(one, items)


# ---------------------------------------------------------------- cli

def parse_args(argv=None):
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--backend", choices=["local", "haiku"], required=True)
    ap.add_argument("--in", dest="inp", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--turns", type=int, default=None, help="keep the last N turns")
    ap.add_argument("--max-turn-chars", type=int, default=None, help="cap each turn at C chars")
    ap.add_argument("--engine", choices=["llamacpp", "transformers"], default="llamacpp")
    ap.add_argument("--instruction", choices=sorted(INSTRUCTIONS), default="default")
    ap.add_argument("--api-key-file", default=None, help="haiku: key file read when ANTHROPIC_API_KEY is unset")
    ap.add_argument("--concurrency", type=int, default=1, help="haiku: parallel requests")
    ap.add_argument("--orientation", choices=["turns-query", "summary-query"], default="turns-query")
    ap.add_argument("--batch", action="store_true",
                    help="score consecutive items with identical turns in one forward pass")
    ap.add_argument("--threads", type=int, default=None)
    ap.add_argument("--hf-model", default=str(DEFAULT_HF))
    ap.add_argument("--gguf", default=str(DEFAULT_GGUF))
    ap.add_argument("--server-bin", default=str(DEFAULT_SERVER_BIN))
    ap.add_argument("--server-url", default=None, help="use a running llama-server instead of spawning one")
    ap.add_argument("--ngl", type=int, default=0, help="layers to offload to GPU (ROCm build)")
    return ap.parse_args(argv)


def main(argv=None):
    args = parse_args(argv)
    with open(args.inp) as f:
        items = [json.loads(line) for line in f if line.strip()]
    rows = run_local(items, args) if args.backend == "local" else run_haiku(items, args)
    with open(args.out, "w") as f:
        for row in rows:
            f.write(json.dumps(row) + "\n")


if __name__ == "__main__":
    main()
