# Axon v0.21 — source and v0.20 review

Review date: **2026-09-24**. Scope: source-grounded architectural/seam review, baseline-package validation, executed Python reference tests, and external primary-source research review. This is not an exhaustive source/security audit, a Rust build, a trained-model evaluation or a deployed-system certification.

## Executive result

**Use v0.21 as an additive, non-destructive research integration release. Do not build seven new competing subsystems.** The uploaded v0.20 pack already assigns owners for Reflex decisions, calibration, evaluation, self-application, working sets, routing/scheduling, evidence, composition and independent admission. The source has newer concrete runtime work than some of its historical planning documents suggest.

The revised pack keeps all 37 specification IDs, original 223 task rows and 360 gate rows, then adds 32 tasks and 64 gates. New profile aliases route through existing owners. CX-36 r0.2 and existing ACE v1 JSON schemas are protected byte-for-byte. The source itself has not been edited by this delivery.

## Inputs and reproducibility

| Input | Confirmed content | Review pin |
|---|---|---|
| `axon-ai-context-20260924.zip` | 1,213 extracted files; Cargo workspace and implementation/governance/experiments | SHA256 `68c2a7d5a49acdd2e91229210adbacf62bcb1f18e2647b013594d744e1711fac` |
| `Axon_Cortex_Build_Files_v0_20(1).zip` | 194 extracted files; 37 specs, 223 tasks, 360 gates | SHA256 `b401e6fe03cb69b0c401e82850a7f24f12e66a1138bee6b3a6857fa7db482a43` |

Full [source inventory](source_inventory.json), [baseline inventory](baseline_inventory.json), [input lock](../integration/V021_INPUT_LOCK.json) and [source excerpts](SOURCE_EVIDENCE_EXCERPTS.md) are included. The archive is not silently represented as a known Git commit. Historical source attachments mentioned inside old specs were not newly supplied in this review. No MiCode source or support archive was supplied.

## Checks actually executed on the inputs

The untouched v0.20 validator returned **PASS**. Its full unittest suite ran **249 tests, all passing**. The source benchmark analyser self-test returned **4 passing cases** (two adversarial, one control, one identity case). Raw logs are retained as `baseline_validate.log`, `baseline_tests.log` and `source_analyser_tests.log`.

Those Python results validate the stated fixture and analyser scopes only. No Cargo/rustc executable was available; Rust code was inspected but not compiled or run. No trained CLM/Jev/router, GPU inference, RRSI search, live cascade, provider latency/quality benchmark, MiCode round trip or product gate was executed. The generated pack's own final checks are recorded separately in [VALIDATION_V021](VALIDATION_V021.md).

## Source findings and disposition

### F01 — much of the proposed architecture already has owners

The v0.20 manifests/specs already contain CX-05 Reflex, CX-06 calibration/routing, CX-01/21 evaluation, CX-28 working sets, CX-29 self-application, CX-26 composition, CX-10/32 evidence and CX-34 scheduler/registry. Jev/RLCD and order/calibration semantics were already materially added in v0.19/v0.20. Calling all of these “missing planes” would overstate the gap and risks duplicate authorities.

**Disposition:** preserve owners; add research-specific contracts and qualification gates. The [owner map](../integration/SOURCE_OWNER_MAP.json) makes the aliases explicit. An Action Registry becomes a view over CX-34, not a new source of capability truth. Canonical history stays CX-10; evidence indexing stays CX-32.

### F02 — source/package CX-35 collision must be reconciled, not erased

The baseline reserves CX-35, while the actual source has `governance/cortex-v015/CX-35-reflex-serving.md` and the `axon-reflex` crate (SRC25). A naive importer or renumbering pass could orphan existing tests, evidence and work.

**Disposition:** retain the source's CX-35 label and map its serving contract to CX-05 and related owners. The package continues to reserve the ID in its own namespace. B223/G00-r21-source-reconcile makes this a prerequisite.

### F03 — Reflex serving exists, but its core decision is a fixture

`crates/axon-reflex/src/lib.rs` contains `axon-reflex/1`, `ReflexBackend`, principal-bound handles, explicit refusals and transport modes (SRC02/SRC03/SRC06). Its `ReflexCore::encode_state` ignores `_input`, and its decision returns `decided(question)` (SRC04/SRC05). Thus the transport/isolation boundary is valuable existing work, not evidence of real CLM/Jev inference.

**Disposition:** preserve the seam and deterministic control; add negotiated bounded adapters. Do not replace isolation with a new generic HTTP path. Keep transport failure distinct from a legitimate decision/abstention. A principal label in a request is not, on its own, proof of authenticated remote identity.

### F04 — real Cortex generation, authorization and final checks already exist

Current source has `PatchGenerator`, deterministic action selection, policy/path restrictions and a running episode pipeline (SRC07–SRC11). `cortex-policy-adapter` invokes the authoritative grant logic rather than selecting a learned route (SRC15). Historical reports describing only an absent future runner are not current implementation evidence.

**Disposition:** SPX and learned decisions produce proposals through these seams. Do not introduce an alternative executor that bypasses authorization, hidden checks or patch validation. The policy adapter's allow result does not claim actual execution or all other security validations.

### F05 — preserve the corrected claimed/observed distinction

Current `EpisodeEvent` separates `Claimed` and `Proposed` from actual `CheckRun` events, and `verified_ok` reflects the final verification (SRC12/SRC13). Some historical discrepancy notes predate these changes.

**Disposition:** keep the current behavior. CheckEval-style judges are evidence claims, not fabricated process checks. A good intermediate patch must not hide a failing final episode. Do not reopen resolved defects based solely on old prose.

### F06 — a content hash is not an external integrity anchor

