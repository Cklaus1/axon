# Tech Spec — R23: Proof Certificates + Minimal Checker ("get Z3 out of the TCB")

**Spec ID:** `R23-proof-certificates`
**Status:** 📝 Draft (2026-06-26)
**Implements:** `VISION_OS.md` §4.1 (pillar "what can be TRUSTED — the proof chain & trust roots")
and gap **G2**. Closes the #1 assurance gap: today the R20 capability proofs and the R21 admission
gate **trust Z3's verdict directly** — a ~500K-line unaudited solver sitting in the TCB, contradicting
"minimal trusted base." R23 makes the prover emit a **proof certificate** that a **tiny, auditable,
independent checker** validates, so **Z3 leaves the trust root**: you trust ~hundreds of lines, not
the solver.
**Audience:** an implementer who builds *strictly* against this document and reads only it.

> **Read this framing first.** The unsoundness we defend against is **the prover lying** (a Z3 bug, a
> tampered solver, or a malicious drop-in returning "UNSAT" for a satisfiable query). The defense, from
> seL4: *don't trust the prover — trust a small checker of the certificate it emits.* R23 builds (a) a
> certificate **format**, (b) the prover **emitting** one for the obligations R20/R21 already prove,
> and (c) a **minimal checker** that independently re-derives the verdict from the certificate without
> calling any SMT solver. The checker, not Z3, becomes the trusted link. R23 does **not** invent new
> proofs — it adds verifiable evidence to the ones that exist.

---

## §0 — Requirement → Section → Acceptance-check index (the build gate verifies none are skipped)

| Req | What | Spec § | Pinned acceptance check (test name) |
|---|---|---|---|
| **A1** | Real user journey + smoke test through the actual CLI | §5, §7 | `acc_a1_smoke_prove_check_verify` |
| **A2** | Real runnable example artifact (a real obligation + its certificate) | §5.6, §7 | `acc_a2_example_obligations_certified` |
| **A3** | Quickstart whose exact commands are executed by a test | §9, §7 | `acc_a3_quickstart_commands_execute` |
| **A4** | Hermetic, isolated checking; the checker NEVER invokes an SMT solver | §4.4, §7 | `acc_a4_checker_is_solver_free` |
| **A5** | Deterministic (same obligation ⇒ byte-identical certificate; check is pure) | §4.5, §7 | `acc_a5_certificate_byte_identical` |
| **A6** | Integrity: the checker is SOUND — accepts only valid certificates, rejects every forgery | §4.3, §7 | `acc_a6_checker_rejects_forged_and_mutated` |
| **Core** | A valid certificate for a TRUE obligation checks ✓ | §4.3, §7 | `valid_certificate_accepted` |
| **Core** | No certificate exists / checks for a FALSE obligation (soundness) | §4.2, §7 | `false_obligation_has_no_valid_certificate` |
| **Core** | The checker is independent of Z3 (would catch a lying solver) | §4.3, §7 | `lying_solver_is_caught_by_checker` |
| **Core** | R20/R21 fail closed when a certificate is absent/invalid under `--require-certificates` | §4.6, §7 | `require_certificates_fails_closed` |
| **Gate** | The acceptance gate fails if any check above is missing/stubbed | §10 | `scripts/r23_acceptance_gate.sh` |

The build is **not done** until every row's check exists, was seen to fail first, and now passes.

---

## §1 — Overview & scope

### 1.1 What it does
R23 adds **checkable evidence** to the obligations Axon already proves:

1. **Certificate format.** A self-contained, serialized `ProofCertificate` describing *why* an
   obligation is valid, in a form re-checkable **without any SMT solver**.
2. **Prover emission.** Extend the R20/R21 proving path so that when an obligation is discharged, it
   also emits a `ProofCertificate` (alongside the existing `Proven` verdict).
3. **Minimal checker.** A small, auditable verifier (`certcheck`) that takes `(obligation,
   certificate)` and independently returns `Valid | Invalid{reason}` — **calling no solver**, touching
   only arithmetic/boolean evaluation it implements itself.
4. **Fail-closed integration.** Under `--require-certificates`, R20's TCB gate (E1610/attestation) and
   R21's admission gate accept a `Proven` obligation **only if** its certificate checks. A missing or
   invalid certificate ⇒ fail closed (no discharge, no admission).

The trust shift: before R23, "proven" = "Z3 said UNSAT" (trust Z3). After R23, "proven" = "a tiny
checker re-derived the verdict from a certificate" (trust the checker; Z3 is now an untrusted *oracle*
that merely *suggests* certificates).

