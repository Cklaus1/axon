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

### O004 — [low] `wasm_browser_examples_run_identically_via_js_host` is build-state sensitive

See O002 — kept as a candidate task: make the floor check deterministic by
asserting *which* examples linked, not *how many*.
