# R51 review — EvidenceGates: can §8 T2–T7 and §9 be executed on gpumaster?

Reviewer: EvidenceGates. Trees: `r51` = `/home/cklaus/projects/axon-lanes/r51` (origin/main 5864c423 + spec e3abe87e, read only, no cargo run there);
`v022` = `/home/cklaus/projects/axon-lanes/v022-ref` (c9ed2ddd). T2 measurements were made on a private clone of
5864c423 at `/tmp/r51/eg-main` (not the r51 worktree). Logs: `/tmp/r51/eg-*.log`.

## 0. Host facts (gpumaster, measured 2026-10-10 ~21:00Z)

| item | state |
|---|---|
| user | root (uid 0), `$HOME=/root`, cargo at `/root/.cargo/bin` |
| CPU / RAM | 32 cores; 245 GiB total, ~58 GiB available, swap 7/7 GiB used; load avg 16–22 during this review |
| `/dev/kvm` | present (`crw-rw----+ root kvm 10,232`) |
| firecracker | `/usr/local/bin/firecracker` = Firecracker v1.10.1 |
| guest image | **absent**: no `dist/guest-linux/{manifest.json,rootfs.sqfs}` in any tree or job clone; no kernel tarball in `~/.cache/axon-guest-build` (1.3 MB, keys + temp dirs only). Build deps present (mksquashfs, busybox-static, flex, bison, bc, gcc, make, libelf-dev); `lld` missing (only for the `axon` kernel backend). Building needs the pinned kernel tarball from kernel.org [UNVERIFIED: network] |
| `axonb263` user | absent → `b263_qualify` SKIPs in the strict gate |
| MiCode peer | absent → `loop_interop_gate`, `v022_micode_gates` SKIP |
| `run_managed.sh`, `release_check.sh` | present on BOTH trees (`scripts/`); they differ (run_managed: small diff; release_check: 125 diff lines) — S0 must take veto's |
| wasm/perf tools (T2) | wasmtime, objdump, c++filt, wasm-objdump, perf (`instructions:u` works as root, paranoid=4), taskset, node, rust targets wasm32-wasip1 + wasm32-unknown-unknown + x86_64-musl: all present |
| concurrent work | a strict gate on c9ed2ddd is RUNNING now: `/var/tmp/axon-evidence/jobs/strict-final/clone/.axon-runs/strict-v022-final-20261010T184044Z-1118700` (started 18:40:44Z; at 21:03Z still at "required crate verification", log line 7528 of ~10.5k) |

## 1. Executability table

Durations: **M** = measured by me in this review, **R** = recorded in a receipt/amendment (source given).

