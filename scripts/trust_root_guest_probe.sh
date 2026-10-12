#!/bin/sh
# trust_root_guest_probe.sh — runs INSIDE the candidate guest (the preflight's
# --guest-cmd launches it there) and reports whether the operator trust root is
# ADDRESSABLE at all: any of its paths existing, or any mount naming it.
# POSIX sh only: the guest image has no bash. Prints ONE JSON line.
ROOT=${1:-/etc/axon/trust}
found=""
for p in "$ROOT" "$ROOT/qualification" "$ROOT/observer" "$ROOT/verifier" "$ROOT/admission" \
         "$ROOT/verifier.json"; do
  [ -e "$p" ] && found="$found $p"
done
if [ -r /proc/mounts ] && grep -q " $ROOT" /proc/mounts; then found="$found mount:$ROOT"; fi
if [ -z "$found" ]; then
  printf '{"schema":"axon-trust-guest-probe/1","root":"%s","addressable":false}\n' "$ROOT"
else
  printf '{"schema":"axon-trust-guest-probe/1","root":"%s","addressable":true,"found":"%s"}\n' "$ROOT" "${found# }"
fi