`Episode::digest` hashes episode identity and ordered content (SRC14). This is useful content binding, but by itself does not prove durable append-only storage or prevent rewriting the entire content plus hash. Existing audit and RecordingHost/ReplayHost systems provide the appropriate integration owners (SRC18).

**Disposition:** reuse those mechanisms and clearly separate content addressing, authenticated evidence and runtime execution. New replay consumes recorded decisions/context/routes instead of making fresh provider calls.

### F07 — the old package gate is incompatible in several independent ways

The source script hardcodes the v0.15 location, expects `cortex-package-sha256/1`, invokes `--report`, rejects zero count fields and expects a hashed validation receipt (SRC19–SRC23). v0.20/v0.21 use a different checksum contract and validator interface, legitimately report zero orphan gates, and exclude validation/checksum outputs to avoid cycles.

**Disposition:** changing only the vendored directory is insufficient. B224 supplies a reviewed migration checklist and a new explicit wrapper template. Preserve the existing registry and old receipts. Do not transform documentation PASS into source gate PASS. The source's currently empty gate registry is not evidence that no code works; it means those links are not recorded there (SRC26).

### F08 — economics already exists, but is not one complete contract

The AI gateway exposes token usage; kernel budgeting uses an authoritative integer spend rate (SRC16/SRC17). Cortex benchmark code has input/output/cache-read/cache-write/cost/latency data, and its analyser has identity-join tests (SRC24 and the executed self-test). A new TEL service would duplicate responsibilities without automatically improving attribution.

**Disposition:** extend existing records with disjoint billable categories, stable task/attempt/call joins, known/unknown usage, full cascade reservations and price identity. Do not reset the kernel budget or double-charge observational telemetry. Preserve failed/cancelled calls and all assigned tasks.

### F09 — source/package statuses have different meanings

The portable pack declares Draft/Not started/NOT_RUN, while current source contains implemented code and its own evidence. Some vendored documentation is v0.15, the uploaded current design is v0.20, and Cargo workspace version is `0.1.0` (SRC01). These are not interchangeable version/status axes.

**Disposition:** maintain a source-backed evidence overlay and explicit document profiles. Do not globally replace versions or reset source task state. The active generated task view now states this distinction.

## Research review and corrected architectural implications

The [source registry](../integration/RESEARCH_SOURCES.json) and [eight-family matrix](../integration/RESEARCH_REQUIREMENTS.json) separate source-derived ideas from Axon adaptations. RRSI is regularized harness evolution; CLM is Contrastive Language Models, not the earlier mistaken context-lifecycle expansion. TypeLLM/pijev is specifically the permutation wrapper. The unrelated similarly named coding-agent work is not silently substituted.

The inspected CLM embedder sends a default truncation limit and caches by text within the upstream instance. Axon needs an explicit effective-input policy and principal-qualified caching; blindly importing defaults is not compliant with existing state/isolation requirements. Pinning the Git repository does not pin the downloaded model/head weights.

CLM's independent scoring supports permutation equivariance as a mathematical expectation for fixed scores, but argmax ties, floating precision, candidate-set changes and calibration remain separate. pijev's finite permutation average is not exact symmetrization or a guarantee of truth probabilities. Best-of-N verifier success is not single-attempt task correctness. RLM-style verification of a draft does not certify later repaired bytes. These limitations are folded into DEC/SPX gates, not left as footnotes.

Published performance claims were not made release acceptance thresholds. The CLM Notion site could not be independently loaded. Its missing contents are not invented from the repository; reviewed repository/model-card material is separately identified.

## Adversarial review folded into v0.21

| Failure scenario | Required resolution | Main work |
|---|---|---|
| A new “plane” bypasses the incumbent owner | Existing-owner map, independent admission and deterministic effect barrier | B223/B225/B236/B242 |
| High score on one bad candidate becomes certainty | Relative probability separated from calibrated correctness; all-bad/singleton tests | B230 |
| Reordered labels or missing permutation responses corrupt a winner | Complete label alignment, frozen schedule, explicit wrapper qualification | B229 |
| CLM truncation/cache/reload changes the actual question | Effective-input identity, scope-isolated keys and atomic epochs | B227/B248 |
| Self-evolution changes its own grader or ignores failed trials | Protected edit surface, preregistration, rejected history and independent admission | B234–B236 |
| Checklist omits difficult criteria or invents N/A | Frozen applicability, total criterion coverage and hard failure precedence | B232/B233 |
| Context drops constraints/tool dependencies or recycles a summary | Original-event projections, required closure, bounded recovery/refusal | B239/B240 |
| Repaired or cancelled draft reaches commit | Exact digest verification, terminal states, effect authorization and at-most-once commit | B241/B242 |
| A “cheap” experiment hides retries, failures or selector cost | Full-task shared reservation, identity joins and explicit unknowns | B243/B244 |
| A new provider activates through registry discovery | Eligibility before scoring, quarantine, revocation and explicit admission | B237/B238/B248 |
| Fixture PASS is imported as live evidence | Separate receipts, locked scopes and real-source/peer gates | B224/B249/B250/B247 |

## Remaining work on the actual repository

The pack is ready for non-destructive intake and bounded implementation. It is not a patch that adds all learned functionality. Real work remains: update the trusted source gate integration, implement negotiated provider records/adapters, wire usage/replay/context/cascade semantics into the existing runtime, run Rust regression tests, qualify actual models and perform an independent low-risk pilot. A separate MiCode source review is needed before its consumer pack can be updated.

B251–B254—outer-loop meta-policy learning, large-candidate ANN retrieval, learned specialization and best-of-N—remain optional. Accepting this pack does not require them to succeed or require any named research system to beat the incumbent.
