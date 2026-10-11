# R51 baseline (Step 0)

Tree: `origin/main` 5864c423 + spec commit e3abe87e (`r51/unify`), host gpumaster, 2026-10-10.

| Command | Result | Wall |
|---|---|---|
| `cargo test -p axon-core --no-default-features --no-fail-fast` | 1772 passed, 0 failed | 213 s |
| `./scripts/gate.sh` (`CARGO_TARGET_DIR` unset) | rc 0, `✅ gate PASSED`; 103 test binaries, 2632 passed, 0 failed | 4 m 29 s |

`gate.sh` (non-strict) skips the codegen/wasm parity wrappers (it prints the list); those are first required at
S6 under `--strict`. The `REDTEAM FAILED: blocking deploy` line in the log is the expected output of a
`manifest::tests` case, not a failure.

Known failures: none.

## Slice gates

| Head | Slice | `./scripts/gate.sh` | Suite (`cargo test -p axon-core --no-default-features --no-fail-fast`) |
|---|---|---|---|
| 12ef0c9b | S0 | FAILED at tests: `harness_integrity::a_mutation_run_restores_the_interpreter_after_every_row`, tree dirtied mid-run by an untracked task file (not a code failure) | - |
| d099fba1 | S0 + S1 + S0b | PASSED except 4 deferred stages (Stage-5 gate tests, Protected Check Isolation, PSV negative matrix, refusal-site coverage), 27 min | 1855 passed, 0 failed, 67 ignored |
| d1a2ed34 | S5 | 1st run FAILED (axon-psv runner: `/tmp` tmpfs full, infra); 2nd FAILED (axon-fabric readiness: `TMPDIR` under mode-750 `/home/cklaus`, infra); 3rd with `TMPDIR=/var/tmp/r51-tmp`: PASSED except the same 4 deferred stages, 25 min | 1861 passed, 0 failed, 79 ignored |

Perf (`vm_perf_gate.sh`, instructions:u, spec §10 budget ≤ 1.01 × 5864c423): S5 fib-recursive 2,320,094,380 (+0.92%, inside).
S2.2 at 43bcb879 (unmerged): fib-recursive 2,351,812,724 (+2.30%) and repros call/fastcall 248.1 (+1.6%) are OVER budget; the fix is assigned to S2.3.
| 4bbc1563 | S2 (S2.1-S2.3 steps 1-3) + S0b-perf | PASSED except the same 4 deferred stages, 24 min, `TMPDIR=/var/tmp/r51-tmp` | 1896 passed, 0 failed, 80 ignored |

