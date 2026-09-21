#!/usr/bin/env bash
# No HTTP server in this workspace may send `Access-Control-Allow-Origin: *`.
#
# Every server here binds loopback, which stops the NETWORK reaching it. A
# wildcard CORS header then hands it to every page the operator's own browser
# visits — the worse half of the exposure, not a leftover.
#
# REPRODUCED against a live `axon-web`: a cross-origin POST to /api/deploy
# carrying a program that calls write_file returned 200 with
# `Access-Control-Allow-Origin: *`, and the file was created. That endpoint
# writes caller-supplied content to a temp .ax and executes it. A
# `Content-Type: text/plain` POST is a CORS simple request, so no preflight
# stands in the way.
#
# WORKSPACE-WIDE on purpose. The same defect was fixed in `axon-signal` first,
# and that commit cited `axon-web` as the correct model because it binds
# loopback — its CORS header was never checked. A guard scoped to one crate
# proves a property about one crate.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

# Exclude tests/: a guard that SEARCHES for the header necessarily contains the
# string, and flagging it would make this check cry wolf on the very test that
# enforces the same rule per-crate. Tests are not servers.
hits=$(git ls-files '*.rs' | grep -v '/tests/' \
       | xargs grep -n 'Access-Control-Allow-Origin' 2>/dev/null \
       | grep -v '^\S*:[0-9]*: *//' || true)
# Guard the guard: the scan must actually be looking at Rust sources.
n=$(git ls-files '*.rs' | wc -l)
if [ "$n" -lt 50 ]; then
  echo "  ✗ only $n .rs files found — this check's premise is gone, not satisfied"
  exit 1
fi
if [ -n "$hits" ]; then
  echo "$hits" | sed 's/^/  ✗ /'
  echo "no_wildcard_cors: FAIL — a loopback server with a wildcard CORS header is"
  echo "  reachable cross-origin from any page the operator visits. These servers"
  echo "  serve their own UI from the same origin and need no CORS header."
  exit 1
fi
echo "no_wildcard_cors: OK — $n .rs files, no CORS header set in non-comment code"
