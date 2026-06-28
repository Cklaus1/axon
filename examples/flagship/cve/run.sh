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

ecode() { "$AXON" check "$1" 2>&1 | grep -c "\"$2\""; }

# exemplar TITLE  DIR  SAFE_FILE  EXPLOIT_FILE  IMPACT  CODE  MINCOUNT
exemplar() {
  local title="$1" dir="$2" safe="$3" evil="$4" impact="$5" code="${6:-E1001}" min="${7:-3}"
  bold "$title"
  local D="$CVE/$dir"
  if checks_clean "$D/$safe"; then
    grn "  ✓ $safe (safe) — axon check clean (exit 0)"
  else
    red "  ✗ $safe should check clean"; fail=1
  fi
  local n; n=$(ecode "$D/$evil" "$code")
  if [ "$n" -ge "$min" ] && ! checks_clean "$D/$evil"; then
    grn "  ✓ $evil (exploit) — REFUSED: $n× $code ($impact)"
    "$AXON" check "$D/$evil" 2>&1 | python3 -c "
import sys, json
code='$code'
for l in sys.stdin:
    if not l.strip().startswith('{'): continue
    d=json.loads(l)
    if d.get('code')==code: print('      '+code+'  '+d['message'].split(chr(10))[0][:88])
" 2>/dev/null
  else
    red "  ✗ $evil should be refused with >=$min $code (got $n)"; fail=1
  fi
  rule
}

exemplar "CVE-2024-34359 — llama-cpp-python (Jinja2 SSTI → RCE)  [AI]" \
  CVE-2024-34359 model_loader.ax model_loader_ssti.ax "RCE / file access / outbound" E1001 3
exemplar "CVE-2024-2624 — lollms-webui (path traversal + file write)  [AI]" \
  CVE-2024-2624 file_store.ax file_store_traversal.ax "traversal read / out-of-lane write / dynamic path" E1001 3
exemplar "CVE-2024-32964 — Lobe Chat (unauthenticated SSRF)  [AI]" \
  CVE-2024-32964 proxy.ax proxy_ssrf.ax "metadata theft / intranet / dynamic URL" E1001 3
exemplar "CVE-2024-5314 — Dolibarr (SQL injection)" \
  CVE-2024-5314 list_records.ax list_records_injection.ax "concatenated + interpolated SQL templates" E1210 2

bold "The point"
echo "  Axon does not detect the bug — it removes what the bug needs: the authority (exec/fs/net)"
echo "  for the first three, and the ABILITY TO BUILD AN UNSAFE QUERY (E1210) for SQL injection."
echo "  Four real CVEs across exec / fs / net / sql, same outcome: the impact is unrepresentable."
echo "  Same shape as ~19 of the 40 CVE-Bench CVEs by capability (TRIAGE.md), + the SQLi class via E1210."
echo
[ "$fail" = "0" ] && grn "cve: OK — every exemplar behaved as claimed" || red "cve: a check did not match its claim"
exit "$fail"
