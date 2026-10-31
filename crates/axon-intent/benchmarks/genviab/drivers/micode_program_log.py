#!/usr/bin/env python3
"""micode-program.sh's second half: micode's jsonl events on stdin -> the
reply text on stdout, one accounting row appended to $GENVIAB_MODEL_LOG.

Exit 0 with the reply; exit 1 (row logged, `error` set) when micode failed,
reported a session error, produced no turn or an empty reply; exit 4 when the
shell half refused the prompt (GENVIAB_REFUSED). A failed call still spent
tokens, so its row is written before the exit.
"""
import json
import os
import sys

E = os.environ


def main():
    events = []
    for line in sys.stdin.read().splitlines():
        try:
            events.append(json.loads(line))
        except ValueError:
            pass
    rc = int(E.get("GENVIAB_MICODE_RC", "0"))
    errors = [e for e in events if e.get("type") == "session_error"]
    retries = [e for e in events if e.get("type") == "retry"]
    start = next((e for e in events if e.get("type") == "session_start"), {})
    ends = [e for e in events if e.get("type") == "turn_end"]
    # The reply is the LAST turn's text (one turn: the mode has no tools).
    text = ""
    for e in events:
        if e.get("type") == "turn_start":
            text = ""
        elif e.get("type") == "assistant_delta":
            text += e.get("text") or ""

    def usage(k):
        return sum((e.get("usage") or {}).get(k) or 0 for e in ends)

    row = {
        "transport": "micode",
        "model": [start["model"]] if start.get("model") else [],
        "requested_model": E.get("GENVIAB_REQUESTED_MODEL"),
        "effort": E.get("GENVIAB_EFFORT") or None,
        "micode_version": start.get("micode_version"),
        "task": E.get("GENVIAB_TASK"),
        "arm": E.get("GENVIAB_ARM"),
        "prompt_bytes": int(E.get("GENVIAB_PROMPT_BYTES", "0")),
        "prompt_sha256": E.get("GENVIAB_PROMPT_SHA256"),
        "cost_usd": None,
        "duration_api_ms": int(E.get("GENVIAB_CALL_MS", "0")),
        "input_tokens": usage("input_tokens"),
        "output_tokens": usage("output_tokens"),
        "cache_read": usage("cache_read_input_tokens"),
        "cache_create": usage("cache_creation_input_tokens"),
        "turns": len(ends),
        "exit": rc,
        "session_errors": [json.dumps(e)[:400] for e in errors],
        "retries": [json.dumps(e)[:400] for e in retries],
        "reply_bytes": len(text.encode()),
    }
    why, code = None, 0
    if E.get("GENVIAB_REFUSED"):
        why, code = E["GENVIAB_REFUSED"], 4
    elif rc != 0:
        why, code = f"micode exited {rc}", 1
    elif errors:
        why, code = "micode reported a session error", 1
    elif not ends:
        why, code = "micode produced no turn_end event", 1
    elif not text.strip():
        why, code = "the reply is empty", 1
    row["error"] = why
    with open(E.get("GENVIAB_MODEL_LOG", "/dev/null"), "a") as fh:
        fh.write(json.dumps(row) + "\n")
    if why:
        sys.stderr.write("driver: " + why + "\n")
        return code
    sys.stdout.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
