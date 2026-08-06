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

### O011 — [low] `persistent_learner` has the same fixed-/tmp-path race as O010

`crates/axon-core/tests/cli_run.rs:11578` uses a hardcoded
`/tmp/axon_persistent_learner.txt`, exactly the pattern that made
`persistent_bandit_demo_accumulates_across_runs` fail intermittently (O010):
corpus-sweep tests run every example, including this one, in parallel with the
dedicated persistence test.

**[DONE — but read the rationale, which changed.]** The example now honours
`AXON_LEARNER_STATE` and the test uses a per-process file.

The original justification given here — "corpus-sweep tests run every example
including this one in parallel" — was WRONG, for the learner and for the bandit
that prompted it. `all_examples_parity.sh` iterates `examples/*.ax` and does not
recurse into `examples/asi/`; nothing sweeps that directory. See the correction
commit following O010.

This was applied anyway, on the narrower and defensible ground that removing a
fixed shared-mutable `/tmp` path is right on its own terms. It is hardening, not
a fix for any diagnosed failure — none was observed for the learner, and the
bandit's actual cause remains **undiagnosed**. Recorded that way deliberately:
"hardening with no known failure" and "fixes bug X" are different claims, and
only the first is supported.

### O012 — [high] F062: native codegen ignores the PER-CALL `tier:` on `ai_*` calls

**RESOLVED in T46.** The blocker below was real and was addressed head-on: a
reusable exhaustive `walk_expr` now exists in `codegen/mod.rs`, `expr_calls` is
rebuilt on top of it, and the per-call tier scan uses it. The original note is
kept for the reasoning, which held up — and note the walker's `_ => false`
catch-all turned out to be dropping `Select` and `WithHandler` already, so the
"next walker-missed-an-arm bug" it warned about had in fact already happened.


Verified in code, attempted, and **deliberately not fixed** — the correct fix is
larger than it looks and I would not ship a hasty version of a safety refusal.

`codegen/mod.rs:944` refuses (E0910) when a fn's ATTRIBUTES request a
non-`balanced` AI tier, because the native runtime routes every call to the
default model and would otherwise "silently call the wrong model". But R3b's
per-call form — `ai_complete("hi", tier: "cheap")` — is carried on
`Expr::Call { tier }`, and `codegen/expr.rs:375-377` explicitly DROPS it under a
comment that is factually wrong ("native AI calls aren't in the codegen path";
`ai_complete` is fully lowered in `codegen/builtins.rs`).

So the attribute path is closed and the per-call path is open, for the same
hazard. The interpreter gives the per-call tier TOP priority, so native and
interp disagree about which model runs — an I-2 divergence, and precisely the
outcome the attribute-level refusal exists to prevent.

Why it is not a small change: the scan needs to find `Expr::Call { tier: Some(t) }`
anywhere in a fn body, and `expr_calls` is a bespoke ~70-line recursion with no
generic visitor. Doing this right means extracting a reusable expression walker
(which several other checks in this file would also benefit from), not
copy-pasting the recursion a second time — a copy would itself become the next
"walker missed an arm" laundering bug, a class already fixed three times here
(R6 taint, @[contained] helpers, string-dispatch/T2).

Fix: add a generic `walk_expr(&Expr, &mut impl FnMut(&Expr))` to codegen/mod.rs,
re-express `expr_calls` in terms of it, then push the same E0910 for any
`Some(t)` where `t != "balanced"` — arguably for ANY `Some(t)`, since an unknown
tier is E1302 in the interpreter and native cannot replicate that.

### O013 — [medium] `wasm_aot_runs_and_matches_interp_on_pure_int` fails under full-suite parallelism

Observed on the final verification run: 429 passed, 1 failed, with the failure
reporting `SKIP dict_closure (wasm build failed)` for 4 of its 7 cases. It
passes cleanly in isolation (all 7 OK).

Several wasm harnesses invoke `cargo build --target wasm32-*` concurrently and
contend on the same target directory, so some builds fail and the case is
skipped; the test then correctly refuses to pass on partial coverage.

**My part in this, stated plainly:** it did not appear in the T14 verification
run (426 passed / 0 failed). Between then and now this audit added three tests
(T16, T17 ×2), which increased concurrent load and made an existing contention
window more likely to be hit. So the behaviour is pre-existing but this run
raised its probability. That is not a code regression, and it is also not
nothing — a suite that gets flakier as tests are added has a scaling problem.

Note the harness does the RIGHT thing here, the same way the browser-parity
floor guard does (O002/O004): it refuses to report green when cases did not
actually run. Three separate harnesses now demonstrate that pattern; the fix is
to stop the contention, not to soften the assertion.

Fix: give each wasm harness its own `CARGO_TARGET_DIR`, or serialise the wasm
builds behind a shared lock. Related: O004 (browser parity, same cause), O010
(bandit, fixed-path contention).


### O013 follow-up — the browser-parity isolation ATTEMPT MADE IT WORSE (reverted)

Applying the per-harness `CARGO_TARGET_DIR` fix to
`wasm_browser_examples_parity.sh` **regressed** it and was reverted.

| condition | examples linked (floor 28) |
|---|---|
| shared target/, standalone | 30 — passes |
| isolated target/, standalone | 30 — passes |
| shared target/, full suite | 26 / 11 / (sometimes >=28) — variable |
| **isolated target/, full suite** | **5 — much worse** |

Cause: a fresh `CARGO_TARGET_DIR` forces `axon-rt` for wasm32-unknown-unknown to
rebuild from cold inside the harness. Standalone there is time; under full-suite
load that rebuild loses the race and most links fail. So isolation traded shared
contention for cold-cache rebuild cost — and the rebuild is the bigger cost.

The same change was a clear win for `wasm_aot_run_parity` (3/7 -> 7/7 under full
load) because that harness's wasm deps are far smaller. Kept there, reverted here.

Lesson for whoever fixes O004 properly: the answer is NOT per-harness target
dirs. Candidates that do not pay the rebuild cost — a shared *prebuilt*
wasm32-unknown-unknown artifact produced once before the suite runs, a lock
serialising the wasm harnesses against each other, or `--test-threads` pinning
for that group. The counts above are the measurements to beat.


### O004 — ROOT CAUSE FOUND: the wasm build lock exists but is only taken by 9 of 21 scripts

`scripts/wasm_browser_examples_parity.sh:22` already serialises wasm sweeps:

```sh
if command -v flock >/dev/null 2>&1; then exec 9>"${TMPDIR:-/tmp}/axon_wasm_parity.lock" && flock 9; fi
```

Nine scripts take that lock. **Twelve wasm-touching scripts do not**, and race
against the ones that do:

  browser_compute_parity.sh      wasm_asyncify_host_await.sh
  wasm_parity.sh                 wasm_browser_host_await.sh
  wasm_fs_parity.sh              wasm_aot_link_probe.sh
  wasm_browser_interp_parity.sh  wasm_unknown_interp_builds.sh
  wasm_host_await_parity.sh      wasm_object_prune.sh
  parity_all.sh                  setup-environments.sh

So the mitigation is real but partial, which is why O004 has been intermittently
red for the whole session despite a lock being present. This also explains why
per-harness CARGO_TARGET_DIR made things worse rather than better: the design
intent is one SHARED warm cache with serialised writers, and isolation fought
that intent instead of completing it.

Fix: add the same two-line guard to each unlocked script that builds for wasm32
(`parity_all.sh` and `setup-environments.sh` are orchestrators — check whether
they should take it or just not build directly). Then re-measure against the
recorded bar: browser parity must link >= 28 of 34 under a full-suite run, and
`wasm_aot_run_parity` must report 7/7.

NOT attempted here: 12 scripts is a mechanical change but it must be verified
under full-suite load, not standalone — standalone measurement is what made the
CARGO_TARGET_DIR attempt look like a fix when it was a regression.

### O014 — [low] `axon-os status --latest` is parsed and discarded (OSK-L03), attempted and reverted

`crates/axon-os/src/cli.rs:543` — the arm is inside **`cmd_status`**, not
`cmd_kill`. (The finding cites the line number but not the command; `cmd_kill`
at :486 has no `--latest` arm at all.)

```rust
"--latest" => {
    // Find the most recently modified .kill file in store.
    i += 1;          // ← advances the index and does nothing else
}
```

The most-recent-`.kill` search it advertises is the fallback at :565, which runs
only when no run-id was given. So `axon-os status <run-id> --latest` silently
ignores the flag and reports on `<run-id>`.

**[RESOLVED — FALSE POSITIVE. Do not "fix" this.]** Ran it. The flag works:

