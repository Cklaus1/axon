#!/usr/bin/env bash
# run.sh — the flagship "escape" demo: AI-written code, provably sandboxed.
#
# Story in four beats:
#   1. The GOOD agent stays in its sandbox  → Axon builds + runs it.
#   2. The EVIL agent tries to exfiltrate    → Axon's COMPILER refuses to build it.
#   3. The SAME evil agent in Python         → runs, and every escape is permitted.
#   4. The point.
#
# Usage:  examples/flagship/run.sh
# Requires: the `axon` interpreter CLI. Build it once with:
#   cargo build -p axon-core --no-default-features --bin axon

set -u
cd "$(dirname "$0")/../.." || exit 1

# Locate the axon binary (debug or release).
AXON="$(find target -maxdepth 3 -name axon -type f 2>/dev/null | head -1)"
if [ -z "${AXON:-}" ]; then
    echo "axon binary not found. Build it first:"
    echo "  cargo build -p axon-core --no-default-features --bin axon"
    exit 1
fi

bold() { printf "\033[1m%s\033[0m\n" "$1"; }
rule() { printf "\033[2m%s\033[0m\n" "────────────────────────────────────────────────────────────"; }
pause() { [ "${DEMO_NOPAUSE:-}" = "1" ] || { printf "\033[2m(enter to continue)\033[0m"; read -r _; }; }

D=examples/flagship

clear 2>/dev/null || true
bold "Axon flagship demo — AI-written code, sandboxed by the compiler"
echo "The hardest problem in shipping AI agents: how do you let a model write"
echo "and run code without it doing something catastrophic? Axon's answer is a"
echo "capability boundary the COMPILER enforces. Watch."
echo
pause

rule
bold "BEAT 1 — The good agent. Task: score some records, local compute only."
echo "Declared:  @[contained(fs: [], net: [], exec: none)]"
echo "\$ axon check $D/agent_task.ax"
"$AXON" check "$D/agent_task.ax"; echo "exit=$?  (0 = compiles clean)"
echo
echo "\$ axon run $D/agent_task.ax"
"$AXON" run "$D/agent_task.ax"; echo "exit=$?"
echo
echo "→ It stays inside its sandbox, so it builds and runs. Good."
pause

rule
bold "BEAT 2 — The EVIL agent. Same task — plus it tries to exfiltrate."
echo "Same declared caps. The body also reads /etc/passwd, calls an LLM, runs curl."
echo "\$ axon check $D/agent_task_evil.ax"
"$AXON" check "$D/agent_task_evil.ax"; rc=$?
echo "exit=$rc  (2 = REFUSED at compile time)"
echo
echo "→ THREE E1001 errors — fs-read, net, exec. The data is never read."
echo "  The packet is never sent. curl never runs. The escape is impossible"
echo "  BY CONSTRUCTION — the program cannot execute even once."
pause

rule
bold "BEAT 2½ — The SUBTLE agent. It abuses a capability it WAS granted."
echo "Granted exactly fs: [write(\"./out/\")]. It writes its report (fine) — then"
echo "tries to write OUT of its lane via a path it builds at runtime."
echo "\$ axon check $D/agent_task_subtle.ax"
"$AXON" check "$D/agent_task_subtle.ax"; rc=$?
echo "exit=$rc  (2 = REFUSED)"
echo
echo "→ E1001: a write target it can't statically prove stays under ./out/ is"
echo "  refused. Holding write(\"./out/\") does NOT grant write to /etc/cron.d via"
echo "  a dynamic path. A real grant can't be abused beyond its declared scope."
pause

rule
bold "BEAT 2¾ — The CREDENTIAL thief. It steals a host secret via its legit net lane."
echo "Granted net: [\"api.anthropic.com\"] because it legitimately calls an LLM."
echo "It does the summary (allowed) — then reads ANTHROPIC_API_KEY from the host"
echo "environment and smuggles it out through that same allowed network call."
echo "\$ axon check $D/agent_task_secrets.ax"
"$AXON" check "$D/agent_task_secrets.ax"; rc=$?
echo "exit=$rc  (2 = REFUSED)"
echo
echo "→ ONE E1001 — on the env read, NOT the network call. The net lane is open"
echo "  for the real task; the environment is an ambient secret channel with no"
echo "  grant clause, so harvesting the host's API key is refused. A granted net"
echo "  capability lets you TALK to the model — not loot the host to feed it."
pause

rule
bold "BEAT 3 — The SAME evil agent, in Python. The 'sandbox' is a comment."
echo "\$ python3 $D/foil_python.py"
python3 "$D/foil_python.py"
echo
echo "→ Every escape was attempted AND ALLOWED. No compiler said no."
pause

rule
bold "THE POINT"
echo "Same malicious code, two languages:"
echo "  • Python  — the sandbox is a policy you hope holds. It ran."
echo "  • Axon    — the sandbox is a compile-time proof. It never built."
echo
echo "Axon is the language where AI-written code is sandboxed by construction:"
echo "the compiler refuses any capability you didn't grant — before the code runs."
echo
echo "Same machinery also gives you: a one-way kill-switch (@[corrigible]),"
echo "logged + replayable AI calls (provenance), and PII taint (@[sensitive])."
rule
