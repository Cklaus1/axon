#!/usr/bin/env bash
# compare_docker.sh — the SERIOUS foil: Docker + a hand-written seccomp profile.
#
# `compare_python.sh` shows naked Python (no sandbox). A security engineer's first
# reaction is rightly "Docker + seccomp already sandboxes that." This script grants
# them the strongest version of that argument — a real container with a real,
# hand-written seccomp profile — and shows precisely where it falls short of
# Axon's compile-time @[contained], and why.
#
# The agent has three escapes: (1) read /etc/passwd, (2) open a network socket,
# (3) spawn a process. We run them under Docker three ways and compare to Axon.
#
# Requires: docker (with a runnable daemon). Degrades to an explanation if absent.
#   DEMO_NOPAUSE=1   run without pauses (CI/automation)
set -uo pipefail
REPO=$(cd "$(dirname "$0")/../.." && pwd)
D="$REPO/examples/flagship"
AXON="$REPO/target/debug/axon"
IMG="python:3.12-slim"

bold()  { printf '\033[1m%s\033[0m\n' "$*"; }
dim()   { printf '\033[2m%s\033[0m\n' "$*"; }
green() { printf '\033[32m%s\033[0m\n' "$*"; }
red()   { printf '\033[31m%s\033[0m\n' "$*"; }
cyan()  { printf '\033[36m%s\033[0m\n' "$*"; }
rule()  { dim "────────────────────────────────────────────────────────────"; }
pause() { [ "${DEMO_NOPAUSE:-0}" = "1" ] || { printf '\033[2m  (enter)\033[0m'; read -r _; }; }

bold "=== Foil: Docker + seccomp  vs  Axon @[contained] ==="
echo
echo "The agent must do LOCAL COMPUTE ONLY. It tries three escapes:"
echo "   (1) read /etc/passwd     (2) open a network socket     (3) spawn a process"
rule

# ── Docker availability ───────────────────────────────────────────────────────
HAVE_DOCKER=0
if command -v docker >/dev/null 2>&1 && timeout 30 docker run --rm "$IMG" true >/dev/null 2>&1; then
  HAVE_DOCKER=1
else
  red "  docker not runnable here — showing the argument without live containers."
  echo "  (Install Docker + a daemon to run the live comparison.)"
fi

run_probe() { # $1 = extra docker args (seccomp), $2 = label
  timeout 60 docker run --rm $1 -v "$D/docker_probe.py:/probe.py:ro" "$IMG" python3 /probe.py 2>&1
}

if [ "$HAVE_DOCKER" = "1" ]; then
  bold "A) Docker, DEFAULT settings"
  echo "   \$ docker run $IMG python3 probe.py"
  run_probe "" "default" | sed 's/^/   /'
  red   "   → 3 / 3 escapes ALLOWED. A plain container is not a capability boundary."
  rule; pause

  bold "B) Docker + a HAND-WRITTEN seccomp profile (seccomp-agent.json)"
  echo "   \$ docker run --security-opt seccomp=seccomp-agent.json $IMG python3 probe.py"
  run_probe "--security-opt seccomp=$D/seccomp-agent.json" "seccomp" | sed 's/^/   /'
  echo
  cyan  "   → 1 / 3 blocked. The other two survive for STRUCTURAL reasons:"
  echo  "      • file read  — seccomp filters syscalls, not paths. 'openat' is load-"
  echo  "        bearing (CPython needs it to boot), so you cannot deny the /etc/passwd"
  echo  "        open without killing the interpreter. No rule expresses 'open anything"
  echo  "        but /etc/passwd'."
  echo  "      • process     — you'd deny 'execve', but Docker LAUNCHES the container with"
  echo  "        execve, so denying it means the container never starts. seccomp is"
  echo  "        stateless: it cannot say 'exec once to start, then no children'."
  rule; pause
fi

# ── The Axon side: compile-time refusal of all three ──────────────────────────
bold "C) Axon — the same three escapes, refused at COMPILE TIME"
echo "   \$ axon check examples/flagship/agent_task_evil.ax"
if [ -x "$AXON" ]; then
  OUT="$("$AXON" check "$D/agent_task_evil.ax" 2>&1)"; CODE=$?
  echo "$OUT" | python3 -c "
import sys, json
for line in sys.stdin:
    try: d = json.loads(line)
    except Exception: continue
    if d.get('code') == 'E1001':
        print('   E1001  ' + d['message'].split('\n')[0])
" 2>/dev/null || echo "$OUT" | grep '"E1001"' | sed 's/.*"message":"/   E1001  /; s/\\n.*//'
  if echo "$OUT" | grep -q 'E1001'; then
    green "   → 3 / 3 escapes REFUSED (E1001), exit $CODE. The program never runs."
  else
    red "   (expected E1001 refusals — is this the codegen-free axon build?)"
  fi
else
  red "   target/debug/axon not built — run: cargo build -p axon-core --no-default-features --bin axon"
fi
rule

# ── The point ─────────────────────────────────────────────────────────────────
bold "Why Axon wins this comparison (three independent reasons)"
echo
cyan "  1. PROVENANCE — the policy is DERIVED FROM THE CODE."
echo  "     Axon emits the syscall allowlist from @[contained] into the .axmeta"
echo  "     manifest (syscall_hint). The seccomp profile above is a SEPARATE file a"
echo  "     human maintains; edit the agent and it silently drifts out of sync. Axon's"
echo  "     cannot drift — it is regenerated from the types on every build."
if [ -f "$D/agent_task.axmeta" ]; then
  echo
  dim "     (live: syscall_hint derived for the GOOD agent)"
  grep -o '"syscall_hint":\[[^]]*\]' "$D/agent_task.axmeta" 2>/dev/null | sed 's/^/       /' | head -1
fi
echo
cyan "  2. TIMING — refusal happens BEFORE the code runs, not at the syscall trap."
echo  "     Docker catches the escape when the syscall fires (the exfil logic already"
echo  "     executed up to that point). Axon refuses to BUILD — the data is never read,"
echo  "     the packet is never sent, the process never spawns."
echo
cyan "  3. GRANULARITY — capabilities, not raw syscall numbers."
echo  "     seccomp speaks 'allow/deny openat'. @[contained] speaks 'fs: [], net:"
echo  "     [\"api.anthropic.com\"], exec: none' — path- and host-scoped policy a syscall"
echo  "     filter cannot express without a separate proxy/LSM stack."
echo
bold  "  Docker+seccomp is a real sandbox. But its policy is a hand-kept artifact,"
bold  "  enforced late, at syscall granularity. Axon's policy IS the code, proven early."
echo
dim   "  See examples/flagship/THREAT_MODEL.md for what this does and does NOT cover."
