# Axon Autonomous Build — Full R3–R10, end-to-end

The loop prompt for building **everything remaining** in the Axon PRD
(`governance/REQUIREMENTS.md`) to completion, unbounded, via the gated 8-step
protocol. Driven by `/loop` or a cron job that re-injects the prompt below.

## Current state (2026-06-01, update as you go)

- **R1 native build: SOLVED** (`BUILD_RESOLVED.md`) — `cargo build -p axon-core`
  finishes ~3s, `axon build foo.ax` emits a native binary matching the
  interpreter. 75%, only the Tier-1 perf benchmark remains.
- **R2 type system: 90%**, **R8 testing: 82%** (forall DONE), **R5 goal: 70%**
  (goal_eval held-out DONE), **R4 zones: 55%**, **R9 alignment: 62%**
  (`@[corrigible]` latching kill-switch DONE), **R3 AI: 40%**,
  **R6 capability: 25%**, **R7 targets: 10%**, **R10 self-improving: 0%**.
- **R3/R4/R6/R7/R10 specs are REVIEWED** (`5504dc6`, adversarial parallel review);
  R1/R1b still Draft. Per `governance/specs/README.md`, **implementation of a
  spec'd requirement may not begin until its spec is `Reviewed`** — the five
  gating specs now clear that bar, so R3/R4/R6/R7/R10 coding is unblocked.

## Build order (re-ranked for current reality — stale work-queue in REQUIREMENTS.md superseded)

Cheapest-and-compounding first; the PRD "Hello Goal" forcing function
(`ROADMAP.md` §5) is the integrating target that R5/R3/R4/R9 serve.

**0. R1 native-codegen gaps — ✅ DONE (native==interp 25/28; all real bugs fixed).**
All native-codegen crashes/correctness bugs FIXED: #40 to_str (51bae16/29fea3b),
#41 closures+enum== (29fea3b/0e61bd2), #42 array heap corruption (5453fbf), tuples
(c2a7c77). The 3 residuals build clean and are NOT codegen bugs (ai_complete/
adaptive_summarize = AI-call env; error_handling = #37 parse_int Err-message gap,
a known minor codegen-message issue, deferred). R1 → 85%. **Next: R8 forall.**
Old detail (kept for history):
**0b. (history) native==interp 23/28 fn-main examples).**
Acceptance: all fn-main examples build native AND match `axon run` (AXON_AI_MOCK=1 for AI).
   - ✅ **#40** to_str silent truncation (f64/bool) + to_str(i32 widening) — FIXED (51bae16, 29fea3b).
   - ✅ **#41** closure-passing ABI (Type::Fn → {ptr,ptr} fat-pointer) — FIXED (29fea3b).
   - ✅ **#41** enum `==` → tag compare (was str_eq) — FIXED (0e61bd2).
   - ⬜ **TUPLES (net-new feature, not a bug):** `Expr::Tuple` returns `None` in codegen
     (expr.rs:243) — tuple literals + numeric field access (`t.0`, nested `t.0.0`) are
     UNIMPLEMENTED. Needs: emit Expr::Tuple as anon LLVM struct; handle numeric
     FieldAccess on tuple. Edge: `t.0.0` lexes as `Dot Float(0.0)` (tuple-dot quirk).
     Suitable for a focused subagent slice.
   - ⬜ **traits_demo MALLOC CORRUPTION (`malloc(): corrupted top size`):** NOT vtables —
     the file uses enum ADTs (Shape::Circle{radius:f64} etc.) + match destructuring + f64
     math (Phase-3 traits not built; enums are the idiom). Corruption is in enum-with-
     **payload** construction or match-field-extraction with f64 fields (heap/GEP/size bug).
     Memory-unsafe — diagnose with a minimal `enum {V{f64}}` + match repro before fixing.
   - Non-bugs: ai_complete/adaptive_summarize need AXON_AI_MOCK; error_handling matches
     (multi-line sweep artifact).

1. **R8 — `forall` property testing.** ✅ DONE (`1be7d5a`). Binary-search shrinking + reproduce seed. R8→82%.
2. **R5 — `#[goal]` first-class.** ⚠️ goal_eval held-out DONE (`0b4065d`, R5→70%); `Goal` type + `#[goal(...)]` attr sugar still pending.
3. **R9 — `#[corrigible]` kill-switch.** ✅ DONE (this commit). `corrigible_halt()` one-way latch; engine refuses `@[corrigible]` calls post-halt before the body; `corrigible_halted()` guard; fail-closed exit 4 (`Flow::Halted`); targeted scope. R9→62%. Demo `examples/asi/corrigible.ax`.
4. **R4 — code zones + provenance.** Spec ✅ Reviewed. ⚠️ `@[experiment]` zone DONE (this commit, R4→62%): injects tagged provenance (I-13) but excluded from goal_run best; JSONL now carries real event/zone/label. Demo `examples/asi/experiment_zone.ax`. Remaining: `@[agent]` mandatory action-log + codegen conformance (R1-gated). **← agent-log NEXT, or move to R3**
5. **R3 — AI primitive.** Spec drafted. Provenance schema → `#[ai(policy)]` → routing.
6. **R6 — capability/registry.** Spec drafted. `axon.lock` + content hash + audit.
7. **R7 — targets.** Spec drafted. Slice A interp→wasm first (now also AOT-wasm, R1 unblocked).
8. **R10 — self-improving harness.** Spec drafted. G1–G3 correctness/safety; G4 perf (R1 now unblocks it).
9. **R1 perf benchmark + R1b str-return migration + R2 edge cases** — finish the partials.
10. **THE FORCING FUNCTION:** wire the `axon intent compile → ast review → goal run → trace → improve → redteam → deploy → replay` pipeline end-to-end (ROADMAP §5). "When this works end-to-end, Axon is real."

