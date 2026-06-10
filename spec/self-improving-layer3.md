# Axon Spec — Self-Improving Compiler, Layer 3: Constrained Free-Form Pass Authorship

**Goal**: Let the proposer (an AI) author *genuinely novel* optimization passes — not just
select from the closed registry — while the existing four-gate firewall remains the sole
arbiter of what may run. The intelligence improves the *proposer*; the *gates stay the moat*.
Every base-model gain makes the compiler better without ever being able to make it unsafe.

**Builds on**: `governance/specs/R10-self-improving-compiler.md` (the firewall: G1 interp
correctness oracle / G2 capability-diff / G3 regression / G4 perf, multi-sig graduation),
and the four shipped registry passes + red-team + corpus-breadth work
(`crates/axon-core/src/improve.rs`, `improve_templates.rs`).

**Status**: design spec for the **unbuilt** Layer 3. Layers 1 (firewall) and 2 (bounded
registry proposer) are built and hardened; this designs the frontier. Implementation is
**gated on the prerequisites in §6** being met first — building L3 on an unhardened firewall
is the exact mistake the whole design exists to prevent.

---

## The three layers (recap, with status)

| Layer | What | Status |
|---|---|---|
| **L1** firewall | `verify_pass`: G1/G2/G3/G4 + multi-sig graduate | ✅ built + red-team-hardened |
| **L2** bounded proposer | AI selects a registry template *by name*; never authors code | ✅ 4 verified passes, corpus-broad |
| **L3** free-form authorship | AI *authors* a novel transform that must still clear L1 | 📋 this spec |

The safety invariant is unchanged across all three: **a pass runs only if it is in the
content-addressed `passes.manifest`, and it enters the manifest only via `graduate`, which
requires a green four-gate verification AND human multi-sig** (R10 §4.5). L3 changes only
*where candidate passes come from* — never how they are admitted.

---

## 1. The core problem: an AI cannot author a `&'static fn`

Today a `Pass` is `dyn Fn(&Program) -> Program` and every registry template's `pass` is a
`&'static fn` — *reviewed Rust, compiled into the binary*. An AI cannot emit Rust that gets
compiled into the running compiler (that would require recompiling + reloading the TCB — the
highest-risk operation imaginable, and it defeats the firewall, since the malicious code runs
*as* the verifier). So L3's central design decision is:

> **An AI-authored pass is DATA, not code — a declarative rewrite spec interpreted by a
> fixed, reviewed Rust engine.** The AI emits a transform *description*; a trusted interpreter
> (compiled into the TCB, never AI-authored) applies it. The AI never executes; only the
> reviewed interpreter does.

This is the same move as L2's closed registry (the AI emits a *name*, validated against
`TEMPLATES`), generalized: in L3 the AI emits a *small rewrite program* in a constrained,
total, side-effect-free DSL, validated against a grammar, and run by a reviewed evaluator.

## 2. The Rewrite DSL (the constrained transform language)

A candidate pass is a list of **rewrite rules**, each `pattern → replacement` over the AST,
with optional guards. The DSL is deliberately tiny, total, and pure:

```
rule        := PATTERN "=>" REPLACEMENT ("if" GUARD)?
PATTERN      := an AST shape with typed metavariables (?x, ?y) and literal holes
REPLACEMENT  := an AST shape built only from the pattern's bound metavariables + literals
GUARD        := a conjunction of total predicates over bound metavariables
                (is_literal, is_pure, equals, in_range, … — a closed predicate set)
```

Example (what `constant-fold` would look like as data):
```
?a:int_lit + ?b:int_lit  =>  fold_add(?a, ?b)   if no_overflow(?a, ?b)
```

**Hard constraints on the DSL (enforced by the validator, §3):**
- **Total**: no recursion in rules, no unbounded loops; rule application is a single bounded
  bottom-up pass with a fixed fuel budget (a candidate that doesn't terminate is rejected
  before it runs, not allowed to hang the verifier).
- **Pure**: a replacement may only *rearrange/​delete* bound subtrees + emit literals via a
  closed set of total builtins (`fold_add` etc., each a reviewed Rust fn matching interp
  semantics). It cannot synthesize a call to an arbitrary fn, inject a capability builtin, or
  introduce a name not bound by the pattern.
- **Capability-monotone by shape**: the replacement grammar has no production for a `Call` to
  a capability builtin (`read_file`/`net`/`exec`/…) — so a capability *cannot even be
  expressed*. (G2 still checks this dynamically as defense-in-depth; the grammar makes it
  unrepresentable as defense-in-design — the I-12 "by construction" posture, cf.
  `principal_mint`.)

## 3. The pipeline: propose → validate → compile-to-pass → verify → graduate

