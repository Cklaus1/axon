# Autonomous ASI Build Loop — Axon

**Operating directive for self-paced, parallel, verified, self-improving build.**

Run under `/loop` (self-paced) or as a standing autonomous loop. **One iteration = one
independently-shippable, gate-green slice** that moves a `REQUIREMENTS.md` row toward DONE without
weakening the TCB. This doc *is* the loop's contract; each iteration executes §1's cycle under §0's
non-negotiables and §10's operating principles.

> Invoke as: `/loop` (no interval → self-paced) with the prompt *"Run one iteration of the ASI build
> loop per governance/ASI_BUILD_LOOP.md."* The loop persists state to `SESSION_STATUS.md` and schedules
> its own next iteration; it hands off (§9) for decisions that are genuinely the user's.

---

## §0 Prime directive + non-negotiables (check every iteration; violating one = STOP)

- Advance `REQUIREMENTS.md` rows toward DONE. Value = **load-bearing × cheapness-to-close**.
- **NEVER weaken an `ARCHITECTURE_INVARIANTS.md` invariant implicitly.** To change one, follow its
  §"How to change an invariant" (proposal + blast-radius enumeration + edit-with-the-code). **I-2**
  (interpreter is the oracle), **I-11** (capability boundary), **I-12** (self-mod can't weaken the TCB)
  are sacred.
- **Spec-first for Structural changes** (`BUILD_PROTOCOL.md` Gate 1): no spec → write it
  (`SPEC_TEMPLATE.md`) before code. Trivial/Standard may skip.
- **Gate-on-green:** never commit/push a red tree. `&&`-chain tests before `git`.
- **Honest reporting:** tests fail → say so with output; step skipped → say so; a claim about the code →
  **VERIFY it against the code before asserting it.**
- Branch, don't touch `main`. Commit only what THIS slice changed (the tree may hold unrelated dirty
  files — never sweep them in).

## §1 The loop (repeat until a STOP condition in §9)

```
SELECT → (SPEC) → PLAN → BUILD → VERIFY → EVAL → RECORD → LOOP-CONTROL
```

## §2 SELECT — what to build next

- Read `REQUIREMENTS.md` + the per-spec §12 open questions + `SESSION_STATUS.md`.
- Pick the highest (load-bearing × cheap-to-close) item whose dependencies/gates are met. Prefer the
  **smallest independently-shippable slice**, not the epic.
- Respect hard gates (an unratified invariant amendment; a genuinely user-owned wedge/strategy fork →
  §9 handoff).
- State the slice's DONE condition (**a NAMED passing test**) before building.

## §3 SPEC — only for Structural changes

- Fill `SPEC_TEMPLATE.md`: surface, semantics table (= the test plan), type rules, error codes (invent
  here, not in code), invariants touched, acceptance = named tests, rollout/rollback, open questions. A
  TODO in a mandatory section = not ready to build.

## §4 PLAN — decompose; decide the execution shape

- Break the slice into steps; classify each INDEPENDENT vs DEPENDENT.
- Parallelize INDEPENDENT work; sequence DEPENDENT work. Default to a **pipeline**, not a barrier,
  unless a step genuinely needs all prior results at once.
- Pick the tool by shape (§5 heuristics). Write the plan to `SESSION_STATUS.md`.

## §5 BUILD — maximize performance via subagents WHEN IT PAYS

Parallelize when work is independent and fan-out adds coverage or speed; stay inline for a quick,
single-file, known-location edit.

- Broad search / "where is X across the tree" / naming sweeps → `Explore`/`general-purpose` subagents,
  fanned out in ONE message.
- Multi-file independent transforms (migration, sweep) → `Workflow` pipeline; worktree isolation only if
  they'd conflict.
- Hard design decision, wide solution space → **judge panel**: N independent attempts (different angles)
  → score → synthesize the winner.
- Adversarial check / review / "is this finding real" → **parallel skeptics**, each prompted to REFUTE;
  believe it only if it survives.
- DON'T parallelize: tight sequential dependencies, or trivial edits where orchestration tax > the work.
  Don't run a search you already delegated.
- Write code that reads like its neighbors (idiom, naming, comment density). Follow pipeline order (I-1)
  and the "Adding a New Builtin" recipe where relevant.

## §6 VERIFY — rigor MATCHED TO RISK (verify when appropriate, not ritually)

- The oracle is the **interpreter (I-2)**. Any dual-path (interp/codegen) feature carries a parity test.
  Non-textual output → the amended oracle (`R16a`).
