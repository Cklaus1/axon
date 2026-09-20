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
| `glob_invoked` | matched by a glob in a script that is itself reachable | `<invoker>::<glob pattern>` | yes |
| `conditional` | invoked, but the site is guarded by an env flag the invoker does not set | `FLAG=<ENV_VAR>` | acknowledged, not continuous |
| `manual_operator` | its only caller is reachable from no root | the invoker path | **no** |
| `recorded_benchmark` | evidence is a recorded measurement | repo-relative path of the file holding it | acknowledged, not continuous |
| `external_hardware` | needs an absent toolchain; must SKIP honestly | `NEEDS=<tool>` | acknowledged, not continuous |
| `orphaned` | nothing executable invokes it | `-` | **no** |

"Reachable from a root" means reachable from `scripts/gate.sh`, a CI workflow, or
a Rust test — the things that run without a human choosing them. Reachability is
TRANSITIVE: a gate invoked only by a gate nobody invokes is still invoked by
nobody. That is why the R26-R29 gates below are not `strict_gate` despite having
a caller.

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

> **This file records the state MEASURED at 7f8d984. It is not aspirational and
> several rows below are failing rows.** The checker is red today, which is the
> correct report: 8 cited gates are invoked by nothing and 6 more only by a human.

| script | class | witness | note |
|---|---|---|---|
| all_examples_parity.sh | cargo_test | `crates/axon-core/tests/cli_run.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| decimal_parity.sh | glob_invoked | `scripts/parity_all.sh::scripts/*_parity.sh` | named nowhere; the aggregator iterates the glob, which is why a filename probe calls it an orphan. |
| ebpf_verify.sh | strict_gate | `scripts/gate.sh` | invoked by name in `scripts/gate.sh`; runs on every gate. |
| exec_parity.sh | cargo_test | `crates/axon-core/tests/cli_run.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| gdt_layout_ir_test.sh | cargo_test | `crates/axon-core/tests/integration_fixtures.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| perf_bench.sh | orphaned | `-` | nothing invokes it. R1 quotes "6.0x faster" from it; to keep that claim, reclassify as `recorded_benchmark` with a witness file holding the measurement. |
| provenance_parity.sh | cargo_test | `crates/axon-core/tests/cli_run.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| r22_acceptance_gate.sh | strict_gate | `scripts/gate.sh` | invoked by name in `scripts/gate.sh`; runs on every gate. |
| r23_acceptance_gate.sh | strict_gate | `scripts/gate.sh` | invoked by name in `scripts/gate.sh`; runs on every gate. |
| r26_acceptance_gate.sh | manual_operator | `scripts/axon_safety_gate.sh` | called only by `axon_safety_gate.sh` (R30 unified gate), which nothing invokes. Transitively dead. |
| r27_acceptance_gate.sh | manual_operator | `scripts/axon_safety_gate.sh` | called only by `axon_safety_gate.sh` / `r30_acceptance_gate.sh`, both of which nothing invokes. |
| r28_acceptance_gate.sh | manual_operator | `scripts/axon_safety_gate.sh` | called only by `axon_safety_gate.sh` / `r30_acceptance_gate.sh`, both of which nothing invokes. |
| r29_acceptance_gate.sh | manual_operator | `scripts/axon_safety_gate.sh` | called only by `axon_safety_gate.sh`, which nothing invokes. |
| r30_acceptance_gate.sh | orphaned | `-` | nothing invokes it, and it is the root of the dead R26-R29 subtree. |
| r31_acceptance_gate.sh | orphaned | `-` | nothing invokes it; its only mention repo-wide is a COMMENT in `r33_acceptance_gate.sh`. |
| r32_acceptance_gate.sh | manual_operator | `scripts/setup-environments.sh` | run only by `setup-environments.sh`, an operator-invoked provisioning script; nothing runs it per change. |
| r33_acceptance_gate.sh | strict_gate | `scripts/gate.sh` | invoked by name in `scripts/gate.sh`; runs on every gate. |
| r34_acceptance_gate.sh | strict_gate | `scripts/gate.sh` | invoked by name in `scripts/gate.sh`; runs on every gate. |
| r39_slice1_gate.sh | strict_gate | `scripts/gate.sh` | invoked by name in `scripts/gate.sh`; runs on every gate. |
| r39_slice2_gate.sh | orphaned | `-` | nothing invokes it; its only mentions are its own header comment and a comment in `r39_slice2_validate.sh` (self-reference + doc-only, the two false-positive classes). |
| r39_slice2_validate.sh | manual_operator | `scripts/r39_slice2_gate.sh` | called only by `r39_slice2_gate.sh`, which is itself an orphan. |
| r39_slice3_gate.sh | orphaned | `-` | nothing invokes it. |
| r39_slice4_gate.sh | orphaned | `-` | nothing invokes it. |
| r39_slice5_dag_check.sh | ci_job | `.github/workflows/ci.yml` | run by a CI workflow step on every push. |
| r39_slice5_gate.sh | orphaned | `-` | nothing invokes it (its own callee `r39_slice5_dag_check.sh` IS run by CI; the gate around it is not). |
| smt_mint_parity.sh | glob_invoked | `scripts/parity_all.sh::scripts/*_parity.sh` | named nowhere; the aggregator iterates the glob, which is why a filename probe calls it an orphan. |
| tee_sim_run.sh | strict_gate | `scripts/gate.sh` | invoked by name in `scripts/gate.sh`; runs on every gate. |
| timer_irq_qemu_test.sh | cargo_test | `crates/axon-core/tests/integration_fixtures.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| unsigned_parity.sh | glob_invoked | `scripts/parity_all.sh::scripts/*_parity.sh` | named nowhere; the aggregator iterates the glob, which is why a filename probe calls it an orphan. |
| verify_all_specs.sh | ci_job | `.github/workflows/ci.yml` | run by a CI workflow step on every push. |
| wasm_aot_run_parity.sh | cargo_test | `crates/axon-core/tests/cli_run.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| wasm_browser_examples_parity.sh | cargo_test | `crates/axon-core/tests/cli_run.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| wasm_examples_parity.sh | cargo_test | `crates/axon-core/tests/cli_run.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| wasm_fs_parity.sh | cargo_test | `crates/axon-core/tests/cli_run.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| wasm_object_prune.sh | cargo_test | `crates/axon-core/tests/cli_run.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| wasm_parity.sh | cargo_test | `crates/axon-core/tests/cli_run.rs` | spawned by a Rust integration test, so `cargo test` runs it. |
| zephyr_qemu_gate.sh | orphaned | `-` | nothing invokes it. Needs a Zephyr/QEMU toolchain; to keep R25s claim, reclassify as `external_hardware` once the script SKIPs honestly on an absent toolchain. |
