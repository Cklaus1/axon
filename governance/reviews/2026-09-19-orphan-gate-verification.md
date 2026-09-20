# Orphan-gate verification — what the nine unwired scripts actually do

Measurement only. No file under `crates/` or `scripts/` was modified. Every
classification below is from a real run on this host, in the `lane/verify`
worktree pinned at `7f8d984`.

Context: the coverage-sweep ledger
(`governance/reviews/2026-09-19-coverage-sweep-ledger.md`) found 16 scripts
invoked by nothing executable, several of them cited in
`governance/REQUIREMENTS.md` as the EVIDENCE for a "landed" claim. Four were
verified and wired already (`r33`, `r34`, `r39_slice1`, `r23`). These are the
remaining nine.

## Results

| script | classification | exit | wall-clock | continuous / conditional | evidence line |
|---|---|---|---|---|---|
| `r39_slice3_gate.sh` | **PASS** | 0 | 35s | **continuous** | `=== R39 Slice 3 gate: 11 passed, 0 failed ===` |
| `r39_slice4_gate.sh` | **PASS** | 0 | 4s | **continuous** | `=== R39 Slice 4 gate: 8 passed, 0 failed ===` |
| `r39_slice5_gate.sh` | **PASS** | 0 | 2s | **continuous** | `=== R39 Slice 5 gate: 10 passed, 0 failed ===` |
| `r30_acceptance_gate.sh` | **FAIL** | 1 | **4900s (82 min)** | conditional (far too slow) | `R30 acceptance gate: 3 passed, 3 failed` |
| `axon_kernel_gate.sh` | **PASS, but two defects inside it** | 0 | 5s | conditional (its real layer is skipped) | `[axon-kernel-gate] PASS (3 checks, 0 failures)` |
| `kernel_enforce_test.sh` | **SKIP-HONEST** | 0 | 0s | conditional | `kernel_enforce_test: kernel not built — skipping` |
| `gfx_wgpu_render_gate.sh` | **PASS** | 0 | 17s (warm cache) | conditional (Vulkan ICD) | `✅ gfx-wgpu render gate PASSED — real wgpu offscreen frame cleared to 0x0080FFFF` |
| `zephyr_qemu_gate.sh` | **FAIL** | 1 | 0s | conditional (Zephyr SDK) | `error: couldn't allocate input reg for constraint '{dx}'` / `FAIL: Axon object was not emitted (exit 1)` |
| `perf_bench.sh` | **SKIP with a misleading reason → VACUOUS as a gate** | 0 | 0s | conditional (benchmark) | `perf_bench: \`axon build\` unavailable (no codegen) — skipping` |

Self-reference check (`grep -c "<own basename>" <script>`) — every one of the
nine names itself in its header, which is exactly why the original orphan probe
mis-scored them as "referenced by something":

| script | self-mentions |
|---|---|
| `r39_slice3_gate.sh` | 1 |
| `r39_slice4_gate.sh` | 1 |
| `r39_slice5_gate.sh` | 1 |
| `r30_acceptance_gate.sh` | 2 |
| `axon_kernel_gate.sh` | 1 |
| `kernel_enforce_test.sh` | 1 |
| `gfx_wgpu_render_gate.sh` | 1 |
| `zephyr_qemu_gate.sh` | 1 |
| `perf_bench.sh` | 2 |

Nine for nine. Self-reference is uninformative about whether anything invokes a
script.

Host toolchain, probed before the runs so that any "absent" skip reason could be
checked rather than believed: `firecracker` present, `/dev/kvm` present,
`qemu-system-arm` present, `qemu-system-x86_64` present, `west` / `cmake` /
`ninja` present, Zephyr base at `~/zephyrproject/zephyr` present, Zephyr SDK at
`~/zephyr-sdk` present, Mesa lavapipe ICD (`lvp_icd.json`) present.

## The failures, and whether they are real

### `r30_acceptance_gate.sh` — FAIL, and the cause is real but not a code regression

