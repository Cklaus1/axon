# Axon 0.22 — targeted integration assessment

Date: 2026-09-24.

## Recommendation

Use **Axon Cortex 0.22 — Closed-Loop Integration** as a coordinated release target for the existing 0.21 owner profiles, a Compute Fabric M0–M2 implementation, a source-aligned MiCode bridge, and one bounded self-improvement pilot. This is an additive integration milestone, not a mandate to implement every research adapter or every backlog feature.

This document is an assessment, **not a v0.22 build pack, source patch, product-gate receipt, or proof of autonomous improvement**. Inspection was targeted to the bridge, experiment, execution, worktree, trust, and release-boundary seams. No Rust compilation, full runtime audit, real backend qualification, live provider run, or end-to-end three-system trial was performed. Cargo was unavailable in this environment.

## Source-derived findings

- MiCode's embedded Axon support docs are v0.3 and label the bridge/challenger program Intended. The prior Axon 0.21 package's MiCode 0.15 item was only a proposed consumer delta. Reconcile these version axes; do not assume that a 0.15 runtime is delivered.
- MiCode already contains worktree lifecycle and local permission mechanisms; extend those owners rather than creating parallel implementations. The inspected spawner documents remaining scope subset-validation limits.
- The named execution-context receipt is specified but its named types were not located in the supplied Rust sources. Treat it as requiring implementation/use-site verification, not as proven absent under every possible name.
- The trust-gate graduation constant remains NoGo. Do not bypass that gate because a new Axon evaluator has been introduced.
- Fabric is a proposed pack. Its protected M1 explicitly includes real Linux microVM qualification; M2 is logical branching, not RAM forking.
- Axon 0.21 treats its plane names as aliases over existing CX owners. Learned ranking is not permission, evidence hashing is not independent verification, and package tests are not product evidence.

## Proposed release boundary (recommendation, not an existing requirement)

1. Source/version reconciliation and one owner map across CX/MX/ACF; retain existing schemas and historical implementation evidence.
2. One versioned exchange joining task/arm/trial/attempt/operation, policy, source/workspace, environment, approvals, usage, and independent outcome evidence. Adapt existing MX-12/CX-16 and the ACF envelope rather than inventing a second ledger or bridge.
3. Fabric M0–M2: durable lifecycle/reservations, immutable workspaces, one physically qualified execution profile, independent checks, logical branches, atomic promotion, and safe cancellation/reconciliation.
4. Real MiCode round trips with observed execution-context preflight, authoritative local permission enforcement, policy consumption, structured evidence return, and unsupported/failure/refusal semantics.
5. One narrowly authorized policy experiment: frozen model/task family/budget/authority/verifier, one mutable tool- or skill-shortlisting policy, independent held-out evaluation, controlled admission, future-task uptake, and rollback.
6. Keep inference coordination in the trusted, permission-enforced MiCode/host path while the first Fabric guest performs offline code execution/checks. Any extension that places API credentials or networking inside a guest must satisfy the additional egress/secret requirements rather than silently weakening the first profile.

Do not require all CLM/Jev/router/cascade implementations to be production-ready to validate integration. Preserve deterministic controls; leave unqualified learned adapters disabled or in shadow. Delay RAM forks, general remote/provider placement, GPU scheduling, generic hosted WASM, outer-loop changes to admission/evaluation, and broad automatic core-code promotion.

## Separate engineering acceptance from evidence of improvement

Engineering acceptance must demonstrate real dispatch, exact context/identity, independent outcomes, rejection of bad/unknown candidates, no lost budget on cancellation, admitted policy uptake on later tasks, and safe rollback. A measured improvement claim additionally needs a prespecified task distribution and statistical decision rule, independent evaluation, complete cost accounting, and evidence supporting the claimed benefit. Do not require a forced winner: reject or report inconclusive when the evidence does not justify promotion.

## Input identities

- `micode-src-8ffc2504(1).zip` — SHA-256 `3c0af45a4f9a7f7228797d8eb825ed8f566501a3502d5f196423aea26c45e3d6`
- `Axon_Cortex_Build_Files_v0_21.zip` — SHA-256 `54faa62c6befbbbdceec2f38fa9139d3ec59182d8c97c45d7dd3cde5d62f4684`
- `Axon_Compute_Fabric_Spec_Pack_v0_1(1).zip` — SHA-256 `cb898888fac8a59a6ec729a88c14c15b34f5ed660463f6c68b40a69805a509fa`

## Source excerpts

Line numbers below refer to the actual extracted input files, not to this assessment. Excerpts are source evidence; recommendations above are explicitly separate.

### S01 — `micode/docs/axon-support/README.md`

MiCode support documentation is labeled v0.3. Its current/intended distinctions are explicit. This does not establish the version of any separately maintained, unavailable support pack.

Source lines 1–37; file SHA-256 `b154dd731e18eb6f9d39a861cd1c033717f79831e86ba7a53db3c5edc94f972f`.

