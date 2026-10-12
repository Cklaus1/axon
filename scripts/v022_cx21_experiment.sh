#!/usr/bin/env bash
# v022_cx21_experiment.sh — the ONLY entry point for the protected CX-21 CODING-STRATEGY-001 experiment.
#
# It REFUSES — it does not warn — unless the derived cx21 readiness is READY
# (governance/readiness/cx21-readiness.json, computed by
# scripts/protected_verifier_ready.py from immutable evidence). Starting and
# finding halfway through that the evidence substrate was not qualified is
# exactly what this prevents. The refusal is machine-readable JSON on stdout.
set -euo pipefail
cd "$(dirname "$0")/.."
python3 -B scripts/protected_verifier_ready.py --require cx21 || exit $?
echo '{"status":"NOT_IMPLEMENTED","reason":"the cx21 runner is not built yet; readiness is READY"}'
exit 2
