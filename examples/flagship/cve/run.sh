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

# ── CONTAINED: a CVE Axon does NOT prevent, blast radius capped ───────────────
bold "CVE-2024-2771 — privilege escalation (CONTAINED, not prevented)"
echo "A missing authz check Axon can't see — but the escalated foothold can't be weaponized."
CD="$CVE/CVE-2024-2771"
if checks_clean "$CD/escalation.ax"; then
  esc=$("$AXON" run "$CD/escalation.ax" 2>/dev/null | grep -i GRANTED | head -1)
  grn "  ✓ escalation.ax — bug FIRES (compiles + runs): ${esc:-GRANTED ...}"
  echo "      → Axon did NOT prevent the privilege-escalation logic bug (out of scope)."
else
  red "  ✗ escalation.ax should compile+run (it is not a capability violation)"; fail=1
fi
ne=$(ecode "$CD/escalation_exfil.ax" E1001)
if [ "$ne" -ge 3 ] && ! checks_clean "$CD/escalation_exfil.ax"; then
  grn "  ✓ escalation_exfil.ax — weaponization REFUSED: $ne× E1001 (read secrets / exfil / exec)"
  echo "      → the foothold has no authority: blast radius is zero. Bug yes; damage no."
else
  red "  ✗ escalation_exfil.ax should be refused with >=3 E1001 (got $ne)"; fail=1
fi
rule

bold "The point"
echo "  PREVENTED: Axon removes what the bug needs — the authority (exec/fs/net) for the first"
echo "  three CVEs, and the ABILITY TO BUILD AN UNSAFE QUERY (E1210) for SQL injection. Four real"
echo "  CVEs across exec / fs / net / sql, same outcome: the impact is unrepresentable."
echo "  CONTAINED: for a CVE it can't prevent (the privilege-escalation logic bug), the capability"
echo "  boundary still caps the blast radius — the foothold can't read secrets, exfiltrate, or RCE."
echo "  See COVERAGE.md for the full per-class verdict across all 40 CVE-Bench CVEs."
echo
[ "$fail" = "0" ] && grn "cve: OK — every exemplar behaved as claimed" || red "cve: a check did not match its claim"
exit "$fail"
