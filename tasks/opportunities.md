# Opportunities — deferred work sink

Anything discovered mid-build with no assigned task and no failing test lands here.
Logged, **not acted on**. Newly-discovered bugs/gaps become candidate tasks with a
proposed severity — never silently fixed.

Never write a secret/credential VALUE here — `file:line` reference only.

---

## From Step 0 (environment baseline)

### O001 — [medium] Baseline failure: `wasm_interp_matches_native_on_pure_compute`

`crates/axon-core/tests/cli_run.rs` · pre-existing at baseline commit `20eb218`.

34/35 pure-compute examples match; `anthropic_stream.ax` differs — native reports
`http_sse_post requires the asi-runtime feature or a network-capable host`, wasm
reports `ANTHROPIC_API_KEY environment variable not set`. Both are exit 1, so this
is a *message* divergence on an unconfigured-network path, not a compute
divergence. Candidate fix: make the missing-feature check precede the missing-key
check on both targets so the first failure reported is the same one.

### O002 — [low] **NOT a baseline failure** — build-state sensitive: `wasm_browser_examples_run_identically_via_js_host`

`crates/axon-core/tests/cli_run.rs:13734`.

An earlier *partial* run in this session showed this failing: 26 examples
linked+matched against a floor of 28; 0 differ, 8 object-only. On the clean full
workspace run at `20eb218` it **passes**. The earlier failure came from a stale
`target/` state, not from a defect — so it is not part of the baseline (see L003).

Still worth logging, for two reasons:

1. **It is build-state sensitive.** The test's pass/fail depends on how many
   examples happen to link, which depends on prior build artifacts. That is a
   latent flake and a candidate task: make the floor check deterministic
   (build the corpus from a clean target, or assert *which* examples linked, not
   how many).

2. **It is the model to copy.** This is the **opposite** of the vacuous-gate
   class that dominates the Pass 6/7 findings — the floor guard refuses to report
   green when coverage silently shrank. That is exactly the behaviour the broken
   aggregators should be repaired *towards*.

Only O001 is a real baseline failure: per the loop's rule it does **not** block,
but no task may make it worse, and "green" for the rest of this run means
"no failing test other than `wasm_interp_matches_native_on_pure_compute`."

---

## From Step 1 (adversarial triage)

### O003 — [high] `principal_root` is ungated — attenuation is bypassable without any forgery

`crates/axon-core/src/kernel.rs` · surfaced while **refuting** F154.

F154 claimed attenuation was defeated by forging a parent handle (handles are
dense `Vec` indices, so `child - 1` reaches the parent). The mechanism is real,
but the impact is nil — because `principal_root` is ungated, an attacker can
mint a root principal outright and never needs to forge anything.

Not covered by any of the 185 findings. It is a **larger** issue than the finding
that surfaced it: the entire principal-attenuation story assumes root authority
is hard to obtain, and it is not.

Not acted on. Candidate task, blocked on a design decision that is genuinely
**needs-human**: what *should* gate `principal_root`? (first-caller-wins? a
build-time capability? an explicit host grant?) Each has different implications
for the embedding story, and picking one is a product decision, not a bug fix.

### O004 — [medium] `wasm_browser_examples_run_identically_via_js_host` is FLAKY under concurrent test execution

Upgraded from [low] with direct evidence. Observed across four full runs of
`cargo test -p axon-core --test cli_run` at three different commits:

| run | result |
|---|---|
| after T1 | passes (420 passed / 1 failed) |
| after T8 | passes (421 passed / 1 failed) |
| after T2 | **FAILS** (420 passed / 2 failed) |
| after T2, immediate re-run, no code change | passes (421 passed / 1 failed) |
| after T2, run in isolation | passes |

Same binary, same commit, opposite results — so it is non-deterministic under
full-suite parallelism, not merely sensitive to a stale `target/`. The likely
mechanism is several tests racing on shared wasm build artifacts, so the "how
many examples linked" count is read while another test is mid-build.

This cost real time: it presented as a T2 regression and had to be
disambiguated by a 6-minute re-run plus an isolated run. A flaky gate is worse
than a missing one — it trains the reader to dismiss failures, which is the
same end state as a vacuous gate, reached from the other direction.

Candidate fix: assert *which* examples linked rather than how many, and give
the test its own build directory so it cannot race.

### O005 — [high] CI's `cargo fmt` job is RED on `main`, at baseline

`.github/workflows/ci.yml:55` runs `cargo fmt -p axon-core -- --check`. That
command fails at baseline with **41 diffs** (42 on nightly), across
`decimal.rs`, `codegen/bpf.rs`, `codegen/builtin_externs.rs`, `codegen/builtins.rs`,
`checker.rs`, `interp.rs`, `main.rs`, `lib.rs`, `error.rs`, `capabilities.rs`,
`builtins.rs`.

