# Tech Spec — R9b: SMT `@[verify]` over loops (invariant inference)

**Status:** 📋 Planned / Not Started (Draft 2026-06-03; corrected 2026-07-31) — the design is
resolved **for v1 (templates-only)**; the CHC mechanism and the static loop-E1102
counterexample path are RE-OPENED as Q4 (see the fork in `README.md`, updated to match). NO
implementation exists: `smt.rs` discharges only
straight-line `i64`/`f64` fragments (`encode_block` returns `Unsupported` for While/For), and
all §-acceptance criteria below are unchecked. This is correctly a 0%-built item, distinct from
R9's shipped straight-line SMT.
An ASI-trajectory review (2026-07-31) folded in three must-fix corrections before implementation
starts — the runtime-check **elision** a `proven` result triggers today (§4.3, v1 decision: loop
proofs do NOT elide), the **frame condition** as an explicit soundness obligation over one
fail-closed walk (§4.2/§4.6), and an **adversarial-authorship** threat model with the stated limit
that residual safety leans on human review (§4.7) — plus Q6–Q8 (author-supplied invariant hints,
R23 certification, and whether the v1 fragment admits any real loop at all).
**Requirement:** `../REQUIREMENTS.md` R9 — *Layer-1/3 alignment; Formal Verification.* Extends `R9-smt-verify.md` (straight-line integer + float fragment, ✅ Reviewed/landed) past its hard boundary: loops.
**Parent boundary:** R9-smt §4.2 / §10 scope loops OUT — *"Loops/recursion need invariants (a Phase-N extension)."* This is that extension.

---

## 1. Motivation

R9's SMT path (`--features smt`, Z3) PROVES `@[verify(value OP K)]` for straight-line integer/float bodies (`+ - *`, `ite`). The single largest unprovable-but-common shape it punts to `W1103 Unsupported` is **the loop** — `while`/`for` accumulators, the exact bodies a numeric `@[verify]` bound most wants to guard (a running sum stays in range, a counter never overflows a cap, a decay stays ≥ 0). Proving a loop requires a **loop invariant**: a predicate true on entry, preserved by each iteration, and strong enough to imply the post-condition. General invariant *synthesis* is undecidable; the tractable v1 is a **bounded, template-driven inference** over the same decidable integer fragment R9 already encodes.

## 2. Requirement link

`../REQUIREMENTS.md` **R9** (88% — corrected 2026-07-31; the draft's 78% predated the composite-predicate, float-fragment, metacognition, and Temporal-decay work). Acceptance anchor unchanged: *`@[verify]` bounds proven by Z3*. This widens the proven fragment from straight-line to **single-loop linear** bodies. Dependencies: **R9-smt** (the *expression/predicate* encoder, `smt.rs`, Z3 Int/Real — reused for terms, guards, and the post-condition), **I-2** (interp is reference — a `proven` loop must behave identically at runtime), **R1f-differential-parity-fuzz** (`scripts/fuzz_parity.sh` — the randomised differential harness reused by §8's adversarial gate), and **R23-proof-certificates** (✅ Landed 100%; `crates/axon-certcheck` + `cert_gate.rs` — the solver-free certificate checker whose whole premise is *"don't trust the prover, trust a small checker of the certificate it emits"*; R9b adds new Z3-trusting machinery and MUST record its relationship to that lever, see §7 and Q7). *(Corrected 2026-07-31: the draft said "reused wholesale" — false. `encode_block` admits only leading `let`s + a tail expression and has NO `Expr::Assign` or statement-`if` handling, so every loop body in scope is inexpressible today; the loop-body **transition-relation encoder is new machinery**, specified in §4.6, and is the majority of the implementation.)*

## 3. Surface (what the user writes / runs)

No new syntax. The *same* `@[verify(value OP K)]` now also discharges a function whose body contains one loop:

```axon
@[verify(value >= 0)]
fn clamp_decay(start: i64, steps: i64) -> i64 {
    let acc = start
    let i = 0
    while i < steps {
        if acc > 0 { acc = acc - 1 }   // monotone non-increasing, floored at 0
        i = i + 1
    }
    acc                                 // invariant: acc >= 0 ∧ acc <= start
}
```

> (Corrected 2026-07-31: the draft wrote `let mut` — Axon has no `mut` keyword (no `Mut`
> token in `token.rs`); mutability is plain `let` + bare reassignment.)

```
cargo build -p axon-core --no-default-features --features smt --bin axon
axon verify clamp_decay.ax   # PROVES value>=0 via inferred invariant
```

> (Corrected 2026-07-31: `--features smt` is a cargo build flag, not an `axon` CLI flag —
> a non-smt binary prints a notice and exits 0; an smt-built binary just runs `axon verify`.)

## 4. Semantics

### 4.1 The provable fragment (the fork resolution)

**Decisive fork: inference strategy — (a) template/Houdini, (b) abstract interpretation (interval/octagon), or (c) CHC/PDR via Z3's `fixedpoint` engine?**

