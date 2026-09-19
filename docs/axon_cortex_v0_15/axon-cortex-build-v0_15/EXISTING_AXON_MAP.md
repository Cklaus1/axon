# Reuse, verify, extend: map to existing Axon

Basis: S1 in [SOURCES.md](SOURCES.md). Status is **documented, not rerun** throughout. Repository paths are taken from the attachment and must be confirmed before editing.

| Existing surface | Source | Reuse | Required check / new work |
|---|---|---|---|
| `interp.rs`, LLVM codegen, parity scripts | pp.2–4, 89 | Keep reference semantics and sound refusal. | Produce feature × engine × enforcement matrix; no native ambient-ceiling assumption. |
| `infer.rs`, checker, R2a type map | p.4 | Use authoritative inferred types. | Thread stable IDs/type information before new cross-engine semantics; no additional ad hoc type heuristic. |
| `capabilities.rs`, effects, contained annotations | pp.5–6, 19–21 | Existing static rules and effect vocabulary. | Cortex dynamic handles need runtime authority, snapshot binding, and per-argument validation. |
| Principal/kernel registry | pp.44–47 | Attenuated grants and budget authority. | Audit identity is not authorization; check each executor use, handle ownership, resource budget semantics. |
| Sandbox APIs | pp.16, 48 | Reference for scoped interpreter effects. | Empty scoped list is not deny-all; untrusted native tools need process isolation outside interpreter checks. |
| AxonHost, host journals and replay | pp.15, 94 | Reuse seam, divergence semantics, recording mechanism. | Record model boundaries, cancellation and ordering; isolate secret-bearing journals from training export. |
| R44 accumulating sessions | pp.6–7, 14 | Interactive typed execution and explicit cell boundaries. | Closures/channels/native handles cannot be assumed persistent across cells; Cortex stores stable artifact refs, not live heap values. |
| Goal search, Plan, Schedule, Feedback | pp.23, 40–41 | Reuse bounded optimization and value-level types. | Real task planner needs explicit state, budgets, completion evidence, and scheduling semantics. |
| World/Counterfactual and MDL | pp.23, 83–84 | Existing small examples and complexity measurement. | Distinguish a numerical demo from a learned software model; add train/eval splits and bounded prediction targets. |
| Distributions and belief helpers | pp.23, 28, 34–35 | Numerical distributions and runtime refinements. | Do not reuse moment checks as a calibration certificate or causal proof. |
| Self-improving rewrite DSL | pp.22, 83–84 | Restricted rewrite candidates and multi-gate admission. | Corpus equality is empirical; formal proof applies only where its scope is explicit. |
| CLI/web approval and risk gate | pp.14, 22–23 | Typed artifact review and explicit human approval. | Bind approval to final content digest; missing required gate becomes rejection in Cortex profile. |
| `axon-os`, audit ledger, compliance monitor | pp.84–85 | Hosted supervisor, kill, resources and evidence. | Confinement profile tests, durable job recovery, privileged parent isolation, cancellation propagation. |
| R26–R34 attestation/quorum | pp.84–86 | Integrate only portions confirmed by required evidence. | Hardware-backed vs simulated/HMAC reports must be distinguishable; cross-VM quorum is not an initial dependency. |
| R36 kernel/platform proposals | pp.86–88 | Preserve proposal and reuse exact unresolved decisions. | Separate confinement and syscall-gate proof obligations before claiming a full OS. |
| R38 embedded runtime proposal | p.86 | Avoid re-deriving SDK packaging. | Check overlap with Cortex adapters and runtime before creating a new product. |
| R39 typed governance/evidence graph | pp.87–88 | Store spec dependencies and rerunnable evidence in existing graph. | Import CX specs after namespace reconciliation; do not build a second governance registry. |
| R40 research compiler proposal | p.88 | Prior art for evidence/experiment/knowledge graphs. | Scope Cortex to runtime and learning experiments, not a replacement project-management platform. |
| R41 polyglot candidate | p.7 | Potential future adapter seam. | Do not make polyglot semantics mandatory for v0. |

## Reconciliation work product

For each dependency, record: path; function or interface; repository commit; source spec; target/engine; gate command; environment; stdout/stderr artifact; actual return status; PASS/FAIL/SKIPPED/UNAVAILABLE; and the precise claim established. A missing repository or a skipped gate is not PASS.

For conflicting documentation, add a discrepancy record with both passages, observed implementation, proposed resolution, owner, and linked invariant change when necessary. Do not edit the old document to claim resolution before running evidence.

## v0.5 intent-layer follow-up

The supplied Axon architecture already documents `axon intent compile`, `axon ast review`, `axon ast approve`, structured prose and the principle that the typed AST is the executable/audit artifact. CX-19 does not assume their current implementation exactly matches the new Intent IR contract. B52 must inspect the live repository first, distinguish what is current versus aspirational, and decide which existing parser/approval artifacts can be reused or migrated. The proposal strengthens the earlier prose→AST path into prose/system intent→typed desired-outcome contract→AIR→`.ax`/actions→evidence.

## v0.9 specialization / neural-program delta

No supplied Axon source evidence establishes a general Cognitive Specialization Compiler or a Neural Program runtime/compiler as built. Existing userland skills, model adapters, generated code, restricted compiler rewrites and AI calls are reuse candidates only after source audit. During implementation, search R38/R40/R41 and any adapter/skill/package infrastructure before introducing new registries or runtime crates. Treat CX-22/CX-23 as Draft gaps until call-site evidence exists.


## v0.10 semantic layer mapping

CX-24 should initially reuse existing observer/retrieval/tool surfaces rather than invent compiler syntax. CX-25 should initially use isolated host workers and immutable artifact registries; it does not imply an in-language online trainer or live weight mutation feature exists in current Axon.

## v0.12 proposed additions to reconcile against live Axon

- Check whether existing scheduler/supervision primitives can host CX-27 before adding a parallel supervisor subsystem.
- Check whether current store/session/replay machinery already distinguishes pinned, recomputable and compressible working state before implementing CX-28.
- Reuse existing query/index/filter infrastructure for semantic predicate pushdown rather than building a separate search stack.
- Preserve current capability/effect and verifier ownership; supervisor/context policies are advisory/control metadata only.
