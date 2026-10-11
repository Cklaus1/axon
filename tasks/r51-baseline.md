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