**→ Resolved (corrected 2026-07-31): (a) template-driven inference is v1 in full — templates miss ⇒ `W1103 Unsupported`. The (c) CHC fallback is RE-OPENED as Q4, not shipped in v1 — never (b) as a separate analyzer.**

> **Correction 2026-07-31:** the original resolution had templates fall through to (c) via
> `z3::fixedpoint`. That API **does not exist** in the pinned dependency: the workspace pins
> `z3 = "0.12"` (`crates/axon-core/Cargo.toml:70`) and the vendored z3-0.12.1 source has no
> Fixedpoint binding at all (zero grep hits; the Rust `z3` crate has never wrapped the
> `Z3_fixedpoint` C API). The known workarounds each change the design — a
> `Solver`-for-`HORN`-logic encoding (via `Solver::new_for_logic`) DOES yield the synthesized
> invariant on **sat** (`Model::of_solver` + `Model::get_func_interp` on the invariant
> predicate — the standard SMT-LIB `(get-model)` CHC idiom), but on **unsat** (bound violable)
> yields only a refutation proof term (`Solver::get_proof`), not an init+iteration-count trace
> — so it preserves Q3 reporting but weakens the E1102 counterexample story; and raw `z3-sys`
> FFI (0.8.1 already declares the full `Z3_fixedpoint_*` extern surface — no new *bindings*
> needed) is a new *unsafe-call* surface the §7 "no new dep" claim doesn't cover. So v1 ships
> templates-only; the CHC mechanism choice is Q4 (§12). *(This block as first written on
> 2026-07-31 claimed the HORN-Solver path had "no invariant or counterexample extraction" —
> half wrong; corrected same day against the vendored z3-0.12.1 source.)*

Rationale:
- **(a) templates** are cheap, deterministic, and explain themselves (the inferred invariant is reportable). v1 enumerates a small fixed template family over the loop's *modified* integer variables: `Σ cᵢ·vᵢ ⋈ K` and per-variable bounds `lo ≤ v ≤ hi` (ranges + linear combinations — the **octagon-expressible** invariants, which cover accumulator/counter/decay loops). For each candidate, Z3 discharges the three Hoare conditions (init ⇒ inv, inv ∧ guard ∧ body ⇒ inv′, inv ∧ ¬guard ⇒ post). A candidate surviving all three is a *proof*.
- **(c) Z3 CHC/PDR** (the Spacer engine) remains the principled general fallback *in principle*: encode the loop as Constrained Horn Clauses and let Z3 *synthesize* the invariant. **Deferred out of v1 (Q4)** — the pinned `z3` crate exposes no fixedpoint binding (see the correction above), so v1's no-template-fits path is `W1103 Unsupported`. If/when built, it runs with a wall-clock bound and, on timeout, yields `W1103 Unsupported` (never a false proof).
- **(b) abstract interpretation** is rejected as a *separate* mechanism: it would be a second semantic model to keep in sync with the encoder (an I-2-adjacent drift risk, like R10's "no second mechanism" posture). The octagon *shape* is captured as templates fed to the one trusted oracle (Z3), not as an independent fixpoint engine. *(Honesty note, 2026-07-31: the §4.6 transition encoder is itself a new statement-level semantic model that must stay in sync with the interpreter — what (b)'s rejection avoids is a second **fixpoint engine** producing invariants outside Z3, not statement encoding, which any strategy here requires. The transition encoder joins the trusted encoding base and must be differentially gated, see §4.6.)*

### 4.2 The loop shape v1 accepts

A function with **exactly one** `while` whose body is straight-line integer arithmetic (the R9 fragment) over a fixed set of reassigned-`let` `i64` locals (Axon has no `mut` marker — mutability is bare reassignment), with a linear guard. Nested loops, loop-carried calls, float loops, `for` loops, and break/continue → `W1103 Unsupported` in v1 (honest boundary; and per §4.3 a v1 loop keeps its runtime `@[verify]` gate whether it is `proven` or not, so the boundary is honest in both directions).

> **Frame condition (added 2026-07-31, must-fix):** the "fixed set of modified variables" is a
> *soundness* obligation, not a convenience — any location the loop can write that is absent
> from the primed state is encoded by the preservation VC as unchanged, which makes a false
> invariant Z3-confirmable and yields a false `proven` with no Z3 bug and no template weirdness.
> The set is therefore NOT "reassigned-`let` locals" as a category discovered by a separate
> scan; it is whatever the single §4.6 admitted-form walk returns, and that walk is fail-closed.
> Concretely, three shapes in this tree are outside the set and MUST abort the whole loop:
> assignment to a **parameter** (the resolver rejects only fn/type/enum/module names —
> `resolver.rs:1090-1108` — so params are `Symbol::Local` and freely reassignable, and they are
> exactly the Z3 constants the postcondition is stated over), **place assignment**
> (`Expr::AssignTo { place, value }` is a DISTINCT node from `Expr::Assign` — `ast.rs:364` vs
> `ast.rs:379` — so `xs[i] = v` / `s.f = v` is invisible to an `Expr::Assign`-only match), and a
> **shadowing inner `let`** in the body.

