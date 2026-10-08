# axon_bin.sh — WHICH `axon` / `axon-run` a harness measures (AX-51).
#
# A parity harness used to `cargo build` its own `target/debug/axon` (default
# features) and `--no-default-features` `axon-run`, then run those. Under
# `cargo test` that was the wrong binary twice over:
#
#   * it was not the binary the test run built. `--target-dir`, `--features`
#     and the profile of the `cargo test` invocation were ignored, so the test
#     measured whatever compiler sat in the worktree's default `target/`;
#   * concurrent harnesses rebuilt those paths while other harnesses were
#     executing them. Measured: an `axon-run` that vanished mid-run (exit 127,
#     reported as an interp<->native divergence), spurious BUILD-FAILs from an
#     interp-only `axon` swapped in under a codegen sweep, and failures that
#     passed when rerun alone.
#
# So the binary is an INPUT. `cli_run.rs` passes the binaries cargo built for
# the test run as AXON and AXON_RUN; a harness given one builds nothing and
# runs exactly that. Run by hand without them, a harness builds the workspace
# binaries as it always did. Fallback paths are absolute, so a harness may
# `cd` elsewhere and still run them.
#
# Source after `cd "$ROOT"`: the builds and fallback paths are the workspace's.

declare -F harness_skip >/dev/null \
  || . "$(dirname "${BASH_SOURCE[0]}")/harness_skip.sh"

# Where the fallback `cargo build`s put binaries: $CARGO_TARGET_DIR, else the
# workspace's target/. Absolute, so a harness may `cd` elsewhere and run them.
_axon_target_dir() {
  case "${CARGO_TARGET_DIR:-}" in
    "") printf '%s/target' "$PWD" ;;
    /*) printf '%s' "$CARGO_TARGET_DIR" ;;
    *) printf '%s/%s' "$PWD" "$CARGO_TARGET_DIR" ;;
  esac
}

# need_axon <harness-name>
#   Sets AXON. A caller-chosen AXON is used as given and nothing is built.
#   Otherwise builds the default-feature (codegen) `axon` and uses
#   target/debug/axon; a build that cannot run (no LLVM) is a SKIP.
need_axon() {
  _axon_built_here=0
  [ -n "${AXON:-}" ] && return 0
  echo "$1: building codegen axon binary…"
  if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
    harness_skip "$1" "codegen build unavailable (LLVM absent)"
  fi
  AXON="$(_axon_target_dir)/debug/axon"
  _axon_built_here=1
}

# axon_has_no_codegen <harness-name>
#   The verdict once $AXON has said it has no native backend (the `codegen`
#   refusal, or E0907 on a wasm target). Never returns. A caller can
#   legitimately hand over an interpreter-only binary (`cargo test
#   --no-default-features` builds one), and interp<->native parity is not
#   measurable with it: a SKIP. A binary this harness built WITH codegen
#   moments ago cannot lack it unless another build replaced it: a FAIL.
axon_has_no_codegen() {
  if [ "${_axon_built_here:-0}" = 1 ]; then
    echo "$1: FAIL — $AXON lost its codegen backend right after it was built:"
    echo "  a concurrent --no-default-features build replaced it. Set AXON to pin a binary."
    exit 1
  fi
  harness_skip "$1" \
    "\$AXON cannot produce native binaries, so interp<->native parity is not measurable with it." \
    "\$AXON ($AXON) is an interpreter-only build — native codegen is not under test"
}

# need_codegen_axon <harness-name>
#   need_axon, then ASK THE BINARY whether it can emit native code: a trivial
#   `axon build` must succeed. No backend -> axon_has_no_codegen; no linker
#   etc. -> SKIP (native_build_unavailable); any other failure is a broken
#   compiler: FAIL, never a skip.
need_codegen_axon() {
  need_axon "$1"
  local d err
  d="$(mktemp -d)"
  printf 'fn main() -> i64 { 0 }\n' > "$d/probe.ax"
  if err="$("$AXON" build "$d/probe.ax" -o "$d/probe.bin" --no-cache 2>&1)"; then
    rm -rf "$d"
    return 0
  fi
  rm -rf "$d"
  case "$err" in
    *'requires building axon with the `codegen` feature'*) axon_has_no_codegen "$1" ;;
  esac
  if native_build_unavailable "$err"; then
    harness_skip "$1" "$(printf '%s' "$err" | head -2)" \
      "native codegen unavailable in this environment"
  fi
  echo "$1: FAIL — \$AXON ($AXON) cannot build a trivial program (a build error is not a skip):"
  printf '%s\n' "$err" | head -5 | sed 's/^/        /'
  exit 1
}

# need_axon_run <harness-name>
#   Sets INTERP to the codegen-free runner. A caller-chosen AXON_RUN is used as
#   given. Otherwise builds `--no-default-features --bin axon-run` and uses
#   target/debug/axon-run; a build that cannot run is a SKIP. (`axon-run` is the
#   same interpreter under any feature set — no interpreter code is cfg'd on
#   `codegen` — so the one `cargo test` built under its features is that
#   runner too.)
need_axon_run() {
  if [ -n "${AXON_RUN:-}" ]; then
    INTERP="$AXON_RUN"
    return 0
  fi
  if ! cargo build -q -p axon-core --no-default-features --bin axon-run 2>/dev/null; then
    harness_skip "$1" "interp build unavailable"
  fi
  INTERP="$(_axon_target_dir)/debug/axon-run"
}