```text
1: # MiCode → Axon Support Program v0.3
2: 
3: **Document type:** implementation roadmap · spec family · task DAG · build protocol  
4: **Primary objective:** evolve MiCode from a coding-agent product into the controlled software-world laboratory, evidence generator, transfer benchmark plane, and knowledge interface that accelerates Axon Cortex and the Axon self-optimizing OS.
5: 
6: ## One-sentence thesis
7: 
8: MiCode should not merely become faster at orchestration. It should become the **instrumented coding world in which Axon learns how software behaves, how coding intelligence succeeds and fails, which bounded decisions transfer across unseen repositories, which abstractions survive protected evaluation, and which repeated behaviors deserve promotion into Axon Reflex, libraries, compiler passes, or runtime primitives.**
9: 
10: ## Scope
11: 
12: This package specifies MiCode-side work. It does **not** assume Axon Cortex is already implemented. MiCode remains independently useful and continues to enforce its own authority model.
13: 
14: The program adds eight platform roles:
15: 
16: 1. **Experience engine** — every eligible coding episode becomes structured, replayable evidence.
17: 2. **Intent/build bridge** — solution descriptions become typed intent, build specs, `/build-loop` execution, and evidence-bound completion.
18: 3. **Software observer** — repository state is represented as typed semantic objects, not only transcript text.
19: 4. **Reflex corpus producer** — shadow decisions become exact state/question/candidate/outcome records for Axon CX-20.
20: 5. **Experimental harness** — planners, retrievers, Reflex backends, generators, and verifiers can be compared under controlled tasks.
21: 6. **Coding Frontier environment** — incumbent/challenger systems run under matched authority, budgets, hidden acceptance checks, and T0–T5 transfer tiers for Axon CX-21.
22: 7. **Knowledge miner** — external repositories and histories yield evidence-backed pattern candidates while protected benchmark worlds remain uncontaminated.
23: 8. **Axon bridge** — admitted traces, datasets, benchmarks, abstractions, tasks, and results flow into Axon Cortex without transferring MiCode authority.
24: 
25: ## Status vocabulary
26: 
27: Use MiCode's own vocabulary literally: **Current**, **Intended**, **Partial**, **Deferred**, **UNVERIFIED**. No `Current` claim in these files means a new behavior exists unless a live use-site anchor is named in the repository.
28: 
29: ## Critical current limitations
30: 
31: The supplied MiCode snapshot states that there is no execution sandbox for approved shell work; `DelegationScope::path_globs` and `tool_allowlist` are not enforcement mechanisms; upward child→parent taint is partial; and several long-lived-work/cancellation semantics remain partial or deferred. Therefore experimental Reflex/world-model components begin as measurement/recommendation layers and may not widen authority.
32: 
33: ## Read order
34: 
35: 1. `ARCHITECTURE.md`
36: 2. `build/BUILD_PLAN.md`
37: 3. `build/LOOPS.md`
```

### S02 — `micode/docs/axon-support/build/STATUS.md`

The embedded support program labels itself Draft / Intended. Status prose is not a substitute for independently inspecting runtime use sites.

Source lines 1–21; file SHA-256 `4a9c6e9010ccd3510c9b44bc2d32d69209d2ca22bcdfa7a0c0e82677c2cf67fa`.

```text
1: # Program Status
2: 
3: This package is a **Draft / Intended** build program. No new MiCode behavior described here should be treated as built solely because the document exists.
4: 
5: ## Milestones
6: - M0: Not started
7: - M1: Not started
8: - M1I: Not started
9: - M2: Not started
10: - M3: Not started
11: - M4: Not started
12: - M5: Not started
13: - M6: Not started
14: - M7: Not started
15: - M8: Not started
16: - M9: Not started
17: - M10: Not started
18: - M11: Not started
19: 
20: ## v0.3 additions
21: MX-18, MX-19, Reflex-shadow corpus production, T0–T5 transfer benchmarking, and CX-20/CX-21 bridge alignment are all Intended and require live source integration/evidence before any status may be raised.
```

### S03 — `micode/docs/axon-support/specs/MX-12-axon-bridge.md`

MX-12 already owns the versioned bridge. Its rule preserves MiCode local authority; its documented exit gate is a dummy-reader round trip, not a complete live Axon integration.

Source lines 1–30; file SHA-256 `cff24178d3f29472321fa54ae817eccf7754a504d4823ac77fd1638c68a68a5a`.

```text
1: # MX-12 — Axon Bridge
2: **Status:** Intended
3: 
4: ## Objective
5: Create a versioned boundary between MiCode's experience plane and Axon Cortex without coupling MiCode internals directly to Axon implementation details.
6: 
7: ## Exportable artifact families
8: - canonical episodes;
9: - software observations and semantic object catalogs;
10: - bounded decision datasets;
11: - failure-attribution records;
12: - knowledge/abstraction candidates;
13: - benchmark/evaluation bundles;
14: - Skill/Tool/Compiler candidate descriptions.
15: 
16: ## Importable artifact families
17: - AIR/Cortex decision policies;
18: - Reflex backends/adapters;
19: - Axon verifier profiles;
20: - capability schemas;
21: - approved ontology/version maps.
22: 
23: ## Rule
24: Importing an Axon artifact does not grant it MiCode authority. It remains subject to local MiCode policy.
25: 
26: ## Exit gate
27: A recorded MiCode episode can be exported, validated against a versioned schema, consumed by a dummy Axon-side reader, and round-tripped without semantic field loss.
28: 
29: ## Intent/build-loop artifact family (v0.2)
30: Additional exportable artifacts:
```

### S04 — `micode/docs/axon-support/specs/MX-08-challenger-lab.md`

MX-08 already defines matched incumbent/challenger controls and a reproducible evidence bundle.

Source lines 1–21; file SHA-256 `86d518fad4f84aaa2ba7ddce66e6eda67f30b93cd5f720b4d879d525b7a2d24b`.

```text
1: # MX-08 — Challenger / Experimental Harness
2: **Status:** Intended
3: 
4: ## Objective
5: Run controlled incumbent-vs-challenger experiments for MiCode and Axon-support components.
6: 
7: ## Replaceable components
8: Observer, retriever, candidate generator, Reflex backend, planner, generator, verification strategy, context policy, failure attributor.
9: 
10: ## Controls
11: - same task corpus and frozen acceptance contracts;
12: - same authority/tool access unless the experiment explicitly studies them;
13: - grouped train/dev/test splits where learning is involved;
14: - failure cases remain in denominators;
15: - cost, latency, tool calls, model calls, build/test calls, human interventions tracked.
16: 
17: ## Exit gate
18: The harness can compare two decision policies on the same resettable task set and produce a reproducible evidence bundle.
19: 
20: ## Coding Frontier alignment
21: The challenger lab is the execution substrate for MX-19 matched incumbent/challenger runs. It must expose frozen authority, budget, task contract, visible-context policy, and acceptance-bundle identities so CX-21 comparisons cannot win by receiving an easier task.
```