```
$ axon-os status --store D older --json
{"run_id":"older", ..., "kill_file":".../older.kill"}

$ axon-os status --store D older --latest --json
{"run_id":"older", ..., "kill_file":".../newest.kill"}     ← differs
```

Two `.kill` files of different mtimes; adding `--latest` changes which file is
reported, from `older.kill` to `newest.kill`. So the flag is NOT parsed and
discarded, whatever the `i += 1` arm looks like in isolation — some other part
of the path honours it.

I had already written the "fix" and reverted it for a *different* reason (could
not verify in the time left). That instinct was right, but for the wrong reason:
the change was not merely unverified, it was **unnecessary**, and applying it
would have altered working behaviour to match a misreading of the code.

Lesson, and the reason this entry is kept rather than deleted: OSK-L03 was
marked `confirmed` by triage on a code read — "VERIFIED at cli.rs:543-546" — and
the code really does look like a no-op at that line. Nobody ran it. That is the
same failure mode this audit documents in the codebase, occurring in the audit's
own evidence: a claim that reads as verified because someone looked carefully at
the wrong thing. Treat `confirmed`-by-code-read differently from
`confirmed`-by-repro; the triage JSON distinguishes them and it matters.

### O015 — [medium] INTERP-H04's claimed budget bypass did NOT reproduce (fix reverted)

`kernel_goal_run` computes `let evals = max_evals.max(0).min(avail);` and every
downstream site reads a non-positive eval count as "no cap" (goal.rs:137, :399,
:698 all test `max_evals <= 0`). On a code read that clearly implies: exhausted
budget → `evals == 0` → uncapped optimizer run.

**It does not happen.** With a principal whose budget is fully spent:

```
axon run <exhausted-budget goal>   →  exit 7
axon: goal budget exhausted: goal `metric` (principal 0) ran 0 of 100
      requested evaluations before its budget was exhausted
```

A downstream guard inside the optimizer catches the exhausted budget and raises
`GoalBudgetExhausted` before any uncapped work happens. The dangerous-looking
clamp is real; the consequence is already covered one layer down.

I wrote an early short-circuit, verified it produced a *clearer* message from a
*better* place — and reverted it, because "clearer error" is not what the
finding claimed and shipping it under this finding's banner would record a
bypass as fixed when no bypass existed.

Worth doing eventually, as HARDENING with an honest label: the early check makes
the invariant local instead of relying on a guard three call-levels away, and it
would survive someone refactoring the optimizer. But it must not be filed as
closing INTERP-H04.

**Second false positive from a code-read `confirmed`, after OSK-L03.** Both were
rated on how the code reads at one site, and both are covered elsewhere. The
`REPRODUCED`-vs-`verified by direct read` distinction in the triage rationales
is now measurably load-bearing: of the findings I worked, every code-read-only
one cost a revert, and every executed-repro one landed cleanly.

### O016 — [high] OSK-P4-H2 CONFIRMED BY EXECUTION: a job that never ran seals as `Completed`

Re-triaged by running it, not reading it (the method O015 argued for). This one
holds — and it is worse in the record than in the console.

A syntactically invalid program, zero statements executed:

```
$ axon-os run bad.axjob --run-id badtest --out D
✓ completed (value=2)   (run-id: badtest, record: D/badtest.json)
$ axon-os exit = 0

$ jq .verdict D/badtest.json
{"kind": "Completed", "value": 2}
```

`runtime.rs:377-415` infers the verdict from a chain of `err.contains(...)`
substring tests over the child's stderr, with a terminal
`else => Verdict::Completed { value: proc.code.unwrap_or(0) }`. The interpreter
reports a parse error as `error: parse error: ...` with exit 2 — no `axon:`
fault line, none of the matched substrings — so it falls through to Completed.

This is an ATTESTATION-INTEGRITY failure, not a containment bypass: the
hash-chained, tamper-evident record confidently attests that a job succeeded
when it never ran. Everything downstream that trusts the record inherits the
lie, and the same else-branch will swallow any future fault whose wording drifts.

Fix (from triage, unchanged): branch on the EXIT CODE first — 2 -> Malformed,
3/4/5/6/7/8/12 -> their carved verdicts, 0 -> Completed, any other non-zero ->
Denied{axis:"runtime"} — never Completed. Keep the stderr scan only to populate
the human-readable `reason`. Longer term, have the interpreter emit a
machine-readable outcome line so the supervisor stops parsing prose.

Not fixed here: it touches the verdict mapping that axon-os's records and tests
are built on, so it needs a full axon-os suite cycle to land safely.

**Method note:** 3 of the 4 code-read findings re-triaged by execution so far
were wrong (OSK-L03, INTERP-H04) or right-but-different (this one is right AND
worse than described). Executing before implementing is cheap and keeps paying.

### O017 — [high] OSK-P4-H4 CONFIRMED BY EXECUTION: the audit ledger never reaches a sandboxed job

Third code-read finding re-triaged by running it. Confirmed, and the contrast
makes it unambiguous:

```
# through axon-os (env_clear strips it)
$ AXON_AUDIT_LEDGER=D/ledger.jsonl axon-os run ok.axjob ...
  ledger file exists: NO

# the same interpreter, invoked directly
$ AXON_AUDIT_LEDGER=D/direct.jsonl axon run io.ax
  ledger file exists: YES (1 entry)
```

`runtime.rs:339` calls `cmd.env_clear()` and then re-adds only `AXON_SEED` and
`PATH`. `AXON_AUDIT_LEDGER` is never forwarded, so the R28 capability-audit
ledger is silently disabled for **every** job run under the supervisor — the one
execution path where an operator most wants a capability audit trail.

The hermetic env_clear is correct and should stay; the bug is the allowlist
missing an entry. Note the failure mode: not an error, not a warning — the
operator sets the variable, the run succeeds, and no ledger appears.

Compounding: this sits next to O016 (the sealed record can attest `Completed`
for a job that never ran). Together, the supervisor's two audit artifacts — the
verdict record and the capability ledger — are respectively unreliable and
absent, on the same code path.

Fix: forward `AXON_AUDIT_LEDGER` (and audit which other AXON_* vars the
sandboxed job legitimately needs — `AXON_AI_MOCK`/`AXON_AI_REPLAY` are likely in
the same position) in the `env_clear` allowlist at runtime.rs:339. Add an
axon-os acceptance test asserting a ledger file is produced for a job that
exercises a capability.

**Execution re-triage scorecard so far: 5 findings, 5 answers changed.**
2 false (OSK-L03, INTERP-H04), 2 true-but-understated (O016, this), 1 true as
written (OSK-L02). Reading the code has not once predicted the outcome.

### O018 — [high] `AXON_AI_MOCK` is also stripped, and exit 5 seals as `Completed` — one repro, both bugs

Follow-up to O017 (predicted) and O016 (strengthened). A job whose grant permits
net, calling `ai_complete`, run under `AXON_AI_MOCK=1`:

```
$ AXON_AI_MOCK=1 axon-os run ai.axjob ...
✓ completed (value=5)
$ grep verdict aitest2.json
"verdict":{"kind":"Completed","value":5}

# same program, same env, interpreter invoked directly
$ AXON_AI_MOCK=1 axon run ai.ax
AI OK
```

Two confirmations in one run:

1. **O017 generalises as predicted.** `AXON_AI_MOCK` does not survive
   `env_clear` either, so the deterministic AI stub is silently inert under the
   supervisor. The job fell through to the live path and hit the AI-policy
   refusal. `AXON_AI_REPLAY` is near-certainly in the same position, which would
   make the R15/§9.5 replay-reproducibility story non-functional through
   axon-os — the exact place reproducibility is supposed to be guaranteed.

2. **O016 is worse than my first repro showed.** Exit **5** is not an
   unrecognised code — it is `AI_POLICY_EXIT_CODE`, one of the project's
   deliberately CARVED fault codes (3=verify, 4=halted, 5=ai-policy, 6=refine,
   7=goal-budget, 8=sandbox, 12=containment). The verdict mapper still sealed it
   as `Completed{value:5}`. So the record does not merely mis-handle unknown
   faults; it mis-handles a fault the codebase went out of its way to define,
   and stores the fault code in the `value` field of a SUCCESS verdict.

Anyone reading these records sees `Completed` with a small integer value and has
no way to distinguish "returned 5" from "refused by AI policy".

Fix: both are one change each in `runtime.rs` (forward the AXON_* allowlist at
:441; branch on exit code before the stderr scan at :377). They should land
together with an acceptance test per carved code asserting the sealed verdict is
NOT `Completed`.

**Execution re-triage scorecard: 6 findings, 6 answers changed.**

