#!/usr/bin/env python3
"""A generator that is NOT told the answer.

Reads the prompt on stdin, writes one proposed body on stdout — the `cmd:`
protocol. It knows nothing about which defect was injected; it proposes
plausible single-token repairs in a fixed order and relies on the loop to tell
it which ones have already been rejected.

That last part is the point. The prompt carries the attempts this episode has
already tried, so successive calls must propose DIFFERENT ideas. A generator
that ignored that would repeat its first guess forever, which is exactly the
failure the feedback mechanism exists to prevent — and until now that mechanism
had only a unit test, never a real consumer.
"""
import re
import sys

prompt = sys.stdin.read()

# The body under repair, from the fenced block after "Its body is currently:".
m = re.search(r"Its body is currently:\n```\n(.*?)\n?```", prompt, re.S)
if not m:
    sys.stderr.write("no body in the prompt\n")
    sys.exit(2)
body = m.group(1)

# Everything already rejected this episode, from the fenced blocks under the
# "Already tried" section. Normalised, because a proposal that differs only in
# trailing whitespace is the same idea.
tried = set()
sec = prompt.split("Already tried in this episode")
if len(sec) > 1:
    for b in re.findall(r"```\n(.*?)\n?```", sec[1], re.S):
        tried.add(b.strip())

EDITS = [
    ("-", "+"), ("+", "-"), ("+", "*"), ("*", "+"), ("*", "-"),
    ("<", "<="), ("<=", "<"), (">", ">="), (">=", ">"),
    ("||", "&&"), ("&&", "||"),
    ("false", "true"), ("true", "false"),
]


def candidates(b):
    for old, new in EDITS:
        # Each occurrence, not just the first: the defect is rarely the first
        # operator in the body.
        for i in range(b.count(old)):
            parts = b.split(old)
            if len(parts) <= i + 1:
                break
            yield old.join(parts[: i + 1]) + new + old.join(parts[i + 1 :])
    # Integer literals, up and down.
    for m2 in re.finditer(r"(?<![\w.])(\d+)(?![\w.])", b):
        v = int(m2.group(1))
        for nv in (v - 1, v + 1):
            if nv >= 0:
                yield b[: m2.start()] + str(nv) + b[m2.end() :]
    # Two simple arguments, swapped.
    for m3 in re.finditer(r"\b([a-z_]\w*)\(\s*([a-z_][\w.]*)\s*,\s*([a-z_][\w.]*)\s*\)", b):
        yield b[: m3.start()] + f"{m3.group(1)}({m3.group(3)}, {m3.group(2)})" + b[m3.end() :]


for cand in candidates(body):
    if cand.strip() and cand.strip() != body.strip() and cand.strip() not in tried:
        sys.stdout.write(cand)
        sys.exit(0)

# Out of ideas. Declined, and the loop reports that rather than inventing one.
sys.stderr.write("no untried single-token edit remains\n")
sys.exit(1)