`decimal.rs` and `codegen/bpf.rs` are **byte-identical to `origin/main`**, so this
is not branch-local — CI is broken on main. Both trace to the parallel R21
(Decimal) / R23 (eBPF) track.

Not fixed here, deliberately: a 41-site reformat of another track's live files
would collide with concurrent work, and it would bury this run's security diff in
unrelated churn. It needs coordination with that track, not a unilateral sweep.

Consequence for this loop: the recorded baseline (`cargo test --workspace`) does
**not** include fmt. "Green" for this run remains test-only. T7 must account for
this — turning on more CI does not help while an existing CI job is already red.

### O006 — [high] Audit every `*_parity.sh` gate for the same vacuous-skip pattern

Found while fixing T11. `codegen_str_reverse_replace_match_interp_on_utf8`
asserted a string the script **cannot emit** (`"str_reverse and str_replace
match the interpreter"`; the script's real success line uses slashes). The test
could therefore only ever pass via its early `skipping` return — and it did,
because the script's own `axon build` was failing from the runtime
`CARGO_MANIFEST_DIR` bug. So a UTF-8 parity gate had been measuring **nothing**,
while reporting green.

Two compounding defects, each of which hid the other: the build bug made the
script skip, and the stale assertion meant that even when it ran, the check
would not have matched. Fixing the build converted a vacuous pass into a real
failure, which is the only reason the stale assertion surfaced.

**[SWEPT — hypothesis was WRONG, and the residual risk is elsewhere.]**

I predicted this was a class across the ~50 harnesses. It is not. A mechanical
sweep of all **47** harness-backed tests — checking that every
`stdout.contains("...")` success literal actually appears in the script it
drives — found **zero** further instances. T11's stale assertion was a one-off.
(Two initial hits were false positives from a regex splitting an `||` chain;
both tests correctly check `contains("SKIP")` first.)

That check is now permanent: `harness_success_assertions_are_strings_their_
scripts_can_emit` in cli_run.rs re-derives the sweep on every run and fails with
the offending test/script/literal. It carries its own vacuity guard
(`checked >= 40`), and was verified to catch the original T11 defect when
reintroduced.

**The real residual risk is different and larger: 44 of the 47 harness-backed
tests have a silent skip early-return.** Each exits green when its script prints
SKIP — missing NDK, no emulator, no LLVM, no libz3, no Docker. On any given
machine or CI runner an unknown subset of the parity suite measures nothing,
and reports the same green as a full pass. That is not a stale-assertion bug; it
is the absence of a skip census. Worth doing:

1. have each harness-backed test report SKIPPED distinctly from PASSED, and
2. assert a floor on how many actually ran in CI, the way the browser-parity
   test already asserts a floor on examples linked (O002/O004) — the one
   harness observed doing this correctly.

**[DONE — and the first census is worse than expected.]** All 44 skip sites now
call `note_harness_skip`, which appends to `target/harness-skips.log` and, under
`AXON_HARNESS_STRICT=1`, PANICS — so CI can demand that the harnesses it thinks
it is running actually ran. Both modes verified.

The first census under `cargo test -p axon-core --no-default-features` — the
exact command CI ran before T7 — shows these silently measuring nothing:

```
codegen unavailable — all-examples parity
codegen unavailable — fuzz parity
codegen/wasm unavailable — AOT-wasm example sweep
codegen/wasm unavailable — browser-target parity
node/codegen/wasm unavailable — browser I/O parity
node/codegen/wasm unavailable — browser example sweep
Android NDK unavailable — lifecycle adapter
Android NDK/emulator unavailable — compute parity
```

**[CORRECTED.]** My first reading of this census overstated it. I wrote that
"the entire native/interp parity suite skipped in CI". That is wrong on two
counts, and the accurate version is narrower:

- **8 of the 44 skip-guarded tests actually skipped here, not 44.** The other 36
  ran for real. Harnesses that build their own codegen binary — `dict_parity`,
  `checked_arith_parity`, `str_utf8_parity` — invoke `cargo build -p axon-core`
  with DEFAULT features, so they get codegen regardless of the flags the outer
  `cargo test` used. They are unaffected.
- **This census was measured on THIS host, which has LLVM 17 installed.** It is
  not the CI census. On a runner without LLVM, every harness that shells out to
  build codegen would also skip — so the CI number is *worse* than these 8, but
  I have not measured it and should not quote 8 as if I had.

What the 8 do establish: the wasm/browser sweeps and the two script-driven
codegen parity harnesses (`all-examples parity`, `fuzz parity`) silently
measured nothing in this configuration and reported green. That is a real hole,
and it is the reason T7's codegen job matters — but "no gate at all" was my
overstatement, not the data.

Recommended next: set `AXON_HARNESS_STRICT=1` on the T7 codegen job (which HAS
LLVM), so a regression that disables codegen can never again present as green.

Directly relevant to O002/O004: the browser-parity floor guard is the one
harness observed doing this correctly, refusing to go green when coverage
shrank. It is the model to converge on.

### O007 — [critical] mmds fail-closed is correct but BLOCKED: the fail-open default is load-bearing

Attempted as T12 (finding OSK-P7-C3) and **reverted**, with new evidence that
makes the finding stronger than reported.

`mmds.rs` grants `EffectSet(0xFF)` — all eight effects — on every miss path (no
boot_params, no cmdline ptr, absent `axon.policy=`, non-array value, decode
failure, `!POLICY_READY`), and `enforce.rs:29` mirrors that default explicitly
"so the kernel boots even if K2 hasn't filled in a real policy yet". Inverting
all six to `EffectSet(0)` is a small, obviously-correct change and it compiles.

**But it breaks Layer 3 of the product's own gate**, and the reason matters:
`scripts/axon_kernel_gate.sh:113` invokes `axon-vm run "$GOOD" --kernel ...
--initrd ... --json` with **no policy argument at all**. Nothing ever sets
`axon.policy=`. So the microVM enforcement layer has only ever worked because
the kernel defaults to granting everything — the fail-open default is not a
safety-net, it is the mechanism by which the demo functions.

Correct sequencing, which is why this is not a one-line fix:

1. `axon-vm` must derive a policy from the job's grant and pass it on the kernel
   cmdline as `axon.policy=<base64>`; and refuse to launch when it has none.
2. **Only then** flip the six defaults to `EffectSet(0)` and the `enforce.rs`
   static to `0`.

Doing (2) without (1) means no agent can run in a microVM at all. Doing (1)
alone is already an improvement and is independently testable.

### O008 — [medium] Rebuilding the guest kernel invalidates the local attestation baseline

`~/.axon/kernel_baseline.sha256` pins the kernel digest, and `axon-vm run`
correctly refuses with "ATTESTATION FAILED: kernel digest mismatch" when the
kernel is rebuilt. That is attestation working as designed — but there is no
documented operator step for re-recording the baseline after a legitimate
rebuild, and `axon-vm attest` does not update it.

Consequence observed here: after `scripts/build-guest-image.sh` succeeds,
`axon_kernel_gate.sh` Layer 3 fails until an operator re-pins. Deliberately NOT
re-pinned by this run — silently re-recording an attestation baseline is exactly
the action that should require a human, and automating it would defeat the
control.

Needs: a documented `axon-vm attest --record-baseline` (or equivalent) plus a
note in the kernel-gate docs that a rebuild requires an explicit re-pin.

### O009 — [medium] Derive the wasm-parity exclusion set from `builtin_effect_row`, not a hand-written regex

`scripts/wasm_parity.sh:50` defines `HOST_BUILTINS` as a hand-maintained
alternation, directly beneath a comment promising "no hand-maintained list to
drift". It drifted: `env_var` and the whole `http_*` family were missing, so
examples that read the environment or open a socket were auto-discovered as
"pure compute" (T14 / P5-03).

The exclusion set should come from `builtins::builtin_effect_row` — any builtin
with a non-empty effect row is host-touching by definition — so a newly-added
capability builtin cannot silently widen the "pure" corpus. Same shape as the
R1d drift test that already keeps `BUILTIN_EXTERNS` honest, and the same shape
recommended for axon-os `scan_effects` in T4.

Note the pattern this is the third instance of: a comment asserting an invariant
("no hand-maintained list", "deny-by-default", "attestation verified") sitting
directly above code that does not implement it. The comment is not evidence.

### O010 — [medium] `persistent_bandit_demo_accumulates_across_runs` depends on ambient leftover state

`crates/axon-core/tests/cli_run.rs:10775`. Fails with
`run 1 should be fresh: ... prior pulls (loaded): 40` when a persisted bandit
state file survives from an earlier suite run; passes in isolation and passes on
a clean tree.

Not caused by any change in this audit — it is a pre-existing state dependency,
surfaced because this run executed the suite many times in a row. Same family as
O004 (build-state-sensitive browser parity): a test whose greenness depends on
ambient state rather than on the code under test.

Fix: have the test create and remove its own state directory (or point the demo
at a temp path via env), so run 1 is fresh by construction rather than by luck.

Practical note for anyone reading a red suite here: `persistent_bandit_demo_*`
and `wasm_browser_examples_run_identically_via_js_host` are the two known
non-deterministic tests. Confirm with an isolated re-run before treating either
as a regression — this session lost time to exactly that twice.
