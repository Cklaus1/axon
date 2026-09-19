---
id: CX-15
title: "Language, compiler and proof integration"
status: Draft
authority: Proposed
depends_on: ["CX-00", "CX-04"]
first_stage: M2
implementation_evidence: []
---

# CX-15 — Language, compiler and proof integration

## Intent and source basis

Promote proven runtime concepts into Axon language/compiler support without multiplying inference paths or weakening the interpreter oracle. S1 pp.2–5, 20–21 and 89–91 supplies the extension/invariant discipline and R2a debt; pp.83–85 distinguishes rewrite experiments and specific formal-proof work. See [SOURCES](../SOURCES.md).

## Decisive fork

Library/host contracts first; syntax and native lowering only when stable usage and evidence justify them. Reuse the current compiler, runtime and proof infrastructure. Do not require adding a new prover solely because the architecture discussion mentions one.

## Phase L0 — Adapter integration

Introduce typed serializable contracts and Axon library wrappers over a narrow registered host seam. Avoid exposing raw authority pointers or arbitrary native FFI to generated code. Document which engine supports each operation and why. Unsupported operations refuse with a properly allocated stable diagnostic; allocation occurs in the actual repository governance process, not this package.

Conformance fixtures cover input/output types, effects, budget accounting, branching, errors and replay. Existing R44 materialization restrictions remain explicit; persistent action handles are reacquired through the host registry. New raw type guesses in codegen are prohibited.

## Phase L1 — Authoritative types and language surface

Before new cross-engine cognitive semantics, persist/reuse an authoritative expression-ID-to-type map as appropriate to R2a's actual status. Define stable expression identities, invalidation under AST rewrites/imports, ownership lifetimes and propagation through mono/lowering. Audit existing implementation instead of assuming the PDF's reported debt is still unchanged.

Only add syntax when at least two concrete workflows demonstrate that the library form causes repeated measurable errors/overhead or cannot express the required semantics. The syntax must have a formal desugaring into AIR/library operations, diagnostics, effect rules, formatter behavior, reference generation and migration guidance.

Probability/refinement syntax does not create empirical calibration. Typed action references cannot be deserialized into authority without registry validation. New distribution or graph types state their representation and ownership cost rather than silently copying large values.

## Phase L2 — Native and optimizer support

Every implemented native feature must preserve the reference semantics for its supported input domain. For model calls, conformance uses recorded replies; fresh stochastic inference is not expected to be bit-identical. Numeric comparisons must preserve the specified exact or tolerance semantics; never relax a safety predicate merely to make parity pass.

Ambient interpreter constraints require explicit native runtime/launcher support before Cortex enables that target. The lack of a call site is not an excuse for quietly ignoring policy. Capability flags are not an optimization hint and cannot be eliminated by an unproved rewrite.

Compiler optimizations include safe graph scheduling, redundant observation reuse under exact invalidation rules, conditional question fusion and restricted deterministic specialization. Compare output, effects, failure behavior, budgets and trace/provenance requirements—not just stdout or speed.

## Proof integration

A `ProofReceipt` names theorem/property, assumptions, subject artifact digest, checker/version, proof artifact, result and evidence. A proof about an old patch or weaker precondition cannot approve a new patch. Report what is proved and what remains tested or trusted.

Use existing SMT/refinement infrastructure where it fits. Lean integration is optional future adapter work, not documented as shipped. The supplied TLA+/Coq work concerns a specific corrigibility mechanism, not the full compiler/OS. Learned proof search can propose tactics/lemmas; the trusted checker and its assumptions stay protected.

## Acceptance gates

**G15-types:** a corpus exercising new type/ownership/refinement paths uses the authoritative type map; deliberately inconsistent lowering refuses rather than guessing.

**G15-parity:** supported interpreter/native cases agree on semantic output, effects, failures and recorded model behavior; unsupported cases refuse explicitly.

**G15-docs:** generated reference and documented surface update with the change; language primer tests measure model authoring/repair fluency without cherry-picking examples.

**G15-proof:** stale subject hashes, changed assumptions and invalid proofs cannot produce a valid receipt or bypass runtime fallback.

**G15-optimization:** a proposed fusion/specialization preserves budgets, authority and control dependencies, including adversarial and failure paths.

## Build slices and exclusions

The audit/interface phase starts early; deep frontend changes are not a prerequisite to the hosted vertical slice. No formal proof of all Axon semantics, unrestricted self-modifying compiler, mandatory bare-metal kernel, or automatic equivalence of learned policies is claimed.

## v0.3 question-design and fusion checks

Compiler/AIR reviews classify deterministic predicates before creating a Reflex question. Snapshot equality, grant expiry, authenticated test outcomes, and other mechanically decidable invariants stay in deterministic code. Semantic judgments may use Reflex only after the question declares scope, evidence needs, answer-consumption policy and deterministic composition.

A compiler transformation that combines or rephrases model questions is semantics-preserving only under a declared backend contract that substantiates that property; otherwise it is an empirical model/policy transformation and cannot rely solely on replay of old answers as proof of fresh-inference equivalence.

## v0.4 native-promotion boundary

CX-18 may nominate repeated tools/rules/library abstractions for compiler/runtime promotion. Such nomination is not evidence. Any compiler/native candidate still requires authoritative type information, interpreter/reference semantics, parity/refusal behavior, capability/effect monotonicity and applicable proof/invariant gates. The default successful outcome for a learned capability is a guarded userland artifact; native integration is a separately justified optimization.

## v0.6 AIR dependency typing and batching legality

AIR makes cognitive data dependencies explicit so the compiler/runtime can optimize execution without changing semantics:

```text
QuestionDependency =
    Independent
  | ConditionallyRelevant { branch }
  | AnswerDependent { source_question }
```

The compiler may co-schedule `Independent` questions and may speculatively execute `ConditionallyRelevant` branches against one immutable state handle. `AnswerDependent` questions form a stage boundary unless a finite branch expansion is explicitly represented in AIR and remains within registered budgets.

A shared-state/prefix-cache optimization is legal only when it preserves question isolation, candidate manifests, trusted prompt roles, model/tokenizer/preprocessing identity, and answer dependency semantics. Prompt concatenation or question fusion that changes which text can attend to which text is a policy/model transformation and must pass fresh conformance/calibration/evaluation rather than compiler parity alone.

**G15-dependency-types:** the AIR validator rejects cycles/missing dependencies and prevents an `AnswerDependent` question from being scheduled in the same stage as its unresolved source. Conditional speculative outputs cannot be consumed outside their branch.

**G15-batch-semantics:** compiler/runtime batching of isolated/shared-state questions reproduces the registered semantic decision contract within the backend's tolerance; a deliberately fused conversational prompt is identified as a changed policy and cannot pass as a transparent optimization.