> (Corrected 2026-07-31: the draft claimed `for v in a..b` "desugars to the `while` form
> already in the AST". It does not — `Expr::For` is a distinct AST node (`ast.rs:389`)
> evaluated directly by the interpreter, and the parser has no for→while desugar. v1 therefore
> scopes to `while` only; encoding `Expr::For` (via a desugar inside the SMT encoder or a
> shared lowering) is an explicit follow-on slice item, see Q5 in §12.)

### 4.3 Why this is still sound

Every proof obligation is discharged by **Z3 on the existing R9 encoder** — the invariant is a *witness*, not a trusted oracle. A wrong template simply fails one of the three Hoare checks and is discarded; only an invariant that Z3 confirms on all three is accepted. So R9b cannot produce a false `proven` even if the template heuristics are weak — the worst case is `Unsupported`, never an unsound proof. (This is the R10-style firewall: propose cheaply, verify with the one trusted mechanism.) That argument covers *template strength*; it does NOT cover the transition relation being a sound over-approximation of the body (§4.2 frame condition, §4.6 trust status), which is a separate obligation.

**What a `proven` result costs today (added 2026-07-31 — must-fix, resolves the "runtime gate still applies" over-claim).** In the current tree a `Proven` from `prove_verify_bounds` is collected by `smt::discharge` into `verify::Discharged.verify_fns` (`smt.rs:976-990`), and that set makes **both** engines SKIP the runtime `@[verify]` check — `run_program_with_discharged` (`main.rs:3605`) and the native path (`main.rs:4616`, *"native checks elided"*). `Discharged`'s own doc (`verify.rs:46-70`) justifies the elision by the ∀-inputs proof making the guarded panic dead code. So "the runtime gate still applies" is true only of the **W1103** outcome; a `proven` result *deletes* the backstop for exactly the failure mode §4.6 admits (a mis-encoded transition ⇒ false `proven`). Because R9b extends `prove_verify_bounds`, loop proofs would enter `verify_fns` **silently** unless the implementer opts out.

**v1 decision: loop-derived `Proven` results are NOT added to `Discharged.verify_fns`.** A v1 loop proof is a *static claim*, reported by `axon verify` (and per Q6/§9 carried into the approval artifact); the runtime `@[verify]` check is **retained** in both engines. This costs nothing that v1 was built for — the value here is the proof and its explanation, not the elided branch — and it means v1 is never both uncertified (no R23 certificate, Q7) *and* unchecked. Elision becomes admissible only after the §4.6 transition encoder has soaked behind the §8 differential fuzz **and** a loop proof emits an R23 certificate that `axon-certcheck` validates solver-free (Q7). This decision is asserted by a test (§9) so it cannot be reversed by accident when wiring into the existing prover.

**Inherited ℤ-vs-i64 assumption (added 2026-07-31).** R9's encoder proves over unbounded ℤ and justifies it (`smt.rs:472-478`) by the fact that an i64 overflow *panics at runtime before* the return check, so eliding a proven check is observably a no-op (I-2). That argument is load-bearing and depends on a **different subsystem** — checked arithmetic in both engines (`interp/value.rs` sized `checked_add/sub/mul`, mirrored natively; see the `native-checked-arithmetic` work). Loops are precisely the shape that exercises it: an accumulator over N iterations is the canonical overflow shape, and §1 sells this slice on "a running sum stays in range". R9b therefore states the assumption explicitly: **every arithmetic operation in an admitted loop body must be runtime-checked in BOTH engines; if any admitted operation ever becomes wrapping/unchecked, loop proofs over ℤ become unsound.** See the §7 dependency row and the §8 overflow test.

### 4.4 Determinism

Template enumeration is a fixed, ordered list; Z3 is deterministic per query. (If the deferred CHC fallback lands per Q4, it must run with a fixed seed + fixed timeout — timeout → deterministic `Unsupported` on a given machine; the timeout bound documented as machine-sensitive and kept generous.) Reproducible per §R9 4.4.

### 4.5 Behavior table

| Function shape vs bound | Result |
|---|---|
| single linear `while` loop, an octagon-template invariant proves the bound | **proven** (+ the inferred invariant reported) — and in v1 the runtime `@[verify]` check is **retained**, not elided (§4.3 decision) |
| single linear `while` loop, no template fits | **W1103 Unsupported** in v1 (CHC synthesis deferred to Q4) |
| single linear `while` loop, bound actually violable | **W1103 Unsupported** in v1 — never `proven` (no candidate survives the Hoare checks); the **runtime `@[verify]` gate still catches the violation (exit 3)**. *(Corrected 2026-07-31: the draft promised static **E1102 + counterexample** here, but templates/Houdini cannot produce one — a failed Hoare check discards a candidate, it says nothing about reachability, so templates-only cannot distinguish "violable" from "no template fits". Static E1102-for-loops needs a reachability mechanism — Spacer trace derivation or a bounded-unroll BMC — deferred to **Q4**.)* |
| nested loops / `for` loop / break / float loop / loop-carried call | **W1103 Unsupported** (v1 boundary) |
| built without `--features smt` | clean no-op notice, exit 0 |

