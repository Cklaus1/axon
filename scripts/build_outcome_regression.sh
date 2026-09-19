#!/usr/bin/env bash
# build_outcome_regression.sh — the instrument that makes a backend change's
# claim CHECKABLE, in both directions.
#
# Two measurements, one script:
#
#   1. BUILD-OUTCOME CORPUS DIFF. `axon build` every `.ax` program under
#      `examples/` and `crates/axon-core/tests/fixtures/` and diff the exit
#      status of each against a committed baseline. The diff is SYMMETRIC:
#      a program that starts FAILING and a program that starts PASSING are both
#      regressions to report. A one-directional baseline stops meaning anything
#      the moment something shifts underneath it — it keeps passing while the
#      thing it was supposed to pin has moved.
#
#   2. INTERPRETER↔NATIVE DIFFERENTIAL on a declared program list. Compares
#      printed STDOUT, never exit codes: Axon remaps 2..=15 and 101 onto 1, so
#      an exit-code-only comparison passes on a wrong value (13 real divergences
#      were found that way once the comparison moved to stdout). The interpreter
#      is reference semantics (invariant I-2).
#
#      A native build that REFUSES (E0910, "sound by refusal") is its own
#      reported outcome — `native-refused-e0910`. It is not a pass (the two
#      engines were never compared) and not a skip (the refusal is a designed
#      promise and is worth pinning). Its baseline row is what makes a refusal
#      that silently BECOMES a miscompile visible.
#
# Occasion: a native-backend defect where a non-Unit body that lowers to no
# value has `const_zero()` synthesized for it. On an aborting build that is a
# harmless placeholder; on a SUCCEEDING build it is an invented answer. Two
# fixes are expected (a codegen-error safety net, then the match-arm payload
# type propagation that makes the construct lower for real), and both of them
# make claims of the form "exactly these programs move, and nothing else does".
# That claim is only checkable against a baseline that is trusted in BOTH
# directions, which is what this is.
#
# ── What this CANNOT establish ───────────────────────────────────────────────
#   * A build exit status is not a correct program. 166 programs build clean;
#     this says only that the same 166 build clean, not that any of them is
#     right. The fabricated-return class is invisible to the sweep BY
#     CONSTRUCTION — that is why part 2 exists.
#   * The stdout differential is blind wherever a fabricated value does not
#     reach stdout. Measured: `examples/asi/search_rank.ax` is one of the two
#     programs that reach the fabricating branch on a SUCCEEDING build, and
#     both engines still print identical stdout (they both stop at the same
#     `@[verify]` failure in `deploy_gate`). Its `agree` row is a
#     change-detector, not a certificate.
#   * Stderr is reported but not compared. The two engines word the same
#     @[verify] failure differently — a real, pre-existing gap documented in
#     `exit_code_parity.sh` — so an equality assertion there would fail for a
#     reason that has nothing to do with this measurement.
#   * The differential covers a DECLARED list, not all 328 programs. Most of
#     the corpus needs stdin, the network, or an API key to run.
#
# ── Usage ────────────────────────────────────────────────────────────────────
#   scripts/build_outcome_regression.sh              # full check (sweep + differential)
#   scripts/build_outcome_regression.sh --build-only # corpus diff only
#   scripts/build_outcome_regression.sh --diff-only  # differential only (fast)
#   scripts/build_outcome_regression.sh --self-test  # prove the comparator FAILS when it should
#   scripts/build_outcome_regression.sh --record     # rewrite BOTH baselines (deliberate act)
#   BASELINE=… DIFF_BASELINE=… JOBS=6 AXON=path/to/axon  scripts/build_outcome_regression.sh
#
# ── Exit codes ───────────────────────────────────────────────────────────────
#   0  no change against the baselines
#   1  a difference was found (named, in both directions)
#   2  CANNOT MEASURE — a precondition failed. Deliberately not 0: "the
#      toolchain looked absent" once hid three real defects in this repo, so
#      there is no silent skip path here at all. Every precondition below is
#      ASSERTED (the binary really produces and runs a native executable),
#      never inferred from the text of an error message.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

