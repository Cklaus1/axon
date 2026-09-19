# Sources, scope, and evidence boundaries

Prepared: 2026-09-18. This package reviews the preceding Axon roadmap and proposes an implementable replacement. It does not audit a checked-out Axon repository, execute Axon, reproduce model benchmarks, or approve any architecture change.

## Evidence vocabulary

**Documented** means an uploaded source states something, not that this review reproduced it. **Observed conflict** means two passages in the supplied material disagree or have different scopes. **Proposed** means a new recommendation in this package. **Verified-package** is reserved for checks on these Markdown files. **Verified-product** requires a future executable acceptance run against an identified repository commit and environment.

Every specification is Draft. Proposed API names, paths, type signatures, operation tags, and diagnostic identifiers are design contracts, not existing Axon syntax. The package's `CX-*` identifiers are provisional and do not reserve Axon's `R*`, diagnostic, or exit-code namespaces.

## S1 — Axon architecture and reference

User attachment: `axon-docs.pdf`, 94 physical pages. The architecture section reports measurements dated 2026-09-17; other bundled sections have different dates and contain historical text. Page references below use physical PDF pages, which match the printed numbers.

| Pages | Relevant evidence |
|---|---|
| 2–8 | Interpreter reference semantics; parity/refusal; repeated type inference; R2a type-map debt; capability/effect/sandbox/replay layers; R44 materialized sessions; status-vs-evidence warnings. |
| 10–12 | Small fluency experiments and withdrawn broader benchmark interpretation; limited ecosystem; native FFI boundary and trusted code. |
| 15–16 | Journals can contain secrets; ambient effects are interpreter-only; `AXON_REQUIRE_CERTS` behavior; audit identity differs from authority; supervisor kill variable is not an interpreter kill mechanism. |
| 20–24 | Refinement and effect implementation descriptions; userland versus kernel primitives; risk pipeline; absence of a gate function described as open; narrow world/counterfactual examples. |
| 26–54 | Generated command and builtin surface; reference signatures are useful but do not independently prove end-to-end guarantees. |
| 48 | Scoped sandbox semantics, including empty scope meaning unscoped, not deny-all. |
| 60 | Reported tests and a known message-parity failure; not rerun here. |
| 62–64 | Product split, AST approval, userland runtime, and explicit reversal of the earlier no-kernel direction. |
| 77 | Token-cost accounting still described as using an estimate rather than response-derived usage. |
| 83–88 | MDL/world prototype; attestation scope; R33/R34 partial work; R36–R40 proposed or specialized directions; R39 evidence/governance graph. |
| 89–94 | Invariants and exit ledger; I-11 wording conflicts with the newer FFI discussion; program exit status is not an authenticated attestation of enforcement. |

Important unresolved contrasts: phase-complete labels do not imply a general learned world model; risk-gate implementation is less strict than the broad pipeline slogan; the OS ambition was reversed in one section but remains described as out of scope elsewhere. This package records these contrasts rather than silently choosing whichever passage is most convenient.

## S2 — Architecture discussion supplied by the user

User attachment: `Jev Arch(1).pdf`, 48 physical pages. This is a prior conversation/design discussion, not the vendor's implementation specification. BFlow-specific portions are excluded from this package's scope.

| Pages | Relevant evidence |
|---|---|
| 1–15 | Bounded semantic judgments; deterministic composition; distinction between adopting a programming pattern and inventing a foundation model. |
| 18–33 | Summarized browser pattern: observed objects, indexed action choices, speculative branch-specific questions, separate generation, stale-state validation, independent completion verification. |
| 33–44 | Representation search, hypotheses, active experiments, abstraction, and crystallization as proposed routes toward transferable problem solving. |
| 44–48 | Per-pillar learning lifecycles, attribution, and repeated discoveries becoming reusable capabilities. |

Several pages contain unavailable-image placeholders. No hidden content in those placeholders was inferred. Browser implementation and performance claims in this discussion are not independently reproduced here.

## S3 — The roadmap being reviewed

The preceding assistant response in this conversation: milestones M0–M10; suggested R45–R61 documents; nested execution/learning/meta loops. It is a proposal, not evidence that features exist. `REVIEW.md` identifies amendments to that proposal.

## External primary-source checks

These checks clarify narrow technical points; they do not replace S1/S2 as the basis for the package. Accessed 2026-09-17.

### W1 — TypeSafe announcement

TypeSafe, its own System One/Jev announcement, dated 2026-09-15. URL: `https://typesafe.ai/blog/introducing-system-one-models-and-jev`.

The vendor describes a custom model/sampler/training stack. Its workflow reference targets include other models' answers; its schema guarantee is not an independent demonstration of correct decisions. This package imports no vendor latency, cost, or accuracy target.