| test | command (exact) | prerequisites | on host | duration | autonomous |
|---|---|---|---|---|---|
| T2a parity | `scripts/vm_parity.sh` (builds `cargo build --release -p axon-core --no-default-features --bin axon`, or `AXON_BIN=`) | release build; bash; 30 s/run limit | y | **M**: release build 53 s; script 16 s (`325 files, 3244 bodies, 111633 lowered ops, 264 tree ops, 0 differ`, PASS, rc 0, 32 jobs) | y |
| T2b wasm depth | `scripts/vm_wasm_depth.sh` | wasmtime, objdump, c++filt, wasm-objdump, wasm32-wasip1; builds axon-run debug+release; runs `scripts/wasm_stack_budget.py` | y | **M**: 71 s, PASS | y — but see EG-6 (will fail after S2/S3 unless `nest_cost` is re-measured) |
| T2c perf | `scripts/vm_perf_gate.sh` (+ `--repros`) | `perf stat -e instructions:u`, `taskset -c 6`, release axon | y | **M**: 6 s (five programs) + 1 s (`--repros`), PASS. Baseline medians at 5864c423 in EG-7 | y |
| T2d cli_run | `cargo test -p axon-core --no-default-features --test cli_run` with `CARGO_TARGET_DIR` **unset** (in-tree `target/`) | wasm toolchain, node; in-tree target dir | y | **M**: 134 s (787 passed). With `CARGO_TARGET_DIR=/tmp/...`: 66 s, **6 FAILED** (EG-8) | y |
| T3 taint unit | `cargo test -p axon-core --no-default-features --lib taint_tests` (on v022: 39 tests) | none | y | **M** on v022: 34 s cold (28.3 s compile, 5.57 s run), 39 passed | y |
| T3 (full seal port, EG-5) | `cargo test -p axon-core --no-default-features --no-fail-fast` | in-tree target for the script harnesses | y | **R** amendment 122: rc 0, 1885 passed (gm); not timed in receipt | y |
| T4 oracle twin | `cargo test -p axon-psv --test sealed_frames -- a_sealed_caller_is_refused_in_the_same_words_for_an_operator_name_and_a_missing_one every_position_the_sealed_walk_visits_has_a_twin_row` | `axon` binary built (the runner execs it); `cargo build -p axon-psv --example psv_dev` (gate.sh:326) | y | **R** whole `sealed_frames` (37 tests incl. twin) 40.29 s run time (ig15-strict2 log:9535) | y |
| T5 receiver PICK | `cargo test -p axon-core --no-default-features --lib interp::taint_tests::an_operator_value_the_candidate_picked_is_never_dispatched_on` + `cargo test -p axon-psv --test sealed_frames -- an_operator_value_the_candidate_picked_is_refused_through_the_runner` | as T3/T4 | y | inside T3 (5.6 s) / T4 (40 s) runs | y |
| T6 runner legs | `cargo test -p axon-psv --test sealed_frames --test runner`; `cargo test -p axon-fabric --test psv_dispatch --test check_effects --test attestation --test cortex_via_fabric --test observer_service` | `cargo build -p axon-cortex --bins` (gate.sh:279-280), `axon` binary, root for fabric fixtures (we are root) | y | **R** (ig15-strict2 log 570–1359, 9459–9535): sealed_frames 37/40.3 s, runner 29/3.2 s, psv_dispatch 54/8.7 s, check_effects 27/4.3 s, attestation 8/7.1 s, cortex_via_fabric 4/3.8 s, observer_service 29/24.6 s (run time, excl. build) | y |
| T7a mutation re-run | `python3 scripts/v022_g01_mutations.py --scope=all --only=<ids> OUT.json` (sample) or `--scope=all --shard=K/N OUT.json` + `--merge` (scripts/v022_g01_mutations.py:17, 8702-8870) | CLEAN committed tree (refuses dirty, :8751); each cell `bounded_run 16G 1800 cargo test -q -p PKG TARGET -- --exact TEST` (:8398, `V022_MUT_MEM`); TMPDIR on /var/tmp (/tmp is 12 GB tmpfs, :45-60) | y | **R** per-row cost on gm for interp-heavy single-shard runs 4.7–11.2 s/row (psv1e-mut 101 rows/11.5 min; c9r15-EQ-1 55 rows/10.3 min at b974655d); 2–3 parallel shards under load 24–34 s/row (i10/i11). **Estimate** for the 426 axon-core/src rows: 50–80 min single shard on a quiet host; 1.5–2.5 h wall if sharded on a loaded host | y |
| T7b refusal coverage | `python3 scripts/v022_refusal_coverage.py` and `python3 scripts/v022_refusal_coverage.py --freeze` | none (python only) | y | **R** am.122: rc 0 / rc 0, REMAINDER 207; seconds [UNVERIFIED timing] | y |
| T7c pci delta | `python3 scripts/pci_delta.py --check` (also run by `crates/axon-core/tests/pci_delta_note.rs::the_pci_delta_note_is_what_git_says`) | full git history incl. BASE 31413ca7 | y | seconds | y — but WILL FAIL on the unified tree (EG-1) |
| §9 strict gate | `scripts/run_managed.sh start <name> --snapshot <commit> --mem-max 40G --deadline 18000 -- ./scripts/gate.sh --strict` | root (fabric launcher tests), 40 GiB cgroup, cgroup v2, `CORTEX_V022_BOOTSTRAP_VENV` (auto under --strict); guest image / MiCode / axonb263 / Android absent → 9 SKIPs (non-fatal unless `AXON_HARNESS_STRICT=1`) | y (with the 9 SKIPs) | **R** ig15-strict2 at d8e8206e: 07:08:11Z→08:36:14Z = **88 min**, 350 suites, 6236 passed, oom 0; strict-final at c9ed2ddd under contention: >2 h 22 m and not finished at 21:03Z | y |
| §9 verify | `scripts/run_managed.sh verify <rundir> --for <commit>` (run_managed.sh:806) | the receipt dir under `<ROOT>/.axon-runs` of the tree it ran in | y | seconds | y |
| §9 release check | `scripts/release_check.sh <commit>` (reads `.axon-runs` receipts; does not run tests) | a CITABLE strict receipt for that exact head; 0 OPEN false greens; every class A/B crate has a required command | y | seconds | y |
| §9 confirmation review | PSV-1, PSV-2, SENTINEL reviewers | reviewer agents (as rounds 1–15) | n/a | hours [UNVERIFIED] | y as AI review; NOT a certifying review (EG-13) |