BASELINE="${BASELINE:-scripts/build_outcome_baseline.tsv}"
DIFF_BASELINE="${DIFF_BASELINE:-scripts/build_differential_baseline.tsv}"
JOBS="${JOBS:-6}"
# A glob that matches nothing passes vacuously. The corpus is 328 programs
# today; anything near-empty or drastically smaller is a BROKEN MEASUREMENT,
# not a clean run, and is refused below.
MIN_CORPUS="${MIN_CORPUS:-300}"
CASE_TIMEOUT="${CASE_TIMEOUT:-120}"

# ── hidden worker mode ───────────────────────────────────────────────────────
# One file, one build, one `status<TAB>path` line. Kept as a re-invocation so
# the sweep can run under `xargs -P` without exporting shell functions.
if [ "${1:-}" = "--measure-one" ]; then
  _f="$2"; _work="$3"; _axon="$4"
  _out="$_work/$(printf '%s' "$_f" | tr '/' '_').bin"
  "$_axon" build "$_f" -o "$_out" --no-cache >/dev/null 2>&1
  _st=$?
  rm -f "$_out"
  printf '%s\t%s\n' "$_st" "$_f"
  exit 0
fi

MODE=full
RECORD=0
for arg in "$@"; do
  case "$arg" in
    --build-only) MODE=build ;;
    --diff-only)  MODE=diff ;;
    --self-test)  MODE=selftest ;;
    --record)     RECORD=1 ;;
    -h|--help)    sed -n '2,50p' "$0"; exit 0 ;;
    *) echo "build_outcome_regression: unknown argument '$arg'" >&2; exit 2 ;;
  esac
done

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

fail=0
note() { printf '%s\n' "$*"; }

# ── preconditions, ASSERTED ──────────────────────────────────────────────────
# Each of these is checked by observing the thing itself, not by pattern-matching
# a message. `timeout` is required because a corpus program may block on stdin or
# the network, and a harness that hangs reports nothing at all.
command -v timeout >/dev/null 2>&1 || {
  note "build_outcome_regression: CANNOT MEASURE — \`timeout\` is not on PATH"
  exit 2
}

AXON="${AXON:-target/debug/axon}"
if [ ! -x "$AXON" ]; then
  note "build_outcome_regression: building the codegen axon binary…"
  if ! cargo build -q -p axon-core --bin axon -j "$JOBS"; then
    note "build_outcome_regression: CANNOT MEASURE — \`cargo build -p axon-core --bin axon\` failed."
    note "  This harness measures the NATIVE backend; there is nothing to measure without it."
    exit 2
  fi
  AXON="target/debug/axon"
fi
[ -x "$AXON" ] || { note "build_outcome_regression: CANNOT MEASURE — \$AXON ($AXON) is not executable"; exit 2; }

# Prove codegen capability by PRODUCING AND RUNNING a native binary. An
# interpreter-only build fails every case with "native build failed", which reads
# like 328 divergences instead of one wrong binary — and an error-string probe
# for that condition goes stale the moment the wording changes.
printf 'fn main() -> i64 { 41 + 1 }\n' > "$WORK/_probe.ax"
if ! "$AXON" build "$WORK/_probe.ax" -o "$WORK/_probe.bin" --no-cache >"$WORK/_probe.log" 2>&1; then
  note "build_outcome_regression: CANNOT MEASURE — \$AXON ($AXON) cannot build a trivial"
  note "  program natively, so no build-outcome or differential measurement is meaningful:"
  sed 's/^/    /' "$WORK/_probe.log" | head -5
  exit 2
fi
[ -x "$WORK/_probe.bin" ] || {
  note "build_outcome_regression: CANNOT MEASURE — the probe build reported success but"
  note "  left no executable at _probe.bin (a reported success is not a produced artifact)"
  exit 2
}
"$WORK/_probe.bin" >/dev/null 2>&1
probe_exit=$?
if [ "$probe_exit" -ne 42 ]; then
  note "build_outcome_regression: CANNOT MEASURE — the probe binary exited $probe_exit, expected 42."
  note "  The backend under measurement does not run correctly on the trivial case, so"
  note "  nothing this harness reports about the other 328 programs can be trusted."
  exit 2
