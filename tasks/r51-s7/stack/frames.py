#!/usr/bin/env python3
"""frames.py <binary> [regex]: native x86-64 frame bytes (pushes + every prologue sub rsp + ret) per fn."""
import re, subprocess, sys

def frames(binpath):
    out = subprocess.run(["objdump", "-d", "--no-show-raw-insn", "-C", binpath],
                         capture_output=True, text=True).stdout
    res = {}
    cur, left, subs, pushes = None, 0, 0, 0
    head = re.compile(r"^[0-9a-f]+ <(.*)>:$")
    for line in out.split("\n"):
        h = head.match(line)
        if h:
            cur, left, subs, pushes = h.group(1), 40, 0, 0
            res[cur] = 8
            continue
        if cur is None or left == 0:
            continue
        left -= 1
        if re.search(r"\bpush\s+%r", line):
            pushes += 1
        m = re.search(r"\bsub\s+\$0x([0-9a-f]+),%rsp", line)
        if m:
            subs += int(m.group(1), 16)
            # a stack probe (`sub $0x1000`) is followed by more subs; stop after a non-page sub
            if int(m.group(1), 16) != 0x1000:
                left = 0
        if re.search(r"\b(call|ret|jmp)\b", line) and subs:
            left = 0
        res[cur] = 8 + 8 * pushes + subs
    return res

if __name__ == "__main__":
    r = frames(sys.argv[1])
    pat = re.compile(sys.argv[2]) if len(sys.argv) > 2 else re.compile(r"interp")
    for k, v in sorted(r.items()):
        if pat.search(k):
            print(f"{v:8d} {k}")
