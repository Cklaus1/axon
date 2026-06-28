#!/usr/bin/env bash
# run.sh — CVE-backed flagship: real critical CVEs whose impact Axon refuses by
# construction. For each exemplar, the safe loader checks clean and the exploit
# payload is refused (E1001) at compile time. See TRIAGE.md for the full 40-CVE table.
set -uo pipefail
REPO=$(cd "$(dirname "$0")/../../.." && pwd)
CVE="$REPO/examples/flagship/cve"
AXON="$REPO/target/debug/axon"

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
dim()  { printf '\033[2m%s\033[0m\n' "$*"; }
grn()  { printf '\033[32m%s\033[0m\n' "$*"; }
red()  { printf '\033[31m%s\033[0m\n' "$*"; }
rule() { dim "────────────────────────────────────────────────────────────"; }

[ -x "$AXON" ] || { red "build first: cargo build -p axon-core --no-default-features --bin axon"; exit 1; }

e1001() { "$AXON" check "$1" 2>&1 | grep -c '"E1001"'; }
checks_clean() { "$AXON" check "$1" >/dev/null 2>&1; }

bold "=== CVE-Bench × Axon — impact refused by construction ==="
echo "40 critical real-world CVEs triaged in TRIAGE.md; ~half are a class Axon refuses."
echo "Each exemplar: the safe version compiles, the exploit payload is a compile error."
rule

fail=0

# ── CVE-2024-34359 — llama-cpp-python Jinja2 SSTI → RCE ───────────────────────
bold "CVE-2024-34359 — llama-cpp-python (Jinja2 SSTI → RCE)  [AI tooling]"
echo "A malicious .gguf model file's chat template achieves RCE on load."
D="$CVE/CVE-2024-34359"
if checks_clean "$D/model_loader.ax"; then
  grn "  ✓ model_loader.ax (safe loader) — axon check clean (exit 0)"
else
  red "  ✗ model_loader.ax should check clean"; fail=1
fi
N=$(e1001 "$D/model_loader_ssti.ax")
if [ "$N" -ge 3 ] && ! checks_clean "$D/model_loader_ssti.ax"; then
  grn "  ✓ model_loader_ssti.ax (SSTI payload) — REFUSED: $N× E1001 (RCE / file access / outbound)"
  "$AXON" check "$D/model_loader_ssti.ax" 2>&1 | python3 -c "
import sys, json
for l in sys.stdin:
    if not l.strip().startswith('{'): continue
    d=json.loads(l)
    if d.get('code')=='E1001': print('      E1001  '+d['message'].split(chr(10))[0])
" 2>/dev/null
else
  red "  ✗ model_loader_ssti.ax should be refused with >=3 E1001 (got $N)"; fail=1
fi
rule

bold "The point"
echo "  Axon does not detect the SSTI bug — it removes the authority the bug needs."
echo "  With exec: none, an attacker who fully controls the template still cannot run code."
echo "  Same shape as ~19 of the 40 CVEs (TRIAGE.md). The AI-stack ones cluster here."
echo
[ "$fail" = "0" ] && grn "cve: OK — every exemplar behaved as claimed" || red "cve: a check did not match its claim"
exit "$fail"
