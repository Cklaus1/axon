# Asyncify linear-memory runaway, 2026-09-24

> **Ported to the v0.22 programme line** from the superseded v0.20 donor line
> (`upgrade/cortex-v0_20`, `/home/cklaus/projects/axon`). False-green IDs in this
> record are the **v0.22** IDs. The donor used FG-042/FG-043 for these, but on
> v0.22 those IDs already name different findings, so they were NOT reused:
> donor FG-041 → **FG-044**, donor FG-042 → **FG-045**, donor FG-043 → **FG-046**.
> Donor commit hashes below are kept as written; the v0.22 port commits are listed
> in `UPGRADE_V0_22_DONOR_PORT.md`.

On the afternoon of 2026-09-24 a gate run grew a node process's RSS at roughly
0.5 GiB/s until the WSL VM died and took the whole development environment with
it. The workload was `scripts/wasm_asyncify_host_await.sh`, the browser
`host_await` harness (R15 §13 B3), which runs the axon-wasm interpreter under
`wasm-opt --asyncify` in node.

Two sessions worked this after the reboot (`cklaus-70` and `axon-2e`). This
record keeps the **first diagnosis as well as the correct one**, because the
first one was plausible, specific, and wrong, and it came close to shipping as
the root cause.

## Root cause — corrected

| | first diagnosis (retracted) | measured cause |
|---|---|---|
| claim | `axon_eval` reclaims the caller's buffer (`Vec::from_raw_parts`) and the Asyncify rewind loop re-enters it with the same pointer → double-free → corrupted allocator → unbounded growth | `wasm-opt --asyncify` was run with **no optimization level**. The unoptimized (-O0) instrumented module runs away on ANY program, including one with no `host_await` at all |
| basis | reading the source | execution under a hard cgroup cap |
| status | **retracted**: the double-free is real, but it does not produce the runaway | **established as the trigger; mechanism unconfirmed** |

The first diagnosis made **two** errors, both retracted: it named the double-free
as the cause with more confidence than a code reading supports, and it then named
the binaryen version. The correction came from a capped reproduction by `axon-2e`
and was independently re-verified by `cklaus-70`.

Evidence (every run under `scripts/lib_bounded_run.sh`, 1 GiB `memory.max` +
wall-clock deadline):

* **-O0 vs -O2, same binaryen (120):** -O0 → 5,414,077-byte module, exhausts 1 GiB
  on `println("hello")`; `axon_eval_borrowed` never returns. -O2 → 2,400,878
  bytes, returns in ~0.1 s. Declared initial memory (1028 pages), export count and
  import count are identical between the two.
* **No suspension needed:** the -O0 module exhausts the cap when called directly
  from a 10-line node script that never touches the Asyncify state machine. The
  same source compiled WITHOUT Asyncify prints `hello` at 34 MB.
* **Not the binaryen version:** binaryen 120 and 127 produce byte-identical -O0
  output (`cmp` clean, md5 `7ad2db52…`). The first session's conclusion, "install
  binaryen 108 as R15 pins it", was inferred from that pin and never tested
  against a second version. It is retracted.
* **Not the double-free:** the PRE-fix tree (`a25c581`, original one-shot
  `axon_eval` re-entered with the same `srcPtr` on every rewind), built and
  asyncified at -O2, runs the three-iteration `loop.ax` correctly and bounded:
  `r=a|r=b|r=c`, 3 host requests, ~260–690 MB, exit 0. Reproduced independently
  by both sessions.

Why the -O0 module runs away is **not established**: plausibly frame/local bloat
from uninstrumented-then-instrumented locals exhausting the shadow stack or
linear memory, but that is a hypothesis, not a measurement. What *is* established
is enough to act on: -O0 triggers it and -O2 does not, on both binaryen versions
available.

## What was changed

Root-cause fix: **`8953b8b`** (-O2 on both asyncify sites). Separately:
`60aef9f` (the borrowed-source ABI; latent UB, not the cause), `dc738cf`
(`bounded_run` + an 8-check regression, each layer mutation-proven), and
`9e8e4da`/`0b664bd` (FG-045). Verified from committed state: the asyncify gate
passes all four fixtures, and `cli_run` passes 671/671 including the
previously blocked asyncify test.