```
AI proposer  ──emits──▶  RewriteSpec (JSON/DSL text)
                              │
                    ┌─────────▼──────────┐  E15xx on any failure (new band)
                    │ 1. PARSE+VALIDATE   │  grammar + totality + purity + cap-shape
                    └─────────┬──────────┘  (fail-closed: malformed → rejected, never run)
                              │ a validated RewriteSpec
                    ┌─────────▼──────────┐
                    │ 2. COMPILE-TO-PASS  │  reviewed Rust: RewriteSpec → impl Fn(&Program)->Program
                    └─────────┬──────────┘  (the interpreter of the DSL — in the TCB)
                              │ a runnable Pass
                    ┌─────────▼──────────┐
                    │ 3. VERIFY (L1)      │  the EXISTING four gates, UNCHANGED
                    └─────────┬──────────┘  G1 oracle / G2 caps / G3 regress / G4 perf
                              │ a green VerifyRecord
                    ┌─────────▼──────────┐
                    │ 4. GRADUATE         │  EXISTING multi-sig gate, UNCHANGED
                    └────────────────────┘  + the RewriteSpec is stored verbatim in the
                                             manifest entry (content-addressed, auditable)
```

Steps 3–4 are **literally the existing `verify_pass` + `graduate`** — no change. L3 adds only
steps 1–2 (validate + compile-to-pass), both reviewed Rust, both fail-closed.

## 4. Design forks (proposed resolutions)

- **F1 — code or data?** → **Data (a RewriteSpec DSL).** Resolved above (§1): AI-authored
  Rust in the TCB is unsafe and defeats the firewall. The AI emits a transform *description*;
  a reviewed evaluator runs it. **CLI-reachable**: `axon improve verify <corpus> --spec <file>`
  reads a RewriteSpec (one rule name per line), VALIDATES it (E15xx, fail-closed — unknown rule
  E1411 / empty E1409 / over-budget), COMPILES it with the reviewed `rewrite_dsl::compile`
  evaluator, and runs the same four gates as a registry template. The unknown-rule error lists the
  closed vocabulary from `RewriteRule::ALL` (single source, drift-guarded by a test). Previously the
  DSL was test-only; this makes the "passes as data" surface usable end-to-end.
- **F2 — how rich is the DSL?** → **Start minimal** (literal-fold + identity-collapse +
  branch-fold shapes — i.e. exactly the four shipped passes, re-expressed as data), then widen
  only as each new shape is red-teamed. A richer DSL = more expressivity = more attack surface;
  grow it under the same discipline that grew the registry. **First widening LANDED**:
  `fold-logical` (short-circuit-sound logical folding — a genuinely NEW optimization, not a
  re-expression of a shipped pass), with its soundness boundary unit-tested (folds `false && R`,
  refuses `L && false`) and a red-team proving the unsound drop-left variant is caught by G1.
  **Second widening LANDED**: `fold-bound-builtin` (folds the pure, total integer bound builtins
  `min_i64`/`max_i64`/`abs_i64` over literal args — `max_i64(3,7)`→`7`), with the same CHECKED
  discipline as `fold-int-literal` (it REFUSES to fold `abs_i64(i64::MIN)`, whose runtime overflow
  panic must be preserved — a built-in red-team in its firewall corpus proves G1 catches a fold
  that erased the panic). Capability-free by construction (it only ever replaces a builtin Call
  with an Int literal, never emits a Call). Mirrors the checker / comptime / SMT constant folders
  for these builtins, so all four evaluators agree.
  **Third widening LANDED**: `fold-comparison-literal` (folds a comparison of two int literals to its
  `bool` result — `3 < 5`→`true`, `7 == 7`→`true`). Comparisons are total (never panic), so it is
  unconditionally behavior-preserving. It is the missing link in the constant-folding chain:
  `fold-int-literal` folds the operands (`(2+1) < 5` → `3 < 5`), THIS folds the comparison to a bool,
  and `fold-const-branch` then collapses the enclosing `if` — the three compose to fully evaluate a
  constant-condition branch. Firewall-cleared (corpus exercises the full arith→compare→branch chain
  plus a non-literal comparison left intact). Capability-free.
- **F3 — does a malformed/over-budget spec hang the verifier?** → **No.** Validation (§3 step
  1) rejects non-total specs *before* execution; the compile-to-pass evaluator runs under a
  fixed fuel budget and a wall-clock cap, and a runaway is a rejection (E15xx), never a hang.
- **F4 — is the AI ever in the trust path?** → **Never.** Correctness is the interpreter
  oracle (G1, R10 §7/E1406 — AI judgment of correctness is forbidden); the AI only *proposes*
  a spec, which is then mechanically validated, mechanically compiled, mechanically verified,
  and human-signed. Every step after the proposal is reviewed Rust.
