# harness_skip.sh — shared classification of a NON-RESULT.
#
# Separate jobs, deliberately not conflated (3. `rust_target_installed` and the
# `cargo test` vacuity guard are documented at their definitions below):
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
#
#   4. `harness_says_skip <output>` — the READER side: did a harness that
#      exited 0 skip? The one shell copy of the rule; see its definition.
#
#   5. `cargo_build_or_skip <name> <prereq> <command...>` — a harness's own
#      top-level `cargo build`. Skips only on a PROVEN missing prerequisite.
#
#   6. `wasm_link_unavailable`, `wasm_unlinked_probe`, `wasm_case` — what a
#      wasm `axon target build` that exited 0 without linking means; the skip
#      is only the compiler's own `wasm link unavailable:` line.

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

#   3. `rust_target_installed <target>` — distinguish "rustup says this target
#      is not installed" (a legitimate SKIP) from "the probe itself failed"
#      (a FAIL). Fifteen wasm harnesses used the shape
#
#          if ! rustup target list --installed 2>/dev/null | grep -q <target>; then
#            echo "... target not installed — skipping"; exit 0
#          fi
#
#      where the pipe discards rustup's exit status and stderr is thrown away.
#      REPRODUCED: with rustup absent entirely, that expression concludes
#      "target not installed" — rustup never ran, so the probe reported a
#      specific fact it never established. A misconfigured toolchain then skips
#      silently instead of failing loudly, and fifteen harnesses report
#      NON-RESULTS that read like deliberate environment coverage.
#
#      Same rule as the other two: a skip must prove its own reason.
#
#      Exit codes: 0 = installed, 1 = rustup ran and the target is absent,
#      2 = the probe could not be performed (caller must FAIL, not skip).
rust_target_installed() {
  local target="$1" out rc=0
  command -v rustup >/dev/null 2>&1 || { echo "rustup not found on PATH" >&2; return 2; }
  out="$(rustup target list --installed 2>&1)" || rc=$?
  if [ "$rc" -ne 0 ]; then
    echo "rustup target list failed (exit $rc): $(printf '%s' "$out" | head -2 | tr '\n' ' ')" >&2
    return 2
  fi
  # An EMPTY list is also a failed probe, not an empty toolchain: rustup always
  # reports at least the host target when it is working.
  [ -n "$out" ] || { echo "rustup target list produced no output" >&2; return 2; }
  printf '%s\n' "$out" | grep -qx -- "$target" && return 0
  return 1
}

# ── 4. harness_says_skip <output> — the skip DETECTOR ───────────────────────
# Rule-identical to `tail_says_skipping` in crates/axon-core/tests/cli_run.rs
# (that file reads stdout and stderr separately; parity_all.sh hands this the
# combined output). The two used to DISAGREE: parity_all.sh judged by `tail -1`
# alone while cli_run.rs scanned the last three non-empty lines, so a harness
# that printed its skip prose and then elaborated (handler_resume,
# exit_code_parity's catch-all — both ended on build-error text) was SKIP to one
# reader and PASS to the other. Same run, two verdicts. scripts/
# test_harness_skip.sh holds the same verdict rows as cli_run.rs's
# `skip_rule_table`; change the rule in both places and both tables, or neither.
# Its reason rows (`harness_skip_reason`, below) are shell-only: cli_run.rs
# prints no reasons.
#
# A non-zero exit is a FAILURE and never a skip — callers decide that on the
# exit status before consulting this. Rules, in order:
#   1. A skip line is HARNESS-LEVEL: it starts at column 0. An indented
#      "  SKIP <case>" is one case of many and says nothing about the verdict.
#   2. The FINAL non-empty line decides whenever it states anything: a skip
#      marker means skip, a verdict (PASS/FAIL/OK) means not-skip.
#   3. Otherwise a skip may have been followed by elaboration: look at the last
#      three non-empty lines.
# Preferred shape is the explicit final marker `<name>: SKIP — <reason>` that
# `harness_skip` emits; the prose forms are grandfathered.
_skip_re=': SKIP|skipping|this is a SKIP'
_verdict_re='PASS|FAILED|FAIL|: OK|OK —'
harness_says_skip() { # <output>
  local body final
  body="$(printf '%s\n' "$1" | grep -v '^[[:space:]]*$')"
  final="$(printf '%s\n' "$body" | tail -1)"
  printf '%s\n' "$final" | grep -qE "^[^[:space:]].*($_skip_re)" && return 0
  printf '%s\n' "$final" | grep -qE "^[^[:space:]].*($_verdict_re)" && return 1
  printf '%s\n' "$body" | tail -3 | grep -qE "^[^[:space:]].*($_skip_re)"
}