fi
note "build_outcome_regression: \$AXON = $AXON (codegen proven: built and ran a native binary)"

# The sweep must not be steerable by whatever shell launched it. These are the
# AXON_* vars that change a BUILD outcome or a run's observable output; they are
# neutralised so a developer's ambient environment cannot silently move the
# baseline. AXON_AI_MOCK is set PER CASE below where a program needs it.
unset AXON_STRICT AXON_RECORD AXON_REPLAY AXON_AI_MOCK AXON_INTENT_GEN \
      AXON_ALLOWED_EFFECTS AXON_BUDGET_TOKENS AXON_SEED AXON_CLOCK AXON_AI_REPLAY

# ── corpus enumeration ───────────────────────────────────────────────────────
CORPUS="$WORK/corpus.txt"
find examples crates/axon-core/tests/fixtures -name '*.ax' -type f | LC_ALL=C sort > "$CORPUS"
corpus_n=$(wc -l < "$CORPUS")
if [ "$corpus_n" -lt "$MIN_CORPUS" ]; then
  note "build_outcome_regression: CANNOT MEASURE — corpus is $corpus_n programs, below the"
  note "  floor of $MIN_CORPUS. A sweep that matches (almost) nothing passes vacuously; that"
  note "  is the failure mode this floor exists to refuse. Check the search roots."
  exit 2
fi

# ── the comparator, as a function ────────────────────────────────────────────
# compare_build <observed.tsv> <baseline.tsv>
# Symmetric by construction: it diffs the two sorted files and classifies every
# differing line, rather than looping over one side and looking the other up.
# Looping over one side is exactly how a one-directional check is written by
# accident, and it cannot see a row the baseline does not have.
compare_build() {
  local obs="$1" base="$2" rc=0
  local nobs nbase
  nobs=$(wc -l < "$obs"); nbase=$(wc -l < "$base")
  if [ "$nobs" -eq 0 ] || [ "$nbase" -eq 0 ]; then
    note "  FAIL: observed=$nobs baseline=$nbase rows — an empty side cannot agree with anything"
    return 1
  fi

  local obs_paths="$WORK/cmp.obs.paths" base_paths="$WORK/cmp.base.paths"
  cut -f2 "$obs"  | LC_ALL=C sort > "$obs_paths"
  cut -f2 "$base" | LC_ALL=C sort > "$base_paths"

  # Membership, both directions. A program that VANISHES from the corpus is a
  # silent loss of coverage and reads as "clean" to any status-only diff.
  local added removed
  added="$(LC_ALL=C comm -23 "$obs_paths" "$base_paths")"
  removed="$(LC_ALL=C comm -13 "$obs_paths" "$base_paths")"
  if [ -n "$added" ]; then
    note "  CORPUS GREW — programs present now, absent from the baseline:"
    printf '%s\n' "$added" | sed 's/^/    + /'
    rc=1
  fi
  if [ -n "$removed" ]; then
    note "  CORPUS SHRANK — programs in the baseline that no longer exist:"
    printf '%s\n' "$removed" | sed 's/^/    - /'
    rc=1
  fi

  # Status, both directions.
  local joined="$WORK/cmp.joined"
  LC_ALL=C join -t "$(printf '\t')" -1 2 -2 2 \
    <(LC_ALL=C sort -t "$(printf '\t')" -k2,2 "$base") \
    <(LC_ALL=C sort -t "$(printf '\t')" -k2,2 "$obs") \
    > "$joined"
  local compared
  compared=$(wc -l < "$joined")

  local newly_fail newly_pass changed
  newly_fail="$(awk -F'\t' '$2==0 && $3!=0 {printf "    %s  (was build-clean, now exit %s)\n", $1, $3}' "$joined")"
  newly_pass="$(awk -F'\t' '$2!=0 && $3==0 {printf "    %s  (was exit %s, now build-clean)\n", $1, $2}' "$joined")"
  changed="$(awk -F'\t' '$2!=0 && $3!=0 && $2!=$3 {printf "    %s  (exit %s -> %s)\n", $1, $2, $3}' "$joined")"

  if [ -n "$newly_fail" ]; then
    note "  NEWLY FAILING (build was clean, now refuses):"; printf '%s\n' "$newly_fail"; rc=1
  fi
  if [ -n "$newly_pass" ]; then
    note "  NEWLY PASSING (build refused, now clean) — as much of a signal as a new failure:"
    printf '%s\n' "$newly_pass"; rc=1
  fi
  if [ -n "$changed" ]; then
    note "  STATUS CHANGED (both non-zero, different code):"; printf '%s\n' "$changed"; rc=1
  fi

  # Assert the comparison actually happened on every row. A join that silently
  # matched nothing (mismatched separator, unsorted input) would otherwise print
  # a clean sheet.
  if [ "$compared" -ne "$nobs" ] && [ "$rc" -eq 0 ]; then
    note "  FAIL: compared $compared rows but observed $nobs — the join lost rows,"
    note "        so a clean result here would be an artefact of the comparison itself"
    rc=1
  fi
  note "  compared $compared programs against $base"
  return $rc
}