### S05 — `micode/crates/micode-git/src/worktree.rs`

Worktree lifecycle behavior is present in source. Workspace separation should not be misrepresented as OS-level confinement.

Source lines 1–36; file SHA-256 `68db1756a956c0376c20c94a2eb7325e488178741077e95df5a2b0048cefc904`.

```text
1: //! `GIT_SPEC.md` G2 — worktree lifecycle. Unblocks `SUBAGENT_SYNTHESIS_SPEC.md` S3.
2: //!
3: //! S3 asked for isolated child workspaces with three properties and could not name a mechanism.
4: //! This is the mechanism.
5: //!
6: //! # Refuse, don't degrade
7: //!
8: //! S3's own ruling (its Q2): silently sharing the tree with a caller who asked for isolation is
9: //! worse than saying no. So [`WorktreeManager::create`] returns [`Unavailable`] as a *value*, not
10: //! an error to be logged and swallowed — the caller must decide what to do about it, and cannot
11: //! accidentally proceed believing it has isolation it does not have.
12: //!
13: //! **What counts as "unavailable" is narrower than the spec's wording**, deliberately. G2 says "a
14: //! non-git directory or a dirty index"; taken literally, *any* uncommitted change would refuse,
15: //! which would make the feature unusable during normal development — the exact moment isolation is
16: //! most wanted. The rule implemented here is: a dirty **index** refuses, a dirty **working tree**
17: //! does not. The distinction is about what a caller is likely to believe. A new worktree checks
18: //! out `HEAD`, so neither staged nor unstaged edits reach the child; but someone who has *staged*
19: //! work has taken a deliberate step toward committing it and is much more likely to assume the
20: //! child sees it. Unstaged scratch edits carry no such expectation.
21: //!
22: //! # Where worktrees live
23: //!
24: //! Outside the repository, always — `<base>/<session>/<child>`, defaulting to
25: //! `~/.micode/worktrees`. Never nested inside the repo, so `ignore`-based discovery and
26: //! `CodeGraphIndex` walks never see them. MiCode has a documented history of working-tree pollution
27: //! (`MICODE_SESSION_LOG=0`, `MICODE_SKILLS_BUILTINS=off`, `MICODE_CAPTURE_TRUST_CASES=off` all exist
28: //! because of it), and a worktree that survives its child is that failure in a new place.
29: //!
30: //! # Nothing is deleted that holds work
31: //!
32: //! Both [`WorktreeManager::remove_if_unchanged`] and the [`WorktreeManager::reap`] startup sweep
33: //! refuse to destroy a worktree containing changes. "Auto-cleanup when unchanged" is the whole
34: //! claim: a child that wrote nothing leaves nothing, and a child that wrote something leaves it
35: //! for a human. A reaper that deleted uncommitted work to reclaim disk would be a far worse bug
36: //! than the leak it fixes.
```

### S06 — `micode/crates/micode-delegate/src/spawner.rs`

The source documents real tier-ceiling checks and remaining path_globs/tool_allowlist subset-validation limitations. This is a specific gap, not a claim that MiCode lacks all authorization controls.

Source lines 77–102; file SHA-256 `7454a73638668549704d331c15356315d3887ac132859f8fbec41d90670915be`.

```text
77: //! `SpawnError::CapExceeded` — reused rather than adding a new cross-crate `SpawnError` variant
78: //! for one specific cap kind; see this crate's doc comment discipline elsewhere) any requested
79: //! `DelegationScope::tier_ceiling` exceeding the IMMEDIATE parent's own tracked scope, when the
80: //! parent is itself a tracked child of a previous `spawn()` call.
81: //!
82: //! **The "untracked parent" gap is now CLOSED (ROADMAP_EXECUTION_SPEC §15.2 (c)).** Previously an
83: //! "untracked parent" was trusted as "the true top-level root" and had its ceiling check skipped —
84: //! and because `SessionHandle.task_id` was a plain public field, a freshly-fabricated handle this
85: //! spawner never returned was indistinguishable from the real root, so a `Critical`-ceiling child
86: //! spawned under it with NO ceiling check (reproduced empirically by the reviewer). Two changes
87: //! seal it: (1) `SessionHandle.task_id` is now crate-private to `micode-core`, so no downstream crate
88: //! can fabricate a handle by struct literal at all (the compile-time half — `SubagentSpawner`'s
89: //! `root_handle` doc carries the `compile_fail` repro); (2) `spawn` now fails **closed**
90: //! (`CapExceeded`) on any parent whose id it did not itself register, and the genuine top-level root
91: //! is registered via [`SubagentSpawner::root_handle`] rather than being an untracked handle (the
92: //! runtime half). Even the documented `SessionHandle::from_registered` escape cannot smuggle an
93: //! *unregistered* id past `spawn`. Residual, disclosed: a caller who already holds a *registered*
94: //! id can still build a handle for it and spawn under it (consuming that parent's own child slots —
95: //! a minor same-tree DoS, not a ceiling bypass). The formerly-residual *cross-subtree resume* half
96: //! of this is now closed — see the parent-scoping note above.
97: //!
98: //! **`path_globs`/`tool_allowlist` are NOT validated as a subset of the parent's** — only
99: //! `tier_ceiling` and (`SUBAGENT_SYNTHESIS_SPEC.md` §7.10 gap #10) `spawn_allowlist` are checked.
100: //! A real subset check for the globs needs glob-matching semantics this milestone doesn't specify;
101: //! disclosed, not silently skipped (see `BUILD_STATE.md`'s knowledge graph). The spawn allowlist
102: //! has no such excuse — it is plain set membership — which is why it did not join the disclosure.
```

