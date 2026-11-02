#!/usr/bin/env python3
"""Mirror of axon-vm `bpf_allowlist` (crates/axon-vm/src/main.rs:2100-2151 at b3a9c56e):
LD nr; JEQ per hinted syscall -> ALLOW; default KILL_PROCESS; no arch check.
usage: hintfilter.py AXMETA [extra syscall names...] -> base64 on stdout
"""
import base64
import json
import re
import struct
import sys

SRC = "/home/cklaus/projects/axon-lanes/b2-ce9/crates/axon-vm/src/main.rs"
text = open(SRC).read()
table_src = text[text.index("static SYSCALL_TABLE"):]
table_src = table_src[: table_src.index("];")]
TABLE = {n: int(v) for n, v in re.findall(r'\("([a-z0-9_]+)",\s*(\d+)\)', table_src)}

names = json.load(open(sys.argv[1]))["syscall_hint"] + sys.argv[2:]
nrs = []
for n in names:
    if n in TABLE:
        nrs.append(TABLE[n])
    else:
        sys.stderr.write(f"unknown syscall '{n}', skipping\n")
n = len(nrs)
p = [struct.pack("<HBBI", 0x20, 0, 0, 0)]
for j, nr in enumerate(nrs):
    p.append(struct.pack("<HBBI", 0x15, n - j, 0, nr))
p.append(struct.pack("<HBBI", 0x06, 0, 0, 0x80000000))
p.append(struct.pack("<HBBI", 0x06, 0, 0, 0x7FFF0000))
sys.stdout.write(base64.b64encode(b"".join(p)).decode())
