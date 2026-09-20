# harness_skip.sh — shared classification of a NON-RESULT.
#
# Two separate jobs, deliberately not conflated:
#
#   1. `native_build_unavailable <stderr>` — decide whether a failed
#      `axon build` means "this environment cannot attempt a native build"
#      (a legitimate SKIP) or "the compiler produced a bad build" (a FAIL).
#      The rule is a POSITIVE allow-list of unavailability reasons: a skip
#      must prove its own reason. Anything unrecognised is a FAIL.
#
#      This exists because ~20 parity harnesses used the shape
#
#          if ! "$AXON" build "$prog" -o "$bin" >/dev/null 2>&1; then
#            echo "..._parity: native build failed for $label — skipping"; exit 0
#          fi
#
#      which discards stderr and reports a codegen regression IN THE VERY
#      BUILTIN UNDER TEST as "skipping". A sibling harness hid a real
#      `arr_max_by`-over-structs invalid-IR bug exactly this way.
#
#   2. `harness_skip <name> [reason-lines...]` — emit a skip in the ONE
#      shape both detectors understand. The FINAL line is always the
#      explicit marker `<name>: SKIP — <reason>`; any elaboration is
#      printed BEFORE it. Harnesses that printed the marker first and then
#      elaborated were read as SKIP by `cli_run.rs` (3-line window) and as
#      PASS by `parity_all.sh` (last line only) — the same run, two verdicts.

# native_build_unavailable <stderr-text>
#   0 (true)  → genuinely could not attempt the build
#   1 (false) → the build was attempted and FAILED: that is a result
native_build_unavailable() {
  case "$1" in
    # An interpreter-only `axon` (built --no-default-features). The binary
    # registers the `build` verb regardless of the feature, so only the real
    # refusal text is trustworthy — a --help flag probe never fires.
    *'requires building axon with the `codegen` feature'*) return 0 ;;
    *'requires building axon with the '*'codegen'*' feature'*) return 0 ;;
    *'no codegen feature'*) return 0 ;;
    # No C toolchain to link with.
    *'cc: not found'*|*'cc: command not found'*|*'linker `cc` not found'*) return 0 ;;
    *'error: linker'*'not found'*) return 0 ;;
    # A concurrent build holding the target-dir lock: transient, not a result.
    *'Blocking waiting for file lock'*|*'could not lock'*) return 0 ;;
    *'Text file busy'*) return 0 ;;
    *) return 1 ;;
  esac
}

# harness_skip <name> [reason lines...]
#   Prints the elaboration first, the explicit marker LAST, then exits 0.
harness_skip() {
  local name="$1"; shift
  local last="${1:-reason not stated}"
  if [ "$#" -gt 1 ]; then
    # Everything but the final argument is elaboration, printed first.
    while [ "$#" -gt 1 ]; do
      printf '  %s\n' "$1"
      shift
    done
    last="$1"
  fi
  printf '%s: SKIP — %s\n' "$name" "$last"
  exit 0
}

# native_build_failed <name> <label> <stderr>
#   The whole decision in one call, for the common per-case shape. SKIPs
#   (and exits 0) when unavailable; otherwise prints a FAIL block with the
#   real stderr and returns 1 so the caller can set its fail flag.
native_build_failed() {
  local name="$1" label="$2" berr="$3"
  if native_build_unavailable "$berr"; then
    harness_skip "$name" \
      "$(printf '%s' "$berr" | head -2)" \
      "native codegen unavailable in this environment"
  fi
  echo "$name: FAIL ($label) — native build FAILED (a build error is not a skip):"
  printf '%s\n' "$berr" | head -5 | sed 's/^/        /'
  return 1
}

# ── Vacuity guard for `cargo test <filter>` ──────────────────────────────────
#
# `cargo test <filter>` EXITS 0 WHEN THE FILTER MATCHES NOTHING:
#
#     running 0 tests
#     test result: ok. 0 passed; 0 failed; ... 686 filtered out
#
# so a harness that increments its "ran" counter on exit status alone counts a
# renamed, moved or #[ignore]d test as evidence. The only honest signal is the
# PASSED count on the `test result:` line.
#
# cargo_test_must_run <label> <command...>
#   0 → the command exited 0 AND at least one test passed
#   1 → anything else; a diagnosis is printed naming which of the two failed.
cargo_test_must_run() {
  local label="$1"; shift
  local out status passed
  out="$("$@" 2>&1)"; status=$?
  # Highest "N passed" across all `test result:` lines (a run can print one per
  # target). Absent line → treated as zero, which is the failure we want.
  passed="$(printf '%s\n' "$out" \
            | grep -oE 'test result: [a-zA-Z]+\. [0-9]+ passed' \
            | grep -oE '[0-9]+ passed' | grep -oE '^[0-9]+' \
            | sort -rn | head -1)"
  passed="${passed:-0}"
  if [ "$status" -ne 0 ]; then
    echo "  FAIL [$label]: the test command exited $status"
    printf '%s\n' "$out" | grep -E '^(error|test result|---- )' | head -5 | sed 's/^/        /'
    return 1
  fi
  if [ "$passed" -eq 0 ]; then
    echo "  FAIL [$label]: exited 0 but ZERO tests passed — the filter matched"
    echo "        nothing (renamed / moved / #[ignore]d?). An empty filter is not a pass."
    printf '%s\n' "$out" | grep -E 'test result|running [0-9]+ tests' | head -3 | sed 's/^/        /'
    return 1
  fi
  return 0
}