## 2. Findings

### EG-1 — MAJOR — `pci_delta.py --check` cannot PASS on the unified tree without unplanned classification work
- **Spec:** §8 T7 "`pci_delta.py --check` PASS"; §11 S6.
- **Evidence:** `scripts/pci_delta.py` (v022) lines 5-17, 27: every commit in `git log 31413ca7..HEAD -- crates/axon-core/src` must have a THEMES entry, else "unclassified commit(s)" FAIL; `--check` also fails if any commit after the note's pinned head touches `crates/axon-core/src`. `31413ca7` is NOT an ancestor of main (`git merge-base 31413ca7 5864c423` = 4cceb892). `git rev-list --count 31413ca7..5864c423 -- crates/axon-core/src` = **82** main-side commits, all unclassified. Every R51 slice commit adds more. The same check runs inside the strict gate via `crates/axon-core/tests/pci_delta_note.rs:15` (`the_pci_delta_note_is_what_git_says`), so §9 fails too.
- **Proposed edit:** add to S0 (or S6): "classify every main-side commit under `crates/axon-core/src` (82 at 5864c423) and every R51 commit in `scripts/pci_delta.py` THEMES (theme `R50/AX data model, unsealed: no change to what a sealed run refuses` or per-commit), regenerate the mechanical block of `governance/notes/v022-pci-delta.md` at the S6 head, and update the pinned head last". State whether BASE stays 31413ca7 (then the R50 VM commits are listed as interpreter changes — correct, since they touch dispatch).

