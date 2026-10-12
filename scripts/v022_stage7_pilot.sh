#!/usr/bin/env bash
# v022_stage7_pilot.sh — the ONLY entry point for the protected Stage-7 pilot.
#
# It REFUSES — it does not warn — unless the derived stage7 readiness is READY
# (governance/readiness/stage7-readiness.json, computed by
# scripts/protected_verifier_ready.py from immutable evidence). Starting and
# finding halfway through that the evidence substrate was not qualified is
# exactly what this prevents. The refusal is machine-readable JSON on stdout.
set -euo pipefail
cd "$(dirname "$0")/.."
python3 -B scripts/protected_verifier_ready.py --require stage7 || exit $?
echo '{"status":"NOT_IMPLEMENTED","reason":"the stage7 runner is not built yet; readiness is READY"}'
exit 2
