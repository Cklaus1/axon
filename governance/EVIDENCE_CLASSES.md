# Evidence classes — how each gate cited by REQUIREMENTS.md actually runs

`governance/REQUIREMENTS.md` cites `scripts/*.sh` gates as the EVIDENCE for a
requirement's status. This file records, for each cited script, **how it is
executed** — and `crates/axon-core/tests/requirements_evidence.rs` verifies every
row against the repository's real invocation graph.

The class is DECLARED here and CHECKED there. It is never inferred from the
filename, and it cannot be obtained for free:

* a row may not declare a class **stronger** than the invocation facts support
  (`strict_gate` fails unless `scripts/gate.sh` really calls the script, in
  command position, outside a comment);
* a row may not keep a **weaker** excuse once the script became continuously
  invoked (stale labels fail too — drift is gated in both directions);
* every class carries its own witness obligation, listed below;
* every row needs a written reason. An empty note is a parse error.

| class | means | witness column must be | counts as active verification |
|---|---|---|---|
| `strict_gate` | invoked by name from `scripts/gate.sh` | `scripts/gate.sh` | yes |
| `ci_job` | invoked by a CI workflow step | the `.github/workflows/*.yml` path | yes |
| `cargo_test` | spawned by a Rust integration test | the `.rs` path | yes |
| `glob_invoked` | matched by a glob in a script that is itself reachable, or called by name (unguarded) from such a script | `<invoker>::<glob pattern>`, or the calling script's path | yes |
| `conditional` | invoked, but the site is guarded by an env flag the invoker does not set | `FLAG=<ENV_VAR>` | acknowledged, not continuous |
| `manual_operator` | its only caller is reachable from no root | the invoker path | **no** |
| `recorded_benchmark` | evidence is a recorded measurement | repo-relative path of the file holding it | acknowledged, not continuous |
| `external_hardware` | needs an absent toolchain; must SKIP honestly | `NEEDS=<tool>` | acknowledged, not continuous |
| `orphaned` | nothing executable invokes it | `-` | **no** |

"Reachable from a root" means reachable from `scripts/gate.sh`, a CI workflow, or
a Rust test — the things that run without a human choosing them. Reachability is
TRANSITIVE: a gate invoked only by a gate nobody invokes is still invoked by
nobody. That was the R26-R29 gates' state until they were wired directly into
`gate.sh` on 2026-09-19.

A call through a RUNNER counts as invocation: a shell function that runs its
arguments (`"$@"` in command position, after shifting a label off), such as
`gate.sh`'s `run_quiet "R22 acceptance gate" ./scripts/r22_acceptance_gate.sh`,
and the `env`/`timeout`/`nice` wrappers. A function that only prints its
arguments is not a runner, and a glob inside a quoted argument is text.

## Reading a failure

A violation is never reported as a bare verdict. Each one prints the shortest
path from a root (or `NONE`), and then WHY — the nearest mention with its
`file:line` and the reason it does not count (`COMMENT`, `SELF-REFERENCE`,
`ECHO TEXT`, `EXISTENCE TEST`, `NOT A COMMAND`, `STRING MESSAGE`, `PROSE`), or
the dead caller and why *it* is dead. A reviewer who greps and finds a mention
is told, in the same report, that the mention is a comment.

The violations are then grouped by the single upstream wiring that would clear
them, largest first. **Wire the root and recompute; do not patch leaves** — five
gates hanging off one unwired umbrella are one fix, and four of them stop being
violations the moment the umbrella is reachable.

Regenerate the observed ceiling for a new row with:

```bash
cargo test -j 6 -p axon-core --no-default-features --test requirements_evidence \
    evidence_class_observed_dump -- --nocapture
```