# ── 1. build-outcome sweep ───────────────────────────────────────────────────
OBSERVED="$WORK/observed.tsv"
run_sweep() {
  note ""
  note "build_outcome_regression: building $corpus_n programs (JOBS=$JOBS)…"
  # NOTE ON EXIT CODES: the sweep's own status is never read off the end of a
  # pipeline. `xargs … | sort` would report SORT's status, and `cmd | tail` is
  # how a failure has been reported as success in this repo before. Each per-file
  # status is captured inside the worker and carried as DATA on stdout.
  xargs -a "$CORPUS" -P "$JOBS" -I{} \
    "$0" --measure-one "{}" "$WORK" "$AXON" > "$WORK/observed.raw"
  LC_ALL=C sort -t "$(printf '\t')" -k2,2 "$WORK/observed.raw" > "$OBSERVED"

  local n
  n=$(wc -l < "$OBSERVED")
  if [ "$n" -ne "$corpus_n" ]; then
    note "build_outcome_regression: CANNOT MEASURE — swept $n of $corpus_n programs."
    note "  A partial sweep must never be diffed against a full baseline: the missing"
    note "  rows would read as a shrunken corpus or, worse, as agreement."
    exit 2
  fi
  note "build_outcome_regression: swept $n programs — $(awk -F'\t' '$1==0' "$OBSERVED" | wc -l) exit-0, $(awk -F'\t' '$1==1' "$OBSERVED" | wc -l) exit-1, $(awk -F'\t' '$1!=0 && $1!=1' "$OBSERVED" | wc -l) other"
}