```
FAIL: acc_a1_gate_passes_on_clean_repo (gate exited non-zero)
FAIL: acc_a5_skip_unavailable_stages (skip logic incorrect — check gate source)
FAIL: acc_a6_exit_code_matches_failure (expected exit 0, got 1)
R30 acceptance gate: 3 passed, 3 failed
```

All three trace to ONE upstream failure. `acc_a1` runs
`scripts/axon_safety_gate.sh`, which stops at

```
✗ GATE FAILED at Stage 6 (R29_COMPLIANCE) — do not deploy
```

`acc_a5` and `acc_a6` then fail as consequences (a failed stage is neither
`passed` nor `skipped`; a failed gate does not exit 0). So this is one defect
reported three times, not three.

Chasing Stage 6: `scripts/r29_acceptance_gate.sh` exits **101** on
`cargo test -p axon-os --test acceptance` — `9 passed; 2 failed`:

* `acc_a2_example_jobs_run_and_overreach_denied` — `summarize wrote its output`
* `acc_a6b_verdict_seed_and_run_id_are_sealed_into_the_chain_p6_exit_03` —
  `fixture shape changed — "verdict":{"kind":"Completed","value":0} not found`

Both have the same cause, measured directly:

```
$ ./target/debug/axon-os run examples/jobs/summarize.axjob --run-id dbg --out /tmp/a2dbg
✓ completed (value=1)
```

`value=1` is `summarize.ax`'s `Err` arm for `write_file("./out/summary.txt", …)`.
The write fails because `examples/jobs/out/` **does not exist** — `git ls-files
examples/jobs/` lists only the `.ax`/`.axjob` files and `data/report.txt`; there
is no `out/` and no `.gitignore` entry for it. It DOES exist in the main
checkout at `/…/axon` as untracked local state left behind by an earlier run.

So: **two axon-os acceptance tests pass only in a working copy where a previous
run happened to create an untracked directory, and fail in any fresh clone or
worktree.** That is a real defect — the job (or the test) must create its own
output directory — and it is environment-shaped in the sense that it is
invisible to whoever has run it before. It is NOT contention flake: it
reproduced deterministically and the mechanism is fully explained.

Separately, `r30_acceptance_gate.sh` probes `r27_acceptance_gate.sh`, finds it
failing, and sets `SKIP_R27=1` — i.e. the gate adapts around a broken sub-gate
and can still print PASS lines. `r27`'s own failure was not chased here.

Two structural notes on this gate, independent of today's failure:

* **`acc_a2` is a grep over the gate's source text**, not a behavioural test.
  It asserts `OVERALL_OK=false`, `exit 1` and `FAIL (exit` appear in
  `axon_safety_gate.sh`. It passed today while the thing it claims to test —
  the gate failing on a broken stage — was in fact happening for real in
  `acc_a1`. It would keep passing if the strings were present and the wiring
  were not.
* **82 minutes.** It invokes the full `axon_safety_gate.sh` five times, and each
  invocation runs `cargo test -p axon-core`. A first (cold) attempt was killed
  by the harness at ~50 min; the 4900s figure is a complete warm-cache run.

### `zephyr_qemu_gate.sh` — FAIL, and it is a real compiler defect

Every SKIP guard was passed (west, cmake, ninja, qemu-system-arm, `ZEPHYR_BASE`,
`arm-zephyr-eabi` toolchain all found and named in the output), so this is not
an environment skip dressed as a failure:

```
1/3 compiling Axon app → arm-zephyr-eabi object (/tmp/…/axon_app.o)
error: couldn't allocate input reg for constraint '{dx}'
FAIL: Axon object was not emitted (exit 1)
```