### O019 — [high] OSK-P4-H3 CONFIRMED BY EXECUTION: axon-os deadlocks on a job with large stdout

The triage flagged this one honestly as *"Confirmed by code read (NOT reproduced
— building axon-os was out of budget, so confidence is code-level)"*. Reproduced
now, and it is real:

```
$ axon run big.ax                      -> 0.09s        (20,000 println lines)
$ axon-os run big.axjob                -> DENIED: timed out after 30000 ms
                                          (axis: time)
```

Same program. 0.09s standalone; killed by the 30-second timeout under the
supervisor.

`run_bounded` (`runtime.rs:132-190`) takes the child's stdout/stderr handles at
:138-139 but does not read them until :181-182 — strictly AFTER the
`try_wait`/timeout loop. So the child blocks writing into a full 64 KiB pipe
buffer, the parent blocks waiting for the child to exit, and neither proceeds.
The wall-clock timeout is the only thing that breaks it.

Severity is higher than "a slow job gets killed":

- **Any** job producing more than a pipe buffer of output is unrunnable under
  axon-os, and the failure is attributed to the wrong cause — the record says
  `Denied{axis:"time"}`, blaming the job for being slow when it was fast.
- The timeout MASKS the deadlock, so this presents as a mysterious performance
  problem rather than a hang. That is why it survived: the symptom is plausible.

Fix: drain stdout/stderr concurrently with the wait — spawn a reader thread per
pipe before the loop and join them after, or use a poll-based reader. The
handles are already `take()`n at :138, so the change is local to `run_bounded`.

Add an acceptance test with a job emitting >64 KiB that must complete, not time
out — it fails today in ~30s and passes in ~0.1s once fixed.

**Execution re-triage scorecard: 7 findings, 7 answers changed.** This one was
explicitly labelled unreproduced by the triage and turned out true AND
mis-attributed. Two were false. Two were true-but-understated. Reading has still
never predicted the outcome.

### O020 — [high] OSK-P4-H6 CONFIRMED BY EXECUTION: `--killable --monitor` silently disables the kill switch

Eighth execution re-triage. Confirmed, and it is a safety-control failure with a
success banner on top.

```
$ axon-os run job --run-id km --killable --monitor IO
$ ls km.*
  km.axjob   km.json   km.monitor.kill        ← NO km.kill

$ axon-os kill --store D km
🛑 kill tripped for run `km` (reason: operator shutdown)
$ echo $?
0
```

`cli.rs:273` is `if killable && monitor_effects.is_none()`, so passing BOTH
flags takes the else branch and `$out/$run_id.kill` is never created;
`AXON_KILL_FILE` is then overwritten at :286 to point at `.monitor.kill`.
`cmd_kill` reconstructs `$store/$run_id.kill` by naming convention, writes that
file, prints the tripped banner, and exits 0 — against a path nothing polls.

