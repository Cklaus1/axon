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

`scripts/` holds ~50 `*_parity.sh` harnesses and most follow this shape:
`if output contains "skipping" { return }` plus a `contains(...)` assertion on a
prose line. Every one is a candidate for the same failure. Worth a mechanical
sweep:

1. for each harness, assert its success marker is a string the script can
   actually produce (grep the script for the literal), and
2. count skips — a harness that skips in CI or on a dev box is not a gate, and
   should say so loudly rather than returning 0.

Directly relevant to O002/O004: the browser-parity floor guard is the one
harness observed doing this correctly, refusing to go green when coverage
shrank. It is the model to converge on.