### W2 — TypeSafe public API introduction

URL: `https://docs.typesafe.ai/introduction`.

Documents Choice, Score, Noul, and isolated parallel questions against shared state. This supports the bounded-question interface pattern. It does not establish that errors across questions are statistically independent or disclose a reproducible training recipe.

### W3 — Calibration research

Guo, Pleiss, Sun, Weinberger; ICML 2017, PMLR 70. URL: `https://proceedings.mlr.press/v70/guo17a.html`.

Provides empirical evidence that neural prediction probabilities can be miscalibrated, and studies post-processing calibration. Our workload-specific calibration contracts and promotion rules are proposed engineering policy, not guarantees furnished by that paper.

### W4 — Model-based planning research

Janner, Fu, Zhang, Levine; NeurIPS 2019, arXiv version 3 revised 2021. URL: `https://arxiv.org/abs/1906.08253v3`.

Studies model bias and short simulated rollouts rooted in real data. Our software-world evaluation and planning rules are proposals; the paper does not establish those rules for compilers or operating systems.

### W5 — Small-model parallel decoding reference

Author-published model card. URL: `https://huggingface.co/harshatheg/Qwen-2.5-1B-RLCD`.

Describes shared-prefix/KV-cache parallel bounded-field inference. The card's title does not establish reproduction of TypeSafe's RLCD, and normalizing logits does not by itself establish calibration. Treat it as an experimental implementation lead, not a verified baseline or an endorsed dependency. No code or weights from it are bundled.

## S4 — User-provided Jev/System-One ecosystem survey (2026-09-18)

The user supplied a launch-week ecosystem map covering official TypeSafe SDKs/adapters, community Jev/OpenJev implementations, calibration/benchmark projects, and bounded-action agent examples. It is treated as **design input**, not as independently reproduced evidence in this package.

Patterns incorporated into the build process:

- stable System-One-like API with interchangeable backends;
- official-adapter pattern for comparing ordinary LLMs against the same typed decision shape;
- direct option-logit and sequence-probability scoring;
- variable runtime candidate sets and option-conditioned learned scorers;
- shared-state/prefix-cache parallel decision experiments;
- dynamic indexed action spaces and separate decision versus generation;
- calibration/error benchmarks before consequential automation;
- grouped split/leakage controls and shuffled-context tests;
- ecosystem claims must be rerun on Axon workloads before adoption.

Representative links supplied by the user include `typesafe-ai/typesafe-sdk-js`, `typesafe-ai/typesafe-sdk-python`, `typesafe-ai/system-one-adapter-python`, `TheoLeeCJ/openjev`, `ekzhang/openjev-sglang`, `bnsd55/jevmlx`, `vinnylarouge/jevlike`, `daseinlabs/open-jev`, `Heman10x-NGU/Verdict-open-jev`, `browser-use/jev-ultrafast`, `shiftynick/jev-axi`, `AbdelStark/jev-benchmarks`, and `abhixhek/jevcal`. This package does not assert their current behavior or benchmark values without a future source/reproduction pass.

## Naming

**Axon Cortex** is a proposed working name for the architecture. **Axon Reflex** is its bounded typed-decision component. **AIR**, in this package, is explicitly defined as **Axon Intelligence Representation**, the typed cognitive execution graph. This expansion is a new naming decision, not attributed to the attachments. The name is not a trademark or domain-availability clearance. “Jev” remains only a source/vendor attribution, not the name of an Axon component.

## v0.3 ecosystem-review claim boundaries

The System-One/Jev ecosystem is used as a source of interface, serving, calibration and agent-loop implementation ideas. Community README benchmark numbers are not Axon evidence unless reproduced under matched Cortex task/evaluation contracts. Official TypeSafe primitive semantics should be checked again at implementation time against the pinned SDK/docs version. Open implementations may have backend-specific candidate/context limits, preprocessing/truncation and deployment defaults; those become adapter/adoption metadata rather than AIR-wide assumptions.

Relevant repository classes for intake include official TypeSafe SDK/adapter; OpenJev-style direct logit/prefill implementations; sequence scorers; MLX/local variants; learned option-conditioned scorers; calibration/agent-failure benchmarks; dynamic browser/computer-use loops; guard/triage/search/tree patterns. Each individual dependency still requires commit/license/security/transitive-artifact review under `build/DEPENDENCY_ADOPTION.md`.

## S5 — MiCode platform and Axon-support build plan (2026-09-18)

User-supplied `MICODE_PLATFORM_MANUAL.pdf` describes MiCode's current architecture and explicitly distinguishes current/intended/partial/deferred behavior. Relevant design facts for the bridge include a central composition root, one resolved-call permission choke point, durable persistence, delegation/worktrees/verification, and the explicit absence of an execution sandbox in the current snapshot. These are MiCode-side facts and must not be projected onto Axon guarantees.