So an operator who asks for BOTH the kill switch and compliance monitoring gets
neither a working kill switch nor any indication of that. The failure is
maximally quiet: the flag is accepted, the command succeeds, the banner says
"kill tripped". R27 documents sub-second operator kill as a headline guarantee
(it is Layer 3 of the flagship demo's four).

Fix options, needing a product call on intended semantics:
  (a) make the two flags share one kill file, so either path trips the same
      latch — probably right, since both mean "stop this run";
  (b) have `cmd_kill` trip whichever kill file exists for the run, rather than
      reconstructing one path by convention; and
  (c) at minimum, refuse the combination at parse time instead of silently
      dropping one — never accept a flag that will be ignored.

(b) alone fixes the observed symptom and needs no semantic decision; (a) is the
better end state. Not attempted: `cmd_kill`, `cmd_status`, the R29 monitor
thread and `run_bounded`'s poll all key on these paths, so it wants one coherent
change plus tests for each flag combination.

**Execution re-triage scorecard: 8 findings, 8 answers changed.**

---

## O021 — `axon fmt` emitted source it could not re-parse (FIXED)

**Found by executing, not reading.** The four `examples/*.ax` files dirty in the
working tree at session start were not somebody's edit — running `axon fmt` on a
pristine checkout reproduced the diff byte-for-byte. `fmt` was the author.

That prompted a look at the float renderer (`fmt.rs::emit_literal`), which was:

```rust
let s = format!("{f}");
self.write(&s);
if !s.contains('.') && !s.contains('e') { self.write(".0"); }
```

Two defects, both demonstrated end-to-end against the real binary:

1. **Source destruction.** `1e400` overflows to `f64::INFINITY` during lexing.
   `format!("{}", inf)` is `inf`, which contains neither `.` nor `e`, so the
   suffix fired and the formatter wrote `inf.0`. `axon check` on the
   formatter's *own output* then failed:
   `E0001 cannot find name 'inf' in this scope`. A valid file in, an
   unparseable file out, exit 0, written in place.
2. **Unbounded expansion.** `1e100` was written as a 103-character literal and
   `6.02e23` as 26 digits.

Fixed in `fmt.rs::format_float_literal`: non-finite values emit `1e400`
(which re-parses to infinity exactly), NaN emits `(0.0 / 0.0)` (defensive — no
literal the lexer accepts yields NaN), and plain renderings over 20 chars fall
back to exponent form when that is shorter. Ordinary literals are untouched.

**The test gap is the real lesson.** `fmt.rs` already had five round-trip tests.
Every one asserts `out1 == out2` — *idempotence only*, never comparing against
the input. A stable-but-wrong rendering satisfies that, which is exactly how
both defects survived. Added `assert_float_fidelity`, which asserts the two
properties that were missing: the output must parse, and every float must
survive with an identical bit pattern. Verified fails-before: the two
bug-targeting tests fail against the old renderer, the two guard tests pass in
both states.

Same class as the golden-IR shape-vs-content gap and the vacuous-pass coverage
guard: a test that checks a property adjacent to the one that matters.

**Still open (needs a design call, not a fix).** The formatter is AST-based, and
`Literal::Float` holds only an `f64`, so the original lexeme is gone before
`fmt` runs. Three losses therefore remain and cannot be fixed here:
`1.5e2` -> `150.0`, `Circle { radius }` -> `Circle { radius: radius }`, and
blank lines inside function bodies dropped entirely. None changes program
meaning; all degrade the source. Exact preservation needs either a
lexeme-carrying `Literal::Float(f64, String)` (16 match sites across 12 files)
or a token/span-based formatter. Worth noting `emit_program` carries an
`AUDIT T9` comment about `mod` declarations having been silently *deleted* by
this same tool — that is three data-loss bugs in one formatter, which argues
the architecture is the problem rather than any individual arm.

---

## O022 — `codegen_random_i64_degenerate_bounds_match_interp` is intermittent (UNDIAGNOSED)

Failed once in a full `cargo test -p axon-core --no-default-features` run
(432 passed / 1 failed), then passed in isolation and passed again in a second
full run of the same suite (433/0) on the same commit. So it is genuinely
non-deterministic, and it is NOT caused by the T29/T30 changes — those touch the
purity walker, the builtin effect catalog, and `--risk` parsing, none of which
the test exercises.

**Cause unknown.** My first hypothesis was build contention: the test shells out
to `scripts/random_i64_parity.sh`, which does a native LLVM build, and only
14 of 56 harness scripts take the shared `flock` (see O023). I tested that
directly — ran `random_i64_parity.sh` concurrently with four other unlocked
native-building harnesses — and it passed. So the contention hypothesis is
NOT confirmed, and I am not recording it as the explanation.

The diagnostic mistake worth avoiding next time: the first failing run was
captured through `grep -E "^test result|FAILED|^error"`, which threw away the
assertion body. The harness prints the script's stdout+stderr on failure and
that is the only real evidence — capture full output when chasing an
intermittent. (Second instance of a pipe destroying the evidence needed for the
decision, after the `| tail` exit-code masking.)

Sibling of the still-undiagnosed bandit flake. Two known intermittents now.
Next step when one recurs: full output, plus `--test-threads=1` to establish
whether cross-test interference is involved at all.

---

## O023 — harness lock coverage is 14 of 56 (LEAD, not a confirmed defect)

O004 fixed the wasm subset (9 of 21 scripts took the shared `flock`). The wider
picture: of 56 harness scripts, only 14 take it, and **38 of the unlocked ones
invoke `cargo build`/`cargo run`** — `checked_arith_parity`, `exit_code_parity`,
`fuzz_parity`, `random_i64_parity`, `qemu_boot_test`, and 33 more. They run
concurrently as cargo integration tests, each shelling out to its own build.

I could not make this produce a failure on demand (see O022), so this is a lead
rather than a diagnosed cause. Two cautions before anyone "fixes" it by adding
`flock` everywhere:

  * cargo's own target-dir lock already serialises these builds, so the flock
    changes queueing discipline, not whether they serialise;
  * an earlier attempt this session to wrap an ALREADY-locked harness in a
    second flock deadlocked and left a stale holder blocking later runs.

Worth measuring (does lock coverage correlate with the observed intermittents?)
before changing.

---

## O024 — chain truncation was undetectable in BOTH verify paths (PARTIALLY FIXED)

`chain.rs`, the `axon-vm chain` CLI help, and the R34 spec all asserted that
"removing / substituting / reordering any run is detectable by `chain verify`".
Removing is not. Executed against a real 3-entry chain:

| attack                                  | before                  |
|-----------------------------------------|-------------------------|
| modify an entry in place                | `CHAIN BROKEN` exit 15  |
| delete an INTERIOR entry (the tested case) | `CHAIN BROKEN` exit 15 |
| **truncate the tail** (hide the last runs) | `CHAIN OK: 1 entries` exit 0 |
| **erase the chain entirely**            | `CHAIN OK: 0 entries` exit 0 |
| **truncate, then re-export**            | `EXPORT OK: 1 entries` exit 0 |

The mechanism is not a bug in the hash — it is what linkage means. Every prefix
of a valid chain is itself a valid chain, so linkage proves the entries you were
GIVEN are well-formed and says nothing about entries you were not given.
`--genesis` pins the ROOT; truncation moves the TIP. The auditor-facing export
path was blind for a second reason: `head` is written by whoever produced the
export, so truncate-then-re-export yields a head consistent with the shortened
list and the existing head check passes.

Only `verify_detects_missing_entry` (INTERIOR deletion) was ever tested — the
same shape as the golden-IR shape-vs-content gap and the fmt idempotence gap:
the test next to the hole tested the neighbouring property.

**Fixed:** `--expect-head` / `--expect-count` on both `chain verify` and
`chain verify-export`, plus `ChainStore::verify_pinned` / `verify_export_pinned`
and a `ChainVerifyFailure` enum that distinguishes `Broken` / `CountMismatch` /
`HeadMismatch`. All five attacks above are refused (exit 15) when a pin is
supplied. Unpinned behaviour is unchanged for compatibility but now prints
"(unpinned — truncation undetectable, see --expect-head)" instead of a bare
`CHAIN OK`. The false claims in chain.rs, the CLI help, and the R34 spec were
corrected. `scripts/r34_acceptance_gate.sh` still passes (its checks are
substring matches).

**NOT fixed — needs an architecture decision.** Nothing in the system *stores* a
pin, so the guarantee is only as strong as the relying party's own bookkeeping.
Where the tip should live — R33 quorum state, the R28 capability ledger, or an
external attestation service — is a design call. Until that exists, an operator
who never records a head is exactly as exposed as before; the CLI can now tell
them so, which is the most a local fix can do.

A characterization test (`unpinned_verify_cannot_detect_a_truncated_tail`) pins
the limitation so it cannot silently regress or be silently "fixed" unnoticed.

---

## O025 — [CORRECTED, folded into T33] guest never powers off; the run is reported as OK

**Correction (same session).** As first written this entry claimed `axon-vm run`
"has no boot timeout" and "hangs the host command forever." **That is wrong.**
`run_in_firecracker` has a bounded wait — `AXON_VM_TIMEOUT_SECS`, default 45s
(main.rs ~1914) — that kills the child and returns 124 with a clear message. I
inferred "hangs forever" from a run I had wrapped in an external `timeout 40`,
which fired 5s BEFORE the internal deadline and so hid the very mechanism I was
claiming was absent. Measuring with an instrument that preempts the thing being
measured; the same shape as the earlier `| tail` exit-code masking and the `grep`
that destroyed a flake's evidence.

What IS real, re-measured without the external timeout:

```
AXON_VM_TIMEOUT_SECS=12 axon-vm run examples/flagship/agent_task.ax \
  --kernel dist/guest/vmlinuz --initrd dist/guest/initramfs.cpio.gz --no-attest --json
  → {"ok": true, "exit_code": 124, "error": null}   and the PROCESS exits 0
```

The guest reaches its intended end state and calls `clean_halt()`, which writes
ACPI S5 to port 0x604 — **a no-op under Firecracker**, whose guest-initiated
shutdown is the i8042 reset (hence `reboot=k` in the boot args). So the guest
spins in `hlt` until the deadline. This is the second sub-cause already recorded
in **P7-KRN-04**, which this entry duplicates; it is being fixed there as T33,
along with two things P7-KRN-04 names and one it does not:

- `"ok": result.is_ok()` reports whether the launcher drove the Firecracker API,
  not what the guest did — hence `ok:true` beside `exit_code:124`
- the `-VIOLATION8` serial sentinel the guest kernel emits has no parser in
  `axon-vm` at all (`grep VIOLATION crates/axon-vm/src/main.rs` → nothing)
- **not in the finding:** in `--json` mode `cmd_run` prints and falls off the end
  of the function, so `axon-vm run --json` exits **0 unconditionally**, whatever
  the guest did. Only the non-JSON path does `process::exit(r.exit_code)`
  (verified: same run, no `--json`, exits 124). Every caller that checks `$?`
  around a `--json` invocation is reading a constant.

---

## O026 — the in-guest effect policy defaults to OPEN when a program has no manifest

Found while fixing T33 — the reason `axon_kernel_gate.sh`'s evil-agent check had
never once observed the violation it claims to test.

`cmd_run` derives the guest's `allowed_effects` as: `AXON_VM_ALLOWED_EFFECTS`
override → the program's `.axmeta` `effect_union` → the principal's grant →
**`None`**. The guest reads `None` as no restriction and prints:

```
[axon-kernel] enforce: gate active — 8 effect bit(s) allowed (0xff)
```

So `axon-vm run prog.ax` on any program without a manifest and without a named
principal boots it with **every effect granted**, including FS, Net and Exec.
The syscall gate is active and working; its ceiling is just wide open.

That is fail-open on the load-bearing control, and it bites hardest exactly where
it matters: a program the compiler REFUSES cannot have a manifest at all
(`axon check examples/flagship/agent_task_evil.ax` → 3× E1001, so
`build --emit-manifest` produces nothing), so the least trustworthy programs are
the ones that reach the guest with the widest grant.

Not fixed here: closing it means picking a default (deny-all? IO-only?) and
deciding what `run` does for a program with no manifest — refuse, or run with a
minimal grant. Both change the behaviour of every unmanifested run, which is a
call for the operator, not a bug fix to slip into an unrelated commit. It is
closely related to **O007** (axon-vm policy delivery before mmds can fail
closed) and probably wants deciding alongside it.

Worth stating plainly: the flagship demo's Layer-2 claim rests on this path. The
containment story is sound where a manifest exists; where one does not, the VM
currently grants everything.

---

## O028 — the default gate takes 4× longer if it actually verifies I-2 (needs a call)

Measured while fixing T36/GATE-03, so the tradeoff is on record rather than
guessed at.

`scripts/parity_all.sh` — the aggregator that enforces the two-engine invariant
I-2 — takes **7m49s** on this box (2026-08-04: 44 passed, 5 skipped, 0 failed, of
49 harnesses; the 5 skips need an Android NDK or headless Chrome). It runs under
`gate.sh --strict` only. GATE-03's fix sketch proposed promoting it into the
default gate and estimated ~2 min; the real number is nearly four times that.

So the default `./scripts/gate.sh` proves nothing about I-2 — its test stage is
`--no-default-features`, which makes every codegen parity wrapper in `cli_run.rs`
report `ok` while asserting nothing — and it still prints `✅ gate PASSED`.
T36 made that non-silent (the run now lists every skipped harness and says
outright that parity was not verified) but did not change what runs.

The actual decision, which is yours:

- **(a) Promote it.** Every gate run verifies I-2, and every gate run costs ~8
  minutes more. Honest, slow, and likely to get bypassed by developers in a tight
  loop — which is how ad-hoc parity running caused #27/#36/#38/#39 in the first
  place.
- **(b) Leave it in `--strict`, make CI use `--strict` + `AXON_HARNESS_STRICT=1`.**
  Local loops stay fast; the merge gate is the one that must be honest. This is
  the existing needs-human item #4 and probably the right answer, but it means
  accepting that a local green gate is not evidence of I-2.
- **(c) Split it.** A fast subset (the ~10 harnesses that need no exotic
  toolchain) in the default gate, the full suite under `--strict`. More work, and
  someone has to own which harnesses are in the fast set and why.

Related: the wrappers already call `note_harness_skip`, so the counting
machinery (O006b) is built and waiting on the policy decision.

## O027 — the AI endpoint pin only binds programs that declared a net grant

Shipped as part of T35 (RT-02) and worth stating as a limitation rather than
leaving it implied by the code.

`pin_net_allowlist` constrains the resolved AI endpoint host only when the
program declares at least one `@[contained(net: [...])]`. A program with NO
containment annotation is unpinned, so `AXON_AI_BASE_URL` / `ANTHROPIC_BASE_URL`
still redirect it anywhere — verified after the fix: an unannotated program
still reached the 127.0.0.1 sink.

That is deliberate for this slice. An unannotated program made no claim to
violate, and pinning it would break the self-hosted-gateway workflow that
`ANTHROPIC_BASE_URL` exists to serve (the trainloop gateway among them). But it
does mean the guarantee reads: *"a program that says where it will connect
cannot be redirected"* — not *"AI traffic cannot be redirected."*

Closing the gap properly needs a decision, not a patch: should an AI call from
an unannotated program be refused outright (deny-by-default, breaking every
current gateway user), warned about, or left as is? It is the same shape as
**O026** (the microVM policy defaulting to open when a program has no manifest)
and the two probably want deciding together — both are "no declaration" mapping
to "no restriction" in a system whose thesis is that declarations are the
enforcement surface.

---

## O029 — `axon build`'s cache made a codegen fix silently not take effect (fixed as T38, but the class is wider)

Found the hard way: I fixed a real codegen bug (T37/F061), rebuilt the compiler,
re-ran the repro, and got the OLD wrong answer. Twice. The fix was correct and
present in `--emit-llvm` output; the linked binary was a **cached object from the
previous compiler**.

Mechanism: `axon build`'s incremental cache key is
`SHA256(source-path ‖ source-bytes ‖ triple ‖ VERSION)` where `VERSION` is
`0.1.0 (<git-short-sha>)`, with `-dirty` appended when the tree is dirty. But
`build.rs` declared only:

```
cargo:rerun-if-changed=../../.git/HEAD
cargo:rerun-if-changed=../../.git/index
```

Editing a tracked source file makes the tree dirty **without** touching
`.git/index` (that moves only on `git add`), so the build script never re-ran,
`AXON_GIT_SHA` kept reporting the last CLEAN sha, and the cache key was
unchanged across a semantically different compiler. `axon --version` printed
`0.1.0 (0718794)` with two modified files in the tree.

Fixed both layers: `build.rs` now also watches `src`, and the cache key mixes in
the compiler executable's own path+size+mtime — the version string is a *claim*
about the build, the executable *is* the build.

**Why this is worth a follow-up rather than just a fix.** For the window this
existed, any codegen work done without committing first could be verified
against a stale artifact. That includes the parity harnesses: they build from
the working tree, so a harness could compare a stale native binary against a
fresh interpreter and report PASS. I have no way to tell retroactively which
past "verified" codegen results were affected — the cache lives in
`~/.cache/axon/` and is keyed by content, not dated.

Worth considering:
1. A gate check that `axon --version` reports `-dirty` iff `git status
   --porcelain` is non-empty. One command, catches the whole class.
2. Whether the parity harnesses should pass `--no-cache` unconditionally. They
   are measuring the compiler, and a cache is exactly the wrong thing to have in
   that loop.
3. Whether any OTHER build-identity consumer (attestation digests, `.axmeta`,
   the R34 chain) trusts `VERSION` the same way. A stale identity in an
   attestation record is a worse failure than a stale object file.

---

## O030 — `sql_query`'s escaping cannot be correct for every SQL dialect at once

Surfaced by T39 (P5-25) and left as a decision rather than patched over.

The escaping now doubles `\` before doubling `'`, which closes the demonstrated
MySQL/MariaDB injection. But that fix is **dialect-specific and lossy elsewhere**:

| dialect | `\` in a param | doubling it |
|---|---|---|
| MySQL / MariaDB (`NO_BACKSLASH_ESCAPES` off — the default) | escape character | **required** — without it the param escapes its own closing quote |
| PostgreSQL, SQLite (`standard_conforming_strings` on — the default since PG 9.1) | ordinary literal | **corrupts the value** — one backslash becomes two |

So the builtin now trades a MySQL injection for a Postgres/SQLite data-corruption
on any parameter containing a backslash. That is the right trade — silent data
corruption is much less bad than remote SQL execution, and the doc now says so
explicitly — but it is a trade, not a fix, and it should not stay implicit.

The honest framing is that **rendering a query string is the unsafe pattern**.
Real safety is driver-side parameter binding, where the value never becomes part
of the statement text at all. `sql_query` cannot do that because Axon has no
database sink; it builds a string and hands it to the caller.

Options, none free:

1. **Take a dialect argument** — `sql_query(template, params, dialect)`, or a
   `@[sql(dialect: postgres)]` attribute. Correct, and an API change.
2. **Refuse a parameter containing `\`** — dialect-neutral and fail-closed, in
   keeping with the language's posture, but rejects legitimate values (Windows
   paths, regexes, LaTeX).
3. **Emit a placeholder query plus a params array** — `SELECT … WHERE a = $1`
   and `["…"]`, i.e. stop rendering values into the text entirely and let the
   eventual driver bind them. This is the only option that is actually safe, and
   it changes what the builtin returns.

(3) is the correct destination. Until a database sink exists there is nothing
forcing the choice, which is exactly why it is worth deciding deliberately rather
than discovering it when the first driver lands.

---

## O031 — native codegen silently upgrades the AI runtime from *offline* to *live*

*Found while reproducing F141 / P6-EXIT-04 (T41).* Needs a decision; not fixed.

`axon-core`'s default feature set does **not** include `asi-runtime`, so the
interpreter takes the offline branch in `interp/builtins.rs` (~4570): a declared
`@[ai(policy(fallback: …))]` is returned as `Ok(fallback)`, and with no fallback
the call is **E1300, a fatal exit 5**. The comment there is explicit: *"a program
that wants to run offline MUST declare a fallback."*

Native codegen links `axon-ai` **unconditionally** (`codegen/link.rs:333`,
`build_axon_ai`), so the same compiler binary produces an AOT program that dials
the real model. Executed, one source file, one `axon` binary:

```
$ env -u ANTHROPIC_API_KEY -u AXON_AI_MOCK axon run offline.ax
axon: ai policy: [E1300] `ai_complete` cannot run: no model reachable and no
      @[ai(policy(fallback: …))] in scope …
exit=5

$ env -u ANTHROPIC_API_KEY -u AXON_AI_MOCK axon build offline.ax -o off.bin && ./off.bin
err=ANTHROPIC_API_KEY (or AXON_AI_API_KEY) is not set
r=2
exit=0
```

Native reached the *live dispatch path* and failed only for want of a key. With a
key present it would have made a real network call to `api.anthropic.com` — in a
build configuration where `axon run` refuses to make AI calls at all.

Two separate defects fall out of this:

1. **The capability difference is silent.** `--features asi-runtime` is
   documented as what turns on live `ai_complete`. It does so for the
   interpreter only. Nothing tells a user that `axon build` ignores the flag.
2. **`fallback:` is unimplemented natively.** Where the interpreter returns
   `Ok(fallback)` and stamps `mode:"fallback"` in the audit trail, native returns
   a live `Err`. Neither the value nor the provenance record matches.

The fix is not a refusal — refusing every `ai_complete` would be far too broad,
and the divergence is invisible at compile time (it depends on `AXON_AI_MOCK` and
on whether a key is present at run time). Two workable shapes:

1. **Honor the feature gate natively.** When `axon-core` is built without
   `asi-runtime`, have codegen tell the linked runtime so — then `ai_complete`
   natively means: mock if `AXON_AI_MOCK`, else the declared fallback, else halt
   with E1300 and exit 5. The fallback string is a *compile-time constant* from
   the attribute, so codegen can pass it down; this is the one policy field
   native can honor exactly rather than refuse.
2. **Extend the ABI to carry the policy.** `__axon_ai_complete(prompt, fallback,
   budget_ptr, budget_n, …)` and move the whole R3 policy table into `axon-ai`,
   where it is ordinary Rust. This also retires the T41 budget refusal (an
   `alloca` counter in the fn prologue is per-activation by construction, which
   is exactly R3c's "per-fn-activation" rule) and the tier refusal with it.

(2) is the destination — it makes one Rust implementation of the policy table
serve both backends, which is what stopped tier/budget drift in the first place.
(1) is a smaller step that closes the security-relevant half.

**Decision needed:** whether native `ai_complete` should honor `asi-runtime` at
all, or whether "native is always live" is intended. If it is intended, it must
be *documented* and the interpreter's E1300 must stop claiming the program
"cannot run" — because natively it can, and it will.

---

## O032 — an `@[ai(policy(budget: N))]` is escaped by extracting a helper

*Found while reproducing F141 (T41).* Spec-blessed today; worth revisiting.

The meter keys on the fn that is *current* when the call happens, so an
`ai_complete` made from an un-budgeted helper is not counted:

```axon
fn helper() -> i64 { let x = ai_complete("hidden")  1 }

@[ai(policy(budget: 1))]
fn ask() -> i64 { helper()  helper()  helper()  3 }
```

Executed: three AI calls, `exit 0`, budget never consulted. Only a `W1310`
("AI call in `helper` has no @[ai(policy)] — cost is unmetered") marks it, and a
warning is not a ceiling.

`governance/specs/R3c-ai-budget-meter.md` §3 says the budget "counts the
`ai_complete` calls made *while that fn is the current fn*", and §"Nested"
confirms A's budget does not cover B's. So this is **implemented to spec** — but
it undercuts the spec's own stated value proposition, *"a fn can declare 'I may
make at most N model calls' and the runtime enforces it"*. A ceiling that an
`Extract Function` refactor removes is not enforcing much.

Note the contrast with the R4 agent action-log, which deliberately uses the
**enclosing** fn (`enclosing_agent`, `interp.rs` ~2078) precisely so *"a
capability builtin called from a helper of an agent is still logged to that
agent's action trail (the audit can't be escaped by indirection)"*. The budget
had the same choice available and made the opposite one. This is the same
transitive-laundering class as the R6 `@[sensitive]` taint and the `@[contained]`
sandbox, both of which were closed by call-following.

Changing it is a spec amendment, not a bug fix, and it is a real behavior change
(a program that runs today would start hitting E1301). It is also the safe
direction — fail-closed, more enforcement. **Decision needed:** should the budget
cover the callee's dynamic extent (minus nested budgeted fns, which meter
themselves)?

Note this does *not* affect the T41 refusal's soundness: the refusal condition
mirrors the enforcement condition exactly, so both backends agree on the
laundered case (both unmetered). If the budget becomes enclosing-scoped, the
refusal must widen to match — transitively, over the call graph.

---

## O033 — sandbox handles are still indices, and `sandbox_run` takes them raw

*Found while fixing P7-SEC-03 (T42).* Lower severity than the principal case;
worth closing for uniformity.

T42 made **principal** handles unforgeable tokens. **Sandbox** handles
(`sandbox_create` → an index into `self.sandboxes`, resolved with
`sbs.get(active as usize)`) were left as indices, so the same
`handle ± 1` reachability exists there.

Why it is not the same severity: a sandbox is created by the program that then
runs inside it, and the nesting guard reads `self.active_sandbox` — the
interpreter's own state — rather than a caller-supplied handle. So the "may only
narrow" ceiling that T1/T3 enforce is not bypassed by forging a handle. What a
forged sandbox handle *can* do is name a different sandbox's allowed-effect set
in a `sandbox_run`, which is worth closing before anything starts trusting a
sandbox handle as a capability the way `principal_*` did.

The fix is mechanical now that the pattern exists: give `sandboxes` the same
`by_token` map and `fresh_token()` draw. The one thing to check while doing it is
`active_sandbox`, which stores `-1` for "none" — that sentinel collides with a
token space that includes negative values, so it needs to become `Option<i64>`
first. That sentinel-vs-full-range collision is exactly what forced removing the
`h >= 0` guards on the principal builtins in T42.

Related, and deliberately left alone: `SandboxEntry.principal` is stored but only
ever rendered into an error message, never resolved for authority. It is a label,
not a capability, so a stale value there misleads a human reading a diagnostic
but does not grant anything.

---

## O034 — the guest substrate needs FS, and that is not the program's authority

*Found while closing OSK-P7-C3 (T48).* Needs a decision; the gate states the
grant explicitly meanwhile.

Closing the guest kernel's fail-open defaults surfaced a modeling question the
`EffectSet(0xFF)` default had been hiding. `examples/flagship/agent_task.ax` has
an honest effect union of `["IO"]` — it only prints. Run it in the microVM with
exactly that grant and it dies:

```
VIOLATION: syscall 257 blocked (FS not in policy)
```

Syscall 257 is `openat`. The guest runs the **interpreter**, which must open
`/axon/program.ax` before a single line of Axon executes. So the effective grant
a guest needs is *the program's effects ∪ what the substrate itself requires*,
and those are different things with different owners.

Three ways to resolve it, none free:

1. **axon-vm unions a documented substrate baseline** (`FS`) into any
   manifest-derived grant. Simple, and it makes the common case work — but every
   guest then holds FS, so a program that declares no file access can still read
   any file the guest can see. The capability story would be weaker than the
   `@[contained]` story one layer up, which *does* scope by path.
2. **Load the program before installing the gate**, so the substrate's own I/O
   happens outside the policy. Correct in principle; it moves the trust boundary
   and needs the loader to be part of the TCB rather than gated code.
3. **Scope FS at the syscall gate by path**, so the substrate may open
   `/axon/program.ax` and nothing else. Strongest, and the most work: the gate
   currently classifies by syscall number, not argument.

(2) or (3) is the destination — (1) trades away the property the microVM exists
to provide. Until then `scripts/axon_kernel_gate.sh` passes `IO,FS` explicitly
for the good-agent case with a comment saying why, so the gate is honest about
what it grants rather than relying on a default that grants everything.

**Decision needed:** whether the substrate's own syscalls belong inside the
policy at all. Note this was invisible while the default was open — the gate
"passed" by granting full authority, which is exactly what Layer 3 claims to
test.

---

## O035 — quorum votes are not authenticated, and cannot be without a key decision

*Found while working P4-OS-12 / P7-SEC-04 / P7-KRN-07 (T49).* The remaining half
of that finding. **Decision needed.**

A `.vote` file carries no signature or MAC. `collect_responses` reads every
`*.vote` in a directory, and `voter_tcb` is a string the vote declares about
itself. Executed against the CLI:

```
$ printf '{"voter_tcb":"axtcb1-ext:0000…","run_id":"deploy-prod",
           "approved":true,"reason":"forged","lineage_root":"h1"}' > forged1.vote
   (×3)
