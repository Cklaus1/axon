> **Current release: 0.22.** The [closed-loop integration contract](CLOSED_LOOP_INTEGRATION_V022.md) adds the Axon/Fabric/MiCode slice. Historical sections below retain their original version/scope; they do not establish current peer implementation or runtime qualification. Use `../integration/V022_INPUT_LOCK.json` for current input identity.

> **v0.21 Ownership amendment.** Use [research integration](RESEARCH_INTEGRATION_V021.md), [source upgrade](UPGRADE_V021.md) and [v0.21 bootstrap](BOOTSTRAP_PROMPT_V021.md) for the current additive delta. Historical profiles below retain their original version scope. Package statuses do not reset live source evidence.

# Ownership and compatibility — v0.16 review resolution

These are review amendments to the supplied v0.15 package, not claims about live Axon code. Preserve all existing CX and B identifiers. A spec dependency names a **contract dependency**, not a requirement to finish every feature of that spec. The task DAG and release profiles define executable implementation order.

## One owner per concern

| Concern | Canonical owner | Consumers / limits |
|---|---|---|
| Grants and realized effects | CX-03; confinement CX-13 | All dispatch, imports, builds and model workers recheck current grants. |
| Admission and activation | CX-11 | Labs, compilers and registry cannot self-approve. |
| Replay/data-use/retention | CX-10 | Evidence graph is an index, not a competing raw store. |
| Intent meaning and approval | CX-19 | Source/spec renderers cannot relax clauses. |
| Candidate research / final coding outcomes | CX-20 / CX-21 | Training metrics do not substitute for task verification. |
| Choice of specialization target | CX-22 | Neural Programs, rules and composition are alternatives, not mandatory sequential tiers. |
| Neural Program compiler/runtime lifecycle | CX-23 | Existing implementation owner retained; no second runtime introduced by CX-36. |
| Neural Program naming, `.nps`/`.np` encoding and portable contracts | CX-36 | Narrows/extends CX-23; wire formats and package integrity live here. |
| Typed AIR operations | CX-04 | `SELECT`, `COPY`, etc. may be workflows, not new syntax/opcodes. |
| Semantic perception | CX-24 | A learned extraction is not an authoritative observation merely because it has a span. |
| Production candidate/model publication | CX-25 plus CX-11 | Learner and shadow credentials cannot update active pointers. |
| Supervisor / context policies | CX-27 / CX-28 | Advisory policies; protected pins, kill latch, completion and authorization stay outside them. |
| Platform self-application | CX-29 | Starts with passive instrumentation, not automatic code mutation. |
| Per-project contract / cross-project reuse | CX-30 / CX-31 | Receiving-project re-admission required. |
| Provenance graph / causal protocol | CX-32 / CX-33 | Never promote predicted evidence to realized outcomes. |
| Active capability registry and dispatch policy | CX-34 | Neural Program Registry is a filtered view of this registry, not another authority. |

## CX-35 is not present

The conversation proposed a CX-35 serving/model-state spec, but the supplied v0.15 ZIP contains only CX-00–CX-34. It is **reserved, not implemented or silently added** here. Embedded/local-sidecar/remote Neural Program deployment contracts are bounded by CX-23/CX-36 and existing CX-13 host requirements. A complete general Reflex service or latent-state compiler remains separately scoped work; `.np` support does not depend on it.

## Neural Program migration

CX-36 is the reviewed revision of the supplied standalone proposal. CX-23 retains training/import/load/invoke ownership; its manifest notation now refers to CX-36. CX-34 owns registry entries and CX-11 owns deployment status. Candidate artifacts can be loaded for evaluation in isolated, explicitly authorized research profiles without being active production capabilities.

An extension is an Axon convention, not a global reservation. `.np` never means arbitrary bytes can be executed. `.paw` remains the external ProgramAsWeights format; renaming it does not convert it. PAW citations, importer names and genuine external references are preserved.

## Package versus wire versus runtime versions

`v0.16` versions these documents. `axon.nps/1`, `axon.np/1` and each protocol's schema version version actual proposed contracts separately. Compiler commit, backend digest, tokenizer digest, model digest, runtime profile and platform target are separate identities. Support is negotiated per operation and target; unsupported state/fork/load/export operations refuse rather than pretend compatibility.

MiCode continues to own its permission gate and `/build-loop`. This Axon-only revision provides a bridge migration note; it does not modify the MiCode v0.8 package or claim that its producer already exports the new fields.

## v0.18 ACE ownership resolution

The [ACE profile](../schemas/ACE_EXECUTION_PROFILE.md) consolidates physical execution under existing owners. CX-04/15 own typed host mapping; CX-05/06 result/features/provenance; CX-13 host budgets/cancellation; CX-23/36 learned lifecycle/artifact; CX-28 effective input/state; CX-32 evidence; CX-34 dispatch; CX-11 admission; CX-16 MiCode exchange. No reciprocal spec dependency or parallel registry is introduced. The profile is now Axon-owned at document level, not a claim of a deployed API. CX-35 remains reserved and all reviewed CX-36 bytes remain unchanged.

## v0.20 schema / lightweight Reflex supplement

Apply [schema frontend](SCHEMA_DECISION_FRONTEND.md) and [lightweight distillation](LIGHTWEIGHT_REFLEX_DISTILLATION.md) under existing owner contracts. B210–B213 are model-free schema/active-output/source/authority seams; B214–B216 are governed fixed-task data/training/independent acceptance; B217–B219 qualify runtime/disposition and real peer use; B220 maintains offline package conformance. B221/B222 are optional context/richer-learner hypotheses, never success prerequisites for the new bounded profiles.

Preserve exact task/labels/source/runtime identity, partition/evaluator custody, null-threshold/OOD fallback, local permissions and independent completion. No new .reflex/ABI owner, schema-plan-to-trained-artifact shortcut, in-place self-training or head-only speed claim. [Coordinated apply order](APPLY_V020_WITH_MICODE_V014.md) preserves current live evidence.