# ── 2. interpreter↔native differential ───────────────────────────────────────
# Declared list. Each row: <key>|<path>|<ai_mock 0|1>. Synthetic cases are
# written by the harness itself so the mechanism under test is pinned by the
# harness and not by a corpus file somebody may edit for an unrelated reason.
DIFF_CASES=""
add_case() { DIFF_CASES="${DIFF_CASES}$1|$2|$3
"; }

write_synthetic() {
  # The isolated repro: a field read off an Uncertain<T> bound by a match arm.
  # `pick`'s arms are 7, 3 and 5 — so a native 0 is OUTSIDE its range, which is
  # what makes "the value is invented" a measurement rather than an argument.
  cat > "$WORK/syn_matcharm_field.ax" <<'AX'
fn pick(s: str) -> i64 {
    match ai_extract_uncertain_i64(s) {
        Ok(u) => { if u.value > 0 { 7 } else { 3 } }
        Err(_) => 5
    }
}
fn main() -> i64 {
    println(to_str(pick("seven")))
    0
}
AX
  # Control A: the SAME match with no field read. Isolates "match-arm binding"
  # from "field access" — if this one ever diverges the mechanism is not what
  # the ledger says it is.
  cat > "$WORK/syn_matcharm_nofield.ax" <<'AX'
fn pick(s: str) -> i64 {
    match ai_extract_uncertain_i64(s) {
        Ok(_) => 7
        Err(_) => 5
    }
}
fn main() -> i64 {
    println(to_str(pick("seven")))
    0
}
AX
  # Control B: the same field read off a LET binding, whose payload type codegen
  # does carry. Agreement here is what makes the binding's ORIGIN the variable.
  cat > "$WORK/syn_let_field.ax" <<'AX'
fn pick() -> i64 {
    let u = uncertain_new(5, 0.9)
    if u.value > 0 { 7 } else { 3 }
}
fn main() -> i64 {
    println(to_str(pick()))
    0
}
AX
  # A case native REFUSES by design (`sandbox_run` is interpreter-only and
  # E0910-refused by codegen). It is here so the `native-refused-e0910` outcome
  # is EXERCISED rather than merely defined: an unreached classification branch
  # is indistinguishable from a broken one, and a refusal silently turning into
  # a built binary is precisely the regression that branch exists to catch.
  cat > "$WORK/syn_native_refuses.ax" <<'AX'
fn inner(n: i64) -> i64 {
    println("inside")
    0
}
fn main() -> i64 {
    let p = principal_root("p", true, true, true, 100)
    let sb = sandbox_create(p, "IO")
    sandbox_run(sb, "inner", 0)
}
AX
}

# classify_case <key> <path> <ai_mock> -> prints "<state>\t<key>"; details to stderr-ish log
classify_case() {
  local key="$1" path="$2" mock="$3"
  local env_prefix=()
  [ "$mock" = "1" ] && env_prefix=(env AXON_AI_MOCK=1)

  local iout="$WORK/$key.interp.out" nout="$WORK/$key.native.out"
  timeout "$CASE_TIMEOUT" "${env_prefix[@]}" "$AXON" run "$path" >"$iout" 2>"$WORK/$key.interp.err"
  local i_status=$?

  local blog="$WORK/$key.build.log" bin="$WORK/$key.bin"
  rm -f "$bin"
  timeout "$CASE_TIMEOUT" "${env_prefix[@]}" "$AXON" build "$path" -o "$bin" --no-cache >"$blog" 2>&1
  local b_status=$?

  local state
  printf '%s\t%s\n' "$i_status" "$b_status" > "$WORK/$key.status"
  if [ "$b_status" -ne 0 ]; then
    if grep -q 'E0910' "$blog"; then
      state=native-refused-e0910
    else
      state=native-build-failed
    fi
  elif [ ! -x "$bin" ]; then
    state=native-no-binary
  else
    timeout "$CASE_TIMEOUT" "${env_prefix[@]}" "$bin" >"$nout" 2>"$WORK/$key.native.err"
    printf '%s\t%s\t%s\n' "$i_status" "$b_status" "$?" > "$WORK/$key.status"
    if [ "$i_status" -eq 124 ]; then
      state=interp-timeout
    elif [ "$(cat "$iout")" = "$(cat "$nout")" ]; then
      state=agree
    else
      state=diverge
    fi
  fi
  printf '%s\t%s\n' "$state" "$key"
}

run_differential() {
  write_synthetic
  add_case corpus_search_rank            examples/asi/search_rank.ax                        1
  add_case corpus_ai_extract_uncertain   crates/axon-core/tests/fixtures/ai_extract_uncertain.ax 1
  add_case synthetic_matcharm_field      "$WORK/syn_matcharm_field.ax"                      1
  add_case synthetic_matcharm_nofield    "$WORK/syn_matcharm_nofield.ax"                    1
  add_case synthetic_let_field           "$WORK/syn_let_field.ax"                           1
  add_case synthetic_native_refuses      "$WORK/syn_native_refuses.ax"                      0

  note ""
  note "build_outcome_regression: interpreter↔native differential (stdout, not exit codes)…"
  : > "$WORK/diff_observed.tsv"
  local n=0
  while IFS='|' read -r key path mock; do
    [ -z "$key" ] && continue
    classify_case "$key" "$path" "$mock" >> "$WORK/diff_observed.tsv"
    n=$((n + 1))
  done <<< "$DIFF_CASES"

  if [ "$n" -eq 0 ]; then
    note "build_outcome_regression: CANNOT MEASURE — zero differential cases ran"
    exit 2
  fi
  LC_ALL=C sort -k2,2 "$WORK/diff_observed.tsv" -o "$WORK/diff_observed.tsv"
  note "build_outcome_regression: ran $n differential cases"

  # Report every case with its evidence, then diff against the baseline.
  while IFS=$'\t' read -r state key; do
    case "$state" in
      diverge)
        note "  DIVERGE  $key"
        note "    interp: $(tr '\n' '|' < "$WORK/$key.interp.out")"
        note "    native: $(tr '\n' '|' < "$WORK/$key.native.out")"
        note "    (exits interp/build/native: $(tr '\t' '/' < "$WORK/$key.status"))"
        ;;
      native-refused-e0910)
        note "  REFUSED  $key — native E0910 (reported outcome; NOT a pass, NOT a skip):"
        grep -m1 'E0910' "$WORK/$key.build.log" | sed 's/^/      /'
        ;;
      native-build-failed)
        note "  BUILD-FAILED  $key (not an E0910 refusal):"
        head -2 "$WORK/$key.build.log" | sed 's/^/      /'
        ;;
      agree)
        # Exit statuses are PRINTED but deliberately not part of the state: Axon
        # remaps 2..=15 and 101 onto 1, so a status comparison passes on a wrong
        # value. They are here as evidence for a human reading the report.
        note "  AGREE    $key — stdout: $(tr '\n' '|' < "$WORK/$key.interp.out")"
        note "    (exits interp/build/native: $(tr '\t' '/' < "$WORK/$key.status"))"
        ;;
      *)
        note "  $state  $key"
        ;;
    esac
  done < "$WORK/diff_observed.tsv"
}