### S07 — `micode/crates/micode-verify/src/graduation.rs`

The published verdict for this trust-gate graduation remains NoGo in the inspected code. This is not a blanket readiness verdict for all MiCode functionality.

Source lines 70–89; file SHA-256 `29ca1c93336b20a7f16d38b18f4760e8b0d5a397337e4aafc93ca5f430d44c2b`.

```text
70: 
71: use micode_core::{AssistantTurn, ContentFailure, TurnContext};
72: 
73: use crate::judge::{GateMode, JudgeCall, JudgeGate, JudgeModel, JudgeOutcome, TieredTrustGate};
74: use crate::replay::{self, CaseOrigin, Expected, ReplayCase, ShadowModeReport};
75: use crate::HeuristicGate;
76: 
77: /// REMAINING_WORK_SPEC §1.2c/§1.2d: the committed, drift-pinned graduation verdict. This is the ONE
78: /// constant the composition root (`micode::SessionAssembly`) reads to decide whether
79: /// `GateMode::Enforcement` is even constructable: requesting enforcement while this is anything but
80: /// `Go` is a hard, fail-closed startup error. It is regenerated ONLY in the same commit that
81: /// regenerates `ENFORCEMENT_GRADUATION_DECISION.md` from a real `evaluate_with` run, and a drift-pin
82: /// test (`published_verdict_matches_the_committed_decision_doc`) asserts the two never disagree —
83: /// the same discipline `BENCHMARK_RESULTS.md`'s regression pins use. Enforcement is therefore
84: /// reachable only after a regenerated GO decision lands here, never by env var alone.
85: pub const PUBLISHED_VERDICT: GraduationVerdict = GraduationVerdict::NoGo;
86: 
87: /// PRD/04 NFR (Tier B latency budget), the denominator half of the
88: /// `tier_b_p95_latency_within_budget` criterion: "Tier B p95 added latency < 15% of mean turn
89: /// time." 0.15 is that 15%.
```

### S08 — `micode/EXECUTION_CONTEXT_RECEIPT_SPEC.md`

The repository contains a detailed expected/observed execution-context receipt specification. Named type searches below did not establish its implementation.

Source lines 34–90; file SHA-256 `991de8603a0bf0b947f1f1155dae223d7be64529748ab73b606d5856b0206563`.

```text
34: 
35: ## The invariant
36: 
37: ```
38: TaskSpec
39:   ↓
40: ExpectedExecutionContext          (declared by the parent, part of task identity)
41:   ↓
42: MiCode provisions worktree
43:   ↓
44: child INDEPENDENTLY observes its own context
45:   ↓
46: expected == observed ?
47:       yes → ExecutionContextReceipt issued → task may transition to InProgress
48:       no  → TASK_NOT_STARTED, reason = EXECUTION_CONTEXT_MISMATCH
49: ```
50: 
51: Two properties carry the weight:
52: 
53: - **The child observes, it is not told.** A parent that passes down its own belief and asks the
54:   child to echo it verifies nothing. The check has value only because the observation is
55:   independent of the expectation.
56: - **It gates the first model turn.** Not the first commit, not the result — refusal must happen
57:   before any tokens are spent, because the whole cost of this bug is work done before anyone looks.
58: 
59: ## `ExecutionContextReceipt`
60: 
61: ```
62: ExecutionContextReceipt {
63:     task_id,
64:     parent_run_id,
65:     repo_id,
66:     expected_base_commit,
67:     observed_head,
68:     expected_branch,
69:     observed_branch,
70:     worktree_id,
71:     working_directory,
72:     write_scope,
73:     read_scope,
74:     build_namespace,        // dedicated cargo target dir
75:     model_identity,         // provider route + model actually resolved
76:     subagent_role,
77:     created_at,
78: }
79: ```
80: 
81: Three parties, one artifact:
82: 
83: - the **task engine** stores it,
84: - the **worker** cannot start without it,
85: - the **integrator** cannot admit a result without it.
86: 
87: ## Hard conditions (implementation worker)
88: 
89: Fail closed on any:
90: 
```

### S09 — `axon21/axon-cortex-build-v0_21/integration/MICODE_V015_PROPOSED_DELTA.md`

The earlier v0.21 delivery explicitly did not deliver or test a MiCode 0.15 counterpart.

Source lines 1–19; file SHA-256 `8a27f578f361807acefa8b1fffd493b713ac32148274908c3d1143c9a7ab6d29`.

