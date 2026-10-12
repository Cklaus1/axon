#!/usr/bin/env python3
"""Check `axon test --json --completion-key-stdin` output against the key.

usage: check_tokens.py KEY_HEX NDJSON_FILE TEST_NAME...

A test counts as passed only if its line says "status":"ok" AND carries
"completion" = HMAC-SHA256(key, b"axon-test-completion/1\\0" + name). A test
that ended through exit(0) or a plain early return is reported "ok" with no
token; candidate output on stdout cannot produce a valid token without the key.
Exit 0 iff every named test has a valid token.
"""
import hashlib
import hmac
import json
import sys

key = bytes.fromhex(sys.argv[1])
want = sys.argv[3:]
verified = set()
for line in open(sys.argv[2]):
    try:
        rec = json.loads(line)
    except ValueError:
        continue
    name = rec.get("name")
    if name not in want or rec.get("status") != "ok":
        continue
    tok = hmac.new(key, b"axon-test-completion/1\0" + name.encode(), hashlib.sha256).hexdigest()
    if hmac.compare_digest(rec.get("completion", ""), tok):
        verified.add(name)
for name in want:
    print(("verified  " if name in verified else "NO TOKEN  ") + name)
sys.exit(0 if verified == set(want) else 1)