compare_diff() {
  local obs="$1" base="$2" rc=0
  local joined="$WORK/diffcmp.joined"
  LC_ALL=C join -t "$(printf '\t')" -1 2 -2 2 \
    <(LC_ALL=C sort -t "$(printf '\t')" -k2,2 "$base") \
    <(LC_ALL=C sort -t "$(printf '\t')" -k2,2 "$obs") > "$joined"
  local nj nobs nbase
  nj=$(wc -l < "$joined"); nobs=$(wc -l < "$obs"); nbase=$(wc -l < "$base")
  if [ "$nj" -ne "$nobs" ] || [ "$nj" -ne "$nbase" ]; then
    note "  DIFFERENTIAL CASE SET CHANGED: baseline has $nbase, observed $nobs, matched $nj"
    LC_ALL=C comm -3 <(cut -f2 "$base" | LC_ALL=C sort) <(cut -f2 "$obs" | LC_ALL=C sort) | sed 's/^/    /'
    rc=1
  fi
  local moved
  moved="$(awk -F'\t' '$2!=$3 {printf "    %s: %s -> %s\n", $1, $2, $3}' "$joined")"
  if [ -n "$moved" ]; then
    note "  DIFFERENTIAL STATE CHANGED (either direction is a signal):"
    printf '%s\n' "$moved"
    rc=1
  fi
  note "  compared $nj differential cases against $base"
  return $rc
}