$ axon-vm quorum check --responses-dir . --n 3
QUORUM MET: 3/3 approvals        exit 0
```

Anyone who can write that directory is the entire quorum. The module header
described these as "attested VoteRequest/VoteResponse"; nothing verified either,
and T49 corrected the claim rather than leave the tool asserting a property it
does not have.

T49 closed the two holes that need no key material — votes are now bound to the
run they name (`--run-id`), and the operator can pin the expected identity
(`--expect-tcb`). Both raise the bar; neither makes a vote authentic. An attacker
who knows the real run id and the real TCB digest — both of which appear in the
request file the voters were given — can still forge the whole quorum.

Real authenticity needs each voter to sign its response, which needs a decision
this module cannot make:

1. **What key?** The R26/R31 attestation path already establishes a per-guest
   identity. Reusing it binds a vote to an attested TCB, which is the property
   the design wants — but it couples the quorum to hardware attestation being
   live, and today the software-TPM stand-in lane is the only one that runs.
2. **Distributed how?** The aggregator needs each voter's public key, and the
   binding from key to voter identity is the whole trust question. A file of
   pinned keys next to the responses directory is the smallest thing that works
   and is itself a thing an attacker would target.
3. **Or avoid keys entirely** by making the transport authenticated instead —
   the R33 vsock path (§5.2.2, not yet built) could carry votes over
   per-guest channels the host controls, so a vote's provenance comes from the
   channel rather than from its contents.

(3) is likely the right destination for the microVM fleet, since the transport
already exists as a design and the file-based exchange is explicitly a scoped
stand-in. Until one is chosen, **the responses directory is part of the TCB** —
now stated in the module header and the CLI help, so an operator reading either
learns it before relying on the gate rather than after.

---

## O036 — the differential parity harnesses run, or don't, depending on what built `target/debug/axon` last

*Found while fixing GATE-04 (T51).* P6-GATE-03, with its mechanism confirmed and
its conclusion corrected by execution. **Decision needed.**

`scripts/fuzz_parity.sh` resolves the codegen binary as *"prefer an
already-built one … if THAT fails (no LLVM / build lock held by a parent cargo),
skip cleanly"*, then probes whether that binary can actually emit native code —
a `--no-default-features` build leaves an `axon` that can `run` but not `build`.

So the wrapper's outcome is keyed on a **shared, mutable build artifact**, and
both outcomes were observed in one session:

```
run A:  target/harness-skips.log records "codegen unavailable — fuzz parity"
        (test returns in milliseconds, asserting nothing)
