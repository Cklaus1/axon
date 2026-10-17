#!/usr/bin/env bash
# wasm_browser_examples_parity.sh — R7c breadth: real examples/*.ax run on the
# BROWSER target (wasm32-unknown-unknown, wasi-free) with stdout identical to the
# interpreter, driven by the Node host (scripts/wasm_browser_host.js, standing in
# for the page's WebAssembly glue).
#
# This is the browser counterpart of wasm_examples_parity.sh (which targets the
# headless wasip1 backend via wasmtime). Here the module is wasi-free and
# `println` is routed through the `axon_host_write` host import, so the Node host
# captures stdout. Host/non-deterministic examples are skipped; an example that
# doesn't link (e.g. needs a not-yet-browser-shimmed libc symbol) is reported
# object-only but doesn't fail; a linked example whose output DIFFERS fails, and a
# FLOOR guard catches a mass-skip regression. Skips (exit 0) without node/toolchain.
set -u
# shellcheck source=lib/harness_skip.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib/harness_skip.sh"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
. "$ROOT/scripts/lib/axon_bin.sh"
# Serialize the wasm parity scripts under one shared lock: each builds its
# wasm artifacts next to the source (examples/$base.*.wasm), so concurrent runs
# (cargo's parallel test threads invoke several of these at once) clobber each
# other's intermediates — a file race that surfaces as spurious DIFFER /
# "No such file". flock makes the wasm sweeps run one at a time (orthogonal to
# the ~370 other tests, which keep running in parallel). No-op without flock.
if command -v flock >/dev/null 2>&1; then exec 9>"${TMPDIR:-/tmp}/axon_wasm_parity.lock" && flock 9; fi

FLOOR=28   # a second guard behind the per-example classes; census 2026-10-17
           # (gaps2 H6): 30 of 34 match, 4 refused by E0910 (the HTTP/SSE examples)

command -v node >/dev/null 2>&1 || { echo "wasm_browser_examples_parity: no node — skipping"; exit 0; }
HOSTJS="scripts/wasm_browser_host.js"
[ -f "$HOSTJS" ] || { echo "wasm_browser_examples_parity: host harness missing — skipping"; exit 0; }
if [ -n "${AXON:-}" ]; then
  named_bin AXON wasm_browser_examples_parity
else
  echo "wasm_browser_examples_parity: building codegen axon binary…"
  cargo_build_or_skip wasm_browser_examples_parity llvm cargo build -q -p axon-core --bin axon
  use_built AXON axon
fi
# Distinguish an ABSENT target from a BROKEN build. The probe used to be
# `if ! cargo build ... 2>/dev/null` reporting "unavailable - skipping", which
# said the same thing for both and threw away the compiler error naming which.
# That is how a `data:`/`ptr:` typo in a `#[cfg(target_arch = "wasm32")]` arm
# (4a600f2) took all 8 wasm_* harnesses dark for a day while parity_all printed
# "SKIP (toolchain absent)" -- both wasm32 targets were installed the whole time.
# A skip must be honest about WHY, or it is a silent loss of coverage.
# A skip must prove its own reason: the previous probe discarded
# rustup's exit status, so a missing rustup concluded "not installed".
rust_target_installed wasm32-unknown-unknown; _rt=$?
if [ "$_rt" -eq 2 ]; then
  echo "wasm_browser_examples_parity: cannot determine whether wasm32-unknown-unknown is installed — refusing to call that a skip" >&2
  exit 1
elif [ "$_rt" -ne 0 ]; then
  echo "wasm_browser_examples_parity: wasm32-unknown-unknown target not installed - skipping"; exit 0
fi
if ! _rt_err="$(cargo build -q -p axon-rt --target wasm32-unknown-unknown 2>&1)"; then
  echo "wasm_browser_examples_parity: FAIL - axon-rt does NOT build for wasm32-unknown-unknown (target IS installed):"
  echo "$_rt_err" | sed 's/^/    | /'
  exit 1
fi
PROBE="$(mktemp -d)/p.ax"; printf 'fn main() { println("ok") }\n' > "$PROBE"
_probe_err="$("$AXON" target build "$PROBE" --target wasm32-unknown-unknown 2>&1)"; _probe_rc=$?
if [ ! -f "${PROBE%.ax}.linked.wasm" ]; then
  # E0907: this `axon` has no codegen backend. An interp-only $AXON is a skip
  # (require_codegen_axon says so and exits); E0907 from a binary that CAN build
  # native code is a broken compiler, not an absent toolchain.
  if echo "$_probe_err" | grep -q E0907; then
    require_codegen_axon wasm_browser_examples_parity
    echo "wasm_browser_examples_parity: FAIL — \$AXON ($AXON) builds native code yet answered E0907:"
    printf '%s\n' "$_probe_err" | head -5 | sed 's/^/        /'
    exit 1
  fi
  # The target and axon-rt for it were proven above. A build that FAILED is a
  # result. A build that succeeded without a linked module is a skip only when
  # the compiler says the link is unavailable (wasm_unlinked_probe).
  if [ "$_probe_rc" -ne 0 ]; then
    native_build_failed wasm_browser_examples_parity "wasm probe build" "$_probe_err" || exit 1
  fi
  wasm_unlinked_probe wasm_browser_examples_parity "$_probe_err"