```text
1: # MiCode support 0.15 — proposed consumer delta, not delivered peer files
2: 
3: **Evidence:** only the Axon source snapshot and Axon v0.20 build pack were supplied. The previously paired v0.14 requirements are retained as historical owner contracts. This addendum does not modify, package, pin or test MiCode source or its support archive.
4: 
5: ## Required consumer mapping
6: 
7: MiCode should consume existing Axon owner exports with a reviewed v0.21 lock after its actual files are available. Map candidate/proposal IDs, principal scope, negotiated decision profile, model/head/wrapper/calibration identity, effective context projection, usage and refusal/cancellation through the existing experience bridge. Do not add a second MiCode authority for Axon admission or allow an Axon ranking to bypass MiCode PermissionGate.
8: 
9: Use CVM as the context execution policy while Axon owns learned policy/evidence. Preserve original transcripts/artifacts under approved retention and rehydrate only authorized data. SPX branches remain isolated proposals until MiCode's effect and independent-completion checks allow the exact selected candidate. A generated patch is not a committed patch; a model choice is not permission to transmit private source.
10: 
11: ## Compatibility acceptance
12: 
13: Test existing v0.14-compatible paths, new profile negotiation, unsupported profile refusal, incomplete/transport results, candidate-set mismatch, expired calibration, cancellation, next-turn recovery, unknown effect reconciliation, total budget and principal/tenant isolation. Model and wrapper revisions survive round trips without conversion of conditional score to correctness confidence.
14: 
15: Pin actual Axon/provider/MiCode revisions and exported bytes before claiming B247 peer completion. A schema fixture, simulated consumer or copied export file cannot satisfy the real paired gate. Never invent task IDs in the unavailable consumer manifest; attach real IDs after inspecting that pack.
16: 
17: ## Operator handoff
18: 
19: When the consumer source is reviewed, create a separate consumer upgrade plan against its actual evidence. Preserve existing local implementation and task state just as for Axon. Until then, its status is `PROPOSED_NOT_SUPPLIED`, not “MiCode 0.15 complete.”
```

### S10 — `axon21/axon-cortex-build-v0_21/build/RESEARCH_INTEGRATION_V021.md`

The v0.21 aliases map to existing owners, retain independent admission, permit unqualified adapters to remain unsupported, and distinguish offline fixtures from runtime evidence.

Source lines 1–54; file SHA-256 `137f024fd041409c70869710913897c8108343f46b002b1302a6523578faeda9`.

```text
1: # v0.21 research integration contract
2: 
3: **Normative scope:** additive profiles for existing CX owners. This document supersedes earlier conversational proposals for seven independent planes. It does not replace source authority, declare a new runtime ABI or assert implementation.
4: 
5: ## Ownership and version rules
6: 
7: EVO/DEC/EVL/RTR/CVM/SPX/TEL are stable navigation aliases. The authoritative mapping is [SOURCE_OWNER_MAP](../integration/SOURCE_OWNER_MAP.json). CX-11 retains independent admission, CX-03 and source authorization retain effect permission, CX-10 retains canonical evidence, CX-28 retains working-set projections, CX-34 retains capability registry/scheduling, and CX-37 retains ACE provider/runtime ownership. CX-36 bytes and neural wire formats are unchanged.
8: 
9: The live source's CX-35 Reflex work maps to CX-05 and related existing owners without renaming the source ID or consuming the package's reserved ID. New work uses B223–B254 and explicitly prefixed `Gxx-r21-*` gates. Old requirements, source mappings and MiCode v0.14 origin references remain historical contracts, not proof of current peer delivery.
10: 
11: No new `.reflex`, `.clm`, `.dec` or alternate neural container is introduced. Model/head revisions belong in the existing artifact lifecycle. A provisional research record schema is a package reference/negotiation payload, not an automatically accepted `axon-reflex/1` request or new top-level ACE wire tag.
12: 
13: ## Shared flow
14: 
15: ```text
16: canonical observations + current grants + registry epoch
17:   -> authorized candidate view (CX-03/CX-34)
18:   -> bounded state projection (CX-28)
19:   -> eligible deterministic / CLM / Jev decision provider (CX-05)
20:   -> qualified calibration and routing policy (CX-06)
21:   -> optional isolated draft/verify proposal (CX-26)
22:   -> existing permission and transactional effect barrier (CX-03)
23:   -> actual final checks and evaluation evidence (CX-01/CX-21)
24:   -> existing replay, audit, usage and learning records (CX-10)
25:   -> independent strategy/head admission, never self-activation (CX-11)
26: ```
27: 
28: Eligibility is checked before sending private context or candidates to a provider. Ranking an action is not authorization to execute it. Evaluation may consume DEC judgments, but deterministic completion and independent admission are not delegated to DEC. EVO can propose policies for these components but cannot change the frozen evaluator, safety floor or approval authority during an experiment.
29: 
30: ## Seven profile guides
31: 
32: The [EVO guide](EVO_REGULARIZED_EVOLUTION.md), [DEC guide](DEC_RESEARCH_PROVIDERS.md), [EVL guide](EVL_CHECKLIST_EVIDENCE.md), [RTR guide](RTR_MODEL_ROLE_ROUTING.md), [CVM guide](CVM_CANONICAL_PROJECTIONS.md), [SPX guide](SPX_BOUNDED_CASCADES.md), and [TEL guide](TEL_WHOLE_TASK_ECONOMICS.md) define implementation invariants. Every guide is an Axon adaptation, not a claim that its research source already implements these protections.
33: 
34: ## Common record and lifecycle
35: 
36: Use collision-free `task_id`, `episode_id`, `attempt_id`, `candidate_id` and `call_id`, plus principal/tenant/workspace scope. Repeated trials on the same task and arm must not share identity. Bind records to the source snapshot, candidate manifest, effective input, model/provider runtime, tokenizer, pooling, head, wrapper/threshold policy and context projection. Information omitted for privacy must be a permission-qualified reference with explicit availability status, not fabricated content.
37: 
38: A decision result distinguishes raw scores, candidate-conditional probabilities, separately calibrated correctness and a reasoned disposition. `ABSTAIN`, `REFUSED`, `TRANSPORT_ERROR`, `CANCELLED`, `UNSUPPORTED` and a legitimate negative judgment are not interchangeable. Missing evidence/usage is unknown, never automatically zero or false. Existing refusal semantics remain intact.
39: 
40: Record exact nondeterministic results for replay. Replaying must not query live models, recompute a new context policy or reuse a different cache epoch. An episode digest binds content but is not, alone, externally anchored append-only integrity. Integrate the existing audit/recording mechanisms rather than advertising a second hash-based proof layer.
41: 
42: ## Research versus release requirements
43: 
44: A P0 research priority means a required contract, migration seam and falsification plan. It does not mean all providers must be installed, all papers reproduced, or all models perform better than the incumbent. The initial release accepts a deterministic provider and explicit unsupported outcomes where learned adapters are not qualified.
45: 
46: The package conformance closure is deliberately small. Runtime build tasks remain explicit and model-free fixture success cannot satisfy them. Real pilots need a reviewed source revision, authenticated endpoints, frozen evaluation policy, budget, independent final outcome and rollback. A rejected/inconclusive research arm is a valid result; forcing a named method into production is not an acceptance criterion.
47: 
48: Optional scopes B251–B254 are outer-loop meta-policy learning, ANN-scale candidate selection, specialization and best-of-N. Native latent cache handoff, new inference engines, unrelated sandboxes, new ABI work and multimodal CLM support are outside this delta.
49: 
50: ## Compatibility posture
51: 
52: Keep the existing deterministic Cortex policy adapter and grant boundary. Extend a negotiated Reflex capability rather than stuffing new fields into its strict existing wire. A legacy `/1` request retains existing meaning; unknown versions/profiles refuse. Provider output is bounded and schema-validated before entering the host; score fields cannot modify the grant snapshot. Permission denial must not trigger a retry as another principal.
53: 
54: Use existing feature flags/admission records. Start new research profiles disabled, then offline, then shadow, then one explicitly authorized low-risk scope. The rollback target is the prior source/registry/model/config revision, not merely a different model name. Sidecar/model processes are opt-in and confined; no installer edits shell startup files, starts background daemons or downloads weights during package validation.
```

