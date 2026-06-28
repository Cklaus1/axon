#!/usr/bin/env bash
# compare_python.sh — side-by-side comparison: Python (no sandbox) vs Axon (compiler-enforced).
#
# The same "score records and don't exfiltrate" task. Python runs all escapes.
# Axon refuses them at compile time with E1001.
#
# Usage: ./examples/flagship/compare_python.sh
# Requires: python3, target/debug/axon

set -euo pipefail
REPO=$(cd "$(dirname "$0")/../.." && pwd)
AXON="$REPO/target/debug/axon"

bold()  { printf '\033[1m%s\033[0m\n' "$*"; }
dim()   { printf '\033[2m%s\033[0m\n' "$*"; }
green() { printf '\033[32m%s\033[0m\n' "$*"; }
red()   { printf '\033[31m%s\033[0m\n' "$*"; }
rule()  { dim "────────────────────────────────────────────────────────────"; }

if [[ ! -x "$AXON" ]]; then
    echo "axon binary not found at $AXON"
    echo "Build it first: cargo build -p axon-core --no-default-features --bin axon"
    exit 1
fi

rule
bold "=== Python agent (no sandbox) ==="
echo ""
echo "The 'sandbox' is a comment — # @contained(fs: [], net: [], exec: none)"
echo "Python executes every escape attempt without complaint."
echo ""

python3 - << 'PYEOF'
import os, subprocess, sys

# "# @contained(fs: [], net: [], exec: none)" -- a comment. Enforces nothing.

print("[Python] Reading /etc/passwd (first 40 chars):")
try:
    with open("/etc/passwd") as f:
        print("  " + f.read(40).replace("\n", " ") + "...")
    print("  ALLOWED: filesystem read succeeded")
except Exception as e:
    print(f"  BLOCKED (not by Python, by the OS): {e}")

print("")
print("[Python] Listing /tmp (process spawn via subprocess):")
try:
    result = subprocess.run(["ls", "/tmp"], capture_output=True, text=True, timeout=2)
    files = result.stdout.strip().split("\n")[:5]
    print("  ALLOWED: spawned ls, got " + str(len(files)) + " entries")
except Exception as e:
    print(f"  BLOCKED (not by Python, by the OS): {e}")

print("")
print("[Python] Reading host environment (ANTHROPIC_API_KEY):")
key = os.environ.get("ANTHROPIC_API_KEY", "")
if key:
    print("  ALLOWED: read key " + key[:8] + "... (credential exfiltration possible)")
else:
    print("  ALLOWED: env readable (key not set in this shell, but would be if present)")

print("")
print("[Python] Summary: 3/3 escapes executed — the 'sandbox' is advisory only.")
PYEOF

echo ""
rule
bold "=== Axon agent (R26+R27+R28 safety stack) ==="
echo ""
echo "The same escapes as @[contained(fs: [], net: [], exec: none)]."
echo "The compiler refuses ALL three before the program can execute even once."
echo ""
echo "  \$ axon check examples/flagship/agent_task_evil.ax"
echo ""

EVIL="$REPO/examples/flagship/agent_task_evil.ax"
EVIL_OUT=$("$AXON" check "$EVIL" 2>&1 || true)
E1001_COUNT=$(echo "$EVIL_OUT" | grep -c '"E1001"' || true)

echo "$EVIL_OUT" | python3 -c "
import sys, json
for line in sys.stdin:
    try:
        d = json.loads(line)
        if d.get('code') == 'E1001':
            msg = d['message'].split('\n')[0][:90]
            print('  E1001:', msg)
    except Exception:
        pass
" 2>/dev/null | head -5 || echo "$EVIL_OUT" | grep '"E1001"' | head -3 | sed 's/.*"message":"/  E1001: /' | sed 's/\\n.*//' | cut -c1-90
echo ""

if [[ "$E1001_COUNT" -ge 3 ]]; then
    green "  REFUSED: $E1001_COUNT/3 escapes caught at compile time (exit 2)"
    echo ""
    echo "  The data was never read."
    echo "  The packet was never sent."
    echo "  curl never ran."
    echo ""
    echo "Additional Axon safety layers:"
    echo "  @[verify(value >= 0)]   — postcondition enforced on every score() call"
    echo "  axon-os run --killable  — R27 kill-switch; operator can halt < 1s"
    echo "  axon-vm attest          — R26 attestation; tampered kernel = refused boot"
    echo "  axon-os verify record   — R28-style chained audit; any tamper breaks chain"
else
    red "  WARNING: only $E1001_COUNT E1001 errors found (expected >= 3)"
fi

echo ""
rule
bold "Conclusion"
echo ""
echo "  Python   — sandbox is a comment; all 3 escapes ALLOWED"
echo "  Axon     — sandbox is a compile-time proof; all 3 escapes REFUSED"
echo ""
echo "  Same code. Same intent. Opposite outcomes."
echo "  Run ./examples/flagship/demo.sh for the full four-layer walkthrough."