run B:  codegen_fuzz_parity_finds_no_divergence runs 75s and asserts
        "fuzz_parity: PASS"
```

Nothing about the harness or the test changed between them — only which command
had last written `target/debug/axon`. My own earlier interpretation, that these
harnesses "never run inside `cargo test`", was wrong: they run whenever the
binary at that path happens to be codegen-capable. That is worse than never
running, because it means a green suite carries no information about whether the
primary I-2 differential guard executed.

T51 makes a *failing* harness impossible to mistake for a skip, and
`AXON_HARNESS_STRICT=1` turns any skip into a hard failure — so the condition is
now detectable. It is not yet prevented.

Two ways out:

1. **Delete the in-suite wrappers** and promote `parity_all.sh` from `--strict`
   into the standard gate. Honest, and it stops the unit suite implying coverage
   it cannot guarantee — but it moves that coverage out of the loop most
   contributors actually run.
2. **Give the harnesses their own binary** — `target/parity/axon`, built once by
   the gate and resolved via an env var. The feature and lock collisions both
   disappear, the artifact is not shared with whatever a developer last built,
   and the wrappers assert deterministically.

(2) keeps the coverage where it is useful and removes the non-determinism, which
is the actual defect. **Decision needed:** which. This interacts with the pending
"`AXON_HARNESS_STRICT=1` in CI" decision — under (2) strict mode becomes
enforceable; under (1) there is nothing in-suite left for it to guard.

---

## O037 — `r31_acceptance_gate.sh` greps for test NAMES; it never runs them

*Noticed while fixing P4-OS-11 (T52).* Small, and the same vacuous-gate class as
GATE-04/GATE-05.

The gate's core loop is:

```bash
for name in "${REQUIRED_NAMES[@]}"; do
    if grep -q "$name" "$LIB_SRC" "$VM_SRC" 2>/dev/null; then pass "found: $name"
