# Axon Cortex — specification and build package

**Prepared:** 2026-09-18. **Revision:** v0.7 Kev/transfer-first Reflex refinement. **Status:** Draft architecture proposals. **Product implementation/tests:** not performed in this package.

Axon Cortex is the proposed cognitive control plane for a **self-optimizing, intent-first Axon language/compiler/runtime/OS**: natural/system intent → typed Intent IR → observed state → bounded typed decisions → authorized execution → independent verification → eligible experience → controlled specialization/crystallization. It learns from Axon's own executions, MiCode's coding-world episodes, approved external repositories/histories, and generated curricula under one evidence and admission discipline.

**Axon Reflex** is the bounded typed-decision engine. **AIR** is defined here as **Axon Intelligence Representation**, the typed execution graph. These names distinguish the full architecture from a model, and distinguish Axon's work from the external Jev reference. This is a working-name decision, not trademark clearance.

## Start here

For a single document, use [AXON_CORTEX_MASTER.md](AXON_CORTEX_MASTER.md). The modular files remain authoritative for editing.

Read [REVIEW.md](REVIEW.md) for the substantive gaps/corrections, then [BUILD_PLAN.md](build/BUILD_PLAN.md) for milestone sequencing. Use [BOOTSTRAP_PROMPT.md](build/BOOTSTRAP_PROMPT.md) to start a coding agent on repository intake, not on a giant implementation sprint.

| Document | Purpose |
|---|---|
| [Review](REVIEW.md) | Nineteen issues and concrete changes to the original roadmap, including the System-One backend bakeoff revision. |
| [Architecture decisions](DECISIONS.md) | Proposed forks, tradeoffs and owner choices. |
| [Existing Axon map](EXISTING_AXON_MAP.md) | Reuse/verify/extend matrix and unresolved source discrepancies. |
| [Specification index](specs/INDEX.md) | 20 detailed Draft specs with dependencies, contracts and negative acceptance cases. |
| [Build plan](build/BUILD_PLAN.md) | Milestones M0–M9, intent-first entry path, parallel tracks and the first vertical slice. |
| [Build tasks](build/TASKS.md) | 69 dependency-tracked work packages; optional research/native/kernel branches are explicit. |
| [Builder protocol](build/BUILD_PROTOCOL.md) | Intake, implementation loops, integration, evidence and definition of done. |
| [Loop protocol](build/LOOPS.md) | Runtime inner/outer loops, learning/meta loops and separate builder loops. |
| [Acceptance gates](build/ACCEPTANCE_GATES.md) | 122 proposed product gates; none is presented as already executed. |
| [Experiment register](build/EXPERIMENT_REGISTER.md) | Baselines, ablations and research stop/pivot criteria. |
| [Reflex conformance](build/REFLEX_CONFORMANCE.md) | Primitive/batch semantics, backend feature negotiation, effective-input receipts, prompt-role isolation and adapter fixtures. |
| [Reflex runtime](build/REFLEX_RUNTIME.md) | Shared-state handles, dependency-aware branch scheduling, candidate-order manifests, cache/accounting and isolation semantics. |
| [Dependency adoption](build/DEPENDENCY_ADOPTION.md) | External repo/model/dataset intake, transitive pinning, licenses/security and benchmark comparability. |
| [Reflex backend bakeoff](build/REFLEX_BACKEND_BAKEOFF.md) | Backend-neutral ABI; generative/logit/sequence/learned-head comparison; shared-state measurements and destructive controls. |
| [Kev-style Reflex pilot](build/REFLEX_KEV_PILOT.md) | Early small learned coding-decision reference arm; transfer-first training/evaluation and stop/pivot rules. |
| [Reflex decision corpus](build/REFLEX_DECISION_CORPUS.md) | Dynamic candidate examples, grouped splits, label lineage, error attribution and question decomposition. |
| [Reflex calibration lab](build/REFLEX_CALIBRATION_LAB.md) | Probability provenance, calibration artifacts, selective risk/coverage and VUC. |
| [MiCode experience bridge](build/MICODE_EXPERIENCE_BRIDGE.md) | Cross-system artifact contracts, authority separation and semantic replay. |
| [Knowledge ingestion](build/KNOWLEDGE_INGESTION.md) | Repository/history evidence pipeline and counterexample/reproduction discipline. |
| [Self-optimizing OS loop](build/SELF_OPTIMIZING_OS.md) | Experience sources, crystallization ladder, coupled learning loops and timescales. |
| [Intent compiler](build/INTENT_COMPILER.md) | Natural/system intent → typed Intent IR → semantic approval → constrained AIR → evidence. |
| [Protocol schemas](schemas/PROTOCOLS.md) | Versioned data contracts and action/admission state machines. |
| [Worked repair episode](examples/SAFE_REPAIR_EPISODE.md) | Concrete safe coding-world scenario and negative variants. |
| [Sources and limits](SOURCES.md) | Attachment page references, narrow external checks and claim boundaries. |
| [Status](build/STATUS.md) | What was prepared versus what still requires repository evidence. |
| [Glossary](GLOSSARY.md) | Meaning of observation, belief, world model, calibration, proof and authority. |