### 4.6 Loop-body transition encoding (NEW machinery — added 2026-07-31)

The existing `encode_block` (`smt.rs`) is *expression-only*: it accepts leading `Expr::Let`
statements plus a tail expression and returns `Unsupported` for anything else — it has **no
`Expr::Assign` handling at all** (zero occurrences in `smt.rs` today). But every loop body in
scope — including §3's flagship `clamp_decay` (`if acc > 0 { acc = acc - 1 }`, a statement-`if`
mutating a local) — consists of reassignments and statement-level conditionals. The Hoare VC
`inv ∧ guard ∧ body ⇒ inv′` therefore requires a **transition-relation encoder** that this
slice must build; it is the majority of the implementation, not a reuse.

**Admitted statement forms** (anything else → the body is out of fragment → W1103):
- `Expr::Assign { name, value }` where `name` is a scanned `i64` local and `value` is in the
  R9 expression fragment over the current environment;
- statement-level `if cond { … }` / `if cond { … } else { … }` whose arms contain **only**
  admitted assigns (no nested loops, no calls, no early exit), with `cond` in the R9 boolean
  fragment.

**Single fail-closed walk (added 2026-07-31, soundness clause).** The modified-variable set MUST
be derived from the *same* admitted-form walk that builds the transition relation — **one walk,
not two analyses** — and the walk MUST return `None` for the **entire loop** on encountering any
node it does not explicitly admit. The whitelist is closed and fail-closed: `Expr::AssignTo`,
`Expr::Let` inside the body (shadowing), assignment to a **parameter**, `Expr::Break`,
`Expr::Continue`, `Expr::For`, any `Expr::Call`, and every node not named above ⇒ W1103, never a
best-effort encoding. Do **not** reuse the borrow checker's assignment scan: it is a *may*-analysis
built for a different purpose and carries no completeness obligation, so agreeing with it is not
the same as being sound here. Widening this whitelist is a change to a security boundary and
requires re-running the §8 gates (see §4.7).

**Lowering:** sequential strongest-postcondition threading (SSA-style). Each modified local
`v` starts as an unprimed Z3 const `v`; statements are threaded left-to-right, each assign
producing a fresh version bound by substitution; a statement-`if` merges its arms per-variable
with `ite(cond, v_then, v_else)`. The final versions are the primed state `v′` used in the
preservation VC. This reuses R9's `encode_expr`/`encode_bool` for every term — the *new* code
is the statement threading and the `ite` merge.

**Trust status:** the transition encoder joins the trusted encoding base exactly like R9's
`encode_expr` — a mis-encoded transition *could* yield a false `proven` (the §4.3 witness
argument protects against weak *templates*, not against encoder bugs). Mitigation is the same
as R9's: keep the admitted forms minimal, and gate with differential tests (a `proven` loop is
also executed and its runtime result checked against the bound — see §8).

### 4.7 Adversarial fragment — who writes the code under test (added 2026-07-31)