```

So a required test is satisfied by its **name appearing anywhere** in either
file — including in a comment, a doc string, or a `#[ignore]`d body. The gate
proves the string exists, not that the property holds.

This is exactly how P4-OS-11 survived: `extended_tcb_wired_into_run` was present
and passing, and the gate was green, while the thing it was named after —
`--extended-tcb` actually gating the boot — did nothing. The test measured and
compared a digest *in a unit test*; the CLI path never called `verify_extended`
at all. A name-presence check cannot tell those apart.

Fix is mechanical: run the tests and assert on the result —
`cargo test -p axon-vm -p axon-attest -- --exact <name>` per required name, or a
single `cargo test` run whose output is parsed for each name reporting `ok`.
That also catches the `#[ignore]` case, which name-grepping cannot. Worth
sweeping the other `REQUIRED_NAMES`-style gates for the same shape at the same
time — `grep -l REQUIRED_NAMES scripts/*.sh` finds them.

---

# Opportunities — build-loop over `AXON_FOR_RLM.md` (2026-08-06)

Deferred work found during the RLM build. Logged, not acted on.

## O-RLM-01 — ten more verbs still throw their diagnostics away (proposed: HIGH)

T-R3 fixed `run`. `run_check_pipeline` — whose entire body flattens typed
diagnostics to `[CODE] message`, dropping `help`/`file`/`line`/`col`/`expected`/
`found` — has **eleven** callers, and the other ten are untouched:
`main.rs:2354, 2447, 2526, 3032, 3136, 3974, 4519, 5014, 5485, 5936` (`test`,
`deploy`, `ast review`, `doc`, `redteam`, and others).

So `axon test` and `axon deploy` report the same location-free, help-free
diagnostics `run` did. Decision D2 assumed this wrapper had one caller and could
simply be deleted; it is shared, not obsolete. Converting the remaining ten is a
mechanical but real task, and the corpus equivalence test from T-R3 generalises
to it directly — one `verb × corpus` matrix asserting every verb agrees with
`check`.

## O-RLM-02 — `const` and `var` get no help, at the resolve tier (proposed: MEDIUM)

`AXON_FOR_RLM.md` §1 names both. Probing showed they lex as ordinary identifiers
and fail at name resolution (`cannot find name \`const\` in this scope`), not at
the parse tier, so `parse_help` is never called for them and cannot be. They are
exactly as unrepairable as `let mut` was. The fix is the same shape one tier
down: a help row on the unresolved-name diagnostic when the name is a known
foreign keyword. Pinned as a negative test today
(`parse_help_probe.rs::const_and_var_do_not_reach_the_parse_tier`) so the
tier fact is not re-discovered.

## O-RLM-03 — repo-wide `cargo fmt` is red, and it blocks the project's own gate (proposed: MEDIUM)

~40 files under `crates/axon-core/src/` are unformatted at HEAD (`builtins.rs`,
`capabilities.rs`, `checker.rs`, `codegen/*`, …), pre-existing and from the
parallel R21/R23 track. Recorded as O005 by the previous build loop and still
open, so this is its second sighting.

The practical cost this run: `cargo fmt -- --check` cannot be used as a gate at
all, so each task had to diff its own files against HEAD to prove it added no
formatting debt. A one-shot `cargo fmt` would fix it and collide with whatever
that track has in flight — which is why it was declined before and is declined
again, not because it is hard.

## O-RLM-04 — the clippy gate is an allowlist, and six crates are outside it (proposed: MEDIUM)

`scripts/gate.sh` lints `axon-core` (under `--no-default-features` only) plus
`axon-rt/ai/surface/gfx-mock/domain/vm/attest/ledger`. Not linted:
**`axon-intent`, `axon-os`, `axon-web`, `axon-audit`, `axon-certcheck`,
`axon-signal`**. `axon-os` currently carries ~11 warnings including a dead
`append_entry` (`monitor.rs:287`).

This is the third recorded instance of the same class. The fix is one line in
`gate.sh`; the reason it keeps recurring is that a green gate reads as coverage.

## O-RLM-05 — the audit ledger cannot detect tail truncation (proposed: HIGH)

`crates/axon-audit/src/lib.rs:349` `verify_chain` walks entries checking
`seq`/`prev_hash`/`entry_hash`, but nothing anchors the tip or the expected
length. `Ledger::open` reads whatever lines are present, so deleting the last N
lines of the JSONL yields a chain that verifies **clean** — an audited program
can erase its own trailing FS/Exec records. Found by reading the code during the
pre-build survey; independently reproduced three times by the 2026-08-01 triage
as finding P6-COV-02, which is still open. Needs an externally-anchored tip or a
signed entry count.

## O-RLM-06 — `axon-web` has no auth and `Access-Control-Allow-Origin: *` (proposed: MEDIUM)

`crates/axon-web/src/server.rs:70`. Every `POST /api/*` shells attacker-supplied
program text into the interpreter. It binds loopback only, which is the
mitigation, but there is no Origin check and no CSRF token — and a `fetch` with a
default `text/plain` body is a CORS *simple* request, so no preflight blocks it.
Any page the user visits while the server is up can drive it, and `ACAO: *` lets
that page read the results. Triage finding P4-PROD-11, still open.

## O-RLM-07 — the repair round is unprimed, so the diagnostics are measured through a channel that discards them (proposed: HIGH — and cheap)

Found by the T-R5 gate measurement. `repair_prompt`
(`atlas/spikes/rlm-engine/src/axon_engine.rs`) takes **no primer**: every repair
call in R9 is zero-shot, whatever the generation arm was given.

The consequence is measured, not theorised. Post-repair was 5/8 against a
first-try of 5/8 — **+0** — and on the vowel task the *first* generation wrote
`let i = 0` correctly while the *repair* introduced `let mut i`. The language
card suppressed the habit and the unprimed repair round put it back, faster than
the diagnostic could correct it.

So the headline conclusion "better diagnostics did not improve repair" is not
supported by this run, in either direction: the experiment cannot see it. The
fix is to pass the primer into `repair_prompt` and re-run — one afternoon, ~24
model calls — and it is the measurement that would say whether Axon's ceiling
with a card is 5/8 or higher. It should be run **before** any decision about
§4/§5, because the gate D6 asks about is exactly that ceiling.

Do it as a fourth arm rather than by changing `run_arms`, so the published
zero-shot-repair numbers stay comparable.

## O-RLM-08 — the remaining 3 failures are three more table rows, not a new problem (proposed: MEDIUM)

With the card, `mut` is gone from first generation. What fails instead:
`or`/`and` where Axon wants `||`/`&&`; method syntax on arrays (`v.max()`,
`s.len()`); and one lexer-level rejection (`unexpected character`). The first two
are exactly the shape `parse_help` already handles and would be two more rows
plus two probe cases. The third needs a look at what the model actually emitted.