# harness_skip_reason <output> — the line `harness_says_skip`'s rule matched,
# for a harness it calls a skip: the final non-empty line when that decided,
# else the LAST column-0 skip line in the three-line window. parity_all.sh
# prints it as the reason; the final line alone named build-error text that
# followed the skip (`error: IR verification failed`) as the skip's reason.
harness_skip_reason() { # <output>
  local body final
  body="$(printf '%s\n' "$1" | grep -v '^[[:space:]]*$')"
  final="$(printf '%s\n' "$body" | tail -1)"
  if printf '%s\n' "$final" | grep -qE "^[^[:space:]].*($_skip_re|$_verdict_re)"; then
    printf '%s\n' "$final"
    return 0
  fi
  printf '%s\n' "$body" | tail -3 | grep -E "^[^[:space:]].*($_skip_re)" | tail -1
}

# ── 5. cargo_build_or_skip <name> <prereq> <command...> ─────────────────────
#
# A harness's own top-level build, e.g.
#
#     cargo_build_or_skip dict_parity llvm cargo build -q -p axon-core --bin axon
#
# Forty-odd harnesses used `cargo build … 2>/dev/null || { echo "codegen build
# unavailable (LLVM absent) — skipping"; exit 0; }`: ANY build failure — a
# compile error in the tree, a broken Cargo.lock — was reported as a missing
# toolchain it never checked for, and the suite stayed green. The rule here is
# the one `native_build_unavailable` applies to `axon build`: a skip must PROVE
# its reason. <prereq> names the one thing whose absence may excuse a failure:
#
#   llvm             LLVM 17 (codegen links llvm-sys 170): skip only when no
#                    $LLVM_SYS_170_PREFIX/bin/llvm-config, llvm-config-17 or
#                    llvm-config reports a 17.x version.
#   target:<triple>  rustup reports <triple> not installed (rust_target_installed;
#                    a probe that cannot run is a FAIL).
#   z3               libz3 cannot be linked (the `smt` feature needs it).
#   interp           nothing: a --no-default-features build has no external
#                    prerequisite, so its failure is ALWAYS a FAIL.
#
# Success → returns 0 silently. Proven-missing prerequisite → harness_skip (exit
# 0). Anything else → prints a FAIL with the build's stderr and exits 1. Runs in
# the caller's shell (never inside `$(…)`), so the exit ends the harness.
llvm17_present() {
  local c v
  for c in ${LLVM_SYS_170_PREFIX:+"$LLVM_SYS_170_PREFIX/bin/llvm-config"} llvm-config-17 llvm-config; do
    command -v "$c" >/dev/null 2>&1 || continue
    v="$("$c" --version 2>/dev/null)" || continue
    case "$v" in 17.*) return 0 ;; esac
  done
  return 1
}
libz3_present() {
  command -v cc >/dev/null 2>&1 || return 1
  echo 'int main(){return 0;}' | cc -xc - -lz3 -o /dev/null >/dev/null 2>&1
}
cargo_build_or_skip() {
  local name="$1" prereq="$2" err rc
  shift 2
  err="$("$@" 2>&1)" && return 0
  case "$prereq" in
    llvm)
      llvm17_present || harness_skip "$name" \
        "\`$*\` failed and no LLVM 17 llvm-config was found (\$LLVM_SYS_170_PREFIX, llvm-config-17, llvm-config)" \
        "codegen build unavailable (LLVM 17 absent)"
      ;;
    target:*)
      rc=0; rust_target_installed "${prereq#target:}" 2>/dev/null || rc=$?
      [ "$rc" -eq 1 ] && harness_skip "$name" "rust target ${prereq#target:} not installed"
      ;;
    z3)
      libz3_present || harness_skip "$name" "libz3 not found (install to enable)"
      ;;
    interp) ;;
    *) echo "$name: FAIL — cargo_build_or_skip: unknown prerequisite '$prereq'"; exit 2 ;;
  esac
  echo "$name: FAIL — \`$*\` failed and its prerequisite ($prereq) is not proven missing (a build error is not a skip):"
  printf '%s\n' "$err" | grep -v '^[[:space:]]*$' | head -20 | sed 's/^/        /'
  exit 1
}

# ── 6. the outcome of `axon target build --target wasm32-*` ─────────────────
# The build exits 0 whether or not it LINKED: no `.linked.wasm` means
# "object-only", and the compiler says why on stderr (main.rs try_link_wasm):
#   `wasm link unavailable: <rust-lld|wasi libc|rustup toolchains> not found
#    (searched …)`   the toolchain is absent — the ONLY skip;
#   `wasm runtime not found: libaxon_rt.a for <triple> (searched …)`
#                    the harness built axon-rt somewhere the compiler does not
#                    look — a misconfiguration, never a skip;
#   `wasm link failed …`   the linker ran and refused — a result.
# Harnesses used to read every missing `.linked.wasm` as "link unavailable —
# skipping", which hid a compiler that could not find its own runtime (five
# harness SKIPs and 29 per-example ones in one strict run, 2026-10-15).

# wasm_link_unavailable <stderr> — 0 iff stderr carries the unavailable marker.
wasm_link_unavailable() {
  printf '%s\n' "$1" | grep -q '^wasm link unavailable:'
}