### S11 — `fabric/axon_compute_fabric_v0_1/README.md`

The Compute Fabric pack is Draft / Proposed; no fabric implementation is included.

Source lines 1–26; file SHA-256 `1afdaa3b2bc955d1452cc62a12b9d132e26b170e24d637a428e19408ba17355c`.

```text
1: # Axon Compute Fabric — source-reviewed specification pack v0.1.0
2: 
3: **Status: Draft / Proposed. No fabric implementation is included.**
4: 
5: This package reviews the supplied Axon source and specifies an additive Compute Fabric around existing authority and runtime seams. It does not overwrite the source, rename existing Cortex types, claim a v0.20 package review, or mark any new product gate as passed.
6: 
7: ## Read first
8: 
9: [ACF-01 specification](specs/ACF-01-AXON-COMPUTE-FABRIC.md) defines architecture, ownership, profiles, authority, lifecycle, budgets, storage, checkpoints, forks, Cortex integration and the optional provider boundary.
10: 
11: [Source review](review/SOURCE_REVIEW.md) explains what exists, what is missing, and which earlier assumptions the code changes. [Source evidence excerpts](review/SOURCE_EVIDENCE.md) provide 36 exact path/line ranges. [Adversarial review](review/ADVERSARIAL_REVIEW.md) records the second-pass challenges and the resolutions folded into the specification.
12: 
13: [Implementation plan](build/IMPLEMENTATION_PLAN.md) contains 14 dependency-ordered work packages. [Acceptance gates](build/ACCEPTANCE_GATES.md) define 55 product gates, all **NOT_RUN**. [Bootstrap prompt](build/BOOTSTRAP_PROMPT.md) is ready to hand to an implementation agent against the actual working checkout.
14: 
15: ## Principal decisions
16: 
17: **Axon retains authority.** Reuse `axon-os` grants, approval and supervisor admission; `Runtime` for compatible program execution; `AxonHost` for interpreter effects; and Cortex's authorized action and evaluation boundaries. Native and remote backends supply resources, not identities or permission.
18: 
19: **Describe reality, not a tier diagram.** The supplied `axon-vm` is the Firecracker microVM launcher, not a separate full-VM service. `axon-wasm` is a browser interpreter build. The custom guest kernel leaves full interpreter/VFS execution unfinished. Profiles describe exact engine/enclosure/guest/OS/architecture/placement combinations with explicit evidence status.
20: 
21: **Start with one protected execution path.** Implement a real registered Cortex check in an isolated Linux profile, immutable workspaces, journaled lifecycle, reservations, cancellation and trustworthy results. Add logical branches and CAS promotion next. RAM forks, a bounded WASM host, egress, one remote provider and learned routing are independently gated later work.
22: 
23: **Preserve provenance and compatibility.** Link existing episode, observation and run-record digests through a new versioned envelope. Separate task/trial/attempt/operation IDs, durable workspace contents and runtime-specific checkpoints. Never convert a worker's exit status or hash into independent verification.
24: 
25: ## Source binding and scope
26: 
```

### S12 — `fabric/axon_compute_fabric_v0_1/build/IMPLEMENTATION_PLAN.md`

The intended M1/M2 dependency chain includes actual hardened Linux microVM qualification and a registered CheckExecutor before logical branching. M0-M2 is not just lightweight schema work.

Source lines 53–107; file SHA-256 `2c68d29639289c25df2a43883ddcafdd2823c701ffa90a96db224440c1fad6ee`.