| change | owner | why |
|---|---|---|
| `-O2` on the `wasm-opt --asyncify` line in `scripts/wasm_asyncify_host_await.sh` **and** `examples/browser/build-interactive.sh` | cklaus-70 | the trigger. Both consumers had it. |
| rebuilt `examples/browser/axon_interp.async.wasm` (untracked; was a stale -O0 build whose size, 2.42 MB, looked like an -O2 one) | cklaus-70 | the page `interactive.html` loads would have run away the same way. **The artifact predates 2026-09-24**: it exhausts 1 GiB on a program with no `host_await` through the one-shot `axon_eval`. So the -O0 defect was LATENT in the checkout before today's run; the harness run surfaced it rather than introduced it |
| `axon_eval_borrowed` + `axon_free`; the rewind loop migrated to them; `axon_eval` kept one-shot for existing callers | cklaus-70 | the **latent UB**: re-entering a function that freed its argument is wrong whether or not it happens to blow up. Not the incident's cause, still a real defect |
| `axon_asyncify.mjs` refuses a pre-fix `.wasm` with an explicit "rebuild it" error | cklaus-70 | the one-shot ABI cannot be driven across a suspend by any shim: a fresh `axon_alloc` per re-entry TRAPS (`unreachable`) because allocating during rewind perturbs the state the unwound frames resume against |
| `scripts/lib_bounded_run.sh` + `scripts/wasm_asyncify_bounded_regression.sh` | cklaus-70 | containment: cgroup `memory.max`/`swap.max` + deadline, classified OK / FAIL / TIMEOUT / RESOURCE_EXHAUSTED off `memory.events` (a wrapper cannot launder that). 8 checks, each layer mutation-proven |
| `scripts/lib/child_exit.sh`; 9 parity harnesses + `timer_irq_qemu_test.sh` | axon-2e | **FG-045**: harnesses compared stdout and never read the exit status, so a child killed after printing (137) passed. Mutation: 8/8 passed before, fail after (`9e8e4da`) |

## Containment facts worth keeping

* **V8 limits do not bound Wasm.** `node --max-old-space-size=512` reached
  24.8 GiB RSS; Wasm linear memory and `Buffer` live outside V8's old space.
* **cgroup `memory.max` does**, for the process and every descendant it forks,
  at exactly the configured ceiling.
* **`systemd-run --user` is unavailable on this WSL host** (no
  `XDG_RUNTIME_DIR`, no session bus, uid 0). Writing `+memory` into our own
  cgroup's `subtree_control` fails with EIO (internal-process constraint). The
  working mechanism is a cgroup created at `/sys/fs/cgroup` root, with the child's
  PID written to `cgroup.procs`. Where no cgroup can be made, `bounded_run`
  degrades to deadline-only and SAYS so; it does not claim protection it lacks.
* **Unoptimized build under a cap is safe to reproduce; uncapped it is not.**
  Every reproduction above ran capped. No known-bad workload was run uncontained
  after the reboot.

## What the incident teaches

1. **A code-reading root cause is a hypothesis.** The double-free story explained
   every symptom and was wrong. Only running the pre-fix code at -O2 under a cap
   separated "a real bug" from "the bug that happened".
2. **A pin in a spec is not evidence the pin matters.** "Binaryen 108" read as
   the fix until a second version produced the same bytes.
3. **Harness "skips" hide failures.** `wasm_asyncify_host_await.sh` exited 0 with
   "skipping" when the wasm build or `wasm-opt` failed: the harness_skip.sh class
   (a skip must prove its own reason). Closed as FG-046.

## Follow-ups closed after the gate (same day)

| item | resolution | evidence |
|---|---|---|
| `wasm_asyncify_host_await.sh` turned a wasm BUILD failure or a `wasm-opt` failure into "skipping", exit 0 | absent tools still SKIP (via `harness_skip`); a tool that is PRESENT and FAILS is now FAIL, and so is an artifact missing its asyncify exports. Recorded as **FG-046** | mutation: `wasm-opt` exiting 1, exiting 0 with no output, and emitting an uninstrumented module each gave exit 0 before and FAIL after; the real toolchain still passes |
| the browser page loaded whatever `.wasm` the last local build left (the stale one was -O0 and exhausts host memory) | **artifacts stay generated and gitignored** (the existing policy: `examples/browser/.gitignore` ignores `*.wasm`; nothing browser-side has ever been tracked). `build-interactive.sh` now writes `axon_interp.async.wasm.stamp.json` (-O level + sha256), and `interactive.html` REFUSES a module with a missing, mismatched, or non-O2 stamp before running it | `scripts/browser_artifact_guard.mjs`, wired into the asyncify gate as check (5): fresh stamped -O2 accepted AND runs; no stamp, stale -O0 bytes under a current stamp, and an honest -O0 stamp are all refused. Mutant (`verifyArtifact` accepts all) → gate FAIL |
| R15 said "Binaryen-108 feature flags required" | reworded: the `--enable-*` flags are a validation requirement on every version tested, **not a version pin**; `-O2` is the requirement, and "change binaryen version" is named as the wrong fix | `governance/specs/R15-resume-runtime.md` |

Certification **on the donor line** (not this line): `scripts/release_check.sh` → **RELEASE-VERIFIED `dc738cf`** (strict gate
`strict-dc738cf-20260924T220431Z-285858`: 100/100 suites, 2614 passed, 0 failed, clean
`AXON_*` env, run inside a cgroup capped at 20 GiB; tag `release-verified/dc738cf`). The
follow-ups above land after that commit; the donor's final state `5891b8d` is also
RELEASE-VERIFIED (tag `release-verified/5891b8d`). **On the v0.22 line these fixes are
certified only by the Stage-2 re-certification** recorded in `.axon-v022/stage2/`, not by
the donor tags.

## Open

* `.wslconfig` limits (memory/swap) are proposed as defense in depth. Applying
  them needs `wsl --shutdown` from Windows, which restarts Docker containers, so
  it is an operator action. It is not required: the repo is now safe on a
  default-configured host.

* The -O0 runaway **mechanism** is unconfirmed.
