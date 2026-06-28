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

# exemplar TITLE  DIR  SAFE_FILE  EXPLOIT_FILE  IMPACT
exemplar() {
  local title="$1" dir="$2" safe="$3" evil="$4" impact="$5"
  bold "$title"
  local D="$CVE/$dir"
  if checks_clean "$D/$safe"; then
    grn "  ✓ $safe (safe) — axon check clean (exit 0)"
  else
    red "  ✗ $safe should check clean"; fail=1
  fi
  local n; n=$(e1001 "$D/$evil")
  if [ "$n" -ge 3 ] && ! checks_clean "$D/$evil"; then
    grn "  ✓ $evil (exploit) — REFUSED: $n× E1001 ($impact)"
    "$AXON" check "$D/$evil" 2>&1 | python3 -c "
import sys, json
for l in sys.stdin:
    if not l.strip().startswith('{'): continue
    d=json.loads(l)
    if d.get('code')=='E1001': print('      E1001  '+d['message'].split(chr(10))[0])
" 2>/dev/null
  else
    red "  ✗ $evil should be refused with >=3 E1001 (got $n)"; fail=1
  fi
  rule
}

exemplar "CVE-2024-34359 — llama-cpp-python (Jinja2 SSTI → RCE)  [AI]" \
  CVE-2024-34359 model_loader.ax model_loader_ssti.ax "RCE / file access / outbound"
exemplar "CVE-2024-2624 — lollms-webui (path traversal + file write)  [AI]" \
  CVE-2024-2624 file_store.ax file_store_traversal.ax "traversal read / out-of-lane write / dynamic path"
exemplar "CVE-2024-32964 — Lobe Chat (unauthenticated SSRF)  [AI]" \
  CVE-2024-32964 proxy.ax proxy_ssrf.ax "metadata theft / intranet / dynamic URL"

bold "The point"
echo "  Axon does not detect the bug — it removes the authority the bug needs (exec/fs/net)."
echo "  Three capability types, three real CVEs, same outcome: the impact is unrepresentable."
echo "  Same shape as ~19 of the 40 CVE-Bench CVEs (TRIAGE.md). The AI-stack ones cluster here."
echo
[ "$fail" = "0" ] && grn "cve: OK — every exemplar behaved as claimed" || red "cve: a check did not match its claim"
exit "$fail"