`{dx}` is an **x86** register constraint, emitted while targeting **thumbv7m**.
Source: `crates/axon-core/src/codegen/expr.rs:795`, in
`synthesize_freestanding_trap`, which builds x86 `outb $1, $0` inline asm with
`"{dx},{al},~{memory}"` for any `--freestanding` build regardless of target
triple (the doc comment says so explicitly: "writes `marker` to the QEMU
debugcon port (0xE9)"). Three more x86 port-I/O constraint strings live at
lines 10056/10089.

`governance/REQUIREMENTS.md` line 70 cites this script as the evidence for
`R25 | Zephyr RTOS target | ✅ Slice 1 landed`. It does not pass on a host with
the full Zephyr toolchain installed.

Reported, not fixed, per this lane's remit.

## `axon_kernel_gate.sh` — exits 0, and two things inside it are wrong

It PASSES (exit 0, 5s, `PASS (3 checks, 0 failures)`), so it is not a failure.
But the header claims it "verifies the three-layer containment model end-to-end
by running the flagship good/evil agent pair through axon-vm", and what actually
ran does not match that.

**1. The evil-agent containment check can never report success.** The script
runs under `set -euo pipefail` and does:

```bash
if "$AXON" check "$EVIL" 2>&1 | grep -q "E1001\|E1004\|E1310"; then
```

`axon check` exits **2** when it emits diagnostics. `pipefail` propagates that,
so the pipeline is non-zero *exactly when the errors are present*, and the `ok`
branch is unreachable. Measured:

```
$ bash -c 'set -euo pipefail; if ./target/debug/axon check …evil.ax 2>&1 | grep -q "E1001"; then echo caught; else echo not-caught; fi'
BRANCH=not-caught
$ bash -c 'set -eu;            if … same …; then echo caught; else echo not-caught; fi'
BRANCH=caught
```

The gate's own Layer-4 demo output, three lines further down, prints
`agent_task_evil.ax: REFUSED — 3 E1001 errors`. So the gate reports
`[info] evil_agent.ax not caught by axon check` about a file its own demo
proves IS caught. The failure mode is benign today (it degrades to an `[info]`
and defers to the runtime gate) but the security assertion is dead code, and the
good-agent branch above it is correct only by luck — a clean file exits 0, so
its pipeline succeeds for the wrong reason.

**2. Layer 3 — the microVM layer, the only part that needs Firecracker and the
only part the header's "end-to-end" claim rests on — did not run:**

```
[axon-kernel-gate] Layer 3: microVM enforcement
  [skip] guest image missing — run ./scripts/build-guest-image.sh first
```

`dist/guest/` does not exist in this worktree. The skip is honest at the point
of skipping, but the final verdict is not: `PASS (3 checks, 0 failures)` says
nothing about a skipped layer. That is the absent-vs-passed collapse this
session's sweep exists to find, inside a gate. Firecracker and `/dev/kvm` ARE
present on this host — the only blocker is that nothing builds the guest image.

Layer 4 also contains a vacuous branch: if `demo.sh` produces no `PASS:` lines
at all, the gate calls `ok "flagship demo ran (all active layers passed or
skipped)"`. Today the demo really did print 6 passes, so that branch was not
taken.

## `kernel_enforce_test.sh` — SKIP-HONEST, with a caveat

```
kernel_enforce_test: kernel not built — skipping
```

The stated reason is TRUE: `target/x86_64-axon-metal/release/axon-guest-kernel`
does not exist. The script then prints the exact `cargo build … -Z build-std …`
command needed. Its other two guards (firecracker, `/dev/kvm`) would both have
PASSED on this host — so the freestanding kernel binary is the only thing
standing between this test and a live run.

The caveat: nothing in the repo builds that target as part of any gate, so the
skip is self-perpetuating. This is the "end-to-end K3+K5 enforcement claim"
(its own words) and it has no path to ever executing automatically.

## `perf_bench.sh` — a SKIP whose reason is true of the artifact and false of the repo

As found, with `target/debug/axon` left behind by a sibling script's
`--no-default-features` build:

```
perf_bench: `axon build` unavailable (no codegen) — skipping
```

That is literally true of the binary on disk and misleading about the project:
codegen is ON by default (`CLAUDE.md`, `BUILD_RESOLVED.md`). The script only
builds when the binary is ABSENT (`if [ ! -x "$AXON" ]`), so a stale
codegen-less `axon` makes it skip forever. Rebuilding the default binary took
**4s** and the bench then ran:

```
perf_bench: native is 7.3x faster than the interpreter on average (9 programs, min-of-5)
```

Two defects in it as a GATE:

* **No threshold.** It prints a number and exits 0 whatever the number is. It
  exits 1 only if ZERO programs benchmarked. `REQUIREMENTS.md` line 30 cites it
  as `perf-tier benchmark ✅ (scripts/perf_bench.sh, 6.0x faster than interp)` —
  a regression to 0.5x would still exit 0. A per-program `build failed` also
  just `continue`s.
* **The corpus double-counts.** `CORPUS` lists `algorithms.ax` twice, so "9
  programs" is 8 distinct ones — visible in the table above, which prints
  `algorithms.ax` at both 7.6x and 7.8x.

Measured today: 7.3x, above the 6.0x the register quotes.

## Recommendations

**Wire into the strict gate now — cheap, deterministic, no special hardware:**

| script | measured | why |
|---|---|---|
| `r39_slice5_gate.sh` | 2s | pure markdown/DAG analysis; `r39_slice5_dag_check.sh` is already in CI, its gate is not |
| `r39_slice4_gate.sh` | 4s | store→render purity, synthetic fixtures |
| `r39_slice3_gate.sh` | 35s | re-runs the R32/R33/R34 evidence commands; the most expensive of the three and still cheap |

Total 41s for 29 assertions. All three are cited in `REQUIREMENTS.md` R39 as
"ALL CHECKS PASSED" and are invoked by nothing.

**Classify as conditional / external evidence, do not wire:**

| script | why |
|---|---|
| `gfx_wgpu_render_gate.sh` | PASSES for real here (llvmpipe, `read_pixel = 8454143`) but needs a Vulkan ICD; 17s measured on a WARM target dir that already held the wgpu dependency graph — a cold wgpu build is minutes and was not measured |
| `kernel_enforce_test.sh` | needs the freestanding kernel built with `-Z build-std`; honest skip |
| `perf_bench.sh` | a benchmark, not a gate — it has no pass/fail criterion at all |
| `r30_acceptance_gate.sh` | 82 minutes. Even green it does not belong in a per-invocation gate |

**Defective — fix before anyone cites them:**

1. `zephyr_qemu_gate.sh` FAILS on a fully-equipped host. The defect is in
   `codegen/expr.rs` `synthesize_freestanding_trap`, which emits x86 port-I/O
   inline asm for every `--freestanding` target including ARM. R25's "✅ landed"
   rests on this.
2. `axon_kernel_gate.sh`'s evil-agent check is unreachable under `pipefail`
   (`axon check` exits 2 on diagnostics), and its PASS verdict does not mention
   that Layer 3 never ran.
3. `perf_bench.sh` has no threshold and a duplicated corpus entry.
4. The axon-os acceptance suite depends on an untracked `examples/jobs/out/`
   directory. This is what makes `r29_acceptance_gate.sh` — and therefore
   `axon_safety_gate.sh`, and therefore `r30_acceptance_gate.sh` — red in a
   fresh worktree while green in the main checkout.
5. `r27_acceptance_gate.sh` fails too (observed as `r30`'s pre-flight probe,
   not chased here). `r30` routes around it with `SKIP_R27=1`.

## Could not determine

* Whether `gfx_wgpu_render_gate.sh` is cheap from cold. The wgpu rlib was
  already present; forcing a cold rebuild was out of scope for a measurement
  lane.
* Whether `r30_acceptance_gate.sh` would pass with the `examples/jobs/out/`
  cause fixed. That needs a re-run, i.e. another 82 minutes, and a source change
  this lane may not make.
* The root cause of `r27_acceptance_gate.sh`'s failure.
* Whether `axon_kernel_gate.sh`'s Layer 3 passes. It has never run here, and
  building the guest image was out of scope.
* Host contention was continuously heavy during the `r30` run (a concurrent
  agent was running `axon_safety_gate.sh` in another checkout, plus an unrelated
  30-minute `cargo test` from another project). The `r30` FAILURE is not
  attributable to it — the cause is fully explained and reproduces in isolation
  — but the 4900s figure is an upper bound rather than a clean measurement.