- Tiered:
  | Change class | Verification |
  |---|---|
  | Trivial edit | `cargo build` + the touched unit tests |
  | Standard feature | `scripts/gate.sh` (fmt + clippy + tests) |
  | Structural / dual-path | `gate.sh` + `scripts/parity_all.sh` (or the specific `*_parity.sh`) + the spec's named acceptance tests |
  | Safety / TCB-touch | the above + an **adversarial self-review**: spawn skeptics to find the laundering hole / bypass / divergence BEFORE believing it's closed |
- **VERIFY YOUR OWN CLAIMS:** before asserting "X exists / Y is wired / Z is closed," grep/read the
  actual code. (This loop's own history: an asserted "schema already exists" was false on inspection —
  distrust confident memory.)
- A finding/feature isn't real until a test proves it AND it survives a refute attempt. Default to "not
  done" under uncertainty.

## §7 EVAL — when the slice is complete, measure (don't assume)

- Run the spec's **named acceptance tests**; they must pass by name (`DEFINITION_OF_DONE.md`: "it works"
  is not done; "test `foo` passes" is).
- Re-run `gate.sh` + the relevant parity scripts; assert the suite count didn't silently shrink
  (vacuous-pass guard: `passed > 0`).
- For AI/ASI/optimization features: run the eval/oracle harness (the four-gate firewall shape where
  applicable — correctness oracle, capability-monotonicity, regression, perf). Report the **measured
  delta**, not a vibe.
- If the eval shows the DONE condition wasn't met → it's not done; loop back, don't rationalize.

## §8 RECORD — close the loop honestly + compound the learning

- Update `REQUIREMENTS.md` %/Status/Gap AND the spec AND examples **in the same commit** that changes
  the truth (**I-15**: spec and behavior don't drift). Cite the passing acceptance test in the commit
  body for any →DONE move.
- Conventional commit; end with the `Co-Authored-By` line; push only the slice.
- Write durable learnings to **memory** (one fact per file + `MEMORY.md` pointer): non-obvious gotchas,
  corrected assumptions, decisions + why. Don't memorize what the repo already records.
- Update `SESSION_STATUS.md`: landed / verified / next.

## §9 LOOP-CONTROL — continue, hand off, or stop

- **CONTINUE** if green-gateable work remains and no decision is owed to the user. Persist state;
  schedule the next iteration (self-paced).
- **HAND OFF** (ask the user) ONLY for a genuine fork that is theirs: a strategy/wedge call, an invariant
  change needing ratification, an irreversible/outward-facing action, or a spec §12 BLOCKER. Don't ask
  about choices with a sensible default — pick it, state it, proceed.
- **STOP** when no slice is gate-green-achievable without a blocked decision, or the budget is exhausted.
  Report state + the single next action.

## §10 ASI operating principles (cross-cutting — this is "build like an ASI")

1. **Match the artifact to the decision.** Don't over-spec; produce the thing that resolves the live
   question (a spike for "is it real", a spec for "how").
2. **Adversarially verify before believing** — your own claims, your own findings, your own "done."
   Independent skeptics > self-assurance.
3. **Diverse lenses** for hard calls (judge panel); **loop-until-dry** for discovery; **completeness
   critic** ("what modality/claim/source did I not check?").
4. **Falsifiability:** every nontrivial claim states how it could be wrong.
5. **Compound learning:** each iteration updates the plan/world-model and memory; never re-derive a
   settled fact or re-litigate a made decision.
6. **Smallest reversible step;** bounded blast radius; fail closed.
7. **Leverage discipline:** spend effort where (load-bearing × cheap) is highest; log anything you cap or
   skip (no silent truncation).

## §11 Anti-patterns (do NOT)

- Commit red. Change an invariant implicitly. Skip the spec on a Structural change. Self-grade a result
  that needs an external/ground-truth judge.
- Parallelize trivial work (orchestration tax) or run a search you delegated.
- Assert a code fact from memory without checking. Mistake a timeline VIEW / a passing-but-vacuous suite
  / a rigged-to-pass test for real progress.
- Manufacture false confidence: a green that doesn't test the thing that matters.

---

### Ultracode posture (optional, opt-in)

When the user enables **ultracode** (or says "build like an ASI, token cost no object"), flip the §5/§6
defaults: author and run a `Workflow` per phase by default, make **adversarial multi-lens verification
the default** (not risk-gated), and run several workflows in sequence for multi-phase work
(understand → design → implement → review), staying in the loop between them. Revert to the risk-gated
posture when ultracode is off.