R9 and R9b as drafted contained **zero** occurrences of *adversar*/*malicious*/*generated*/*LLM*
(grepped 2026-07-31, 0 hits in both). Their trust framing treats the **template** as the untrusted
proposer and Z3 as the trusted oracle — correct for the mechanism, but it silently assumes the
*program text* is a neutral input. `ROADMAP.md` §2.1 commits the opposite: the typed language is
*"an IR, not a human-authored surface … optimize for unambiguous audit, machine generation"*. When
the author is a strong generator, the program text **is** the adversarially-selected input, chosen
against the published admitted-form whitelist and against `smt.rs` itself.

Stated assumption: **the generator is assumed to be OPTIMIZING against this gate, not merely
careless.** Consequences, all normative:

1. **Fail-closed whitelist.** Any form not explicitly admitted ⇒ W1103. Never a best-effort
   encoding, never a "close enough" transition (§4.6).
2. **The transition encoder is a security boundary.** It joins the trusted encoding base; its
   admitted-form list may be widened only with a re-run of the §8 gates and an updated §7 row.
   The unique route to a false `proven` that needs no Z3 bug is a frame-condition miss (§4.2),
   which is exactly what a search against the acceptance gate would find first.
3. **Corpus differentials are the wrong instrument.** §8's original mitigation ("every `proven`
   loop *in the corpus* is also executed") samples shapes a human chose to write down — precisely
   the distribution an adversarial generator is not drawn from. It is replaced, not supplemented,
   by a randomised differential fuzz over *generated* loop bodies reusing the R1f harness
   (`scripts/fuzz_parity.sh`).

**Stated limit (expiring assumption).** This spec's residual safety story leans on a human
reviewing the artifact — the `axon ast review` / Phase-12 approval pane surfacing which functions
are `proven` and which are runtime-only (§9). That is a real limit, not a background truth: it
holds only while generated-code volume stays inside a reviewer's attention budget. When it does
not, the W1103 set stops being a triage list and the retained runtime gate (§4.3) becomes the
whole defence — which is a further argument for keeping it in v1 and for Q7's certificate path.

## 5. Type rules

No type changes. The encoder reads the existing AST; the loop's modified-variable set falls out of the §4.6 admitted-form walk. *(Corrected 2026-07-31, twice: (i) the scan is NOT the only new analysis — the loop-body transition-relation encoder of §4.6 is new machinery, and the draft's framing understated the implementation; (ii) the draft's "straightforward assignment scan (already needed for the borrow checker)" is **withdrawn** — it specified a SECOND analysis over the same body, free to disagree with the §4.6 whitelist, and a disagreement is a false-`proven` frame-condition miss. There is one fail-closed walk, and the borrow checker's may-analysis is not it.)* Domain: `i64` locals (the v1 fragment); a float loop is `Unsupported` (Z3 Real loop invariants are a follow-on).

## 6. Error codes

| Code | Trigger | Message |
|---|---|---|
| **E1102** | *(deferred to Q4 for loops — corrected 2026-07-31)* templates-only v1 has no mechanism that can establish a loop bound is *violable*, so v1 never emits E1102 for a loop fn (straight-line E1102 from R9 is unchanged). If Q4 lands a reachability mechanism, the message is: | `` @[verify] bound `{pred}` is violated for `{fn}` at {init} after {n} iterations (SMT counterexample) `` |
| **W1103** | (reused) loop outside v1 (nested / break / float / no invariant found in time) **or bound possibly violable** (templates exhausted — v1 cannot tell these apart) | `` @[verify] on `{fn}`: could not infer a loop invariant (v1: single linear i64 loop); runtime gate still applies `` |

No new error band — R9b reuses E1102/W1103. (The *reason* string distinguishes "out-of-fragment" from "invariant search exhausted".) v1's loop outcomes are exactly **{proven, W1103}**.

## 7. Invariants touched

- **I-2 (interp is reference):** the loop proof is an additional static guarantee; a `proven` loop runs identically. **Preserved** — the invariant is Z3-checked, never trusted (§4.3) *and*, in v1, the runtime `@[verify]` check is retained rather than elided, so "runs identically" does not depend on the proof being correct. *(Corrected 2026-07-31: as drafted this row was true only if the proof was right — which is the thing in question, since a `Proven` normally elides the check in both engines.)*
- **Runtime-check elision (`verify::Discharged.verify_fns`):** v1 loop proofs do **not** enter the discharged set (§4.3 decision); `axon run` / `axon build` behaviour is unchanged by a loop proof. Asserted by a test (§9) so the decision cannot be made accidentally by wiring into `prove_verify_bounds`.
- **Checked arithmetic in both engines (dependency, added 2026-07-31):** loop proofs run over unbounded ℤ and are sound only because an i64 overflow panics *before* the checked postcondition (`smt.rs:472-478`). Every arithmetic operation in an admitted loop body must stay runtime-checked in BOTH interp and native; making any admitted operation wrapping/unchecked breaks loop-proof soundness. **Assumption recorded, not merely inherited.**
- **TCB (R23 certificates) — accepted regression, explicit:** v1 loop proofs trust Z3 + the §4.6 transition encoder **directly**; they emit no R23 proof certificate, so `axon-certcheck` cannot independently re-derive the verdict solver-free. This is a knowing widening of the trusted base relative to R23's *"get Z3 out of the TCB"* posture, accepted only because it is paired with the §4.3 decision to retain the runtime gate — v1 is never both uncertified and unchecked. Certification is a follow-on (Q7) and is a precondition for elision.
- **Determinism:** §4.4 (fixed template order + seeded/timeout-bounded CHC).
- **Dependency isolation:** still entirely under `--features smt`; the default build never links Z3. v1 (templates-only) adds no new dependency. **Preserved** — but note the correction (2026-07-31): the draft claimed the CHC fallback rode on `z3::fixedpoint` "same crate, no new dep"; that binding does not exist in z3 0.12, so a future CHC slice must re-justify this claim per its chosen mechanism (Q4 — a `HORN`-logic `Solver` keeps the no-new-dep property AND invariant extraction via `Model::get_func_interp` on sat, but yields no counterexample *trace* on unsat, only a refutation proof term; raw `z3-sys` FFI needs no new bindings — z3-sys 0.8.1 already declares `Z3_fixedpoint_*` — but is a new unsafe-*call* surface).

## 8. Test plan (maps 1:1 to §4.5)

Red test first: **`smt_proves_loop_accumulator_bound`** — `@[verify(value >= 0)]` on the `clamp_decay` body (§3) proves via an inferred `acc >= 0` invariant; an off-by-one variant (`acc = acc - 1` without the `acc > 0` guard) must **never be proven** — v1 yields W1103 (no candidate survives the Hoare checks) and the runtime `@[verify]` gate still catches the violation (exit 3). *(Corrected 2026-07-31: the draft's "yields E1102 with a counterexample" half is unimplementable in a templates-only v1 — deferred with Q4.)* Fails today (loops → W1103).

- [ ] **Unit (smt):** modified-variable scan; §4.6 transition lowering (assign sequence, statement-`if` per-variable `ite`, out-of-form body → None); the three Hoare VCs built for a candidate invariant; a surviving template → proven, all-fail → next template → templates exhausted → Unsupported.
- [ ] **Proof (template):** a monotone accumulator / bounded counter / floored decay → proven, with the invariant reported.
- [ ] **No-template-fits:** a loop outside the template family → W1103 Unsupported (no false proof, no hang). *(The CHC-fallback proof test is deferred with Q4.)*
- [ ] **Violable-is-never-proven:** an unbounded accumulator vs a cap → W1103 (not `proven`), and a differential run confirms the runtime gate fires (exit 3). *(The static E1102-with-counterexample test is deferred with Q4.)*
- [ ] **Frame-condition negatives (added 2026-07-31, §4.2/§4.6):** three loops that must each yield **W1103 and must never be `proven`** — (i) a loop reassigning a **parameter**, (ii) a loop containing a place assignment `xs[i] = …` (`Expr::AssignTo`), (iii) a loop containing a shadowing inner `let`. Each is a false-proof route that needs no Z3 bug.
- [ ] **Differential (I-2), adversarial:** a randomised differential fuzz over **generated** loop bodies (reusing the R1f harness, `scripts/fuzz_parity.sh`) finds no `proven` loop whose interpreted run violates the bound. This **replaces** the corpus-only differential (a corpus samples shapes a human wrote; §4.7). The corpus run is kept only as a cheap smoke pass.
- [ ] **Elision decision (§4.3):** a `proven` loop's runtime `@[verify]` check is **retained** — assert the fn name is absent from `verify::Discharged.verify_fns` and that `axon run` output/exit is identical with and without `--features smt`.
- [ ] **ℤ-vs-i64 (§4.3):** a `proven` loop whose accumulator overflows i64 panics (exit 101) on **both** interp and native, and never returns a bound-violating value.
- [ ] **Boundary:** nested loop / `for` loop / `break` / float loop / any call in the body → W1103, not a false proof.
- [ ] **Isolation:** default build (no `smt`) unaffected — proven by the standard gate.

## 9. Acceptance criteria (the done gate)

- [ ] `smt_proves_loop_accumulator_bound` passes under `--features smt` (proof half + violable-variant-is-W1103-not-proven half; *the counterexample half is deferred with Q4 — corrected 2026-07-31, see §4.5 row 3*).
- [ ] A loop no template fits yields `W1103 Unsupported` (no false proof, no hang). *(The former "at least one CHC-fallback proof passes" criterion is dropped with the Q4 deferral — corrected 2026-07-31: it was unbuildable as specified, `z3::fixedpoint` does not exist in z3 0.12.)*
- [ ] The three §8 frame-condition negatives (parameter reassignment, `Expr::AssignTo`, shadowing inner `let`) each yield W1103 and are never `proven`.
- [ ] A randomised differential fuzz over generated loop bodies (R1f harness) finds no `proven` loop whose interpreted run violates the bound.
- [ ] A `proven` loop's runtime `@[verify]` check is **retained** (not in `Discharged.verify_fns`), asserted by a test — the §4.3 decision is gated, not incidental.
- [ ] `axon verify` reports the inferred invariant on a `proven` loop (explainability).
- [ ] **Fragment coverage measured before implementation (added 2026-07-31, feeds Q8):** run the proposed v1 fragment predicate over (a) every loop in `examples/**.ax` and (b) a sample of machine-authored `.ax` (`examples/goals/*.ax`, `AXON_INTENT_GEN=1 axon intent compile` output), and record the hit count. **The v1 fragment must admit ≥ N real loops** (N fixed when the count lands). Three tree facts predict a near-zero count and make this a gate rather than a formality: `@[total]` **refuses** `while` and tells the author to use a bounded `for` (`checker.rs:2248-2266`, E1208) while v1 excludes `for` (Q5); the corpus already leans `for` (~195 vs ~124 grep hits in `examples/**.ax`); and R9's expression fragment excludes Div/Rem (`smt.rs:454-456`), which knocks out the tree's real accumulator loops (Collatz `x = x / 2`, `examples/algorithms.ax:33-38`; Newton sqrt, `examples/error_handling.ax:27-30`) even as `while`.
- [ ] **Proof status reaches the approval artifact:** per-function proof status + the inferred invariant appear in `axon ast review --json` (`axon-ast-review/1`, `main.rs:4938`) and in the provenance/audit record alongside the solver version, with W1103 functions flagged (*"these bounds are enforced only at runtime"*). `ROADMAP.md` §2.4 makes the typed AST the artifact a human approves; a count-only stderr line (`main.rs:3643`) is not attributable and does not survive the run.
- [ ] default build green without `smt` (Z3 never linked).

R9 may rise 88% → ~92% on this slice (single linear-loop invariants). *(Corrected 2026-07-31: the draft projected 78% → ~85%, but REQUIREMENTS.md already scores R9 at 88 — work landed after the 2026-06-03 draft.)* Nested loops, `for`-loop encoding (Q5), CHC synthesis + static loop-E1102 counterexamples (Q4), float-loop Real invariants, and inter-procedural summaries remain explicitly out.

## 10. Performance budget

Template enumeration is O(small fixed family × 3 Z3 calls). (A future CHC fallback, if built per Q4, must be bounded by a wall-clock timeout — default a few seconds, documented machine-sensitive.) `axon verify` on a non-loop fn is unchanged (R9 path).

*(Corrected 2026-07-31: "No effect on `build`/`run`/`check` (Z3 is verify-only)" is **false** in an `--features smt` binary — `smt::discharge` runs on every `axon run` (`main.rs:3605`) and every `axon build` (`main.rs:4616`), both printing "SMT discharged N runtime obligation(s)". So R9b's cost lands on the normal compile path, and the only bound today is R9's **per-query** `AXON_PROOF_TIMEOUT_MS` (`smt.rs:617-639`, 10 ms floor because Z3 wedges below that). Nothing bounds the product |verified fns with loops| × |template family| × 3 VCs × worst-case per-query timeout — trivially reached by a generator emitting many `@[verify]`-annotated loops, no malice required, just volume.)*

**Per-program budget (required).** Give the loop path a per-program cap (query count and/or wall clock) with **deterministic degradation to W1103** on exhaustion. To keep §4.4 determinism, the cap must be a **fixed per-function query quota, not a shared pool** — a shared pool makes "which functions got proven" depend on evaluation order and on how much budget earlier functions consumed, which is exactly the kind of order-sensitivity §4.4 forbids. The loop path is entered only for functions carrying a `@[verify]` attribute (true today; pinned here). Note that the §4.3 v1 decision — loop proofs report via `axon verify` and never enter `Discharged` — already keeps the *elision* cost off the compile path; the *search* cost still needs this cap.

## 11. Rollout & rollback

Additive under `--features smt`: extends `smt.rs` with a loop-invariant module; the straight-line path is untouched. Rollback = the loop branch returns `W1103` (today's behavior). No surface, no default-build, no runtime change.

## 12. Open questions

- **Q1 (template family breadth):** start with intervals + 2-variable octagon relations (covers accumulator/counter/decay). Widening to general `Σcᵢvᵢ` polyhedra is a follow-on if real `@[verify]` loops need it — measure before broadening. *(Amended 2026-07-31: "measure against actual demo bodies" needs a stated population. Hand-written demo bodies are a shrinking sample if `.ax` is a machine-generation IR (ROADMAP §2.1) — the shape distribution is then set by the generator and by the annotations the project pushes authors toward, not by what the encoder happens to support. The §9 coverage measurement covers both populations (corpus **and** generated `.ax`); Q1 resolves against that, and Q6 may make template breadth a secondary concern entirely.)*
- **Q2 (CHC timeout value):** *contingent on Q4.* If a CHC path lands: pick a default that proves the test corpus without hanging CI; make it overridable (`AXON_SMT_TIMEOUT_MS`) like `AXON_MAX_DEPTH`. Confirm the timeout is deterministic *enough* on the gate machine (else gate the CHC test behind a slow-tests flag and keep only the template proofs in the standard `smt` test set).
- **Q3 (reporting the invariant):** print the inferred predicate (`# proven via invariant: acc >= 0 ∧ acc <= start`) — high explainability value, low cost. In v1 for template proofs (the predicate is known). (The draft's "CHC-synthesized invariants reported best-effort" clause is moot until Q4 resolves — and note a `HORN`-logic `Solver` path CAN report them: on sat, `Model::get_func_interp` on the invariant predicate yields the synthesized invariant. *Corrected 2026-07-31 — the same-day first correction wrongly said "could not report them at all".*)
- **Q4 (CHC fallback + static loop-E1102 mechanism — RE-OPENED 2026-07-31):** the draft's resolved (c) path relied on `z3::fixedpoint`, which does not exist in the pinned z3 0.12 crate (the Rust `z3` crate has never wrapped the `Z3_fixedpoint` C API). Q4 now also owns the **static violability path**: templates-only v1 cannot distinguish "violable" from "no template fits" (§4.5 row 3), so E1102-for-loops (counterexample = init + iteration count) requires whatever reachability mechanism Q4 picks. Options: (i) stay templates-only permanently (current v1 posture — loop outcomes stay {proven, W1103}, runtime gate carries violations); (ii) assert quantified Horn clauses into a `Solver` created for the `HORN` logic (`Solver::new_for_logic`) — no new dep, and on **sat** the synthesized invariant IS extractable via `Model::of_solver` + `Model::get_func_interp` (Q3 reporting preserved; the first same-day correction wrongly claimed otherwise); on **unsat** only a refutation proof term is available (`Solver::get_proof`), not an init+iteration-count trace, so E1102's counterexample needs to be reconstructed (e.g. a bounded unroll after unsat) or its message weakened; (iii) raw `z3-sys` FFI to `Z3_fixedpoint_*` — z3-sys 0.8.1 already declares the full extern surface (no new bindings), full capability incl. Spacer trace derivation, but a new unsafe-call surface that must be budgeted and covered by the §7 isolation claim; (iv) a template-independent bounded-model-checking slice (unroll k iterations, sat-check the violated post) — gives true E1102 counterexamples for violations within depth k, degrading beyond-k to W1103 (so the E1102 promise must stay conditional, never "violable ⇒ E1102" unconditionally). Resolve before any CHC/E1102 work starts; v1 does not block on it.
- **Q5 (`for`-loop encoding):** `Expr::For` is a distinct AST node with no existing while-desugar (see §4.2 correction). Decide whether to add a desugar inside the SMT encoder or a shared lowering so `for v in a..b` loops become provable; until then `for` → W1103. *(Raised in priority 2026-07-31: this is not only a coverage gap but a **fragment/annotation conflict** — `@[total]` refuses `while` (E1208) and directs authors to bounded `for`, so the termination-checked code, i.e. the code most likely to carry a safety-relevant `@[verify]`, is structurally excluded from v1. See Q8.)*
- **Q6 (author-supplied invariant hint — proposed follow-on slice, opened 2026-07-31):** §4.3's own architecture is *"propose cheaply, verify with the one trusted mechanism"*, but v1 hardwires a fixed template family as the **proposer**. A declared, checked hint — e.g. `@[invariant(acc >= 0 && acc <= start)]` on the fn or loop — would be **tried first**, discharged by the *identical* three Hoare VCs of §4.6, and fall through to the template family and then to W1103 on failure. Added trusted code: essentially none (the VCs are built regardless); a wrong hint degrades to exactly the v1 miss outcome. It is the same firewall the project already runs for AI-authored compiler passes (R10 Layer-3: passes as untrusted **data**, gated by a verifier), and §4.3's soundness argument transfers verbatim — the hint is untrusted input on the same footing as a template candidate, never a trusted oracle. It also upgrades Q3's explainability: the reported invariant becomes a **declared, reviewable contract attributable to the code's author**, rather than compiler-inferred trivia the reviewer is seeing for the first time — which matters most under the §4.7 authorship assumption, where the code's author is capable of supplying it in the same pass. **Open:** surface syntax, whether the hint attaches to the fn or the loop, and whether a *failed* hint is W1103 or a louder diagnostic (a hint that does not hold is a stronger signal than no hint at all). Not in v1; does not block it.
- **Q7 (R23 certification of loop proofs — opened 2026-07-31):** R23-proof-certificates is ✅ Landed 100% (`crates/axon-certcheck`, `cert_gate.rs`) on the premise *"don't trust the prover — trust a small checker of the certificate it emits"*, yet R9b as drafted referenced none of it while adding new Z3-trusting machinery. A loop invariant is among the easiest things to certify: once a candidate `I` is fixed, the artifact is `(I, refutations of the three implications)`, checking is strictly cheaper than synthesis, and three VCs over a linear i64 fragment are the exact shape `certcheck`'s bounded Farkas-multiplier search over linear-inequality hypotheses already handles. **Open:** certificate schema for the three-VC bundle, and whether the R23 gate treats a loop certificate as advisory or fail-closed. §7 records v1's accepted TCB regression in the meantime; **a validated certificate is a precondition for ever eliding a `proven` loop's runtime check** (§4.3).
- **Q8 (does the v1 fragment admit anything real? — opened 2026-07-31):** the v1 fragment was drawn around what R9's encoder already does, not around the code under test, and three independent constraints predict it proves ≈ nothing in this tree: `@[total]` forbids `while` while v1 excludes `for` (Q5); the corpus leans `for`; and Div/Rem are outside R9's expression fragment, excluding the real accumulator loops (Collatz, Newton sqrt) even as `while`. The §3 flagship `clamp_decay` is a body written to fit the fragment. **Resolve against the §9 coverage measurement, before implementation starts.** If the count is near zero, the scope choice is to pull **Q5 (`for` lowering** — which also reconciles the fragment with `@[total]`**)** and **guarded Div/Rem** (a `divisor != 0` side condition discharged as part of the VC, keeping the operation total) into v1 and re-scope `while` to secondary — rather than shipping a slice whose acceptance gate is satisfiable only by its own example. Not decided here.