- **F5 — what's stored in the manifest?** → **The RewriteSpec verbatim** (content-addressed)
  + its VerifyRecord. A graduated pass is fully auditable and re-verifiable from its spec — no
  opaque blob. Re-running `verify` on the stored spec must reproduce the green record
  (determinism, R10 §4.7).

## 5. New error band (E15xx — proposal/validation, distinct from the E14xx firewall band)

| Code | Condition |
|---|---|
| E1501 | RewriteSpec fails to parse (malformed DSL) |
| E1502 | Spec is not provably total (recursion / unbounded application) |
| E1503 | Replacement references a metavariable not bound by the pattern |
| E1504 | Replacement uses a builtin outside the closed total set |
| E1505 | Replacement shape could introduce a capability (grammar violation — defense in design) |
| E1506 | Compile-to-pass exceeded its fuel/time budget on the corpus (rejected, not hung) |

These are *proposal-stage* rejections — fail-closed, before the firewall even runs. A spec
that passes E15xx validation then faces the unchanged E14xx firewall (G1/G2/G3) and the
unchanged multi-sig graduate (E1404).

## 6. Prerequisites — DO NOT build L3 until these hold

L3's safety rests entirely on L1 being trustworthy. Build order (each de-risks the next):

1. **A broad corpus** — the G1 oracle is only as strong as the programs it compares on. ✅
   (corpus-breadth landing: all passes × 4 gates across recursion/loops/structs/enums/
   closures/strings/Result/panic). *Widen further before L3 ships.*
2. **A red-teamed firewall** — prove G1/G2 reject deliberately-wrong and
   capability-escalating candidates. ✅ (red-team: output-change, exec-injection,
   panic-erasure all rejected).
3. **Multiple verified passes** proving the harness generalizes. ✅ (4 registry passes).
4. **THEN L3** — and even then, the DSL ships minimal (F2) and widens only as each shape is
   red-teamed.

Prereqs 1–3 are now met; L3 is unblocked to *scope and prototype*, but the DSL must ship
minimal and the red-team suite must be extended with **DSL-level adversarial specs** (a spec
that tries to express a capability, a non-total spec, a spec whose replacement subtly changes
behavior) proving the §3-step-1 validator + the G1/G2 firewall reject each.

## 7. Implementation plan (when unblocked)

1. **`crates/axon-core/src/rewrite_dsl.rs`** (NEW) — the RewriteSpec type, the parser/​
   validator (E15xx, fail-closed, totality + purity + cap-shape), and the compile-to-pass
   evaluator (`fn compile(spec: &RewriteSpec) -> Box<dyn Fn(&Program)->Program>`), all
   reviewed Rust. Unit tests including the DSL-level red-team specs (§6).
2. **`improve.rs`** — a `DiscoveryMode::FreeForm` that takes a RewriteSpec (from the AI or a
   fixture), validates + compiles it, and routes the resulting Pass through the *unchanged*
   `verify_pass`. The four shipped passes get a fixture RewriteSpec each, proving the DSL can
   express them and they still clear the gates (a strong equivalence check).
3. **manifest** — store the RewriteSpec verbatim in the graduated entry (F5); re-verify on
   load.
4. **CLI** — `axon improve propose --spec <file>` (validate + verify a candidate spec, no
   graduate). Graduate stays multi-sig.
5. **Docs/tests** — extend the red-team suite with DSL adversarial specs; gate them.

## 8. Verification (when built)

- Each of the 4 shipped passes, re-expressed as a RewriteSpec, validates (E15xx clean) and
  clears the four gates over the corpus — proving the DSL is expressive enough and the
  compile-to-pass is faithful.
- DSL red-team: a spec expressing a capability → E1505 (or G2/E1402 as backstop); a non-total
  spec → E1502; a behavior-changing spec → G1/E1401. Each rejected, none run.
- A graduated RewriteSpec re-verifies deterministically from its manifest entry.
- `scripts/gate.sh --strict` green; a new `scripts/rewrite_dsl_redteam.sh` (or lib tests)
  locks the validator's fail-closed contract.

## 9. The safety argument, stated plainly

L3 lets an AI write optimizations Axon never shipped — yet it is *more* constrained than a
human contributor, not less: the human writes Rust reviewed by humans; the AI writes a
**total, pure, capability-free data spec** that is mechanically validated, mechanically
compiled by reviewed code, mechanically proven behavior-preserving + capability-non-widening
by the interpreter oracle over a broad corpus, and finally human-signed before it can run.
The AI is never in the trust path. The compiler improves itself; the firewall it cannot
weaken decides what that means.