## Major changes from the previous roadmap

The first milestone now reconciles actual Axon support rather than inheriting blanket safety claims. A working hosted repair loop comes before custom neural architecture. Capability IDs receive real authority/freshness/payload/recovery semantics. Evaluation/data/admission move early, and missing required gates fail closed. Statistical prediction never substitutes for proof or realized outcomes. Meta-learning proposes changes but cannot redefine its own exam.

The OS goal remains staged: H0 confinement in the first runtime, H1 hosted-service operation, H2 portability/optional attestation, then a separately approved bare-metal branch. Language syntax/native lowering follow measured runtime usage and type-map/parity evidence.

## Usage and import

Copy this directory into a staging location such as `docs/cortex-proposal/` in the actual repository. Inspect existing templates, R-ID allocation, invariants, exit codes and R39 governance before import. Do not overwrite those systems. All `CX-*` IDs are package-local. All API/type names and code locations not explicitly described as existing in SOURCES are proposals/pseudocode.

The four JSON manifests make specs, tasks, gates and source provenance easy to ingest. `tools/validate_package.py` is an actual utility supplied here; it validates this package's references and DAGs only. It does not implement Cortex or run Axon tests.

```bash
python3 tools/validate_package.py
```

See `package_validation.json` for the latest actual document-only validation report. A planned product gate is not a runnable command until implemented in the repository.

## First deliverable to build

A resettable local Axon example; compact partial observations; session/snapshot-bound grants; one bounded generated patch; isolated registered checks; independent completion verification; and replay without real side effects. Produce a patch/evidence artifact for review, not an automatic merge or deployment.

No proprietary training recipe, vendor speedup, arbitrary confidence threshold, universal program equivalence, or general-intelligence result is assumed.

## v0.3 focus

This revision folds the second System-One/Jev ecosystem review into executable build contracts. The architecture is unchanged; the experimental boundary is tighter. A backend cannot silently reinterpret primitive semantics, match batch results by position, hide truncation/preprocessing, turn repository text into privileged prompt roles, conceal retries/normalization, or claim calibration outside the input contract it actually evaluated. Candidate recall/no-suitable-option handling and learning-label lineage are now explicit.

## v0.5 focus

This revision reconnects Cortex to Axon's original intent-first product thesis. Natural language or system-generated optimization proposals now lower to a typed `IntentIR` before AIR or `.ax`; the intent contract owns objective, hard constraints, preferences, authority requests, budgets, ambiguity and required evidence. Human approval is bound to the exact Intent IR version, and self-improvement is expressed as `ImprovementIntent` through the same authority/admission path rather than unrestricted mutation.

## v0.6 focus

This revision kept the Cortex architecture and milestone ordering intact while sharpening the Reflex implementation. Reflex now has an explicit runtime/model split; shared-state reuse is a measurable backend capability; AIR questions declare independent/conditional/answer-dependent scheduling; candidate order is part of inference/calibration semantics; distributions are primary and confidence summaries derived; and listwise candidate interaction becomes a first-class research arm rather than an assumption. No claim is made that external behavioral probing reveals Jev's exact hidden architecture.

## v0.7 Kev update

The working `jaredpalmer/kev` reconstruction makes the Jev-style model mechanism an executable reference rather than only a hypothesis. v0.7 therefore adds an early small Axon Reflex model pilot once eligible coding data exists, while shifting the main research emphasis to **cross-repository/task transfer, candidate-absence handling, permutation robustness, canonical decision encoding and frozen locked-test research suites**. The broader Intent IR → AIR → Cortex → self-optimizing OS architecture is unchanged.