fi

# Each example's outcome is classified by `wasm_case` (lib/harness_skip.sh):
# an E0910 refusal is counted REFUSED, an unavailable build or link is a case
# SKIP, an object-only example must be listed (wasm32-unknown-unknown rows) in
# scripts/wasm_object_only.txt, and anything else (a build error, `wasm runtime
# not found`, an unlisted object-only example, a listed one that now links) is
# a FAILED example. The floor is a second guard, no longer the only one.
wasm_object_only_check wasm_browser_examples_parity || exit 1
pass=0; diff=0; objonly=0; refused=0; skipped=0; bad=0; total=0
fails=""; objs=""; refs=""; bads=""
for f in examples/*.ax; do
  grep -q "fn main" "$f" || continue
  # Skip host / non-deterministic / time-dependent examples: AI, randomness, the
  # clock (now_ms directly AND temporal_*, which stamps created_at via __axon_now_ms
  # internally — no wasi clock on the browser; a Date.now() host import is the
  # follow-on), I/O, exec, goal search, spawn. (An object-only example must be
  # listed in scripts/wasm_object_only.txt, or it fails below.)
  if grep -qE 'ai_complete|ai_extract|random_|now_ms|now_ns|temporal_|sleep_ms|read_line|read_file|write_file|exec\(|goal_run|goal_step|for!|spawn|llm_|agent_' "$f"; then
    continue
  fi
  total=$((total + 1))
  base="$(basename "$f" .ax)"
  linked="examples/$base.linked.wasm"
  rm -f "$linked" "examples/$base.wasm"
  werr="$("$AXON" target build "$f" --target wasm32-unknown-unknown 2>&1)"; wrc=$?
  wasm_case wasm32-unknown-unknown "$base" "$wrc" "$werr" "$linked"
  case "$WASM_CASE" in
    linked) ;;
    refused) refused=$((refused + 1)); refs="$refs $base"; rm -f "examples/$base.wasm"; continue ;;
    skip) skipped=$((skipped + 1)); rm -f "examples/$base.wasm"; continue ;;
    objonly) objonly=$((objonly + 1)); objs="$objs $base"; rm -f "examples/$base.wasm"; continue ;;
    *) bad=$((bad + 1)); bads="$bads $base"; rm -f "$linked" "examples/$base.wasm"; continue ;;
  esac
  if strings "$linked" | grep -qi wasi_snapshot; then
    diff=$((diff + 1)); fails="$fails\n  WASI-IMPORT: $base"; rm -f "$linked" "examples/$base.wasm"; continue
  fi
  I_OUT="$(AXON_SEED=42 "$AXON" run "$f" 2>/dev/null)"
  W_OUT="$(node "$HOSTJS" "$linked" 2>/dev/null)"
  rm -f "$linked" "examples/$base.wasm"
  if [ "$I_OUT" = "$W_OUT" ]; then
    pass=$((pass + 1))
  else
    diff=$((diff + 1)); fails="$fails\n  DIFFER: $base"
  fi
done

echo "wasm_browser_examples_parity: $pass/$total linked+matched, $diff differ, $refused refused (E0910), $objonly object-only (listed), $skipped skipped, $bad failed"
[ -n "$refs" ] && echo "wasm_browser_examples_parity: refused by codegen (E0910, interpreter-only):$refs"
[ -n "$objs" ] && echo "wasm_browser_examples_parity: object-only (listed in scripts/wasm_object_only.txt):$objs"
if [ "$bad" -ne 0 ]; then
  echo "wasm_browser_examples_parity: FAIL — examples that failed to build or link, or contradict scripts/wasm_object_only.txt (FAIL lines above):$bads"
  exit 1
fi
if [ "$diff" -ne 0 ]; then
  printf "wasm_browser_examples_parity: FAIL — browser output diverged from the interpreter:$fails\n"
  exit 1
fi
if [ "$pass" -lt "$FLOOR" ]; then
  printf "wasm_browser_examples_parity: FAIL — only %d examples ran on the browser target (floor %d); a mass link regression silently skipped the rest\n" "$pass" "$FLOOR"
  exit 1
fi
echo "wasm_browser_examples_parity: PASS — $pass deterministic examples run on the WASI-FREE browser target with stdout identical to the interpreter ✓"
exit 0