Work the highest-ranked requirement that is not yet DONE. Within a requirement,
do one acceptance-criterion-sized slice per tick.

## Per-tick protocol (the 8 gates — BUILD_PROTOCOL.md)

**Step 0 — SPEC GATE (once per requirement, before any code).** If the requirement
has a spec in `governance/specs/` still marked `📝 Draft`: do an adversarial
self-review against `SPEC_TEMPLATE.md` + `CODE_REVIEW_RUBRIC.md`. Fix gaps, confirm
the decisive fork is resolved, all 12 sections sound, error codes real, §12 honest.
Then mark it `✅ Reviewed` in `specs/README.md` and commit. ONLY THEN write code for
it. If no spec exists and the requirement is structural, write one first
(SPEC_TEMPLATE.md). Requirements with an existing non-`specs/` spec (R8→TESTING_STANDARD,
R5→ROADMAP§5) skip straight to Step 1.

Then, for each acceptance-criterion slice:
1. **FRAME** — state the acceptance criterion (a named test), risk class.
2. **RED TEST** — write the failing test first; run it; watch it fail for the right reason.
3. **IMPLEMENT** — smallest change to green.
4. **WIDEN** — edge/adversarial/property tests; **interp↔codegen parity** (now that
   native builds, parity tests can be real, not `#[ignore]`d — un-ignore them).
5. **REVIEW** — self-review against CODE_REVIEW_RUBRIC.md as an adversary.
6. **GATE** — `cargo test -p axon-core --no-default-features` (interp) AND
   `cargo build -p axon-core` (native, ~3s) AND `cargo clippy --no-default-features
   -p axon-core -- -D warnings` ALL green, &&-chained, BEFORE commit. Commit only on green.
7. **VERIFY** — run the feature for real (example/native binary/demo); update docs +
   REQUIREMENTS.md %/Status + the spec + BUG_HUNT ledger if a new bug surfaces.
8. **PUSH** — push after each green commit.

## Rules (non-negotiable)

- **Commit only on green; never commit red.** &&-chain test && native-build && clippy && commit.
- **Native parity is now testable** — every dual-path feature gets a real interp↔codegen
  parity test (un-`#[ignore]` the ones the specs left as tripwires). I-2: interpreter is
  the oracle; codegen that disagrees is the bug.
- **Do NOT enable `codegen` + `serde-json` together** (reintroduces the build stall — BUILD_RESOLVED.md). Native = default features; JSON tooling = `--features serde-json` separately.
- One acceptance slice per tick. Small, verified, reversible. Revert to last green if a tick can't reach Gate 6.
- Cite the requirement (Rn) + acceptance test in every commit body. Update REQUIREMENTS.md %/Status in the SAME commit that moves the truth.
- **Honesty rule (load-bearing — see this session's R1 saga):** verify before believing;
  distinguish confirmed from suspected; when a hypothesis is cheap to test, TEST IT before
  building on it. If three attempts at a fix fail, STOP and re-diagnose — don't keep pushing
  a falsified premise. Surface negative results as prominently as positive ones.
- **Update ARCHITECTURE_INVARIANTS.md** when a spec adopts a proposed invariant (I-15/I-16/I-17).
- Log any new bug in `BUG_HUNT_2026-05-31.md` with severity + area.
- **Use subagents (Sonnet) for mechanical multi-site work** (migrations, wrapping,
  test-fanout); ALWAYS independently verify a subagent's output before committing — they
  pass `cargo check` but can introduce logic bugs (this session: a match-narrowing bug
  `cargo check` couldn't catch).
- **Update this file's "Current state" + build-order** as requirements reach DONE, so the
  next tick has accurate ground truth.

## Stop condition

The loop ends when every R3–R10 acceptance criterion in REQUIREMENTS.md is met, the
weighted-completion line reads ~100% language-core, AND the Hello Goal forcing function
runs end-to-end (the three artifacts of ROADMAP §5 produced from one `signup.intent.md`).
Until then, there is always a next-highest unfinished slice.