```text
53: Implement journal-before-effect, idempotency, fenced leases, aggregate resources, cancellation and outstanding cleanup/billing obligations.
54: 
55: **Proposed edit surface:** compute lifecycle/journal/budget modules; supervisor integration.
56: 
57: **Required gates:** ACF-G11, ACF-G12, ACF-G13, ACF-G14, ACF-G15, ACF-G16.
58: 
59: Completion evidence must name the executed test, invoker, target profile/configuration, source revision, nonzero assertions and observed result. A mock proves only its contract, never the physical backend.
60: 
61: ## ACF-T04 — Immutable workspace store and safe materializer
62: 
63: **Milestone:** M1. **Status:** Not started. **Dependencies:** ACF-T01, ACF-T03.
64: 
65: Store and verify actual file content and metadata, isolate tenants and worktrees, preserve scoped observation digests and pin retention references.
66: 
67: **Proposed edit surface:** compute workspace/artifact modules; Cortex observation projection.
68: 
69: **Required gates:** ACF-G17, ACF-G18, ACF-G19, ACF-G20.
70: 
71: Completion evidence must name the executed test, invoker, target profile/configuration, source revision, nonzero assertions and observed result. A mock proves only its contract, never the physical backend.
72: 
73: ## ACF-T05 — Extract axon-vm library without CLI drift
74: 
75: **Milestone:** M1. **Status:** Not started. **Dependencies:** ACF-T00.
76: 
77: Move reusable launch/config/result logic from main.rs into a library; own cleanup resources. Preserve existing CLI, exit codes and attestation gates.
78: 
79: **Proposed edit surface:** crates/axon-vm/src/lib.rs plus modules; existing main.rs remains thin.
80: 
81: **Required gates:** ACF-G21, ACF-G22.
82: 
83: Completion evidence must name the executed test, invoker, target profile/configuration, source revision, nonzero assertions and observed result. A mock proves only its contract, never the physical backend.
84: 
85: ## ACF-T06 — Harden and verify the Linux microVM profile
86: 
87: **Milestone:** M1. **Status:** Not started. **Dependencies:** ACF-T01, ACF-T03, ACF-T04, ACF-T05.
88: 
89: Pin a working Linux guest/init policy path; enforce full scopes and host resources; prove real registered workload execution and no-NIC/offline confinement.
90: 
91: **Proposed edit surface:** axon-vm Linux backend; axon-guest-init protocol; host-image build and KVM tests.
92: 
93: **Required gates:** ACF-G23, ACF-G24, ACF-G25, ACF-G26, ACF-G27, ACF-G28.
94: 
95: Completion evidence must name the executed test, invoker, target profile/configuration, source revision, nonzero assertions and observed result. A mock proves only its contract, never the physical backend.
96: 
97: ## ACF-T07 — Cortex registered CheckExecutor vertical slice
98: 
99: **Milestone:** M1. **Status:** Not started. **Dependencies:** ACF-T03, ACF-T04, ACF-T06.
100: 
101: Inject a typed check executor into Runner, preserving action authorization and protected grader semantics. Link independent execution/verifier receipts to episodes.
102: 
103: **Proposed edit surface:** axon-cortex/src/runner.rs; new executor seam; cortex-policy-adapter compatibility tests.
104: 
105: **Required gates:** ACF-G29, ACF-G30, ACF-G31, ACF-G32, ACF-G33.
106: 
107: Completion evidence must name the executed test, invoker, target profile/configuration, source revision, nonzero assertions and observed result. A mock proves only its contract, never the physical backend.
```

### S13 — `fabric/axon_compute_fabric_v0_1/specs/ACF-01-AXON-COMPUTE-FABRIC.md`

Independent candidate verification and fenced compare-and-swap promotion have existing fabric ownership; selection does not itself authorize deployment or external effects.

Source lines 250–258; file SHA-256 `472993ba33fc64cf78552f9f51a01e7e4376f663775f2f9da11b67aee519792e`.

```text
250: The existing `Runner::stage_copy` copies only immediate regular files. Keep it a fixture helper until a separate recursively safe materializer is tested; do not advertise it as workspace backup. [E32]
251: 
252: ### 8.3 Promotion
253: 
254: Workers produce immutable candidate versions. An independent verifier runs against the precise candidate plus a protected verifier/environment digest. Promotion checks that the base reference and authority epoch still match, then atomically advances the approved workspace reference with a compare-and-swap. A stale base produces a conflict and explicit rebase/reverification, not an overwrite.
255: 
256: First release: one writer per branch and promotion authority separate from execution authority. Parallel branches have independent writable trees. Shared mutable volumes are not permitted as a shortcut. No automatic RAM merge exists. Branch selection does not replay writes to external services or deploy a candidate.
257: 
258: ## 9. Checkpoints, resume and forks
```

### S14 — `fabric/axon_compute_fabric_v0_1/specs/ACF-01-AXON-COMPUTE-FABRIC.md`

The initial protected execution profile is offline/no-egress. Later guest egress requires a separately qualified boundary.

Source lines 316–322; file SHA-256 `472993ba33fc64cf78552f9f51a01e7e4376f663775f2f9da11b67aee519792e`.

```text
316: ## 11. Egress, secrets, imports and trust
317: 
318: Initial protected work uses offline fixtures and no egress. Later egress requires a trusted broker outside the guest plus enforcement that prevents bypass. Destination, method/path, scope, redirect and credential policy are checked at the actual boundary; raw IPs, DNS changes, proxies, alternate protocols, metadata endpoints and existing connections are included in tests.
319: 
320: Credential injection is permitted only for explicitly integrated protocols/services where destination and request semantics can be authenticated. A generic TLS byte tunnel cannot magically inject HTTP credentials. Other cases use short-lived scoped credentials with a documented exposure profile, or refuse. Never claim that brokered credentials eliminate all sandbox secrets or external effects.
321: 
322: The broker binds each request to principal, attempt, current epoch, grant and remaining reservation. It keeps credentials and audit authority outside worker-accessible memory and disks. Provider API credentials remain in the trusted driver service, not worker images. Artifact upload, package download, preview URL creation and desktop streaming are all separately scoped export/access operations.
```