### EG-2 — MAJOR — Fork 4 row count is wrong and the scope is too narrow
- **Spec:** §4 Fork 4 "Rows anchored in `crates/axon-core/src/interp*` (~263)"; "~2290 active mutation rows" (§4 Fork 1).
- **Evidence** (importing `scripts/v022_g01_mutations.py` on v022): 2439 ids, 149 retired, `in_scope(…,'all')` active = **2281** (2290 in am.122 counts the 9 SIBLING_ONLY rows). Active rows with file under `crates/axon-core/src/interp*` = **360** (eval.rs 89, interp.rs 77, taint.rs 72, conform.rs 71, pin.rs 32, builtins.rs 12, goal.rs 6, proptest.rs 1). A further **66** active rows guard seal code in other axon-core files (resolver.rs 23, main.rs 15, checker.rs 14, lib.rs 5, builtins.rs 3, parser.rs 2, seal_split.rs 2, ast.rs 1, kernel.rs 1); against main's files 61 of those 66 have 0 matches of their `old` text. Am.121 itself re-ran "ALL 427 active rows whose target is under crates/axon-core/src". Across all 2281 rows only 49 apply verbatim on main today; 1959 target files absent on main (arrive with S0); mismatching outside axon-core: axon-cortex 17, axon-guest-init 8, axon-os 3, scripts/lib 2 (files main also changed). Interp rows test via: axon-core `--lib` 190, `--no-default-features --lib` 97, axon-psv `sealed_frames` 25, a target spelled `crates/axon-cortex/src/lib.rs` 44, psv_test_selection 3, dispatch_rule_switch 1; 114 distinct tests.
- **Proposed edit:** Fork 4: "Rows anchored in `crates/axon-core/src` (426 active at c9ed2ddd: 360 `interp*`, 66 resolver/main/checker/lib/builtins/parser/seal_split/ast/kernel) are re-anchored and re-run". S0 acceptance: "every one of the 2281 active rows' `old` text occurs exactly once in the merged tree, or the row is listed for re-anchor in S6" (the harness's own drift check). Replace "~2290 active rows" with "2281 active (+9 sibling-only)".

### EG-3 — MAJOR — "EQUIVALENT retirements in interp* are RESET" names the wrong set and no standard
- **Spec:** Fork 4 "EQUIVALENT retirements in `interp*` are RESET and re-argued … (amendments 53-122's arguments …)".
- **Evidence:** retired rows with an `interp*` file: **2** (M58 interp.rs, M89 interp/builtins.rs), both EQUIVALENT; two more in axon-core/src (M60 resolver.rs, M3260 main.rs). EQUIV_RECORD has 148 entries, 6 mention `interp`. Re-retirement standards in use: four executed cells (`v022_paired_disable.py --only=`, e.g. M3260, am.121) vs the full paired-disable record; the full record for M3260 hung >2 h in a `privileged_launcher` child and was killed (am.121 "Unfinished (a)").
- **Proposed edit:** list M58, M89, M60, M3260 explicitly; say they are re-retired on executed four-cell records at the S6 head (ADR-003 standard), with the full paired-disable record not required; or re-activated if a cell fails.

### EG-4 — MAJOR — refusal-coverage gate has interp anchors, and the NEW guards of S1/S3 have no rows
- **Spec:** T7 "refusal-coverage gate plain and `--freeze` exit 0"; Fork 1 pin + `run_body` check; Fork 3 deep copy.
- **Evidence:** `scripts/v022_refusal_coverage.py`: `REGIONS` anchors two doc-comment lines in interp.rs (:319-321); `SCOPE_FN_REGIONS` selects functions matching `SEAL_FN = (?<!un)seal|conform|cast` (:102) in 12 interp-related entries; `EXEMPT` (1022 entries) has 9 in interp files; `SCOPE_FILES` = conform.rs; REMAINDER is recomputed (207 at am.122). A row whose `old` text is not present exactly once is BAD (:24-25). The engine-pin refusal (`engine_at_build`, `run_body`) and the deep-copy at seal edges sit in functions that do not match SEAL_FN, so the gate will neither see nor require them. T1/T8 have no mutation rows.
- **Proposed edit:** T1 and T8 each add mutation rows + attack markers (`scripts/v022_attack_markers.py`) for: the build-time pin refusal, the `run_body` defence check, each VM entry's consult, and each seal-edge deep copy; name those functions so SEAL_FN selects them (e.g. `seal_engine_pin`, `seal_copy_*`) or add a `REGIONS` span; re-anchor REGIONS/EXEMPT/SCOPE_FN_REGIONS in S6; record the new REMAINDER count.

### EG-5 — MAJOR — T3 covers 39 of the 144 v022-only interpreter tests
- **Spec:** T3 "v0.22's `taint_tests.rs` ported".
- **Evidence:** `cargo test -p axon-core --no-default-features --lib -- --list` on v022 (844) vs main clone (745): **144** tests are v022-only — interp::tests 96, interp::taint_tests 39, resolver::tests 5, seal_split 1, parser 1, kernel 1, interp::conform 1 (`/tmp/r51/eg-v022-only-libtests.txt`). v022-only integration tests in `crates/axon-core/tests`: axon_test_outcomes, dispatch_rule_switch_is_test_only, harness_binaries, harness_integrity, pci_delta_note, pci_isolation, postc9_spec, psv_test_selection, refusal_coverage_gate, test_completion, script_spawn/, common/. `scripts/v022_pci_gates.sh` pins 93 rows of exact test names (51 are axon-core lib rows), e.g. `interp::tests::a_sealed_caller_cannot_tell_an_operator_fn_from_a_missing_one`, `interp::tests::every_global_read_goes_through_global_ref`.
- **Proposed edit:** T3 := "every v022-only axon-core test (the 144 lib tests in the list + the 11 integration files) is ported under the SAME name and passes; `scripts/v022_pci_gates.sh` PASS (93 rows)". Baseline on v022: taint_tests 39 passed in 5.6 s; whole crate 1885 passed (am.122).

### EG-6 — MAJOR — T2 `vm_wasm_depth.sh` "unchanged" will fail: the wasm32 frame budget is pinned to the byte with ~2% depth headroom
- **Spec:** T2 "`vm_wasm_depth.sh` … pass unchanged"; Fork 2 "taint touched only on the tree path".
- **Evidence (M, 5864c423 clone, `/tmp/r51/eg-t2.log`):** `vm_wasm_depth.sh` runs `wasm_stack_budget.py`, which checks per-function frame bytes against `mod nest_cost` (r51 `crates/axon-core/src/interp.rs:1294-1319`, e.g. `EVAL = b(192, 288)`, `CALL_FN_IN = b(160, 176)`). 114 checks, **89 at exactly budget** (e.g. `eval 192 <= 192`, `call_fn_in 160 <= 160`, `dispatch_call 176 <= 176`, `eval_call 352 <= 352`). Depth margins: debug tree `closure` default stack deepest=157 vs required ≥154 (`REQUIRED_DEFAULT = GUARD*12/10+1`, vm_wasm_depth.sh:102). v022's seal code is not cfg'd off wasm32 (`grep -c wasm32 interp/taint.rs` = 0). Any frame growth in eval/call_fn_in from taint/seal fails the budget check; ~2% growth on the tree path fails the depth check.
- **Proposed edit:** T2: "`vm_wasm_depth.sh` exits 0; `nest_cost` constants may be re-measured, each change stated with old/new bytes; required depth ratios unchanged". And decide in Fork 2: either the seal is compiled only for targets that run sealed runs (`cfg(not(target_arch="wasm32"))`, with the pin refusing sealed runs on wasm32), or frame growth is budgeted.

### EG-7 — MINOR — §10 "no measurable change within the gate's existing tolerance" has no tolerance to measure against
- **Evidence (M):** `vm_perf_gate.sh` compares each median to an ABSOLUTE CPython budget; medians at 5864c423 (instructions:u, 3 runs, spread ≤ 5e-6): fib-recursive 2,298,930,456 (budget 3,423,642,492, 67%), collatz 18,580,211,600 (26,058,032,109, 71%), mandelbrot 5,068,914,361 (15,023,124,683, 34%), arr-sum 4,879,384,240 (23,624,147,901, 21%), qsort 11,401,042,655 (18,967,575,334, 60%). Repros: loop 92.2/300, call 244.1/800, part 218.1/250, fold 58.0/350, fastcall 244.1/300, mutcall 274.2/600, foldmod 95.0/200, swap 258.3/730. A 40% regression would still PASS.
- **Proposed edit:** §10: "each of the five medians and each repro row ≤ 1.01 × its value at 5864c423 (numbers above), measured with `vm_perf_gate.sh` and `--repros`".

### EG-8 — MINOR — main's cli_run needs the in-tree `target/`; the lane convention breaks it
- **Evidence (M):** with `CARGO_TARGET_DIR=/tmp/r51/target-EvidenceGates-main`: 781 passed, **6 failed** (host_await_runs_identically_on_wasm_wasip1, wasm_asyncify_host_await_suspends_across_async_r7c, wasm_browser_host_await_round_trips_r7c, wasm_host_io_matches_native_via_wasi, wasm_interp_matches_native_on_pure_compute, wasm_interpreter_evals_identically_to_native_r7c) — the harnesses hardcode `target/` (`scripts/wasm_parity.sh:70,74`, `wasm_asyncify_host_await.sh:44`). With it unset: 787 passed, 134 s.
- **Proposed edit:** T2 names the command and says "`CARGO_TARGET_DIR` unset (in-tree target)"; builders must not treat those 6 as regressions.

### EG-9 — MAJOR — §9 "all of §8 green at one commit" + strict gate: the gate does not run most of §8
- **Evidence:** main `scripts/gate.sh` has no reference to vm_parity/vm_wasm_depth/vm_perf_gate (grep exit 1). v022 `gate.sh` runs refusal coverage plain only (:317), not `--freeze`; it does not run the mutation matrix; it runs `v022_pci_gates.sh` (:297) and the closed-loop crate tests (:281). A commit cannot cite its own receipt (am.122). Strict gate cost: 88 min quiet (ig15-strict2), >2h22m under contention (strict-final, running). The strict gate SKIPs the real-guest boot (`SKIP: no built guest image`, ig15 log:3414) so the sealed run INSIDE the guest (static axon in rootfs) is not exercised on this host.
- **Proposed edit:** §9: "At head H: a CITABLE strict receipt (`run_managed … --snapshot H … gate.sh --strict`, `verify --for H`, `release_check.sh H`), plus separate receipts at H for T2 (four commands), T7 (mutation OUT.json with `commit == H`, refusal coverage `--freeze`), all listed in §14 by path. The guest boot test is a stated non-result (no image on gpumaster); T1 states whether the guest runner path reaches `engine_at_build`." [UNVERIFIED: whether the guest init sets the seal state through the same path.]

### EG-10 — MINOR — T4 description
- **Evidence:** `crates/axon-psv/tests/sealed_frames.rs:1902` — 78 forms (`existence_forms`, :1778) × 8 placements (`existence_placements`, :1877) × 2 runs (operator vs missing name) = 1248 `check` invocations through the runner, static path only (S5); drift test `every_position_the_sealed_walk_visits_has_a_twin_row` (:2046).
- **Proposed edit:** T4 names both tests and says it exercises the static layer (S5), not taint.

### EG-11 — MINOR — T5 omits the "Fails" control and the liveness check
- **Evidence:** `crates/axon-core/src/interp/taint_tests.rs:326-375`: 11 `HONEST` cases (Ok), 1 control "the wrong answer on the operator's own strict impl still fails keyed" (`Fails`), 19 `pick:` attacks (Refused); run under `Rules::TaintOnly` and `Rules::Both`, then `attacks_are_live` (attacks succeed with the rules off).
- **Proposed edit:** T5: "`interp::taint_tests::an_operator_value_the_candidate_picked_is_never_dispatched_on` (11 Ok, 1 Fails, 19 Refused, live with rules off) and `sealed_frames::an_operator_value_the_candidate_picked_is_refused_through_the_runner`".

### EG-12 — MINOR — T7 mutation mechanics unstated
- **Evidence:** v022_g01_mutations.py: `--only` output "is marked and cannot be merged as a shard" (:19); scope `all` + `--shard=K/N` + `--merge` is the only mergeable form; tree must be committed and clean; 17 rows in am.122 gave `BAD baseline=error` under host contention and were re-run alone.
- **Proposed edit:** T7: "`--scope=all --only=<the 426 ids>` in ≤3 lists at the S6 head (as am.121), each OUT.json committed under governance/status with its commit; a `baseline=error` row is re-run alone and reported, never counted from the first pass".

### EG-13 — MINOR — §9 review and "freeze" wording vs ADR-003
- **Evidence:** am.122 / `v022_release_claims.json` engineering_qualified note: no freeze, no signatures, no guest boot, no certifying review under ADR-003. `v022_refusal_coverage.py --freeze` only fails on NOT_YET_SCANNED files (header :27-32); the operator freeze is `v022_freeze_manifest.py`, not required.
- **Proposed edit:** §9: "The confirmation review is a non-certifying engineering review; no freeze manifest, signature or guest boot is produced or claimed (ADR-003)". Then every §8/§9 item is autonomous on gpumaster.

## 3. Items that cannot be met autonomously

None of T2–T7 or the strict gate needs an operator, signature or freeze on this host. Caveats:
- the guest boot leg, `b263_qualify`, MiCode gates, Android: SKIP on gpumaster (non-results, not failures);
- `pci_delta.py --check` needs the classification work in EG-1 before it can pass;
- the §9 review is an AI review round; a certifying (human/independent) review is out of scope under ADR-003 and must not be claimed.