---

# Opportunities — loop 2 (the two follow-up specs), 2026-08-06

## O-RLM-09 — `axon test`'s PARSE tier still flattens (proposed: MEDIUM)

U2 converted every `run_check_pipeline` caller, but `axon test` reaches the
parse tier through `parse_source_files` (`lib.rs:380`), a **public API returning
`Vec<String>`** with four callers. Converting it to typed diagnostics is a
public-interface change and was out of U2's scope.

Visible, not hidden: `cli_run.rs`'s verb matrix carries
`TEST_VERB_PARSE_TIER_IS_A_KNOWN_GAP` and skips exactly those cases, so the hole
is in the test source rather than in a silently-passing assertion.

## O-RLM-10 — the language card does not mention character literals (proposed: HIGH)

The measured cause of all three remaining fluency failures is `c == ' '`. The
compiler now names it (U6), but the **card** still does not, so the model writes
it on first generation every time and spends its one repair round recovering.
Adding one line to `LANGUAGE_CARD` is the obvious next measurement — and it must
be measured ALONE, since changing the card and the diagnostic together is what
A2b was written to prevent.

## O-RLM-11 — the diagnostic did not repair, even when correct and primed (proposed: HIGH — research)

The most interesting negative result of the loop. With the card in the repair
prompt AND a correct, specific diagnostic naming the construct and its
replacement, the score stayed 5/8 and the same three tasks failed. `check`
carried `help` on 9 task-runs, up from 3, so the mechanism fired three times more
often and moved nothing.

Note this was measured with the FIRST version of the char-literal hint, which
was wrong (it claimed `char_at` returns a `str`). The corrected hint has not
been measured — the atlas working tree moved to another branch mid-run. So the
honest status is: *unmeasured with correct advice*. That single re-run is the
cheapest experiment on this list and the one that decides whether the diagnostic
work pays at all.

## O-RLM-12 — two type-checking entry points that must agree, and no test that they do (proposed: MEDIUM)

`lib::check_pipeline` and `main::run_check_pipeline_located` run the same passes
and carry a code comment saying they must stay in sync. They had drifted:
`check_pipeline` dropped every resolver diagnostic's `fix`, so library consumers
saw no help where the CLI showed it (fixed in U5). The comment is not a test.
The same corpus trick the verb matrix uses would work here: run both over the
refused-programs corpus and assert they produce identical diagnostics.

## O-RLM-13 — the containment refusal (E1001) has no line number, in any verb (proposed: MEDIUM)

Found by loop 2's smoke test, which asked `axon deploy` on a containment
violation to show file, line and help. It shows file and help; there is no line,
and `axon check` has none either — so this is not a delivery gap that U2 missed
but a **pre-existing** one in the capability checker, which emits its diagnostic
with a dummy span. `PipelineDiagnostic::json` correctly omits `line`/`col` when
they are 0 rather than faking them.

E1001 is the diagnostic a containment host shows a model when the compiler
refuses its code, so "which call was refused" is exactly the question a reader
has. The message names the call (`read_file("/etc/passwd")`), which is why this
is MEDIUM rather than HIGH — the information is recoverable by searching, just
not by jumping.

Fix: carry the offending call's span through `capabilities::check_capabilities`
into its `Diagnostic`, the same way the resolver already does.

---

# Opportunities — the HIGH-tier loop, 2026-08-06

## O-HI-01 — `r34_acceptance_gate.sh` asserts behaviour that T48 deliberately removed (proposed: MEDIUM)

`r34` fails at HEAD, before and after this loop's changes, in a section this loop
did not touch:

```
✗ run --chain-stamp → exit 2, output: 'axon-vm: no effect grant: … Refusing to
  launch rather than sending a null policy to the guest' (expected CHAIN BROKEN, exit 15)
```

The gate expects a run that T48 **intentionally** made refuse. So the gate is not
detecting a regression; it is asserting the pre-T48 behaviour. Fix is to give the
chain-stamp case an explicit grant (`AXON_VM_ALLOWED_EFFECTS` or a manifest) so
it reaches the chain logic it means to test.

Pre-existing and unrelated to S2 — verified by running the HEAD version of the
script, which fails identically.

## O-HI-02 — O026 was already fixed, and the opportunity entry did not say so (proposed: MEDIUM — process)

O026 ("the in-guest effect policy defaults to OPEN") was carried into this
spec's `needs-human` set as a live decision. It is **closed**: AUDIT T48
(`axon-vm/src/main.rs:1114`) makes the launcher refuse rather than emit a null
policy, and the comment records that the guest now denies on ambiguity too.

The process lesson is the one worth keeping: this loop carefully re-read every
source entry's *text* — which is what caught six "decision needed" markers — but
did not check whether the *code* had moved since the entry was written. An
opportunity can be stale in the CLOSED direction, and handing someone a decision
they already made is its own kind of wrong answer.

Sweep the rest of `opportunities.md` against current code before the next loop
plans from it.

## O-HI-03 — H2/O031 reclassified: native AI builds are an INTENDED capability (proposed: HIGH — decision, not code)

Attempted on the "fix all" instruction and **reverted**, because implementation
revealed what the opportunity entry could not.

O031 frames it as a defect: `axon-core`'s default features exclude
`asi-runtime`, so the interpreter refuses `ai_complete` (E1300), while codegen
links `axon-ai` unconditionally and produces a binary that dials the model live.
That divergence is real and reproducible.

But the repo has **three tests asserting AI programs must build natively** —
`build_refuses_non_balanced_ai_tier_e0910_r3` explicitly requires that "a
balanced-tier ai_complete must still BUILD", and the other two assert that only
*specific* shapes (per-call tier, `@[ai(policy(budget: N))]`) are refused. So
native AI is a supported, tested capability with a deliberately drawn refusal
boundary, not an oversight.

Gating the native link on `asi-runtime` therefore **removes a capability people
rely on**. That is a product decision with external-behaviour exposure, which is
the definition of `needs-human` — and it is why the attempt was reverted rather
than shipped behind a flag.

The choice, restated with what is now known:

1. **Gate native on `asi-runtime`** — the two paths agree; native AI builds stop
   working for anyone not passing the feature, and three tests change.
2. **Make the interpreter live too** — the paths agree in the other direction;
   `axon run` starts making network calls in the default build, which is a much
   larger blast radius.
3. **Document the divergence** and keep both — cheapest, and leaves a capability
   difference decided by execution path rather than by grant.

No recommendation is adopted. Worth noting (1) is still my suggestion, but it is
a capability removal and belongs to whoever owns that contract.

One thing that IS safely fixable without the decision: nothing about the current
behaviour is stated where a user would see it. `axon build --help` and the AI
docs could say that a native build makes live AI calls regardless of
`asi-runtime`, which is true today under every option above.

## O-SESS-01 — `arr_push` is `[i64]`-only, so a list of records cannot be BUILT (proposed: HIGH — blocks the RLM head-to-head)

Found trying to port `atlas/spikes/rlm-engine/src/stateful.rs`'s chain fixture to
Axon for the head-to-head against CPython.

`arr_push` is `params: [("xs", "[i64]"), ("x", "i64")], ret: "[i64]"`
(`builtins.rs:402`), and its own doc says why: *"Concrete-typed for i64 today;
generic [T] form waits on Phase 8."* The whole `arr_*` family is the same shape.

The consequence is sharper than a missing convenience. Axon can **represent** a
list of records — `let rows = [Rec { id: 1, region: "north", amount: 120 }, …]`
round-trips through the session's literal store correctly, verified — but it
cannot **construct** one from parsed input of unknown length. There is no
`arr_push` that accepts a `Rec`.

`stateful.rs`'s chain step 1 is "parse the dataset into a list of records". So
the fixture is **not expressible in Axon today**, and the harness's own rule
applies: *"the chain fixture is not expressible on this engine, so a reuse rate
or a token ratio from it would be a measurement of the harness."*

This is therefore the gate on the entire stateful head-to-head, which is the
measurement the RLM thread has been aimed at. Generic `[T]` for the `arr_*`
family is Phase 8 work per the doc; a narrower unblock would be a generic
`arr_push` alone.

## O-SESS-02 — the session has no `axon session` verb (proposed: MEDIUM)

Cell splitting and module composition live in `scripts/axon_session.py`, using
brace counting rather than the parser — so a `{` inside a string literal or a
comment will mis-split a cell. The value persistence is real and in the
interpreter; the ergonomics are a shell script. Moving the driver into the CLI
and giving it the real parser is the slice that makes this a feature.