The companion user-directed `MiCode_Axon_Support_Build_Plan_v0_1.zip` is a proposal package defining MiCode as a coding-world/experience plane for Axon. Its MX-01 canonical episode, MX-10 repository knowledge, MX-11 crystallization and MX-12 bridge concepts motivate CX-16–CX-18. They are design inputs, not evidence that MiCode or Axon has implemented the bridge.

## v0.5 source note — intent architecture

The supplied Axon architecture/reference is the basis for the claim that the project already intended a structured-prose → typed Axon → review/approve/deploy product path and treated the typed AST as the audit artifact. CX-19 is a proposed refinement of that direction, not a claim that a distinct Intent IR is already implemented.

### W6 — Jev architecture behavioral probing (secondary analysis)

Archer Hume, `Jev's Architecture Unmasked?`, user-supplied URL accessed in this conversation: `https://archerhume.com/posts/jevs-architecture-unmasked/?v=3`.

The article reports behavioral probes consistent with shared-state processing, sibling-question isolation, strong batched-question scaling, candidate-list/order interactions, and direct distribution readout; it proposes possible causal-transformer/MoE/training explanations. This package treats the measured behaviors as hypotheses worth reproducing and the architecture reconstruction as **non-authoritative**. It does not claim the article reveals TypeSafe's exact model architecture or RLCD recipe.

v0.6 incorporates only the robust engineering consequences: split Reflex Runtime from Reflex Model, formalize state handles and question dependencies, make candidate ordering part of the calibration domain, prioritize listwise candidate models in the bakeoff, and treat distributions/proper scoring as first-class. Causal-decoder and MoE details remain optional research arms.

### W7 — Kev: executable Jev-style decision-model reconstruction

Jared Palmer, `jaredpalmer/kev`, model card and implementation inspected 2026-09-18. URLs: `https://github.com/jaredpalmer/kev/blob/main/MODEL_CARD.md`, `https://github.com/jaredpalmer/kev/blob/main/README.md`, and the pinned repository files used during this review.

Kev provides a working research implementation of the architecture hypothesized in W6: causal pretrained backbone, lightweight adaptation, block-causal state/question isolation, branch-local positions and a pointer/listwise softmax readout with no autoregressive answer generation. The released model card reports a laptop-scale 0.5B prototype, exact/near-exact packed-vs-separate isolation tests, option-order sensitivity, and a large degradation on sources outside its training distribution. The repository's current training code includes optional permutation-consistency and ordinal proper-scoring objectives plus frozen-suite/locked-test research infrastructure.

This package treats Kev as an **executable reference and experimental lead**, not proof of TypeSafe's proprietary architecture/training or a production coding model. Its published model is narrow, English-only and not trained on coding; all quality, latency and transfer claims must be reproduced on Axon/MiCode workloads before adoption. v0.7 uses only the engineering implications: move a small learned reference pilot earlier, prioritize transfer, make candidate-absence/permutation training explicit, unify train/serve/replay encoding, and adopt frozen train/calibration/dev/locked-test/transfer suite discipline.

### W8 — ProgramAsWeights (PAW): natural-language specs compiled into small local learned functions

Repository and public SDK/docs inspected 2026-09-19: `https://github.com/programasweights/programasweights-python`.

The public SDK presents a `spec -> compiled program artifact -> local callable` workflow. Its documentation describes shared base interpreters (currently Qwen3 0.6B and GPT-2 paths), small per-program artifacts, local/offline execution after required assets are prepared, optional hosted inference, strict artifact/runtime validation and an advanced logits-processor hook. It also distinguishes durable asynchronous compile jobs from inference and explicitly notes that timeouts do not prove remote work was not accepted.

Cortex uses PAW as an **architecture/artifact-discipline lead**, not as evidence that Axon can already compile arbitrary natural-language specifications into reliable neural programs. The public Python SDK does not disclose enough server-side compilation detail to reproduce the hosted compiler from this package alone. Published size/latency examples are not Axon benchmarks and must be reproduced on Cortex workloads before adoption.

v0.9 imports these engineering consequences only:

- learned fuzzy transformations are a separate specialization target from bounded Reflex decisions;
- shared-base + small immutable adapter/program artifacts may make many local skills economical;
- compiler/base/adapter/template/runtime identities must be pinned together;
- missing/corrupt learned artifacts must not silently fall back to an unadapted base;
- offline readiness is explicit and includes the full dependency closure;
- generated output remains untrusted data and requires Axon type/refinement/capability validation;
- remote compile/inference is optional and carries durable job/provenance/data-disclosure semantics.