# ── --self-test: prove the comparator FAILS when it should ───────────────────
# A harness that has never failed is not a verified harness. This runs the real
# comparator against a DOCTORED copy of the real baseline and asserts (a) a
# non-zero status and (b) that the report NAMES the two doctored programs, in
# the correct direction each. It needs no sweep, so it is cheap enough to run
# every time — and it is the guard for the comparator itself, which is the part
# that cannot be checked by the thing it checks.
self_test() {
  local rc=0
  [ -f "$BASELINE" ] || { note "self-test: CANNOT MEASURE — no baseline at $BASELINE"; exit 2; }

  local pass_row fail_row
  pass_row="$(awk -F'\t' '$1==0 {print $2; exit}' "$BASELINE")"
  fail_row="$(awk -F'\t' '$1==1 {print $2; exit}' "$BASELINE")"
  local drop_row
  drop_row="$(awk -F'\t' '$1==0 {print $2}' "$BASELINE" | sed -n 2p)"
  if [ -z "$pass_row" ] || [ -z "$fail_row" ] || [ -z "$drop_row" ]; then
    note "self-test: CANNOT MEASURE — the baseline lacks both a 0 and a 1 row to perturb"
    exit 2
  fi

  # "Observed" = the baseline with three deliberate perturbations:
  #   * $pass_row flipped 0 -> 1   (must be reported as NEWLY FAILING)
  #   * $fail_row flipped 1 -> 0   (must be reported as NEWLY PASSING)
  #   * $drop_row deleted          (must be reported as a SHRUNKEN CORPUS)
  local doctored="$WORK/doctored.tsv"
  awk -F'\t' -v p="$pass_row" -v f="$fail_row" -v d="$drop_row" 'BEGIN{OFS="\t"}
    $2==d {next}
    $2==p {print 1, $2; next}
    $2==f {print 0, $2; next}
    {print}' "$BASELINE" > "$doctored"

  note "self-test: comparing a DOCTORED sweep against the committed baseline."
  note "self-test:   flipped clean->refused : $pass_row"
  note "self-test:   flipped refused->clean : $fail_row"
  note "self-test:   deleted from the sweep : $drop_row"
  local out status
  out="$(compare_build "$doctored" "$BASELINE" 2>&1)"
  status=$?
  printf '%s\n' "$out" | sed 's/^/    /'

  if [ "$status" -eq 0 ]; then
    note "self-test: FAIL — the comparator returned 0 on a doctored sweep. It is not"
    note "  detecting anything, and every green run it has ever produced is meaningless."
    return 1
  fi
  printf '%s' "$out" | grep -q "NEWLY FAILING" || { note "self-test: FAIL — no NEWLY FAILING section"; rc=1; }
  printf '%s' "$out" | grep -q "NEWLY PASSING" || { note "self-test: FAIL — no NEWLY PASSING section (the direction a one-way check misses)"; rc=1; }
  printf '%s' "$out" | grep -q "CORPUS SHRANK" || { note "self-test: FAIL — a dropped program was not reported"; rc=1; }
  printf '%s' "$out" | grep -qF "$pass_row" || { note "self-test: FAIL — the report did not NAME $pass_row"; rc=1; }
  printf '%s' "$out" | grep -qF "$fail_row" || { note "self-test: FAIL — the report did not NAME $fail_row"; rc=1; }
  printf '%s' "$out" | grep -qF "$drop_row" || { note "self-test: FAIL — the report did not NAME $drop_row"; rc=1; }

  # The control: the UNdoctored baseline against itself must come back clean.
  # Without it, a comparator that fails on everything would pass the checks above.
  local cout cstatus
  cout="$(compare_build "$BASELINE" "$BASELINE" 2>&1)"; cstatus=$?
  if [ "$cstatus" -ne 0 ]; then
    note "self-test: FAIL — the comparator reports a difference between the baseline and ITSELF:"
    printf '%s\n' "$cout" | sed 's/^/    /'
    rc=1
  else
    note "self-test: control — baseline vs itself is clean (so the failures above are the perturbations, not noise)"
  fi

  # The differential comparator gets the same treatment: flip one case's state
  # and drop another, and assert both are caught and NAMED.
  if [ -f "$DIFF_BASELINE" ] && [ "$(wc -l < "$DIFF_BASELINE")" -ge 2 ]; then
    local dkey ddrop ddoctored dout dstatus
    dkey="$(awk -F'\t' 'NR==1{print $2}' "$DIFF_BASELINE")"
    ddrop="$(awk -F'\t' 'NR==2{print $2}' "$DIFF_BASELINE")"
    ddoctored="$WORK/doctored_diff.tsv"
    awk -F'\t' -v k="$dkey" -v d="$ddrop" 'BEGIN{OFS="\t"}
      $2==d {next}
      $2==k {print "agree", $2; next}
      {print}' "$DIFF_BASELINE" > "$ddoctored"
    note "self-test: differential — forced '$dkey' to 'agree' and dropped '$ddrop'"
    dout="$(compare_diff "$ddoctored" "$DIFF_BASELINE" 2>&1)"; dstatus=$?
    printf '%s\n' "$dout" | sed 's/^/    /'
    if [ "$dstatus" -eq 0 ]; then
      note "self-test: FAIL — the differential comparator returned 0 on a doctored case set"
      rc=1
    fi
    printf '%s' "$dout" | grep -qF "$dkey"  || { note "self-test: FAIL — the differential report did not NAME $dkey"; rc=1; }
    printf '%s' "$dout" | grep -qF "$ddrop" || { note "self-test: FAIL — the differential report did not NAME the dropped case $ddrop"; rc=1; }
    if ! compare_diff "$DIFF_BASELINE" "$DIFF_BASELINE" >/dev/null 2>&1; then
      note "self-test: FAIL — the differential comparator differs from the baseline against ITSELF"
      rc=1
    else
      note "self-test: differential control — baseline vs itself is clean"
    fi
  else
    note "self-test: CANNOT MEASURE the differential comparator — $DIFF_BASELINE is absent or too small"
    rc=1
  fi
  return $rc
}