### 1.2 The certified fragment (scope of what R23 proves-with-evidence)
R23 covers exactly the obligation shapes R20/R21 already discharge — **the quantifier-free linear
integer + boolean fragment**:
- **Bound obligations:** `∀ params. R(params) OP K` (the `@[verify]`/refinement-return shape), where
  `R` is built from `+ - *`(by constant)`, min/max/abs` over i64 params and `K` is an integer.
- **Boolean/relational obligations:** the R20 mint shape — implications and conjunctions over boolean
  capability atoms, and linear comparisons relating output to params (`child.cap ≤ parent.rem`,
  `child.net ⇒ parent.net`).

The defended claim is **∀-inputs validity** (the negation is UNSAT). The certificate must let the
checker confirm UNSAT-of-negation *constructively* (§4.1).

### 1.3 What it explicitly does NOT do (out of scope for R23)
- **No new obligations / no wider logic.** R23 certifies the *existing* fragment only. Nonlinear
  arithmetic, quantifier alternation, arrays, bitvectors, reals beyond the shipped f64 path, and the
  deferred struct-whole refinements are **out** — for those, R20/R21 keep their current runtime
  fallback, and `certcheck` returns `Unsupported` (which under `--require-certificates` is **fail
  closed**, treated like Invalid for admission).
- **No replacing Z3 as the *oracle*.** Z3 still *finds* the proof; R23 only makes it *checkable*. Z3
  stays in the *generation* path (untrusted); it leaves the *trust* path.