Perf at one fixed path (`axon-lanes/r51-perf`, §10 base 5864c423): bisect put the `call`/`fastcall` +3.3% (244.1 -> 252.1) on S0b 56389c53 (leaf_call's `sum_return(..).ok()?` copied every leaf result through the stack); S0b-perf afd9e294 brings both to 232.1. S2.3 head d7fc0031 added no unsealed cost (fib-recursive +0.77%, every median <= 1.01x).

## Mutation survivors (accepted, with reason)

| Slice | Mutant | Reason |
|---|---|---|
| S2.3 step 2b | `for` loop variable `ts(bt)&VAL` dropped | Equivalent at S2: every store in the body already carries the loop pc (>= the variable's VAL); only S3 sinks read a binding's value taint without the pc. Re-check at S3. |
| S2.3 step 2b | handler sticky on entries change | Unreachable until S3 (`with_frame` has no production caller); ignored test registered under S3. |
| S2.3 step 2b | handler return-arm `ret` (bare-tail `Ok(v)`) | Needs nested handlers replaying a bare-tail resume; `taint.ret` had no reader before step 4. Re-check at S3. |
| S2.3 step 4 | M2: `t_body` reads `last` instead of `ret` on `Flow::Return` | [INFERENCE] Equivalent on main: every `Flow::Return` sets `last` to the operand's taint, and any tainted control that can raise it goes through `t_branch(has_exit)`, which sets `sticky`, which `t_frame_exit` ORs in. Kept as veto's defence in depth. Re-check at S3 (seal_call may introduce a path where they differ). |
| 688a18d2 | S2 complete (S2.3 step 4 frame/closure taint) | PASSED except the same 4 deferred stages, 35 min (host shared with S3 lanes), `TMPDIR=/var/tmp/r51-tmp` | see S2Frame report: 0 failed |
| 7f290382 | S3a (Kernel split) + S3b (closure rep A, conform.rs) | PASSED except the same 4 deferred stages, 40 min (host shared with S3c/S3d lanes) | 1946 passed, 1 failed (temp-name race, fixed in 4587239d) then green on rerun of cli_run dict_* |
| af91688e | S3 complete (S3a-S3d + merge fixes) | PASSED except the same 4 deferred stages, 36 min | 1979 passed, 0 failed, 78 ignored; psv sealed_frames 6/0/31, fabric check_effects 7/0/20 |

## S6 probe (seal flipped in a scratch worktree, not committed) at 11f36710

Every registry entry run with `SEAL_RUNTIME_COMPLETE = true`: 199 entries, 148 ran ok, 46 failed, 11 lib entries not built (3 unit tests `const { assert!(!SEAL_RUNTIME_COMPLETE) }` stop the lib compiling; S6.1 rewrites them). Failures by cause: refusal_coverage_gate 33 (Fork 4 re-anchor, S6.2); sealed_frames 9 + dispatch_rule_switch 1 (S4 PICK / dispatch / name sinks); check_effects `a_pass_needs_evidence_that_the_test_completed` 1 (main's interpreter already fails an Err-ended test, so the refusal reason is the test failure, not missing completion evidence; S6.1 adapts the reason assertion); psv_dispatch observer 1 (its inner run still carries the `#[ignore]`; clears when the entry is removed). Logs /var/tmp/r51-tmp/s6probe-11f3, script /var/tmp/r51-tmp/s6probe.py.

## T3 non-interp lib tests (05c19cd1)
All 8 present and passing on r51/int: kernel 1, parser 1, resolver 5, seal_split 1 (`cargo test -p axon-core --no-default-features --lib <name>`). Integration files: all 10 present (port map §B). Remaining T3: 74 interp::tests + 30 taint_tests absent by name; S3/S0b/S2 rows in lanes t3a/t3b/t3c, S4 rows (14) in the S4 lanes.
| 27836b1c | S4b merged (receiver dispatch, switches, goal-name edge) | fmt, clippy -D warnings clean; `cargo test -p axon-core --no-default-features --no-fail-fast` 1993 passed, 0 failed, 90 ignored; vm_parity 333 files 0 differ |
| 3bcb42c3 | T3a/T3b/T3c + S4a merged (plus fixes 6007bea4) | fmt, clippy -D clean; `cargo test -p axon-core --no-default-features --no-fail-fast` 2018 passed, 0 failed, 165 ignored; psv sealed_frames 6/0/31; fabric check_effects 7/0/20; vm_parity 0 differ |
| d2353962 | hdone merged (addressed handler completion) + no_control_transfer_escapes_a_frame un-ignored | clippy -D clean; full axon-core suite 2020 passed, 0 failed, 164 ignored; vm_parity 0 differ; pci_delta --check PASS |
| 59a72f8d | S4Finish merged (rule-off harness restored; registry S4 = 0, S6 = 271, S0b = 14) | fmt, clippy -D clean; full axon-core suite 2020 passed, 0 failed, 164 ignored; psv sealed_frames 6/0/31; fabric check_effects 7/0/20; vm_parity 0 differ; pci_delta PASS |
| S6 T7 (r51/s6rows) | rows M3484-M3494 (pin, compile refusal, engine consults, general-call split) in seal_* helpers | refusal-coverage gate plain and `--freeze` exit 0; REMAINDER 208 before (11ec9b1c) / 208 after (the helpers' sites are rowed, none joins the REMAINDER); T7 run lists tasks/r51-s6/t7-lists.json (438 core rows: the 426 re-anchored + M3483 + M3484-M3494) |
| S6 T7 perf (r51/s6rows b72be0bd src vs r51/int 371d3c07, same worktree path, release, instructions:u medians of 3) | seal_pin_vm / seal_frames are #[inline(always)] wrappers of the expression they replace | vm_perf_gate.sh programs and --repros PASS on both; head/base 1.0000 on all 15 measured programs (max 1.0002 part_base) |
| 371d3c07 | S6.1 + gate lookup fix (dd1ea380) | `./scripts/gate.sh` (unset CARGO_TARGET_DIR): **gate PASSED**, rc=0, real 52m07s. First run at 11ec9b1c failed at "v0.22 Protected Check Isolation surfaces" (walk_bound test at the wrong module path; include!d test files not followed) |
| c9266cd0 | S6.2 lanes merged (s6legs T6 legs, s6rows T7 rows M3484-M3494, s6tests audit gaps) | fmt, clippy -D warnings clean; `cargo test -p axon-core --no-default-features --no-fail-fast` 2183 passed, 0 failed, 1 ignored (harness_integrity re-run on the committed tree: 43/43; its one failure was the untracked q1 fixture dirtying the tree); axon-psv 84/0; axon-cortex 78/0; vm_parity PASS (333 files + 3 axon-test files) |
| 986ec715 | T7 first full run (3 lists, 438 rows) | 428 KILLED; not killed: M2622, M2623, M2925, M2946 (survived), M3230, M3231 (new text did not compile), M1996, M3256, M3343, M921 (refused elsewhere). Records: tasks/r51-s6/t7-bad.json |
| e946f06d | T7 fixes merged: r51/t7shape (3e057532: sealed let/param/field no longer re-tags another fixed width, a real gap, M921; row text M3230/M3231/M3256; marker M1996) and r51/t7oracle (11172ce5: sealed `&mut` direct-call existence oracle closed, new row M3495; M2622/M2623 test reaches the guard; M2925/M2946 dominated S4b seal_call removed; M3343 re-anchored onto goal_eval's frame filter) | fmt, clippy -D warnings clean; `cargo test -p axon-core --no-default-features --no-fail-fast` 2185 passed, 0 failed, 1 ignored; axon-psv 84/0; axon-cortex 78/0; axon-fabric 885/0; vm_parity PASS |
| 59295b73 | **T7 evidence** (governance/status/r51-t7-list{0,1,2}.json, run at e946f06d, clean tree, `--scope=all --only`) | **439/439 KILLED by their own attack**, 0 REFUSED_ELSEWHERE, 0 survivors, 0 stale (lists 176/117/146, wall 2357/2435/2017 s in parallel). Every non-retired row's `old` text occurs exactly once (2294 rows: 2275 active + 19 library primitives). Refusal-coverage gate plain and `--freeze` exit 0 |
| 3561022b (T2, r51/depth) / f3a92b26 (S7 stack, code as 4dd118d1) | **T2 receipt** (S7-Evidence-2): `./scripts/vm_wasm_depth.sh` | 3561022b: PASS (recorded in its commit message only; log /var/tmp/r51-tmp/t2-depth.log, no commit id). f3a92b26 (merged with r51/int 9e67b796, `wasm_stack_budget.py` rooted at both `eval_arm` instances, S7-Evidence-6): **PASS rc 0**, log tasks/r51-s7/stack/vm_wasm_depth.log (HEAD line first); reach as spec §10 "Stack" (debug tree closure 158, release tree with 190; every nest probe VM ≥ tree) |
| f3a92b26 | S7 stack (S7-UnsealedParity-10, S7-Evidence-2, S7-Evidence-6; f89f4d43 + 4bb4a0e1 native thread reserve) | fmt, clippy -D warnings clean; `cargo test -p axon-core --no-default-features --no-fail-fast` 2190 passed, 0 failed, 1 ignored; axon-psv 84/0, axon-cortex 78/0; vm_parity 0 differ; refusal coverage rc 0; pci_delta PASS; anchors 0 bad; `vm_perf_gate.sh` PASS (fib-recursive 2,291,907,385, collatz 18,366,339,298, mandelbrot 5,067,023,953, arr-sum 4,879,412,583, qsort 11,353,798,948 instructions:u medians) and `--repros S1,S2` PASS (loop 92.2, call 232.1, part 217.1), log tasks/r51-s7/stack/vm_perf_gate.log; depth sweep tasks/r51-s7/stack/sweep.txt: 0 cells where 5864c423 completed or hit the limit and head differs (release and debug, tree and VM) |

## Q1: sealed suite-verdict wall time (S6.3, spec §10/§12 Q1)

Fixture `tasks/r51-s6/q1` (c9266cd0): a 4-test acceptance suite (fib, sieve, sort, word count) over a candidate module; `run.sh <axon> 5` reports medians of 5 runs. Release binary built from r51/int, host AMD Ryzen 9 9950X3D (32 threads), kernel 7.0.0-34.

| Mode | Median wall (s) |
|---|---|
| `axon test accept.ax --json --seal ../cand` (sealed, tree engine) | 1.386 |
| unsealed, default engine (VM) | 0.179 |
| unsealed, `AXON_ENGINE=tree` | 0.466 |

Per test (ms, sealed / unsealed VM / unsealed tree): fib 176/22/59, sieve 770/53/259, sort 352/67/103, words 69/29/36. Sealed costs about 3x the unsealed tree and about 7.7x the unsealed VM. No target (spec §10). VM speed for sealed runs is not R51 scope; the follow-up is logged in tasks/opportunities.md.