# ── main ─────────────────────────────────────────────────────────────────────
if [ "$MODE" = selftest ]; then
  if self_test; then
    note "build_outcome_regression: SELF-TEST PASS — the comparator fails on a doctored"
    note "  sweep in all three directions and is clean on an undoctored one."
    exit 0
  fi
  note "build_outcome_regression: SELF-TEST FAIL"
  exit 1
fi

if [ "$MODE" = full ] || [ "$MODE" = build ]; then
  run_sweep
  if [ "$RECORD" -eq 1 ]; then
    cp "$OBSERVED" "$BASELINE"
    note "build_outcome_regression: RECORDED build baseline -> $BASELINE ($(wc -l < "$BASELINE") rows)"
  else
    [ -f "$BASELINE" ] || { note "build_outcome_regression: CANNOT MEASURE — no baseline at $BASELINE (use --record)"; exit 2; }
    note ""
    note "build_outcome_regression: build-outcome diff (symmetric):"
    compare_build "$OBSERVED" "$BASELINE" || fail=1
  fi
fi

if [ "$MODE" = full ] || [ "$MODE" = diff ]; then
  run_differential
  if [ "$RECORD" -eq 1 ]; then
    cp "$WORK/diff_observed.tsv" "$DIFF_BASELINE"
    note "build_outcome_regression: RECORDED differential baseline -> $DIFF_BASELINE"
  else
    [ -f "$DIFF_BASELINE" ] || { note "build_outcome_regression: CANNOT MEASURE — no differential baseline at $DIFF_BASELINE (use --record)"; exit 2; }
    note ""
    note "build_outcome_regression: differential diff:"
    compare_diff "$WORK/diff_observed.tsv" "$DIFF_BASELINE" || fail=1
  fi
fi

if [ "$RECORD" -eq 1 ]; then
  note "build_outcome_regression: baselines recorded. Review the diff before committing —"
  note "  a re-baseline is a CLAIM that every moved program was supposed to move."
  exit 0
fi

if [ "$fail" -ne 0 ]; then
  note ""
  note "build_outcome_regression: FAIL — the corpus moved against the committed baselines."
  note "  Every line above names a program and the direction it moved. If a fix INTENDED"
  note "  a move, the intended set must match this list exactly, and the baselines are"
  note "  then re-recorded with --record as a deliberate act."
  exit 1
fi
note ""
case "$MODE" in
  build) note "build_outcome_regression: PASS — all $corpus_n build outcomes match $BASELINE."
         note "build_outcome_regression:   (--build-only: the differential was NOT run)" ;;
  diff)  note "build_outcome_regression: PASS — every declared differential case matches $DIFF_BASELINE."
         note "build_outcome_regression:   (--diff-only: the $corpus_n-program build sweep was NOT run)" ;;
  *)     note "build_outcome_regression: PASS — $corpus_n build outcomes and every declared"
         note "build_outcome_regression:   differential case match the committed baselines." ;;
esac
exit 0