- **No verified checker (yet).** The checker is *small and auditable* (the assurance argument is "you
  can read all of it"), not *formally verified*. A machine-checked checker is a v3 stretch
  (`VISION_OS` §4.1). Documented honestly.
- **No codegen-trust / trusting-trust defense.** That is the LLVM link (`VISION_OS` G3), a different
  spec. R23 is the *prover* link only.
- **No certificate signing / PKI.** A certificate is content-addressed and self-checking, not signed;
  combine with R20 attestation for provenance. A6 documents what is vs isn't authenticated.

### 1.4 Persona / ICP
A **security auditor / TCB owner** who must be able to say, truthfully, "our containment proofs do not
depend on trusting a 500K-line solver — they depend on this ~N-hundred-line checker, which I have
read." Secondarily, the R20/R21 pipelines, which consume certificates programmatically.

### 1.5 Interface & tech constraints
- **Interface:** a CLI `certcheck` (subcommands `prove`/`check`/`explain`), plus a library crate.
- **Language/deps:** Rust, new workspace crate `crates/axon-certcheck`. The **checker** depends on
  **nothing but `std` + `sha2`** (no `z3`, no solver, no heavy deps — this is an enforced acceptance
  check, A4). The **prover-emission** side lives behind the existing `smt` feature (it may use Z3 to
  *generate*). Reuses R20/R21 obligation types.
- **Perf/security:** the checker is pure, total, and solver-free; it must be **sound** (never accept an
  invalid certificate) even at the cost of completeness (it may return `Unsupported`, never a false
  `Valid`).

### 1.6 Domain-specific risks (what matters most here)
- **A lying/buggy solver** silently certifies a false obligation. → independent re-derivation by the
  checker; `lying_solver_is_caught_by_checker`.
- **A forged certificate** that the checker wrongly accepts. → the checker re-derives, doesn't trust
  the certificate's claims; `acc_a6_checker_rejects_forged_and_mutated`.
- **Completeness creep masking unsoundness:** the checker "accepts" by failing to actually verify. →
  the checker must *constructively confirm* UNSAT, and the gate's adversarial corpus forces rejections.
- **Scope drift:** certifying logic outside the sound fragment. → `Unsupported` is fail-closed, never
  a silent `Valid`.

---

## §2 — Architecture & modules

New crate `crates/axon-certcheck/`. **The checker is the trusted artifact: keep it small, pure, total,
and solver-free.** Generation (which may use Z3) is a separate, untrusted module behind `smt`.

```
crates/axon-certcheck/src/
  obligation.rs    Obligation IR: the quantifier-free LIA+bool fragment (§3.1).          [PURE]
  certificate.rs   ProofCertificate format: parse/serialize/canonicalize (§3.2).         [PURE]
  checker.rs       THE TRUSTED CHECKER: (obligation, cert) → Valid|Invalid|Unsupported.  [PURE, solver-free — audited]
  eval.rs          The checker's own LIA+bool evaluator (no external solver).            [PURE — audited]
  emit.rs          Prover-side: discharge an obligation + EMIT a certificate (uses Z3).  [I/O/solver — behind `smt`, UNTRUSTED]
  cli.rs           prove / check / explain dispatch + human output + exit codes.         [I/O — thin]
  lib.rs           Public API; checker re-exported with NO solver dep on its path.       [—]
  main.rs          fn main() → cli::run(env::args).                                       [I/O — thin]
crates/axon-certcheck/tests/
  acceptance.rs    The A1–A6 + Core checks (named exactly per §0).
  corpus/          A forgery/mutation corpus (real .cert files) the checker must REJECT.
examples/proofs/
  mint_o2.obl + mint_o2.cert     A real obligation + its valid certificate (A2).
  carve.obl   + carve.cert       The R22/R20 `_ <= avail` relational obligation, certified.
scripts/r23_acceptance_gate.sh
README-axon-certcheck.md         Quickstart whose commands a test executes (A3).
```

**Dependency graph (acyclic; → = uses). The CHECKER's subtree must not reach a solver:**
```
main → cli → {checker → {obligation, eval, certificate},   // TRUSTED path — std + sha2 ONLY
              emit → (z3, smt-feature)}                     // UNTRUSTED path — generation only
obligation, certificate, eval, checker : depend on std + sha2 only   [ENFORCED by acc_a4]
emit : depends on z3 (behind `smt`)
```
**Enforced rule (A4, `acc_a4_checker_is_solver_free`):** the `checker`/`eval`/`obligation`/
`certificate` modules — and the `certcheck check` code path — must compile and run with the crate
built **without** the `smt`/`z3` feature, and a test asserts no `z3` symbol is reachable from the
checker. The checker can validate a certificate on a machine that has never had Z3 installed.

---

## §3 — Data model

### 3.1 `Obligation` (the thing being proved; the checker's input #1)
A canonical IR for the certified fragment. The prover lowers an R20/R21 obligation into this; the
checker reads only this (not the original Axon AST).
```
Obligation {
    id:       String,           // stable name, e.g. "principal_mint/O2" or "carve/return"
    vars:     Vec<Var>,         // the free variables (params), each Int or Bool
    claim:    Formula,          // the ∀-claim asserted true for all assignments of vars
}
Var   = { name: String, sort: Sort }      // Sort = Int | Bool
Formula =
  | Bool(bool)
  | BVar(name)                              // a boolean variable
  | Not(Formula)
  | And(Vec<Formula>) | Or(Vec<Formula>)
  | Implies(Formula, Formula)
  | Cmp(Op, Term, Term)                     // Op ∈ {Lt,Le,Gt,Ge,Eq,Ne}
Term =
  | Int(i64)
  | IVar(name)
  | Add(Term,Term) | Sub(Term,Term) | MulConst(i64,Term)   // linear only: * is by an integer constant
  | Min(Term,Term) | Max(Term,Term) | Abs(Term)            // the shipped bound builtins
  | Ite(Formula, Term, Term)                                // for clamp/remaining-style terms
```
Serialized form (`.obl`, JSON, schema `axon-obl/1`): the tree above, field-named. **Linearity is a
validation invariant:** `MulConst` is the only multiplication; a `Mul(Term,Term)` of two non-constant
terms is rejected at parse → `Unsupported` (keeps the checker's decision procedure decidable and small).

### 3.2 `ProofCertificate` (the evidence; the checker's input #2; `.cert`, JSON, `axon-cert/1`)
The certificate must let the checker confirm **the negation of `claim` is unsatisfiable** without
search. The R23 fragment is quantifier-free LIA+bool over a **bounded, enumerable** structure, so the
certificate is a **case-split refutation tree**: it reduces the negated claim to a finite set of
**linear-arithmetic contradictions**, each of which the checker confirms by a **Positivstellensatz-
style nonnegative-combination witness** (a Farkas certificate) — no solver needed.
```
ProofCertificate {
    schema:        "axon-cert/1",
    obligation_id: String,        // must equal the Obligation.id it certifies
    obligation_digest: String,    // "axsha256:"+sha256(canonical obligation) — binds cert to obligation
    refutation:    Refutation,    // the proof the NEGATED claim is unsatisfiable
    cert_digest:   String,        // "axcert1:"+sha256(canonical(refutation))
}
Refutation =
  | BoolCase {                    // split on a boolean var: BOTH branches must refute
        var: String, on_true: Refutation, on_false: Refutation }
  | IteCase {                     // split on an Ite condition occurring in the formula
        cond: Formula, on_true: Refutation, on_false: Refutation }
  | LinearContradiction {         // a set of linear ≤/< facts with a Farkas witness summing to 0 < 0
        facts: Vec<LinFact>,      // each derived from the (now branch-resolved) negated claim
        coeffs: Vec<i64> }        // nonnegative multipliers; Σ coeffs·facts ⇒ 0 ≤ -1  (contradiction)
LinFact = { terms: Vec<(i64 /*coeff*/, String /*ivar*/)>, op: Le|Lt, constant: i64 }   // Σ cᵢ·xᵢ  op  k
```
**Why this is checkable without a solver:** after the boolean/Ite case-splits fully resolve every
`Not/And/Or/Implies/Ite`, each leaf is a conjunction of linear facts whose unsatisfiability is
witnessed by **nonnegative coefficients producing `0 < 0`** (Farkas' lemma for the rational/integer
linear fragment). The checker just (a) walks the case tree confirming the split is exhaustive, (b)
checks each leaf's coefficients are nonnegative and the weighted sum of the `LinFact`s is a literal
contradiction. That is pure arithmetic — a few hundred lines.

### 3.3 Checker outputs
```
CheckResult = Valid | Invalid { reason: String } | Unsupported { reason: String }
```
`Invalid` = the certificate does **not** prove the claim (forged, mutated, wrong obligation,
non-exhaustive split, bad Farkas witness). `Unsupported` = the obligation is outside the R23 fragment
(nonlinear, etc.) — **fail-closed** for admission purposes.

### 3.4 Exit codes (consistent with the Axon scheme)
```
0  Valid (cert proves the obligation)
2  Malformed / usage (bad .obl/.cert syntax)
4  Unsupported (obligation outside the certified fragment)
8  Invalid (the certificate does NOT prove the claim — fail closed)
```

---

## §4 — Core logic / algorithms

### 4.1 What "valid" means (the checker's specification)
A certificate is **Valid** iff it constructively shows `¬claim` is unsatisfiable over the obligation's
vars. The checker confirms this by structural recursion on the `Refutation`:
1. **Bind the negated claim.** Compute `neg = Not(claim)` and push it down to a set of literals using
   ordinary boolean normalization the checker implements (`eval::nnf` + literal extraction). *No
   search* — just rewriting.
2. **Case tree exhaustiveness.** For a `BoolCase{var}` the checker requires both `on_true` (assume
   `var=⊤`) and `on_false` (assume `var=⊥`) refutations — together exhaustive. For `IteCase{cond}`
   likewise on `cond`'s truth. The checker substitutes the assumption and continues. An incomplete
   split (a missing branch, or splitting on a var not in the formula) → `Invalid`.
3. **Leaf contradiction (Farkas).** At a `LinearContradiction` leaf, the checker:
   - confirms every `LinFact` is **entailed by the branch-resolved negated claim** (it appears, after
     substitution and normalization to `Σ cᵢxᵢ op k`, among the leaf's hypotheses — the checker
     re-derives the hypothesis set; it does not take the cert's word);
   - confirms every `coeff ≥ 0`;
   - computes `Σ coeffᵢ · factᵢ` as a single linear inequality and confirms it reduces to `0 ≤ -1`
     (or `0 < 0`) — a literal contradiction. Strict/non-strict handled per Farkas (at least one strict
     among combined facts when the result is `0 ≤ 0`-strict).
   If all leaves contradict and the tree is exhaustive ⇒ `¬claim` unsatisfiable ⇒ **Valid**.
4. **Binding.** Before any of the above, confirm `cert.obligation_digest == sha256(obligation)` and
   `cert.obligation_id == obligation.id` and `cert.cert_digest` re-hashes — else `Invalid` (the cert
   is for a different obligation). This is what makes a swapped/forged cert fail.

The checker is **sound by construction**: it only ever returns `Valid` after *itself* deriving a
contradiction from the negated claim; it never trusts a claim in the certificate it didn't re-derive.
Incompleteness is acceptable (`Unsupported`/`Invalid` on anything it can't constructively confirm).

### 4.2 Soundness obligation (the property the whole spec exists for)
**There is no Valid certificate for a false obligation.** If `claim` is *not* ∀-true, some assignment
satisfies `¬claim`; then some case-tree leaf has a satisfiable linear system, for which **no
nonnegative Farkas combination yields `0<0`** (Farkas completeness for linear arithmetic) — so the
checker rejects every candidate certificate at that leaf. Test `false_obligation_has_no_valid_
certificate` constructs a false obligation and asserts *every* attempted certificate (including ones
the prover would never emit) is `Invalid`.

### 4.3 The checker is independent of the solver (the trust shift)
`lying_solver_is_caught_by_checker`: feed the checker a certificate whose `refutation` claims a
contradiction that isn't one (e.g. a Farkas witness with a coefficient that doesn't sum to `0<0`, or a
non-exhaustive case split) — *as if produced by a malicious solver that returned UNSAT for a SAT
query* — and assert `Invalid`. The checker re-derives arithmetic itself, so a lying solver cannot make
it accept. This is the whole point: the solver is now an untrusted oracle.

### 4.4 Hermetic, solver-free checking (A4)
- The `certcheck check` path and the `checker`/`eval` modules build and run with the crate compiled
  **without** the `smt` feature. `acc_a4_checker_is_solver_free` (a) builds the crate
  `--no-default-features` and runs a check, and (b) asserts (via `cargo tree`/symbol grep in the test)
  that no `z3` crate is in the checker's dependency closure.
- Each `check` runs against a single `(obligation, certificate)` with bounded work (the case tree is
  finite; a depth/size limit `MAX_CERT_NODES`, default 100000, bounds a maliciously huge cert →
  `Invalid{"certificate too large"}`, fail closed — no unbounded work, no DoS).

### 4.5 Determinism (A5)
- The certificate is a pure function of the obligation + the prover's deterministic strategy; emission
  under the fixed strategy yields a **byte-identical** `.cert` across runs (`acc_a5_certificate_byte_
  identical` runs `prove` twice and diffs). The checker is a pure total function of `(obligation,
  cert)` — no clock/random/threads. Canonicalization (field order, integer formatting) is fixed and
  specified so digests are stable.

### 4.6 Integration with R20/R21 (`--require-certificates`) — fail closed
- **R20:** when built with R23, `smt::discharge` and `check_mint_tcb_obligation` additionally emit a
  certificate for each `Proven` obligation and run `certcheck::check`. Under `--require-certificates`
  (or env `AXON_REQUIRE_CERTS=1`), an obligation counts as discharged **only if** its certificate is
  `Valid`; `Invalid`/`Unsupported`/absent ⇒ the obligation is **not** discharged → its runtime check
  stays armed, and for the TCB mint obligation specifically ⇒ E1610 fail-closed.
- **R21:** the admission gate, under `--require-certificates`, admits a program's bound only if the
  effect-subset obligation carries a Valid certificate; else `Verdict::Denied` (fail closed, no run).
- **Default (no flag):** behavior is unchanged (certificates emitted+checked in the background, logged,
  but not gating) so adoption is incremental and byte-compatible. `require_certificates_fails_closed`
  proves the gated path rejects an obligation with a missing/invalid cert.

---

## §5 — Public API / interface contract

### 5.1 Library API (`lib.rs`)
```
// THE TRUSTED CHECKER — no solver on this path:
pub fn check(obligation:&Obligation, cert:&ProofCertificate) -> CheckResult;

// Generation (behind `smt`, untrusted):
#[cfg(feature="smt")]
pub fn prove(obligation:&Obligation) -> Result<ProofCertificate, ProveErr>;  // uses Z3 to FIND, emits a cert

// Parsing/serialization (pure):
pub fn parse_obligation(s:&str) -> Result<Obligation, ParseErr>;
pub fn parse_certificate(s:&str) -> Result<ProofCertificate, ParseErr>;
```

### 5.2 CLI (`certcheck`; every subcommand has `--help`; legible output)
```
certcheck prove <obligation.obl> [--out cert.cert]          [requires the `smt` build]
    Find a proof via the oracle and EMIT a certificate. Prints "✓ certified <id>" or "✗ could not
    prove (Unsupported|SAT counterexample)". Exit 0 / 4 / 8.

certcheck check <obligation.obl> <certificate.cert>          [SOLVER-FREE — the trusted op]
    Independently validate the certificate against the obligation. Prints, in plain English:
    "✓ VALID — the obligation holds for all inputs; verified WITHOUT a solver (checker only)."
    or "✗ INVALID at <node>: <reason>" or "• UNSUPPORTED: <reason> (outside the certified fragment)".
    Exit 0 / 8 / 4. A bad file → exit 2.

certcheck explain <obligation.obl> [--cert certificate.cert]
    Pretty-print the obligation (the ∀-claim in readable form) and, if given, a human-readable
    walk of the refutation tree ("split on parent_net; case ⊤: …; Farkas 3·F1+1·F2 ⇒ 0<0"). No
    verdict side effects. Exit 0.
```
Bad usage / missing file → exit 2 with a specific message.

### 5.6 Shipped example artifacts (A2 — real, in `examples/proofs/`, runnable immediately)
- `mint_o2.obl` + `mint_o2.cert`: the R20 budget-carve obligation (`0≤g≤rem ∧ rem_after+g=rem`,
  with the `Ite`/`Max` clamp) and a valid certificate → `certcheck check` → VALID, solver-free.
- `carve.obl` + `carve.cert`: the R22/R20 relational `_ ≤ avail` obligation (`min(g,avail) ≤ avail`)
  and its certificate → VALID.
- `tests/corpus/`: real **forged** certificates the checker must REJECT (wrong obligation_digest,
  non-exhaustive split, negative Farkas coeff, sum-not-contradiction, oversize) — the adversarial A6
  corpus, shipped as artifacts, not hidden in a test fn.

---

## §6 — Build order (TDD: write the named test first, see it fail, make it pass; green before next)

- **S1 — Obligation IR + parse/validate.** `obligation.rs`. Tests: parse `mint_o2.obl`/`carve.obl`;
  reject nonlinear `Mul(x,y)` → `Unsupported`; reject malformed → exit 2.
- **S2 — The checker's evaluator.** `eval.rs` (NNF, substitution, linear-fact normalization, the
  Farkas combination arithmetic). Tests: substitution under a bool assumption; `Σ coeff·fact`
  reduction; strict/non-strict handling; integer overflow in coefficient arithmetic is checked (no
  silent wrap → `Invalid{"coefficient overflow"}`).
- **S3 — The checker.** `checker.rs`. Tests: `valid_certificate_accepted` (both example certs);
  `lying_solver_is_caught_by_checker`; non-exhaustive split → Invalid; wrong `obligation_digest` →
  Invalid; oversize cert → Invalid (bounded).
- **S4 — Soundness corpus.** `tests/corpus/` + `acc_a6_checker_rejects_forged_and_mutated` (every
  corpus forgery → Invalid) and `false_obligation_has_no_valid_certificate` (a known-false obligation:
  no cert checks).
- **S5 — Prover emission (behind `smt`).** `emit.rs`. Tests (smt feature): `prove` an obligation, then
  `check` the emitted cert → Valid; `acc_a5_certificate_byte_identical` (prove twice, diff).
- **S6 — Solver-free guarantee.** Wire the feature split. Tests: `acc_a4_checker_is_solver_free`
  (build `--no-default-features`, run `check`, assert no z3 in the checker closure).
- **S7 — CLI + human output.** `cli.rs`, `main.rs`. Tests: `--help` on every subcommand; `check` of a
  valid/invalid/unsupported cert prints the right legible line + exit code; usage error → exit 2.
- **S8 — R20/R21 integration.** Wire `--require-certificates`. Tests: `require_certificates_fails_
  closed` (R20 mint obligation with a stripped cert under the flag → E1610; R21 admission with an
  invalid cert → Denied); default path unchanged (byte-compatible).
- **S9 — Examples + smoke + quickstart.** Tests: `acc_a1_smoke_prove_check_verify`,
  `acc_a2_example_obligations_certified`, `acc_a3_quickstart_commands_execute`.
- **S10 — Acceptance gate.** `scripts/r23_acceptance_gate.sh` (§10). Green = done.

---

## §7 — Test plan (happy + **adversarial**; every named test is normative)

**Unit / core (pure, fast, solver-free):**
- `valid_certificate_accepted` — `mint_o2` and `carve` certs → `Valid`.
- `false_obligation_has_no_valid_certificate` (soundness) — take `carve` and flip `≤` to `≥`
  (now false: `min(g,avail) ≥ avail` fails for g<avail); assert NO certificate validates it: the
  prover refuses to emit one (SAT), and a hand-forged "cert" → `Invalid`.
- `lying_solver_is_caught_by_checker` (the trust shift) — a certificate with (a) a Farkas witness
  whose weighted sum is `0 ≤ 3` not `0 < 0`, (b) a negative coefficient, (c) a single-branch
  "exhaustive" split — each → `Invalid` at the named node. Models a solver that lied "UNSAT."
- `acc_a6_checker_rejects_forged_and_mutated` — every `tests/corpus/` forgery + a one-byte mutation of
  each valid cert (digest field, a coefficient, a fact constant) → `Invalid`.
- `wrong_obligation_binding` — a valid cert for `carve` checked against `mint_o2` → `Invalid`
  (digest/id mismatch). A cert cannot be reused for a different claim.
- `unsupported_is_failclosed` — a nonlinear obligation → `Unsupported` (exit 4), and under
  `--require-certificates` this is treated as not-discharged.
- `eval_coeff_overflow_is_caught` — adversarial huge coefficients → `Invalid{"coefficient overflow"}`,
  never a wrapped false contradiction.

**Integration:**
- `acc_a4_checker_is_solver_free` — crate built `--no-default-features`; `certcheck check` validates a
  cert; assert z3 absent from the checker's dependency/symbol closure.
- `acc_a5_certificate_byte_identical` — `certcheck prove` (smt build) twice on `mint_o2.obl`; the two
  `.cert` files are byte-identical.
- `require_certificates_fails_closed` — (R20) discharge the mint obligation, strip its cert, run under
  `AXON_REQUIRE_CERTS=1` → E1610 fail-closed; (R21) admit a job whose obligation cert is corrupted →
  `Denied` exit 8. Default (no flag) → unchanged.

**User-journey smoke (A1 — drives the REAL CLI exactly as the auditor would, via subprocess):**
- `acc_a1_smoke_prove_check_verify`: (1) `certcheck prove examples/proofs/mint_o2.obl --out <tmp>.cert`
  (smt build) → "✓ certified principal_mint/O2"; (2) `certcheck check examples/proofs/mint_o2.obl
  <tmp>.cert` → asserts "✓ VALID … verified WITHOUT a solver"; (3) `certcheck explain …` → asserts the
  readable refutation walk; (4) corrupt one byte of `<tmp>.cert` and re-`check` → asserts "✗ INVALID"
  exit 8. Each step asserts stdout text AND the on-disk cert artifact.

**Quickstart (A3):**
- `acc_a3_quickstart_commands_execute` — runs the §9 block verbatim against the built binary.

---

## §8 — Invariants & edge cases

**Invariants (assert in tests):**
- **I-1 Soundness over completeness.** The checker returns `Valid` **only** after itself deriving the
  contradiction; it may return `Unsupported`/`Invalid` on anything it can't constructively confirm, but
  it **never** returns a false `Valid`. (This is the property the TCB rests on.)
- **I-2 Solver-free trusted path.** `check`/`checker`/`eval` reach no SMT solver; the crate validates
  certs built without `z3`.
- **I-3 Binding.** A certificate is valid only for the exact obligation it was issued against
  (`obligation_digest`/`id` match); it cannot be transplanted.
- **I-4 Bounded.** Checking is total and bounded (`MAX_CERT_NODES`); a malicious oversize/cyclic cert
  → `Invalid`, never unbounded work.
- **I-5 Fail-closed integration.** Under `--require-certificates`, absent/Invalid/Unsupported ⇒ not
  discharged / not admitted. Default path byte-unchanged.
- **I-6 Determinism.** Emission is byte-stable; checking is a pure total function.

**Edge cases (named, with resolution):**
- Obligation outside the linear+bool fragment → `Unsupported` (exit 4), fail-closed for gating.
- Integer overflow in a Farkas combination → `Invalid{"coefficient overflow"}` (checked arithmetic;
  never wrap into a spurious contradiction).
- A cert whose case split omits a branch, or splits on an absent var → `Invalid{"non-exhaustive"}`.
- A cert whose leaf `facts` are *not* entailed by the branch-resolved negated claim → `Invalid`
  (the checker re-derives the hypothesis set; it doesn't trust the leaf's stated facts).
- A valid cert for obligation A presented for obligation B → `Invalid` (binding).
- The prover cannot prove a (true-but-out-of-fragment) obligation → `Unsupported`, R20/R21 keep their
  runtime fallback (no regression).
- f64 obligations (the shipped real fragment): R23 v1 certifies the **integer + boolean** fragment;
  f64 bound obligations are `Unsupported` in v1 (documented; a rational-Farkas extension is future).

---

## §9 — Quickstart (`README-axon-certcheck.md`; these exact commands are executed by `acc_a3`)
```bash
# Build the SOLVER-FREE checker (no z3 needed to CHECK):
cargo build -p axon-certcheck --no-default-features --bin certcheck

# 1. Independently verify that a capability obligation holds for ALL inputs —
#    using only the small checker, with NO SMT solver in the trust path:
certcheck check examples/proofs/mint_o2.obl examples/proofs/mint_o2.cert

# 2. Read the proof in plain English (the refutation the checker confirmed):
certcheck explain examples/proofs/mint_o2.obl --cert examples/proofs/mint_o2.cert

# 3. Watch a tampered certificate get REJECTED (exit 8):
cp examples/proofs/carve.cert /tmp/bad.cert
printf 'X' >> /tmp/bad.cert
certcheck check examples/proofs/carve.obl /tmp/bad.cert ; echo "exit=$?"

# (Optional, needs the solver to GENERATE a fresh certificate:)
# cargo build -p axon-certcheck --features smt --bin certcheck
# certcheck prove examples/proofs/mint_o2.obl --out /tmp/fresh.cert
```

---

## §10 — Acceptance gate (pinned; FAILS if any check is missing or stubbed)

`scripts/r23_acceptance_gate.sh` is the single source of "done." It MUST:
1. **Presence check** — assert every §0 check name exists in the test sources:
   `acc_a1_smoke_prove_check_verify`, `acc_a2_example_obligations_certified`,
   `acc_a3_quickstart_commands_execute`, `acc_a4_checker_is_solver_free`,
   `acc_a5_certificate_byte_identical`, `acc_a6_checker_rejects_forged_and_mutated`,
   `valid_certificate_accepted`, `false_obligation_has_no_valid_certificate`,
   `lying_solver_is_caught_by_checker`, `require_certificates_fails_closed`. Missing → **gate fails**.
2. **Anti-stub check** — each acceptance test has a real assertion and is not `#[ignore]`d / `todo!()`
   / `assert!(true)` (grep → fail).
3. **Solver-free proof** — build the crate `--no-default-features` and run `certcheck check` on the
   example; assert success AND that `cargo tree -p axon-certcheck --no-default-features` does **not**
   list `z3` (the trusted-path-has-no-solver gate; if it does → **fail**).
4. **Run** `cargo test -p axon-certcheck` (all green, both with and without `smt`) + the §9 quickstart
   block + `acc_a1` driving the real CLI.
5. **Reproducibility** — (smt build) run `prove` twice, diff the certs byte-for-byte.
6. Exit 0 only if all pass; else print which check failed. Wire into `gate.sh --strict`.

---

## §11 — Definition of Done
**Per slice (S1–S10):** the slice's named tests were written first, seen to fail, now pass; full
`axon-certcheck` suite green (with and without `smt`); no workspace regression.
**Per milestone (R23 complete):** the **solver-free** `certcheck` binary builds and validates the
example certificates with **no z3 in its trust path**; every forgery in the corpus is rejected; a
false obligation has no valid certificate; `acc_a1` passes through the real CLI; emission is
byte-reproducible; `--require-certificates` makes R20/R21 fail closed on a missing/invalid cert while
the default path is byte-unchanged; and `scripts/r23_acceptance_gate.sh` exits 0 with every §0 check
green. **Only then is Z3 out of the trust root.**

---

## §12 — Notes for the implementer (do NOT deviate without updating this spec)
- **The checker is the trusted artifact. Keep it small, pure, total, solver-free, and readable** — its
  whole value is that an auditor can read all of it. Resist cleverness; prefer obvious arithmetic.
- **Soundness beats completeness, always** (I-1). When unsure, return `Unsupported`/`Invalid`. A false
  `Valid` is a TCB breach; a false `Invalid` is merely a missed optimization (the runtime check stays).
- The checker **re-derives** the contradiction; it never trusts a claim stated in the certificate it
  didn't itself confirm (the leaf `facts` are re-derived from the negated claim, the Farkas sum is
  recomputed, the case split is checked exhaustive).
- **No solver on the `check` path** (I-2) — enforced by the gate building `--no-default-features` and
  asserting z3 absent. Z3 lives only in `emit` behind `smt`.
- Use **checked** integer arithmetic in `eval` (overflow → `Invalid`, never wrap). The adversary
  controls the certificate's coefficients.
- The integer+boolean fragment is v1; f64/real is `Unsupported` and documented (§8). Do not silently
  "extend" the checker past the fragment its soundness argument covers.
- Default integration is byte-compatible (certs logged, not gating) until `--require-certificates`.
  Do not change R20/R21 default behavior.