# wasm_unlinked_probe <name> <stderr> — a harness's link PROBE exited 0 and
# linked nothing: the marker → `harness_skip` with the marker line as the
# reason (exit 0); anything else → FAIL with the stderr (exit 1).
wasm_unlinked_probe() {
  local name="$1" err="$2"
  wasm_link_unavailable "$err" \
    && harness_skip "$name" "$(printf '%s\n' "$err" | grep -m1 '^wasm link unavailable:')"
  echo "$name: FAIL — the wasm probe built but linked nothing, and the compiler does not say the link is unavailable:"
  printf '%s\n' "$err" | grep -v '^[[:space:]]*$' | head -20 | sed 's/^/        /'
  exit 1
}

# scripts/wasm_object_only.txt: the cases that build for a wasm triple but do
# not link, pinned (NH1, decided A). Rows `<triple> <case> <reason>`; `#` lines
# and blank lines are ignored. An unlisted object-only case FAILs, a listed
# case that links FAILs (stale entry), a row without a reason FAILs.
_wasm_object_only_file="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/wasm_object_only.txt"

# wasm_object_only_check <name> — every row has a reason; prints the bad rows
# and returns 1 otherwise.
wasm_object_only_check() {
  local name="$1" bad
  [ -f "$_wasm_object_only_file" ] || { echo "$name: FAIL — $_wasm_object_only_file is missing"; return 1; }
  bad="$(grep -vE '^[[:space:]]*(#|$)' "$_wasm_object_only_file" | awk 'NF < 3')"
  [ -z "$bad" ] && return 0
  echo "$name: FAIL — scripts/wasm_object_only.txt rows without a reason (\`<triple> <case> <reason>\`):"
  printf '%s\n' "$bad" | sed 's/^/        /'
  return 1
}

# wasm_object_only_reason <triple> <case> — prints the listed reason; 1 if unlisted.
wasm_object_only_reason() {
  awk -v t="$1" -v c="$2" '$1 == t && $2 == c && NF >= 3 { $1 = ""; $2 = ""; sub(/^ +/, ""); print; f = 1; exit } END { exit !f }' \
    "$_wasm_object_only_file" 2>/dev/null
}

# wasm_case <triple> <case> <rc> <stderr> <linked-path> — classify one case of
# a sweep or a per-case harness. Sets WASM_CASE to:
#   linked   built and linked (and not listed as object-only);
#   refused  exit != 0 with E0910, the honest "no native lowering" refusal;
#   skip     exit != 0 that `native_build_unavailable` proves, or exit 0 with
#            the unavailable marker;
#   objonly  exit 0, no link, listed in wasm_object_only.txt;
#   fail     anything else: a build error, `wasm runtime not found`, an
#            unlisted object-only case, or a listed case that now links.
# Prints one indented line for every outcome but `linked` (with the stderr,
# first 20 lines, for `fail`). For `skip`, WASM_CASE_WHY is the proven reason
# (a harness that skipped every case skips with it). Never exits.
wasm_case() {
  local triple="$1" c="$2" rc="$3" err="$4" linked="$5" why
  WASM_CASE=fail; WASM_CASE_WHY=""
  if [ "$rc" -ne 0 ]; then
    if printf '%s\n' "$err" | grep -q 'E0910'; then
      WASM_CASE=refused
      echo "  REFUSED $c ($(printf '%s\n' "$err" | grep -m1 'E0910' | cut -c1-160))"
      return 0
    fi
    if native_build_unavailable "$err"; then
      WASM_CASE=skip; WASM_CASE_WHY="wasm build unavailable"
      echo "  SKIP $c ($WASM_CASE_WHY)"; return 0
    fi
    echo "  FAIL $c: $triple build FAILED (a build error is not a skip):"
  elif [ -f "$linked" ]; then
    if why="$(wasm_object_only_reason "$triple" "$c")"; then
      echo "  FAIL $c: linked, but scripts/wasm_object_only.txt lists it as object-only for $triple (stale entry: $why)"
      return 0
    fi
    WASM_CASE=linked; return 0
  elif wasm_link_unavailable "$err"; then
    WASM_CASE=skip; WASM_CASE_WHY="$(printf '%s\n' "$err" | grep -m1 '^wasm link unavailable:')"
    echo "  SKIP $c ($WASM_CASE_WHY)"
    return 0
  elif printf '%s\n' "$err" | grep -q '^wasm runtime not found:'; then
    echo "  FAIL $c: built but did not link — the compiler did not find its wasm runtime:"
  elif why="$(wasm_object_only_reason "$triple" "$c")"; then
    WASM_CASE=objonly; echo "  OBJECT-ONLY $c (listed: $why)"; return 0
  else
    echo "  FAIL $c: built but did not link, and scripts/wasm_object_only.txt does not list it for $triple:"
  fi
  printf '%s\n' "$err" | grep -v '^[[:space:]]*$' | head -20 | sed 's/^/        /'
}
