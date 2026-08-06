# Flight plan — RLM follow-ups (loop 2)

**Mission.** Measure the repair ceiling through a channel that can actually see
it, then give every remaining CLI verb the diagnostics `run` and `check` now have.

**Engineering decisions (Step 2)**
- A1 implementation — new duplicate fn / **parameterise `run_arms` with a repair primer** → chosen: duplicating a 60-line measurement loop is the T54 defect; passing `""` from the old bin makes comparability *provable* by diffing against the committed transcript.
- A1 reporting — count only / **which tasks pass** → chosen: 5/8 twice with a different 5 is a different result, and a count cannot show it.
- A2 method-syntax row — build it / **delete it** → chosen: E0403 already carries help through both verbs (verified); nothing to build.
- A2b sequencing — build rows then measure / **measure then build rows** → chosen: new help content confounds the ceiling measurement.
- E1 verb uniformity — identical formatting / **identical information, own presentation** → chosen: `fmt`/`doc` do not type-check and leave scope by inspection.
- E2 string-format consumers — assume none / **grep `scripts/` + tests before deleting** → chosen: the wrapper's format is a de-facto contract until proven otherwise.
- E4 caller #2 (`mobile_emit_object`) — contrive a test / **behaviour-preserving gate only** → chosen: it discards diagnostics, so no honest fail-first test exists.
- ⚠ close call: **running both specs as ONE loop.** Cleaner isolation would be two loops; each costs a 7-min baseline and 7-min regressions, and the specs share file scope. Merged.

**needs-human — excluded**
Spec A Open Q2, growing `r9::TASKS` for statistical power: it is shared by every
engine in the benchmark, so growing it silently re-baselines Rhai, Lua, CPython
and bash and invalidates published numbers. Recommendation recorded (**do not
grow it; run more trials**), decision not taken. Pruned subtree: 1 item.

**Step 1 `[REVISED]` markers** — A1 implementation inverted · A1 outcome
overclaim corrected ("any diagnostic" → this card + this diagnostic + one round)
· A2 method row deleted (probed: E0403 already has help) · A2b added · B1 caller
list re-derived (line numbers were stale at authoring) · B1 external-contract
risk relocated (axon-web reads stdout; diagnostics are stderr) · B3 execution
hazard (deploy/test/goal RUN programs; guarded by check-first).

**Critical path — U2** (convert 10 callers), gated by **U2t**, the verb × corpus
equivalence matrix, written first and failing.

**Shape.** 8 tasks · 3 tiers · longest chain 3 (U2t→U2→U3) · 2 repos · baseline
**1765/0 adopted** (delta since = 2 .md files) · budget unbounded · full suite
~7 min.

**First three tasks**
1. **U1** — parameterise `run_arms`, prove the old bin byte-identical, add the primed-repair arm, 3 trials.
2. **U2t** — the verb × corpus matrix, failing.
3. **U4** — grep for `[CODE] message` consumers before anything is deleted.