## v0.10 ecosystem research sources

- `githubnext/localjev` — useful distinction between Jev wire compatibility and generated/self-reported probability semantics; local model bakeoffs and long-context distraction results.
- `featherless-ai/simple-jev` — common-prefix KV reuse, next-token candidate scoring, versioned prompt contract and RFDT task-specific decision fine-tuning.
- `fastino-ai/GLiNER2` — schema-conditioned local extraction/classification/relation model family; motivates Semantic Perception.
- `uehaj/jev-semgrep` — proposition-level semantic search with deterministic AND/OR/NOT composition; motivates Semantic Match/semantic grep.
- `saurabhdash/parallax` — separated learner/sampler runtimes and asynchronous weight publication; used only as a systems research lead, with Cortex retaining independent admission.

## v0.11 constrained-selection / composition research sources

Primary repository/source material inspected during the 2026-09-19 review:

- `browser-use/jev-ultrafast` — dynamic indexed action spaces, operation-specific speculative targets, fresh DOM-node validation, separate text generation and independent DONE verification.
- `sutro-sh/jev-align` — uncertainty-driven human labeling and iterative semantic/function-definition optimization with explicit accept/reject/rewind rather than automatic promotion.
- `browserbase/stagehand` PR #2955 — experimental Jev extraction path that selects page elements/groups and deterministically copies values before falling back to LLM extraction; draft/open PR, so results are treated as experimental design evidence only.
- LangChain TypeSafe integration / tool-risk-gating documentation — decision layers can gate/route tools inside an agent harness, but classification does not substitute for Axon authority or human/protected approval policy.
- `vercel-labs/json-render` — typed component/action catalogs and constrained spec rendering; its experimental Jev composition path motivates selection/ordering/composition over known capabilities rather than unconstrained artifact generation.

The package imports only the architectural consequences: runtime-owned candidate catalogs, select/copy/compose before generation, semantic-definition optimization as a separate research dimension, and explicit preservation of verifier/authority boundaries. Repository benchmark numbers, draft-PR results and vendor integration claims are not Axon evidence until reproduced under CX-20/CX-21 contracts.

## v0.12 semantic supervisor / working-set ecosystem sources

Primary repositories inspected during the 2026-09-19 research pass include:

- `thruwire/foreman` — independent semantic supervision of Codex workers with progress/stuck/off-track/verification/finish dimensions and deterministic intervention policy.
- `DevMortimer/pi-warden` — layered local guards plus semantic judgment, steering/hold behavior, rule checks, stuck/done/security supervision and traceability.
- `tamaratran/fast-jev-compaction` — semantic keep/drop/truncate decisions over tool-call/result history while preserving user/assistant text verbatim.
- `EliaAlberti/jev-rules` — dynamic rule/codebase-map injection on prompts and edit targets, once-per-session delivery and fail-open mandatory context behavior.
- `kitze/skillbox` — semantic recommendation over an authorized, revisioned skill catalog with rechecks and non-executing recommendations.
- `gargpratyush/jev-router` — per-turn model routing that preserves explicit user selection and accounts for confidence/current-model/cache economics.
- `devagrawal09/jev-review` — staged bounded decision graph for risk/evidence/mechanism/severity/reviewer selection.
- `realZachi/pg-jev` — semantic predicates inside SQL with deterministic filters, batching/read-ahead/caching and spend controls.
- `TheoLeeCJ/SemIf`, `TianyuCodings/NanoJev`, `vinnylarouge/jevlike`, `ekzhang/openjev-sglang` — independent direct-logit/decision-head/model-serving research reinforcing backend plurality, shared-state reuse and runtime/model separation.
- `awlevin/typesafe-computer-use`, `droidrun/mobile-jev`, `fhshaik/typesafe-mario` and related control demos — environment→deterministic structured state→bounded legal actions→semantic decision→validated execution loops.

These repositories are architecture/research leads, not Axon product evidence. Published performance numbers are not imported as acceptance results. The build files adopt only the recurring systems patterns: independent supervision, semantic working-set management, semantic predicates in query planning, explicit cascades, and continued separation between learned judgment and authority.

## v0.14 design note

CX-30/CX-31 are architectural deductions from the existing Axon/MiCode self-application, repository-ingestion, transfer-evaluation and crystallization work in this package. No new external empirical source is claimed for the repository-optimization contracts themselves. The package continues to distinguish design proposals from product evidence.


## v0.15 source note

CX-32–CX-34 are architectural syntheses from the existing package's evidence/provenance, experimentation, scheduling, specialization and OS-runtime requirements. This revision adds no new external empirical claim and therefore no new source attachment.
