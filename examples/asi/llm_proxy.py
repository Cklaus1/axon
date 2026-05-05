#!/usr/bin/env python3
"""
llm_proxy.py — minimal Anthropic API proxy for cost/latency observability.

Stdlib only.  Sits between Axon and api.anthropic.com, logs per-call:
  - request timestamp, prompt length, model, tools-on
  - response wall-clock latency, status code, token usage (input/output)
  - approximate $-cost computed from token counts × per-model pricing

Usage:
  # Terminal A — start the proxy:
  python3 examples/asi/llm_proxy.py
  #   listening on http://localhost:8088 ; logging to ./llm_calls.jsonl

  # Terminal B — point Axon at the proxy:
  export ANTHROPIC_BASE_URL=http://localhost:8088
  export ANTHROPIC_API_KEY=sk-ant-...
  ./examples/asi/run.sh run

  # Terminal C (optional) — analyze:
  python3 -c "
  import json
  for line in open('llm_calls.jsonl'):
      r = json.loads(line)
      print(f\"  {r['latency_ms']:5d}ms  in={r['in_tokens']:4d}  out={r['out_tokens']:4d}  cost=\\${r['cost_usd']:.4f}\")
  "

Replace this with Helicone or Langfuse when you outgrow stdlib.
"""
from __future__ import annotations

import json
import os
import sys
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, HTTPServer

UPSTREAM = os.environ.get("UPSTREAM_BASE", "https://api.anthropic.com")
LOG_PATH = os.environ.get("LLM_PROXY_LOG", "llm_calls.jsonl")
PORT = int(os.environ.get("LLM_PROXY_PORT", "8088"))

# Per-model price table (USD per 1M tokens), input / output.
# Source: anthropic.com/pricing as of 2026-Q2; update as needed.
PRICES = {
    "claude-sonnet-4-6": (3.00, 15.00),
    "claude-opus-4-7":   (15.00, 75.00),
    "claude-haiku-4-5":  (0.80, 4.00),
}


def cost_usd(model: str, in_tokens: int, out_tokens: int) -> float:
    in_p, out_p = PRICES.get(model, (3.0, 15.0))
    return (in_tokens * in_p / 1_000_000) + (out_tokens * out_p / 1_000_000)


def log_event(rec: dict) -> None:
    rec["ts_ms"] = int(time.time() * 1000)
    with open(LOG_PATH, "a") as f:
        f.write(json.dumps(rec) + "\n")


class Proxy(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        pass  # silence default access log; we write our own

    def do_POST(self):
        path = self.path
        upstream_url = f"{UPSTREAM.rstrip('/')}{path}"
        n = int(self.headers.get("content-length", "0"))
        body = self.rfile.read(n) if n else b""

        # Pull out hints from the request body for the log.
        try:
            req = json.loads(body or b"{}")
        except json.JSONDecodeError:
            req = {}
        model = req.get("model", "?")
        prompt_chars = sum(
            len(m.get("content", "")) if isinstance(m.get("content"), str) else 0
            for m in req.get("messages", [])
        )
        tools_on = "tools" in req and bool(req.get("tools"))

        # Forward request to upstream.
        fwd = urllib.request.Request(upstream_url, data=body, method="POST")
        for h in ("x-api-key", "anthropic-version", "content-type"):
            v = self.headers.get(h)
            if v:
                fwd.add_header(h, v)

        t_start = time.monotonic()
        status = 0
        resp_body = b""
        try:
            with urllib.request.urlopen(fwd, timeout=120) as resp:
                status = resp.status
                resp_body = resp.read()
        except urllib.error.HTTPError as e:
            status = e.code
            resp_body = e.read() if e.fp else b""
        except Exception as e:
            status = 599
            resp_body = json.dumps({"proxy_error": str(e)}).encode()
        latency_ms = int((time.monotonic() - t_start) * 1000)

        # Try to extract usage from the response body.
        in_tok, out_tok = 0, 0
        try:
            j = json.loads(resp_body)
            usage = j.get("usage") or {}
            in_tok = int(usage.get("input_tokens") or 0)
            out_tok = int(usage.get("output_tokens") or 0)
        except (json.JSONDecodeError, ValueError):
            pass

        log_event({
            "path": path,
            "model": model,
            "tools_on": tools_on,
            "prompt_chars": prompt_chars,
            "in_tokens": in_tok,
            "out_tokens": out_tok,
            "cost_usd": round(cost_usd(model, in_tok, out_tok), 6),
            "latency_ms": latency_ms,
            "status": status,
            "resp_bytes": len(resp_body),
        })

        # Mirror upstream response back to caller.
        self.send_response(status)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(resp_body)))
        self.end_headers()
        self.wfile.write(resp_body)


def main() -> int:
    addr = ("0.0.0.0", PORT)
    print(f"llm_proxy: listening on http://localhost:{PORT}", file=sys.stderr)
    print(f"           upstream:    {UPSTREAM}", file=sys.stderr)
    print(f"           log:         {os.path.abspath(LOG_PATH)}", file=sys.stderr)
    print(f"point Axon at it: export ANTHROPIC_BASE_URL=http://localhost:{PORT}", file=sys.stderr)
    HTTPServer(addr, Proxy).serve_forever()
    return 0


if __name__ == "__main__":
    sys.exit(main())