### S15 — `fabric/axon_compute_fabric_v0_1/specs/ACF-01-AXON-COMPUTE-FABRIC.md`

The existing Compute Fabric proposal integrates through CheckExecutor, a versioned sidecar evidence envelope, and the existing cognitive scheduler rather than replacing them.

Source lines 342–356; file SHA-256 `472993ba33fc64cf78552f9f51a01e7e4376f663775f2f9da11b67aee519792e`.

```text
342: ## 13. Cortex, Axon evidence and ASI OS integration
343: 
344: Introduce `CheckExecutor` into the runner so a registered check becomes a typed compute job rather than a direct `Command::output`. Preserve the `Authorized` action boundary and grader-not-patchable rules. The worker is not the verifier and stdout is not the supervisor's authoritative result channel. [E29, E31]
345: 
346: A trusted, framed result channel identifies the attempt and executable, records exit/timeout/transport outcome, and validates the expected test inventory/schema. An exit code of zero, missing test summary, zero matching checks, duplicate verdict, late forged summary or worker-provided “passed” cannot produce independent verification.
347: 
348: Append execution evidence keyed by `TaskId / ArmId / TrialId / AttemptId / OperationId`, with links to the existing episode, workspace observation, effective authority digest, environment, backend profile, immutable output and verifier receipt. This is one provenance system with typed nodes and relations, not a collection of disconnected graphs. Graph lineage is evidence, not permission.
349: 
350: Do not rewrite historical `axc1:` hashes, `axon-os-record/1`, `axon-vm-run/2` or episode JSON in place. Use an `acf-execution/1` sidecar envelope referencing existing digests. Any stronger wire format has a new schema/version and explicit reader migration. Old missing fields remain unknown, never retroactively verified. [E09, E18, E26–E27]
351: 
352: The ASI OS `Computer` presentation maps to an execution/workspace/lease. A stateless pure WASM job need not pretend to have a terminal or desktop. Later human attach grants are read-only or read/write separately, short-lived and exclusive where needed. Taking control fences agent input; releasing control rechecks policy and records provenance.
353: 
354: The fabric may publish measured runtime capabilities to CX-34. The cognitive scheduler keeps ownership of goals, actions, model choices and experiments. The compute broker performs constrained placement and lifecycle management. Neither learns away a safety requirement. [E36]
355: 
356: ## 14. Remote providers and portability
```

### S16 — `fabric/axon_compute_fabric_v0_1/specs/ACF-01-AXON-COMPUTE-FABRIC.md`

Fabric rollout, fail-closed behavior, and definition of done separate schema validation from observed runtime execution.

Source lines 376–398; file SHA-256 `472993ba33fc64cf78552f9f51a01e7e4376f663775f2f9da11b67aee519792e`.

```text
376: ## 16. Acceptance, rollout and rollback
377: 
378: `build/ACCEPTANCE_GATES.md` is the product-gate inventory. Every gate starts `NOT_RUN`. Passing the package validator means the proposal is internally consistent; it does not admit any runtime.
379: 
380: Rollout order:
381: 
382: **M0 — source-aligned contracts.** Pin source and existing behavior, add typed profiles, strict request parsing, authority convergence and feature flags. No provider is marked production-verified from inspection.
383: 
384: **M1 — protected native vertical slice.** Add durable reservation/lifecycle state, safe workspaces, hardened Linux execution and a fabric-backed registered Cortex check. The same task passes without touching the operator workspace; hostile and crash fixtures fail closed.
385: 
386: **M2 — logical experimentation.** Branch immutable workspace versions, run isolated alternatives, independently evaluate and promote with CAS and aggregate budgets. No memory fork is required.
387: 
388: **M3 — independently gated extensions.** Memory checkpoints/forks, bounded WASM host, egress broker and one remote driver each ship only for their tested profile. Desktop/GPU and broader OS support remain capability-specific.
389: 
390: **M4 — measured routing.** Rules first; shadow comparison, fixed workload evaluations and operator-approved policy versions precede any learned routing. The hard admission filter is unchanged.
391: 
392: Feature-off preserves old CLI behavior and stored records. A protected workload never rolls back to weaker confinement; it is paused/refused until an admissible backend is available. Before rollback, drain or reconcile active executions, retain their budgets and artifact leases, and keep readers for the new envelope until retention expires.
393: 
394: ## 17. Definition of done
395: 
396: A release is done only when its required product gates actually ran on the declared host/backend profile, negative fixtures produced no unauthorized effects, old behavior/schema tests pass, execution and verifier outcomes remain distinct, repeated trials are independently attributable, lifecycle failures reconcile safely, artifact promotion is atomic, and cost/liability accounting includes failed and abandoned work.
397: 
398: This proposal package is done when the source evidence is traceable, assumptions and absent implementations are explicit, contracts/examples validate, every work package has dependencies and nonvacuous product gates, and the bootstrap prompt prevents overwriting existing work. **The package does not claim that the fabric has been implemented.**
```

## Targeted symbol-search limits

Searched all `.rs` files under the supplied MiCode `crates/` for these literal names. A zero is a search observation, not proof that equivalent behavior cannot exist under another name.

| Literal symbol | Matching Rust files |
|---|---:|
| `ExecutionContextReceipt` | 0 |
| `ExpectedExecutionContext` | 0 |
| `execution_context_receipt` | 0 |
| `observed_head` | 0 |
| `expected_base_commit` | 0 |
| `AxonBridge` | 0 |
| `axon_bridge` | 0 |
| `ComputeJob` | 0 |
| `TrialId` | 0 |
| `trial_id` | 0 |

The supplied MiCode archive extracted to 859 files. This targeted assessment does not claim that every file was reviewed.