> **This file records the state MEASURED at the lane/checker merge (2026-10-27,
> on main 22a0cfbd).** Every row below matches the facts. What remains red is
> `requirements_evidence_is_not_orphaned`: some REQUIREMENTS.md rows claim
> verification and cite a gate nothing runs (`perf_bench.sh`, `r30`,
> `vm_perf_gate.sh`, `vm_wasm_depth.sh`). That test is `#[ignore]`d until those
> claims are wired or restated (`tasks/opportunities.md` O-CK-01).

| script | class | witness | note |
|---|---|---|---|
| all_examples_parity.sh | cargo_test | `crates/axon-core/tests/cli_run.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| decimal_parity.sh | glob_invoked | `scripts/parity_all.sh::scripts/*_parity.sh` | named nowhere; the aggregator iterates the glob, which is why a filename probe calls it an orphan. |
| ebpf_verify.sh | strict_gate | `scripts/gate.sh` | invoked by name in `scripts/gate.sh`; runs on every gate. |
| exec_parity.sh | cargo_test | `crates/axon-core/tests/cli_run.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| gdt_layout_ir_test.sh | cargo_test | `crates/axon-core/tests/integration_fixtures.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| perf_bench.sh | orphaned | `-` | nothing invokes it, AND it writes no artifact — it prints timings to stdout and exits. R1 quotes "6.0x faster over 9 pure-compute programs" from it, and that number exists nowhere but inside the R1 row itself, so `recorded_benchmark` is refused twice over: there is no measurement file to point at, and pointing at the register would make the claim its own evidence. Closing this needs the script to WRITE a dated result, or the row to stop citing it. |
| provenance_parity.sh | cargo_test | `crates/axon-core/tests/cli_run.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| r22_acceptance_gate.sh | strict_gate | `scripts/gate.sh` | invoked by name in `scripts/gate.sh`; runs on every gate. |
| r23_acceptance_gate.sh | strict_gate | `scripts/gate.sh` | invoked by name in `scripts/gate.sh`; runs on every gate. |
| r26_acceptance_gate.sh | strict_gate | `scripts/gate.sh` | wired directly 2026-09-19. Previously reachable only via `axon_safety_gate.sh`, which nothing invoked — the whole subtree hung off nothing. Wired directly rather than via its parent: the parent re-runs BUILD, the unit suite and a demo the gate has already done (~25 min); these four standalone cost 23s total. |
| r27_acceptance_gate.sh | strict_gate | `scripts/gate.sh` | wired directly 2026-09-19; see r26. Measured 7s standalone. |
| r28_acceptance_gate.sh | strict_gate | `scripts/gate.sh` | wired directly 2026-09-19; see r26. Measured 11s standalone. |
| r29_acceptance_gate.sh | strict_gate | `scripts/gate.sh` | wired directly 2026-09-19; see r26. Measured 4s standalone. |
| r30_acceptance_gate.sh | orphaned | `-` | nothing invokes it, and it is the root of the dead R26-R29 subtree. |
| r31_acceptance_gate.sh | strict_gate | `scripts/gate.sh` | run on every gate via `run_quiet "R31 acceptance gate"` (`gate.sh:1258`). Measured orphaned at 7f8d984; wired since. |
| r32_acceptance_gate.sh | strict_gate | `scripts/gate.sh` | run on every gate via `run_quiet` (`gate.sh:1304`). Measured `manual_operator` (only `setup-environments.sh`) at 7f8d984; wired since. |
| r33_acceptance_gate.sh | strict_gate | `scripts/gate.sh` | invoked by name in `scripts/gate.sh`; runs on every gate. |
| r34_acceptance_gate.sh | strict_gate | `scripts/gate.sh` | invoked by name in `scripts/gate.sh`; runs on every gate. |
| r39_slice1_gate.sh | strict_gate | `scripts/gate.sh` | invoked by name in `scripts/gate.sh`; runs on every gate. |
| r39_slice2_gate.sh | strict_gate | `scripts/gate.sh` | run on every gate via `run_quiet` (`gate.sh:1305`), ahead of slices 3-5 because it carries `r39_slice2_validate.sh`. Measured orphaned at 7f8d984. |
| r39_slice2_validate.sh | glob_invoked | `scripts/r39_slice2_gate.sh` | called by name (`bash "$ROOT/scripts/r39_slice2_validate.sh"`) from `r39_slice2_gate.sh`, which `gate.sh` runs. |
| r39_slice3_gate.sh | strict_gate | `scripts/gate.sh` | run on every gate via `run_quiet` (`gate.sh:1306`). Measured orphaned at 7f8d984. |
| r39_slice4_gate.sh | strict_gate | `scripts/gate.sh` | run on every gate via `run_quiet` (`gate.sh:1307`). Measured orphaned at 7f8d984. |
| r39_slice5_dag_check.sh | ci_job | `.github/workflows/ci.yml` | run by a CI workflow step on every push. |
| r39_slice5_gate.sh | strict_gate | `scripts/gate.sh` | run on every gate via `run_quiet` (`gate.sh:1308`); its callee `r39_slice5_dag_check.sh` is also a CI step. Measured orphaned at 7f8d984. |
| smt_mint_parity.sh | glob_invoked | `scripts/parity_all.sh::scripts/*_parity.sh` | named nowhere; the aggregator iterates the glob, which is why a filename probe calls it an orphan. |
| tee_sim_run.sh | strict_gate | `scripts/gate.sh` | invoked by name in `scripts/gate.sh`; runs on every gate. |
| timer_irq_qemu_test.sh | cargo_test | `crates/axon-core/tests/integration_fixtures.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| unsigned_parity.sh | glob_invoked | `scripts/parity_all.sh::scripts/*_parity.sh` | named nowhere; the aggregator iterates the glob, which is why a filename probe calls it an orphan. |
| verify_all_specs.sh | ci_job | `.github/workflows/ci.yml` | run by a CI workflow step on every push. |
| vm_parity.sh | strict_gate | `scripts/gate.sh` | run by name in the fast stage (`gate.sh:390`); `parity_all.sh`'s `*_parity.sh` glob also matches it. |
| vm_perf_gate.sh | orphaned | `-` | nothing invokes it (R50 §10 interpreter performance budget; idle-machine only, refuses a busy host). Its only other mention is a comment at `crates/axon-core/tests/cli_run.rs:39353`. Cited by R50 in REQUIREMENTS.md. |
| vm_wasm_depth.sh | orphaned | `-` | nothing invokes it (R50 §4 wasm32 recursion-depth gate; needs `wasmtime` and an x86-64 host, and fails rather than skips without them). `wasm_checker_depth_parity.sh` and `wasm_nesting_parity.sh` mention it only in comments. Cited by R7 in REQUIREMENTS.md. |
| wasm_aot_run_parity.sh | cargo_test | `crates/axon-core/tests/cli_run.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| wasm_browser_examples_parity.sh | cargo_test | `crates/axon-core/tests/cli_run.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| wasm_examples_parity.sh | cargo_test | `crates/axon-core/tests/cli_run.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| wasm_fs_parity.sh | cargo_test | `crates/axon-core/tests/cli_run.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| wasm_object_prune.sh | cargo_test | `crates/axon-core/tests/cli_run.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| wasm_parity.sh | cargo_test | `crates/axon-core/tests/cli_run.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| zephyr_qemu_gate.sh | strict_gate | `scripts/gate.sh` | run on every gate via `run_quiet "R25 Zephyr/Cortex-M gate" env AXON_BIN=... ./scripts/zephyr_qemu_gate.sh` (`gate.sh:1303`). Without the toolchain it SKIPS through five honest guards (`west`, `cmake`, `ninja`, `qemu-system-arm`, the Zephyr base and the arm-zephyr-eabi toolchain), each naming what is missing and exiting 0. So on a host without them the row is wired but not effective. Measured orphaned at 7f8d984. |
