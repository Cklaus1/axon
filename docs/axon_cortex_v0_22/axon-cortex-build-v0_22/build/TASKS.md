# Axon Cortex implementation task DAG — v0.22

Generated from `task_manifest.json`. Task statuses are **Not started** in this proposed package; they do not reset or establish the existing repository implementation status. Documentation/reference tests do not mark product work complete. Historical IDs are preserved; amendments are logged in `review/BASELINE_GAPS.json`.

See [release slices](IMPLEMENTATION_RELEASE_SLICES.md) for bounded entry points. A late stage denotes maturity; depend only on explicit task edges, not the numeric task identifier. Optional research is not required to succeed.

| Task | Stage | Work package | Depends on | Spec owners | Gate targets | Lane | Optional |
|---|---|---|---|---|---|---|---|
| B00 | M0 | Import and reconcile the package | none | CX-00 | G00-contract | contracts | no |
| B01 | M0 | Audit Axon seams and engine guarantees | B00 | CX-00, CX-15 | G00-missing-gate, G15-parity | contracts | no |
| B02 | M0 | Freeze task, safety and evaluation policy | B01 | CX-00, CX-01 | G00-contract, G01-evidence | evaluation | no |
| B03 | M0 | Build resettable task fixtures and simple controls | B02 | CX-01 | G01-reset, G01-fairness | evaluation | no |
| B04 | M0 | Implement common manifests and event schemas | B01, B02 | CX-00, CX-04, CX-10 | G00-contract, G04-schema, G10-lineage | contracts | no |
| B05 | M1 | Implement partial software observations | B03, B04 | CX-02 | G02-canonical, G02-partial, G02-recall, G02-injection, G02-stale | observer | no |
| B06 | M1 | Implement grants and denial-first catalog | B04, B05 | CX-03 | G03-forgery, G03-budget | executor | no |
| B07 | M1 | Implement the H0 isolated tool host | B01, B04, B06 | CX-00, CX-13 | G13-escape, G13-kill, G13-quota, G00-authority | executor | no |
| B08 | M1 | Implement local patch transactions | B05, B06, B07 | CX-03 | G03-payload, G03-race | executor | no |
| B09 | M1 | Execute registered checks with durable action states | B03, B07, B08 | CX-03, CX-13 | G03-crash, G13-recovery | executor | no |
| B10 | M1 | Implement AIR validation and serial scheduler | B04, B06 | CX-04 | G04-schema, G04-effect, G04-scheduler | runtime | no |
| B11 | M1 | Add mock decision and generation adapters | B10 | CX-05 | G05-schema, G05-branch | models | no |
| B12 | M1 | Implement protected final verification | B03, B09 | CX-01, CX-03 | G03-done, G01-leakage | evaluation | no |
| B13 | M1 | Integrate raw/redacted events and exact replay | B04, B09, B10, B11 | CX-04, CX-10 | G10-replay, G10-secrets, G10-crash, G04-replay | evidence | no |
| B14 | M1 | Run deterministic end-to-end conformance | B11, B12, B13 | CX-00, CX-01, CX-03, CX-10 | G00-mutation, G03-done, G10-replay | integrator | no |
| B15 | M1 | Add an authorized existing-model backend | B02, B07, B10, B13 | CX-04, CX-05 | G04-fallback, G05-schema | models | no |
| B16 | M1 | Compare the real-model vertical slice to controls | B14, B15 | CX-01 | G01-ablation, G01-evidence | integrator | no |
| B17 | M2 | Freeze backend-neutral Reflex ABI and score provenance | B15, B16 | CX-05 | G05-schema, G05-tokenization | models | no |
| B18 | M2 | Build Reflex decision corpus, destructive controls and shared-state/speculative path | B17, B21 | CX-05 | G05-branch, G05-isolation, G05-performance | models | no |
| B19 | M2 | Run calibration lab and implement selective routing | B02, B16, B17, B18 | CX-06, CX-20 | G06-calibration, G06-risk, G06-coverage, G06-budget, G20-calibration | models | no |
| B20 | M2 | Run multi-backend Reflex bakeoff and decide adoption | B18, B19 | CX-01, CX-05, CX-06, CX-20 | G05-performance, G06-shift, G01-evidence, G20-reproduce, G20-system-impact, G20-budget | integrator | no |
| B21 | M2/M3/M5 | Implement eligible learning/decision-corpus export and label lineage | B13, B16 | CX-10 | G10-secrets, G10-lineage, G10-attribution, G10-behavior-policy, G10-multi-valid | evidence | no |
| B22 | M3 | Match concrete action predictions to outcomes | B05, B13, B16, B21 | CX-07 | G07-target, G07-unknown | research | no |
| B23 | M3 | Train short-horizon software predictors | B20, B22 | CX-07 | G07-holdout, G07-unknown | research | no |
| B24 | M3 | Evaluate real planning value and model exploitation | B23 | CX-07 | G07-planning, G07-exploitation | evaluation | no |
| B25 | M4 | Implement hypotheses and permitted probe selection | B19, B24 | CX-08 | G08-hypothesis, G08-authority, G08-progress | research | no |
| B26 | M4 | Evaluate controlled interventions and diagnosis | B25 | CX-08 | G08-controls, G08-value | evaluation | no |
| B27 | M5 | Implement independent artifact admission | B12, B19, B21 | CX-11, CX-20 | G11-independent, G11-authority, G20-admission-boundary | admission | no |
| B28 | M5 | Derive one guarded reusable tool/rule/policy | B20, B27 | CX-11 | G11-scope, G11-noninferiority | learning | no |
| B29 | Research | Run learned option-conditioned Reflex learning curves | B20, B21, B27 | CX-14 | G14-pilot, G14-quality | research | yes |
| B30 | M5 | Demonstrate promotion and rollback | B27, B28 | CX-11 | G11-rollback, G11-independent | integrator | no |
| B31 | M6 | Compare fixed alternative problem encodings | B24, B26 | CX-09 | G09-lineage, G09-ablation | research | no |
| B32 | M6 | Test learned concepts and held-out transfer | B31 | CX-09 | G09-counterexample, G09-transfer, G09-compression | research | no |
| B33 | M7 | Unify independently useful pillar lifecycles | B26, B30, B32 | CX-12 | G12-contract, G12-cause | learning | no |
| B34 | M7 | Add bounded curriculum and meta experiment allocation | B33 | CX-12 | G12-meta, G12-curriculum | research | no |
| B35 | M7 | Run longitudinal stability and transfer checks | B34 | CX-12 | G12-stability, G12-meta | evaluation | no |
| B36 | OS-H1 | Harden hosted service operation | B16 | CX-13 | G13-recovery, G13-quota, G13-tier | os | no |
| B37 | Compiler | Audit stable runtime-to-language seams | B01, B16 | CX-15 | G15-types, G15-docs | compiler | no |
| B38 | Compiler | Integrate authoritative types for new semantics | B37 | CX-15 | G15-types, G15-parity | compiler | yes |
| B39 | Compiler | Add justified native/language/proof support | B20, B38 | CX-15 | G15-proof, G15-optimization, G15-parity | compiler | yes |
| B40 | Research | Investigate specialized shared-state Reflex architecture | B29, B30 | CX-14 | G14-parallel, G14-version, G14-release | research | yes |
| B41 | OS-K | Evaluate the separate bare-metal branch | B01, B36, B39 | CX-13, CX-15 | G13-tier, G15-proof | os | yes |
| B42 | M2 | Implement Reflex conformance fixtures and backend feature/effective-input receipts | B17 | CX-05 | G05-schema, G05-isolation, G05-tokenization, G05-batch-identity, G05-candidate-absence, G05-effective-input, G05-primitive-semantics, G05-prompt-boundary | contracts | no |
| B43 | M0/M2 | Implement dependency/model/dataset adoption manifests | B01, B02 | CX-00, CX-01, CX-13 | G00-contract, G01-evidence, G13-tier, G01-comparability, G13-adoption | supply-chain | no |
| B44 | M1/M2 | Freeze MiCode↔Axon experience bridge and conformance fixtures | B04, B13 | CX-16 | G16-schema, G16-authority, G16-version | contracts | no |
| B45 | M2/M5 | Import one canonical MiCode coding episode into Cortex | B21, B44 | CX-10, CX-16 | G16-lineage, G16-replay, G10-lineage | evidence | no |
| B46 | M8 | Build external repository/history intake and knowledge-candidate extractor | B32, B45 | CX-17 | G17-provenance, G17-license, G17-counterexample | knowledge | no |
| B47 | M8 | Reproduce and evaluate one cross-repository knowledge candidate | B46 | CX-01, CX-17 | G17-reproduce, G17-heldout, G01-evidence | evaluation | no |
| B48 | M9 | Synthesize one guarded skill/tool from admitted knowledge/episodes | B30, B47 | CX-11, CX-18 | G18-ladder, G18-authority, G11-scope | learning | no |
| B49 | M9 | Demonstrate skill/tool promotion, use and deoptimization | B48 | CX-11, CX-18 | G18-deopt, G11-independent, G11-rollback | admission | no |
| B50 | M9 | Demonstrate closed self/external/generated experience loop | B35, B45, B49 | CX-12, CX-16, CX-18 | G12-stability, G16-lineage, G18-ladder | integrator | no |
| B51 | Compiler/OS | Promote a proven capability into Axon library/compiler/runtime | B37, B49 | CX-15, CX-18 | G18-native, G18-equivalence, G15-parity, G15-proof | compiler | yes |
| B52 | M0/M1 | Map existing Axon intent/surface/approval implementation | B01 | CX-19 | G19-parse, G19-render | contracts | no |
| B53 | M1 | Implement versioned Intent IR and round-trip fixtures | B04, B52 | CX-00, CX-19 | G19-parse, G19-ambiguity | contracts | no |
| B54 | M1 | Implement ambiguity resolution and deterministic semantic renderer | B53 | CX-19 | G19-ambiguity, G19-render | surface | no |
| B55 | M1 | Lower approved Intent IR to constrained AIR | B14, B54 | CX-04, CX-19 | G19-authority, G19-trace | integrator | no |
| B56 | M1 | Demonstrate intent-to-evidence vertical slice | B16, B55 | CX-01, CX-19 | G19-evidence, G19-trace | evaluation | no |
| B57 | M9 | Demonstrate self-optimization through ImprovementIntent | B30, B50, B56 | CX-11, CX-18, CX-19 | G19-authority, G19-evidence, G11-independent | learning | no |
| B58 | M2 | Implement Reflex Runtime state handles and branch scheduler | B17, B18, B42 | CX-05 | G05-state-handle, G05-branch | models-runtime | no |
| B59 | M2 | Run question-isolation and candidate-order conformance | B58, B19, B21 | CX-05, CX-06, CX-20 | G05-question-isolation, G05-order-domain, G06-shift, G20-mechanism | evaluation | no |
| B60 | M5 | Compare listwise and option-conditioned Reflex architectures | B20, B21, B27, B29 | CX-14 | G14-listwise, G14-training-score, G14-quality | research | yes |
| B61 | Compiler | Implement AIR question-dependency scheduling optimization | B10, B37, B58 | CX-15 | G15-dependency-types, G15-batch-semantics, G15-optimization | compiler | yes |
| B62 | M2 | Stand up Kev-style Axon Reflex reference pilot | B21, B58, B59 | CX-05, CX-14 | G14-pilot, G14-canonical-encoding | research | yes |
| B63 | M2 | Freeze Reflex train/calibration/dev/locked-test/transfer suites | B21, B02 | CX-01, CX-10, CX-14, CX-20 | G14-locked-test, G01-leakage, G20-registry | evaluation | no |
| B64 | M5 | Run permutation-robustness training study | B62, B63 | CX-05, CX-14 | G14-permutation-training, G05-order-domain | research | yes |
| B65 | M2 | Add typed candidate-absence/control examples | B21, B42 | CX-05, CX-10, CX-14 | G14-absent-candidate | data | no |
| B66 | M2 | Unify canonical decision encoding across train/eval/serve/replay | B58, B63 | CX-05, CX-10, CX-14 | G14-canonical-encoding, G05-schema | runtime | no |
| B67 | M5 | Measure Coding Transfer Frontier | B63, B65, B66 | CX-01, CX-06, CX-14 | G14-transfer-frontier, G14-quality | evaluation | no |
| B68 | M5 | Compare Kev-style transfer improvements | B64, B67 | CX-14 | G14-transfer-frontier, G14-training-score | research | yes |
| B69 | M1 | Define Coding Frontier task/system/run manifests and reset harness | B04, B14, B52 | CX-01, CX-10, CX-19, CX-21 | G21-contract, G21-protected-verifier | evaluation | no |
| B70 | M1 | Freeze first whole-system coding benchmark portfolio | B69, B16 | CX-01, CX-21 | G21-comparability, G21-protected-verifier | evaluation | no |
| B71 | M5 | Add repository/task-family transfer and contamination registry | B21, B44, B63, B70 | CX-10, CX-16, CX-21 | G21-contamination, G21-curriculum-separation | data | no |
| B72 | M5 | Implement Verified Coding Frontier and longitudinal regression reporting | B67, B70, B71 | CX-01, CX-12, CX-21 | G21-frontier, G21-regression, G21-comparability | evaluation | no |
| B73 | M5 | Integrate benchmark evidence with independent admission | B27, B72 | CX-11, CX-21 | G21-admission-handoff, G11-independent | admission | no |
| B74 | M5 | Mine recurring cognitive functions and specialization eligibility | B21, B72 | CX-10, CX-20, CX-21, CX-22 | G22-discovery | data | no |
| B75 | M5 | Build specialized decision-model baselines | B74, B20 | CX-14, CX-20, CX-22 | G22-comparison, G22-applicability | research | no |
| B76 | M5 | Implement specialization ROI, applicability and fallback evaluator | B74, B75 | CX-06, CX-11, CX-22 | G22-roi, G22-applicability, G22-drift, G22-fallback | evaluation | no |
| B77 | M5 | Define neural-program ABI and immutable artifact manifest | B155 | CX-22, CX-23 | G23-artifact, G23-authority | contracts | no |
| B78 | M5 | Local Neural Program backend candidate baseline | B77, B43 | CX-20, CX-22, CX-23 | G23-artifact, G23-typed-output, G23-offline | research | no |
| B79 | M5 | Implement typed neural-program runtime and output validation | B07, B77, B78 | CX-03, CX-13, CX-23 | G23-typed-output, G23-no-fallback, G23-authority, G23-offline | runtime | no |
| B80 | M5 | Implement shared-base skill cache and lifecycle | B79 | CX-13, CX-23 | G23-cache, G23-artifact | runtime | no |
| B81 | M5 | Run neural-program versus specialized/general cognition bakeoff | B76, B79 | CX-20, CX-21, CX-22, CX-23 | G22-comparison, G22-roi, G23-typed-output | evaluation | no |
| B82 | M5 | Implement de-specialization and drift rollback | B76, B80 | CX-11, CX-22, CX-23 | G22-drift, G22-fallback, G23-no-fallback | admission | no |
| B83 | M9 | Connect specialized/neural skills to staged crystallization | B49, B81, B82 | CX-11, CX-18, CX-22, CX-23 | G18-ladder, G18-authority, G22-roi, G23-authority | crystallization | no |
| B84 | M2 | Build semantic perception/extraction reference baseline | B05, B21 | CX-02, CX-20, CX-24 | G24-schema, G24-provenance | research | no |
| B85 | M2 | Build semantic proposition matcher and semantic-grep path | B84, B18 | CX-05, CX-20, CX-24 | G24-semantic-match, G24-composition | research | no |
| B86 | M2 | Implement perception/retrieval router and disclosure policy | B85, B43 | CX-03, CX-06, CX-24 | G24-routing, G24-privacy | runtime | no |
| B87 | M1 | Add completion critic shadow hook | B55, B69 | CX-19, CX-21 | G21-stop-critic | evaluation | no |
| B88 | M1 | Benchmark premature-stop critic and clause extraction | B87, B70 | CX-19, CX-21 | G21-stop-critic, G21-protected-verifier | evaluation | no |
| B89 | M7 | Implement candidate model registry and isolated learner process | B33, B67 | CX-20, CX-25 | G25-candidate-only, G25-version | learning | no |
| B90 | M7 | Implement shadow sampler and candidate replay | B89, B23 | CX-10, CX-20, CX-25 | G25-shadow, G25-version | runtime | no |
| B91 | M7 | Implement guarded weight/adapter transport | B89, B90 | CX-13, CX-25 | G25-transport | runtime | no |
| B92 | M7 | Run mixed-policy/asynchronous learning experiment | B90, B91 | CX-12, CX-20, CX-25 | G25-mixed-policy, G25-version | research | no |
| B93 | M7 | Demonstrate admission-only activation and rollback | B73, B92 | CX-11, CX-21, CX-25 | G25-rollback, G25-candidate-only, G11-independent | admission | no |
| B94 | M2 | Implement typed artifact catalogs and SELECT/PROJECT primitives | B05, B17, B55 | CX-02, CX-05, CX-19, CX-26 | G26-catalog-authority, G26-select-copy, G19-authority | runtime | no |
| B95 | M2 | Implement composition graphs, validator and dependency semantics | B10, B94 | CX-04, CX-15, CX-26 | G26-compose-schema, G26-partial-fallback, G26-dependency-semantics | runtime | no |
| B96 | M2 | Benchmark select/compose against generation | B20, B95 | CX-01, CX-21, CX-26 | G26-composition-benefit, G21-comparability, G01-evidence | evaluation | no |
| B97 | M2 | Pilot Intent-to-Build-Spec/task-DAG composition | B56, B95 | CX-19, CX-21, CX-26 | G26-intent-trace, G19-trace, G26-partial-fallback | integrator | no |
| B98 | M2 | Version semantic decision definitions in the Reflex Lab | B18, B66 | CX-05, CX-20 | G20-definition-lineage, G20-canonical-encoding | research | no |
| B99 | M2 | Run uncertainty-plus-random-audit semantic alignment loop | B63, B98 | CX-20 | G20-active-labeling, G20-no-auto-promote, G20-locked-test | research | no |
| B100 | M5 | Evaluate monolithic versus decomposed decision functions | B96, B99 | CX-20, CX-22, CX-26 | G20-decomposition, G26-composition-benefit, G20-transfer | research | no |
| B101 | M5 | Submit semantic/composition policy candidate through independent admission | B27, B100 | CX-11, CX-20, CX-26 | G20-no-auto-promote, G11-independent, G11-rollback | admission | no |
| B102 | M2 | Freeze semantic supervisor observation/assessment ABI | B13, B54 | CX-10, CX-19, CX-27 | G27-evidence-bound, G27-bounded-observation | runtime | no |
| B103 | M2 | Run shadow semantic supervisor on resettable coding episodes | B69, B102 | CX-21, CX-27 | G27-independent, G27-completion-separation | research | no |
| B104 | M2 | Implement deterministic supervisor intervention policy | B102, B103 | CX-27 | G27-no-authority, G27-deterministic-policy, G27-steer-hysteresis | runtime | no |
| B105 | M2 | Add feature-gated steering and verification requests | B104 | CX-03, CX-21, CX-27 | G27-no-authority, G27-completion-separation, G27-failure-safe | runtime | no |
| B106 | M5 | Benchmark supervisor utility and false-intervention cost | B105, B72 | CX-20, CX-21, CX-27 | G27-independent, G27-steer-hysteresis, G21-regression | research | no |
| B107 | M2 | Freeze semantic working-set artifact and receipt schemas | B04, B54, B102 | CX-02, CX-19, CX-28 | G28-protected-pin, G28-receipt | runtime | no |
| B108 | M2 | Implement authorized rule/skill/map relevance selection | B107, B43 | CX-24, CX-28 | G28-auth-before-rank, G28-fail-safe-rules, G28-stale-reject | runtime | no |
| B109 | M2 | Implement semantic context GC for tool history | B107, B13 | CX-10, CX-28 | G28-context-gc, G28-recompute, G28-protected-pin | runtime | no |
| B110 | M2 | Bind async context decisions to state/catalog freshness | B108, B109 | CX-02, CX-28 | G28-stale-reject, G28-receipt | runtime | no |
| B111 | M2 | Add cache-aware cognitive model routing policy experiment | B19, B107 | CX-06, CX-22, CX-28 | G28-receipt, G28-cascade-trace | research | no |
| B112 | M2 | Add semantic-predicate query planner pilot | B85, B107 | CX-24, CX-28 | G28-auth-before-rank, G24-provenance, G24-semantic-match | runtime | no |
| B113 | M2 | Instrument cognitive cascade execution and fallback reasons | B96, B111, B112 | CX-04, CX-22, CX-26, CX-28 | G28-cascade-trace, G26-partial-fallback | runtime | no |
| B114 | M5 | Run long-horizon working-set and supervisor benchmark | B106, B110, B113 | CX-20, CX-21, CX-27, CX-28 | G27-bounded-observation, G28-protected-pin, G28-receipt, G21-regression | research | no |
| B115 | M5 | Submit supervisor/working-set policy candidates through admission | B27, B114 | CX-11, CX-22, CX-27, CX-28 | G27-no-authority, G28-cascade-trace, G11-independent, G11-rollback | admission | no |
| B116 | M1 | Define cognitive-operation and ImprovementIntent protocols | B04 | CX-10, CX-22, CX-29 | G29-observable, G29-improvement-intent, G29-lineage | contracts | no |
| B117 | M1 | Instrument model routing as first self-application target | B15, B116 | CX-06, CX-28, CX-29 | G29-observable, G29-data-separation | runtime | no |
| B118 | M1 | Instrument working-set and semantic rule/skill selection | B05, B13, B116 | CX-28, CX-29 | G29-observable, G29-kernel-boundary, G28-receipt | runtime | no |
| B119 | M1 | Build frozen replay runner for internal cognitive policies | B13, B116, B117, B118 | CX-10, CX-20, CX-29 | G29-replay-equivalence, G29-data-separation | research | no |
| B120 | M1 | Run first offline challengers for routing and working-set policy | B119 | CX-20, CX-28, CX-29 | G29-replay-equivalence, G29-no-metric-gaming | research | no |
| B121 | M2 | Implement no-effect shadow challenger runtime | B07, B13, B119, B120 | CX-25, CX-27, CX-29 | G29-shadow-no-effect, G29-kernel-boundary | runtime | no |
| B122 | M2 | Shadow-test first self-hosted policy challenger | B121 | CX-21, CX-28, CX-29 | G29-shadow-no-effect, G29-independent-eval | research | no |
| B123 | M5 | Add low-risk self-promotion envelope and canary controls | B27, B30, B122 | CX-11, CX-22, CX-29 | G29-low-risk-envelope, G29-rollback, G29-kernel-boundary | admission | no |
| B124 | M5 | Exercise full internal observe→replay→shadow→admit→rollback lifecycle | B123 | CX-11, CX-21, CX-29 | G29-self-hosting, G29-lineage, G29-rollback | admission | no |
| B125 | M5 | Connect Cognitive Specialization Compiler to self-application episodes | B81, B116, B124 | CX-22, CX-29 | G29-crystallization, G29-independent-eval | research | no |
| B126 | M6 | Add cognitive primitive proposal workflow | B69, B125 | CX-09, CX-12, CX-15, CX-29 | G29-new-primitive, G29-kernel-boundary | research | no |
| B127 | M7 | Enable guarded recursive optimization experiments | B124, B125, B126 | CX-12, CX-20, CX-25, CX-29 | G29-independent-eval, G29-no-metric-gaming, G29-data-separation | research | yes |
| B128 | M1 | Define ProjectImprovementContract and project registry | B04, B54 | CX-19, CX-29, CX-30 | G30-contract, G30-authority, G30-risk-class | contracts | no |
| B129 | M1 | Implement generic repository adapter ABI | B128, B05, B09 | CX-03, CX-10, CX-16, CX-30 | G30-authority, G30-isolation, G30-replay | executor | no |
| B130 | M1 | Create reproducible project baselines and improvement queues | B128, B129 | CX-01, CX-10, CX-30 | G30-baseline, G30-contract, G30-replay | evaluation | no |
| B131 | M2 | Run isolated repository challengers | B07, B129, B130 | CX-03, CX-21, CX-29, CX-30 | G30-isolation, G30-evidence, G30-authority | executor | no |
| B132 | M5 | Add project-local replay, shadow and bounded canary lifecycle | B123, B131 | CX-11, CX-21, CX-29, CX-30 | G30-replay, G30-rollback, G30-evidence | evaluation | no |
| B133 | M2 | Pilot repository optimization across heterogeneous projects | B131 | CX-16, CX-21, CX-30 | G30-contract, G30-baseline, G30-evidence, G30-rollback | evaluation | no |
| B134 | M2 | Add project-family lineage and contamination metadata | B21, B70 | CX-10, CX-17, CX-21, CX-31 | G31-lineage, G31-family-split | research | no |
| B135 | M6 | Implement cross-project pattern candidate miner | B134, B125 | CX-09, CX-17, CX-22, CX-31 | G31-lineage, G31-counterexample, G31-scope | research | no |
| B136 | M6 | Build repository transfer-tier evaluation | B135, B71 | CX-21, CX-31 | G31-family-split, G31-transfer, G31-scope | evaluation | no |
| B137 | M6 | Connect shared capability promotion to CX-11/CX-22/CX-18 | B136 | CX-11, CX-18, CX-22, CX-31 | G31-transfer, G31-scope, G31-native-promotion | research | no |
| B138 | M7 | Implement de-generalization and applicability revision | B137 | CX-11, CX-22, CX-31 | G31-degeneralize, G31-local-contract | research | no |
| B139 | M7 | Run end-to-end project-to-platform improvement pilot | B138, B133 | CX-12, CX-17, CX-29, CX-30, CX-31 | G30-evidence, G31-counterexample, G31-local-contract, G31-degeneralize | evaluation | no |
| B140 | M1 | Freeze Universal Evidence Graph node/edge/claim schemas | B04, B54 | CX-10, CX-32 | G32-node-identity, G32-typed-edges, G32-claim-strength | contracts | no |
| B141 | M1 | Integrate existing receipts and verifier evidence into the evidence graph | B13, B14, B140 | CX-19, CX-27, CX-28, CX-32 | G32-effective-input, G32-admission-closure | runtime | no |
| B142 | M2 | Implement contradiction, invalidation and authorization-aware evidence queries | B141 | CX-11, CX-31, CX-32 | G32-contradiction, G32-invalidation, G32-access-control | runtime | no |
| B143 | M2 | Bind admission and self-application evidence closure to CX-32 | B12, B142 | CX-11, CX-29, CX-32 | G32-admission-closure, G29-lineage, G32-effective-input | evaluation | no |
| B144 | M4 | Freeze causal experiment and intervention schemas | B43, B140 | CX-08, CX-33 | G33-preregister, G33-causal-label, G33-reversibility | contracts | no |
| B145 | M4 | Implement matched cognitive replay experiments | B144, B121 | CX-20, CX-29, CX-33 | G33-single-delta, G33-counterfactual | research | no |
| B146 | M4 | Add active experiment selection by information gain, cost and risk | B145, B08 | CX-08, CX-33 | G33-info-gain, G33-reversibility | research | no |
| B147 | M6 | Run causal mechanism and transfer pilot | B146, B136 | CX-09, CX-31, CX-33 | G33-contamination, G33-mechanism-transfer | evaluation | no |
| B148 | M2 | Build Shared Capability Registry schema and lifecycle | B04, B06, B140 | CX-11, CX-18, CX-22, CX-23, CX-31, CX-34 | G34-registry-identity, G34-admission-only | contracts | no |
| B149 | M2 | Implement hard-constraint cognitive scheduler | B148, B10, B12 | CX-06, CX-13, CX-34 | G34-authorized-candidates, G34-hard-before-soft, G34-abstention | runtime | no |
| B150 | M2 | Add hardware/cache/risk-aware scheduler inputs and receipts | B149, B13, B07 | CX-13, CX-28, CX-32, CX-34 | G34-utility-lineage, G34-risk-reversibility, G34-hardware-aware, G34-cache-freshness | runtime | no |
| B151 | M5 | Benchmark heterogeneous cognitive scheduling and rollback | B150, B81 | CX-21, CX-22, CX-34 | G34-hard-before-soft, G34-utility-lineage, G34-admission-only | evaluation | no |
| B152 | M0 | Resolve Neural Program terminology and ownership | B00 | CX-23, CX-36 | G36-naming | contracts | no |
| B153 | M1 | Implement strict .nps source contract | B152, B04 | CX-19, CX-36 | G36-source | contracts | no |
| B154 | M1 | Implement bounded .np inventory reader | B153 | CX-13, CX-36 | G36-container | runtime | no |
| B155 | M1 | Bind content identity and detached release | B154 | CX-11, CX-23, CX-36 | G36-identity, G36-trust | contracts | no |
| B156 | M2 | Separate compiler and runtime lifecycle interfaces | B155, B07 | CX-23, CX-36 | G36-jobs | runtime | no |
| B157 | M3 | Optional ProgramAsWeights importer | B156 | CX-13, CX-23, CX-36 | G36-import | adapters | yes |
| B158 | M3 | Integrate typed proposal execution | B79, B156 | CX-03, CX-23, CX-36 | G36-typed-io, G36-source-bound, G36-no-authority | runtime | no |
| B159 | M3 | Test shared-base/adapter isolation | B80, B158 | CX-13, CX-23, CX-36 | G36-adapter-isolation | runtime | no |
| B160 | M3 | Enforce applicability and bounded fallback | B158, B149 | CX-06, CX-34, CX-36 | G36-applicability, G36-fallback, G36-offline | runtime | no |
| B161 | M5 | Bind releases to existing capability registry | B155, B148, B27 | CX-11, CX-34, CX-36 | G36-trust | admission | no |
| B162 | M5 | Neural capability scheduler integration | B160, B161, B149 | CX-34, CX-36 | G36-no-authority, G36-fallback | scheduler | no |
| B163 | M3 | Compile eligible experience into candidate artifacts | B21, B153, B79 | CX-10, CX-20, CX-23, CX-36 | G36-reliability, G36-replay | learning | no |
| B164 | M5 | Source-bound diagnostic Neural Program pilot | B162, B163, B30, B159 | CX-21, CX-23, CX-36 | G36-self-host, G36-source-bound, G36-replay | evaluation | no |
| B165 | M0 | Reconcile generated package and gate coverage | B00 | CX-00, CX-01 | G00-assurance-separation | contracts | no |
| B166 | M5 | Test admission invalidation and rollback races | B27, B155, B161 | CX-11, CX-32, CX-34, CX-36 | G11-release-binding, G32-closure-snapshot, G34-revocation-race, G36-rollback | admission | no |
| B167 | M2 | Calibrate named events and protect evaluator feedback | B19, B63 | CX-01, CX-06, CX-20, CX-21, CX-36 | G01-feedback-budget, G06-reliability-event, G20-target-separation, G21-policy-outcome, G36-reliability | evaluation | no |
| B168 | M3 | Bound shadow effects and fallback dispatch | B07, B121, B149 | CX-03, CX-13, CX-34 | G03-fallback-recheck, G13-shadow-egress, G34-bounded-fallback | executor | no |
| B169 | M3 | Preserve evidence pins and bound reflexive recursion | B142, B110, B116 | CX-10, CX-24, CX-27, CX-28, CX-29 | G10-retention-replay, G24-extraction-strength, G27-finality, G28-pin-overflow, G29-recursion-budget, G29-change-risk | evidence | no |
| B170 | M4 | Separate causal estimates from realized outcomes | B145 | CX-33 | G33-estimate-class | evaluation | no |
| B171 | M3 | Version Axon/MiCode bridge migration fixtures | B44, B155, B140 | CX-16, CX-19, CX-23, CX-36 | G19-contract-approval, G23-format-owner | integration | no |
| B172 | M6 | Test scoped transfer and semantic runtime profiles | B136, B147 | CX-05, CX-15, CX-31 | G05-feature-profile, G15-neural-contract, G31-transfer-dimensions | evaluation | no |
| B173 | M6 | Close reviewed end-to-end safety and product gates | B124, B164, B166, B167, B168, B169, B170, B171, B172 | CX-00, CX-11, CX-29, CX-36 | G00-assurance-separation, G29-self-hosting, G36-self-host | integration | no |
| B174 | M2 | Reconcile ACE owner contracts and live source map | B01, B165 | CX-00 | G00-ace-owner-map | integration | no |
| B175 | M2 | Bind ACE axes and interpreter host mapping | B174, B10 | CX-04, CX-15 | G04-ace-lowering, G15-ace-parity | integration | no |
| B176 | M2 | Negotiate deployment features and typed result fidelity | B175, B11 | CX-05, CX-06 | G05-ace-features, G05-ace-results, G06-ace-provenance | integration | no |
| B177 | M2 | Capture physical attempts and enforce root budgets | B176, B07, B13, B140 | CX-13, CX-32 | G13-ace-attempts, G13-ace-locality, G32-ace-attempt-lineage | integration | no |
| B178 | M2 | Bind logical context and optional physical state receipts | B176, B05, B13 | CX-28 | G28-ace-reuse | integration | no |
| B179 | M2 | Integrate branch-aware batching and result joins | B176, B177, B178 | CX-04 | G04-ace-joins | integration | no |
| B180 | M2 | Bind physical profiles to existing hard-filtered dispatch | B176, B177, B149 | CX-34 | G34-ace-dispatch, G34-ace-fallback | integration | no |
| B181 | M2 | Exercise real scorer-specific conformance | B176, B15 | CX-05 | G05-ace-scoring | model-conformance | yes |
| B182 | M2 | Close one real Axon ACE core consumer slice | B16, B175, B176, B177, B178, B179, B180 | CX-16 | G16-ace-core | integration | no |
| B183 | M2 | Reconcile and test the MiCode peer contract | B182 | CX-16 | G16-ace-peer | integration | no |
| B184 | M3 | Run optional three-family Reflex comparison | B182, B181, B20 | CX-20 | G20-ace-three-family | research | yes |
| B185 | M5 | Exercise reviewed Neural Program lifecycle through ACE | B182, B164, B166, B171 | CX-23 | G23-ace-lifecycle | research | yes |
| B186 | M5 | Qualify optional actual native-state reuse | B182, B181, B159 | CX-28 | G28-ace-reuse | research | yes |
| B187 | M2 | Close ACE document coverage and migration checks | B174, B175 | CX-00 | G00-ace-coverage | integration | no |
| B188 | M1 | Migrate current execution naming to ACE and publish legacy aliases | none | CX-37 | G37-naming | integration | no |
| B189 | M2 | Implement immutable ACEExecutionPlan binding | B188 | CX-34, CX-37 | G37-execution-plan | runtime | no |
| B190 | M2 | Define ACE provider logical ABI and transport mapping | B189 | CX-37 | G37-provider-abi | runtime | no |
| B191 | M2 | Implement typed general ACE cognitive-result union | B189 | CX-04, CX-05, CX-23, CX-37 | G37-result-union | runtime | no |
| B192 | M2 | Define ACE streaming and finalization semantics | B190, B191 | CX-37 | G37-streaming | runtime | no |
| B193 | M2 | Enforce ACE cognition versus world-effect boundary | B190, B191 | CX-03, CX-37 | G37-effect-boundary | security | no |
| B194 | M2 | Add provider DataHandlingProfile and scheduler hard filters | B189, B190 | CX-28, CX-34, CX-37 | G37-data-handling | security | no |
| B195 | M2 | Harden native-state isolation, leases and release | B190, B194 | CX-28, CX-37 | G37-isolation | runtime | no |
| B196 | M2 | Add determinism class and execution fingerprint | B190 | CX-10, CX-32, CX-37 | G37-determinism | evidence | no |
| B197 | M2 | Standardize durable ACEExecutionHandle lifecycle | B190, B196 | CX-10, CX-13, CX-37 | G37-durable-handle | runtime | no |
| B198 | M2 | Enforce semantic-preserving fallback graph | B189, B191, B194 | CX-06, CX-34, CX-37 | G37-fallback-semantics | routing | no |
| B199 | M2 | Run ACE provider/transport conformance slice | B190, B192, B193, B194, B195, B196, B197, B198, B183 | CX-16, CX-37 | G37-transport-conformance | integration | no |
| B200 | M2 | Implement calibration registry and calibrated ACE claim checks | B176, B199 | CX-05, CX-06, CX-37 | G05-calibrated-distribution, G06-calibration-registry, G06-proper-score, G37-calibrated-claim | calibration | no |
| B201 | M3 | Build pairwise and multiway Outcome-Calibrated Decision Training reference arms | B20, B200 | CX-05, CX-06, CX-14 | G14-rlcd-hypothesis | research | no |
| B202 | M3 | Prototype packed branch-isolated decision serving | B179, B201 | CX-05, CX-14 | G05-fanout-equivalence, G14-packed-equivalence | research | yes |
| B203 | M3 | Run decision falsification and consistency suite | B200, B201 | CX-05, CX-06, CX-14 | G14-consistency-suite | evaluation | no |
| B204 | M2 | Implement high-cardinality staged decision path | B179, B203 | CX-05, CX-26 | G05-high-cardinality, G26-high-cardinality | runtime | no |
| B205 | M2 | Lower Neural Program learned decisions to fan-out/join scheduling | B155, B179, B200 | CX-23, CX-26, CX-36 | G23-decision-lowering | compiler | no |
| B206 | M2 | Enforce consequence-aware calibrated execution policy | B200, B204, B205 | CX-00, CX-06, CX-26 | G06-risk-policy, G26-calibrated-gate | policy | no |
| B207 | M4 | Join production outcomes and detect calibration drift without self-promotion | B140, B200, B206 | CX-06, CX-10, CX-11 | G06-drift | learning | no |
| B208 | M2 | Reconcile MiCode v0.13 calibrated-decision peer contract | B183, B204, B205, B207 | CX-16 | G16-v019-peer | integration | no |
| B209 | M2 | Close v0.19 RLCD/Jev-informed package and adversarial coverage | B200, B201, B202, B203, B204, B205, B206, B207, B208 | CX-00 | G00-v019-coverage | integration | no |
| B210 | M2 | Freeze schema/frontend and fixed-task qualification owner seams | B02, B165 | CX-03, CX-05, CX-22, CX-26 | G05-fixed-task-binding | contracts | no |
| B211 | M2 | Implement fail-closed normalized schema subset | B210, B04 | CX-03, CX-26 | G26-schema-subset | compiler | no |
| B212 | M2 | Implement active decode, exact projection and invocation validation | B211 | CX-05, CX-26 | G05-active-output-presence, G26-exact-span, G26-active-composition | runtime | no |
| B213 | M2 | Connect schema proposals to trusted effect and event gates | B212, B06, B12 | CX-03, CX-06, CX-37 | G03-schema-trust, G06-full-invocation-event, G37-schema-proposal-only | policy | no |
| B214 | M3 | Build governed teacher jobs and independent fixed-task corpus | B210, B21 | CX-10, CX-20 | G10-teacher-job-scope, G10-grouped-role-isolation, G10-label-adjudication | data | no |
| B215 | M3 | Compare simplest lightweight fixed-task training arms | B214 | CX-20, CX-22 | G20-lightweight-baselines | research | no |
| B216 | M3 | Qualify independent advisory thresholds and protected evaluation | B215, B19, B167 | CX-06, CX-20 | G06-independent-threshold, G06-no-recommendation, G20-protected-release-evaluation | evaluation | no |
| B217 | M2 | Qualify runtime-bound lightweight artifacts and shared encoders | B210, B155, B176, B148 | CX-05, CX-23, CX-34 | G05-runtime-profile, G23-lightweight-qualification, G34-shared-encoder-isolation | runtime | no |
| B218 | M4 | Run shadow, full-cost admission and drift/revocation recovery | B213, B216, B217, B27 | CX-22, CX-25, CX-34 | G22-fixed-task-economics, G25-no-live-self-training, G34-reflex-revocation | learning | no |
| B219 | M4 | Prove paired low-risk Axon v0.20 / MiCode v0.14 use | B218, B183 | CX-16 | G16-v020-peer | integration | no |
| B220 | M2 | Maintain v0.20 package, profile fixtures and owner export integrity | B210 | CX-00 | G00-v020-coverage | documentation | no |
| B221 | M4 | Optional learned working-set relevance experiment | B218, B118 | CX-28 | G28-reflex-context-pilot | research | yes |
| B222 | M4 | Optional richer learners and schema-family extension experiments | B218 | CX-20 | G20-reflex-extension-review | research | yes |
| B223 | M2 | Reconcile the v0.20 contract with the uploaded source snapshot | none | CX-00 | G00-r21-source-reconcile, G00-r21-owner-aliases | contracts | no |
| B224 | M2 | Upgrade package-gate compatibility without rewriting source history | B223 | CX-00 | G00-r21-upgrade-preflight, G00-r21-evidence-overlay | integration | no |
| B225 | M2 | Specify negotiated decision records at the existing Reflex boundary | B223 | CX-05 | G05-r21-decision-negotiation, G05-r21-decision-semantics | contracts | no |
| B226 | M2 | Expose a candidate view over the existing capability registry | B225 | CX-34 | G34-r21-candidate-view, G34-r21-filter-before-score | scheduler | no |
| B227 | M2 | Build an opt-in CLM adapter and isolated embedding cache | B225, B226 | CX-05 | G05-r21-clm-input-cache, G05-r21-clm-epoch-order | inference | no |
| B228 | M2 | Qualify Jev and RLCD-compatible typed decision adapters | B225 | CX-05 | G05-r21-jev-capabilities, G05-r21-jev-error-boundary | inference | no |
| B229 | M2 | Add pijev-inspired order-robustness wrappers | B225, B228 | CX-06 | G06-r21-permutation-alignment, G06-r21-permutation-scope | calibration | no |
| B230 | M2 | Freeze calibration, abstention and candidate-set qualification | B225, B226 | CX-06 | G06-r21-calibration-binding, G06-r21-candidate-stress | calibration | no |
| B231 | M2 | Train CLM heads only from qualified and split-safe evidence | B227, B230 | CX-25 | G25-r21-clm-training-lineage, G25-r21-clm-head-admission | learning | no |
| B232 | M2 | Add CheckEval-inspired evidence-linked checklists | B223 | CX-01 | G01-r21-checklist-totality, G01-r21-checklist-freeze | evaluation | no |
| B233 | M2 | Protect whole-task outcomes and independent evaluation | B232, B224 | CX-21 | G21-r21-whole-task-truth, G21-r21-judge-independence | evaluation | no |
| B234 | M2 | Build a regularized harness-mutation ledger | B223, B232 | CX-29 | G29-r21-evolution-history, G29-r21-protected-mutations | evolution | no |
| B235 | M2 | Preregister leakage criticism and noise-aware comparisons | B234, B233 | CX-08 | G08-r21-evolution-preregister, G08-r21-evolution-generalization | experiments | no |
| B236 | M2 | Keep pruning and promotion under independent admission | B235 | CX-11 | G11-r21-prune-protections, G11-r21-evolution-independent-admit | admission | no |
| B237 | M2 | Implement a deterministic routing control with session identity | B226 | CX-34 | G34-r21-route-session, G34-r21-route-control | scheduler | no |
| B238 | M2 | Qualify TinyRouter, TRINITY and Semantic Router adapters | B237, B230 | CX-06 | G06-r21-router-bakeoff, G06-r21-router-recursion | routing | no |
| B239 | M2 | Add canonical-history context projections | B223, B225 | CX-28 | G28-r21-context-originals, G28-r21-context-retention | context | no |
| B240 | M2 | Preserve critical context, pair closure and recovery | B239 | CX-28 | G28-r21-context-pins, G28-r21-context-recovery | context | no |
| B241 | M2 | Specify bounded response-level draft/verify cascades | B225, B230 | CX-26 | G26-r21-cascade-budget, G26-r21-cascade-verification | composition | no |
| B242 | M2 | Isolate speculative proposals from side effects | B241 | CX-03 | G03-r21-speculation-no-effects, G03-r21-speculation-terminal | executor | no |
| B243 | M2 | Unify whole-task evidence and priced usage | B223 | CX-10 | G10-r21-usage-partitions, G10-r21-usage-unknown | telemetry | no |
| B244 | M2 | Enforce shared budgets and controlled economic comparisons | B243, B241 | CX-13 | G13-r21-budget-authority, G13-r21-economic-controls | resources | no |
| B245 | M2 | Replay decision, route, context and cascade evidence | B225, B239, B241, B243 | CX-10 | G10-r21-research-replay, G10-r21-evidence-integrity | replay | no |
| B246 | M2 | Register the eight research families as qualified experiments | B223, B230, B233, B243 | CX-20 | G20-r21-research-traceability, G20-r21-research-status | research | no |
| B247 | M2 | Define a proposed MiCode consumer delta without asserting delivery | B224, B225, B240, B242, B245 | CX-16 | G16-r21-peer-proposed, G16-r21-peer-roundtrip | integration | no |
| B248 | M2 | Exercise revocation and atomic runtime transitions | B225, B226, B237 | CX-34 | G34-r21-research-revocation, G34-r21-research-fallback | runtime | no |
| B249 | M2 | Maintain v0.21 package conformance and preserved contracts | B223, B224, B225 | CX-00 | G00-r21-v021-coverage, G00-r21-v021-immutability | documentation | no |
| B250 | M2 | Run one explicitly authorized real end-to-end pilot | B227, B228, B229, B233, B236, B238, B240, B242, B244, B245, B246, B248 | CX-21 | G21-r21-v021-live-pilot, G21-r21-v021-live-disposition | integration | no |
| B251 | M4 | Optional outer-loop evolution-policy experiment | B236, B250 | CX-12 | G12-r21-meta-policy-bounds, G12-r21-meta-policy-holdout | research | yes |
| B252 | M4 | Optional large-candidate CLM retrieval experiment | B227, B246 | CX-24 | G24-r21-ann-recall, G24-r21-ann-epoch | research | yes |
| B253 | M4 | Optional specialized heads and learned router optimization | B231, B238, B246 | CX-22 | G22-r21-specialization-benefit, G22-r21-specialization-rollback | research | yes |
| B254 | M4 | Optional best-of-N and parallel response speculation | B250 | CX-26 | G26-r21-bon-total-cost, G26-r21-bon-selected-bytes | research | yes |
| B255 | M0 | Reconcile Axon, MiCode and Fabric source truth | none | CX-00 | G00-r22-source-rebase, G00-r22-preservation | integration | no |
| B256 | M0 | Negotiate the existing CX-16 / MX-12 bridge profile | B255 | CX-16 | G16-r22-closed-wire, G16-r22-negotiation | integration | no |
| B257 | M0 | Bind identities and immutable evidence sidecars | B256 | CX-10, CX-32 | G10-r22-trial-identity, G32-r22-sidecar-bindings | integration | no |
| B258 | M0 | Converge compute and peer requests at existing authority | B256, B257 | CX-03, CX-13 | G03-r22-authority-intersection, G13-r22-profile-eligibility | integration | no |
| B259 | M1 | Preserve the truthful legacy process adapter | B258 | CX-13 | G13-r22-legacy-scope, G13-r22-no-weak-fallback | integration | no |
| B260 | M1 | Implement durable operation journal and aggregate reservations | B258, B259 | CX-13 | G13-r22-journal-before-effect, G13-r22-aggregate-reservation, G13-r22-unknown-reconcile | integration | no |
| B261 | M1 | Materialize immutable workspaces with per-trial isolation | B260 | CX-03, CX-28 | G03-r22-workspace-import, G28-r22-workspace-not-context, G03-r22-trial-isolation | integration | no |
| B262 | M1 | Extract axon-vm as a profiled backend without CLI drift | B255 | CX-13 | G13-r22-vm-cli-parity, G13-r22-guest-truth | integration | no |
| B263 | M1 | Physically qualify the protected Linux microVM profile | B258, B260, B261, B262 | CX-13, CX-03 | G03-r22-physical-isolation, G13-r22-profile-qualification, G13-r22-launch-cleanup | integration | no |
| B264 | M1 | Wire one real registered Cortex CheckExecutor | B260, B261, B263 | CX-01, CX-03 | G01-r22-registered-check, G01-r22-verifier-separation, G03-r22-check-effects | integration | no |
| B265 | M1 | Join preflight, execution and independent outcome evidence | B257, B264 | CX-32 | G32-r22-receipt-roles, G32-r22-artifact-recheck | integration | no |
| B266 | M1 | Gate MiCode workers on observed execution context | B256, B261 | CX-16 | G16-r22-preflight-start, G16-r22-preflight-return, G16-r22-role-scope | integration | no |
| B267 | M1 | Enforce MiCode scope intersection and preserve trust graduation | B258, B266 | CX-16, CX-03 | G16-r22-local-authority, G16-r22-trust-graduation, G03-r22-dispatch-recheck | integration | no |
| B268 | M1 | Consume policies through the real MiCode bridge | B256, B266, B267 | CX-16 | G16-r22-real-consumer, G16-r22-candidate-shortlist, G16-r22-provider-host-boundary | integration | no |
| B269 | M1 | Export complete MiCode episodes to actual Axon intake | B265, B268 | CX-16, CX-10 | G16-r22-real-producer, G10-r22-all-attempts | integration | no |
| B270 | M1 | Reconcile whole-task inference and execution economics | B260, B269 | CX-10, CX-13 | G10-r22-full-task-cost, G13-r22-billing-settlement, G10-r22-cohort-denominator | integration | no |
| B271 | M2 | Run logical A/B branches with fenced promotion | B261, B264, B270 | CX-08, CX-11 | G08-r22-logical-branches, G11-r22-workspace-cas, G08-r22-branch-cancellation | integration | no |
| B272 | M2 | Bind the 0.21 profiles to a deterministic pilot baseline | B268, B270 | CX-05, CX-06, CX-26, CX-28, CX-34 | G34-r22-pilot-controls, G05-r22-decision-semantics, G26-r22-speculation-disabled, G28-r22-context-provenance | integration | no |
| B273 | M2 | Generate one bounded EVO policy candidate | B272 | CX-29 | G29-r22-bounded-mutation, G29-r22-hypothesis-memory | integration | no |
| B274 | M2 | Freeze the independent experiment and admission plan | B269, B273 | CX-21, CX-33 | G21-r22-protected-splits, G33-r22-decision-rule-freeze, G21-r22-inconclusive-valid | integration | no |
| B275 | M2 | Execute controlled real incumbent/challenger trials | B271, B272, B274 | CX-08, CX-21 | G08-r22-real-paired-execution, G21-r22-order-cache-controls | integration | no |
| B276 | M2 | Evaluate exact candidates outside subject authority | B264, B265, B275 | CX-01 | G01-r22-nonvacuous-outcome, G01-r22-independent-issuer, G01-r22-unknown-outcome | integration | no |
| B277 | M2 | Apply the frozen independent policy-admission decision | B274, B276 | CX-11 | G11-r22-independent-admission, G11-r22-admission-disposition | integration | no |
| B278 | M2 | Prove fenced policy activation and future-task uptake | B268, B277 | CX-11, CX-16 | G11-r22-policy-cas, G16-r22-future-task-uptake, G16-r22-inflight-policy-pin | integration | no |
| B279 | M2 | Exercise rollback, revocation and safe paused states | B260, B278 | CX-11, CX-13 | G11-r22-rollback-revalidate, G13-r22-rollback-lifecycle, G11-r22-regression-observed | integration | no |
| B280 | M2 | Fault-test cancellation, budgets and restart across peers | B260, B263, B264, B271 | CX-13, CX-16 | G13-r22-restart-matrix, G16-r22-peer-failure-matrix | integration | no |
| B281 | M2 | Enforce learning eligibility and data-retention boundaries | B269, B274 | CX-10, CX-25 | G10-r22-eligibility-projection, G25-r22-no-self-label-loop | integration | no |
| B282 | M2 | Close joint adversarial and authority-bypass tests | B267, B276, B279, B280, B281 | CX-03, CX-32 | G03-r22-joint-bypass, G32-r22-evidence-laundering | integration | no |
| B283 | M0 | Integrate compatible package and cross-project source gates | B255, B256 | CX-00, CX-16 | G00-r22-package-gate-upgrade, G16-r22-source-ci-scope | integration | no |
| B284 | M0 | Validate the complete 0.22 specification and reference pack | B255, B256, B257 | CX-00 | G00-r22-pack-integrity, G00-r22-honest-status | contracts | no |
| B285 | M2 | Qualify the bounded operational closed loop | B279, B282, B283, B284 | CX-16, CX-29 | G16-r22-operational-loop, G29-r22-no-forced-winner, G16-r22-bounded-activation | integration | no |
| B286 | M3 | Report evidence for or against a real improvement claim | B285 | CX-21, CX-29 | G21-r22-measured-claim, G29-r22-claim-separation | research | yes |

## Work-package outputs

### B00 — Import and reconcile the package

Owner-reviewed naming, scope and CX-to-repository ID mapping; preserve existing governance.

### B01 — Audit Axon seams and engine guarantees

Commit-pinned evidence matrix; record native ceiling, missing-gate, FFI, R44 and OS discrepancies.

### B02 — Freeze task, safety and evaluation policy

Reviewed initial workload, required gates, host profile, budgets, split rules and evaluation margins.

### B03 — Build resettable task fixtures and simple controls

Tiny repair fixtures plus stale/malicious/crash cases; strong-model and rules control definitions.

### B04 — Implement common manifests and event schemas

Validated Run/Action/Evidence contracts, versions, canonical digests and negative schema tests.

### B05 — Implement partial software observations

Content-based snapshots, incomplete AST/diagnostics and scope-expansion path.

### B06 — Implement grants and denial-first catalog

Principal/session/snapshot-bound grant registry; forged and stale references denied.

### B07 — Implement the H0 isolated tool host

Tested local sandbox, environment projection, process-tree cancellation and quotas.

### B08 — Implement local patch transactions

Immutable patch artifacts, constrained apply, locked/CAS commit and rollback.

### B09 — Execute registered checks with durable action states

Approved test/build adapter, write-ahead action state and crash reconciliation.

### B10 — Implement AIR validation and serial scheduler

Typed graph execution with explicit dependencies, bounded iteration and effect checks.

### B11 — Add mock decision and generation adapters

Deterministic fixture adapters with clear mock provenance, invalid-result tests and no authority.

### B12 — Implement protected final verification

Agent cannot alter hidden checks; final artifact digest and task contract determine completion.

### B13 — Integrate raw/redacted events and exact replay

Record model/tool seam; replay without effects; secret-safe review view.

### B14 — Run deterministic end-to-end conformance

Safe repair golden path plus adversarial, cancellation and recovery demonstrations.

### B15 — Add an authorized existing-model backend

Pinned model adapter for bounded decisions and untrusted patch generation; explicit fallback and accounting.

### B16 — Compare the real-model vertical slice to controls

Protected task-family results, full costs, quality intervals and independent milestone evidence.

### B17 — Freeze backend-neutral Reflex ABI and score provenance

Choice/Binary/Ordinal contracts, dynamic candidate manifests, typed probability origins, and generative/direct-logit/sequence/learned adapter conformance scaffold.

### B18 — Build Reflex decision corpus, destructive controls and shared-state/speculative path

Grouped verified decision corpus; shuffled/empty/wrong/stale-state and option-order/ID controls; branch-specific targets; serial/parallel/shared-prefix timing.

### B19 — Run calibration lab and implement selective routing

Pinned calibration artifacts by probability origin; proper scoring, risk/coverage/VUC, deterministic fallback, explicit OOD/abstention and budget control.

### B20 — Run multi-backend Reflex bakeoff and decide adoption

Compare generative, direct-logit, sequence and learned-head families on one protected corpus; retain only measured useful configurations; baseline remains active when evidence is inconclusive.

### B21 — Implement eligible learning/decision-corpus export and label lineage

Purpose-scoped dynamic-choice datasets, grouped protected splits, verified/human/teacher/weak/delayed/unknown labels, attribution records and revocation lineage.

### B22 — Match concrete action predictions to outcomes

Patch/environment-bound prediction schema and simple dependency/base-rate baselines.

### B23 — Train short-horizon software predictors

Diagnostic/test outcome models with held-out calibration and declared applicability.

### B24 — Evaluate real planning value and model exploitation

No-model/simple-model/challenger ablations and adversarial candidate-search evidence.

### B25 — Implement hypotheses and permitted probe selection

Bounded outer planner and concrete experiment artifacts over approved checks.

### B26 — Evaluate controlled interventions and diagnosis

Matched reset/control experiments; scoped causal claims and baseline comparison.

### B27 — Implement independent artifact admission

Protected candidate registry, digest-bound gate receipts and shadow/canary states.

### B28 — Derive one guarded reusable tool/rule/policy

Candidate with applicability predicate, empirical/proof scope and approved fallback.

### B29 — Run learned option-conditioned Reflex learning curves

Small-model/decision-head experiment over runtime candidate sets with grouped splits, destructive state controls and calibration; pursue or stop based on measured value.

### B30 — Demonstrate promotion and rollback

Independently admitted specialization, non-effecting shadow comparison and regression recovery.

### B31 — Compare fixed alternative problem encodings

Two traceable representations evaluated against compute-matched controls.

### B32 — Test learned concepts and held-out transfer

Typed concept/representation proposals with counterexamples, fit and novel-family evidence.

### B33 — Unify independently useful pillar lifecycles

At least two evaluated update pipelines share manifests, admission and rollback without shared authority.

### B34 — Add bounded curriculum and meta experiment allocation

Meta proposals with budgets; protected policy/audit boundary; task-generator lineage.

### B35 — Run longitudinal stability and transfer checks

Pinned-version multi-episode improvement report, regression audit and no self-grading.

### B36 — Harden hosted service operation

Job queues, durable restart, observability, revocation and configured host compatibility matrix.

### B37 — Audit stable runtime-to-language seams

Actual R2a status, wrapper APIs, type ownership map, language-surface friction evidence.

### B38 — Integrate authoritative types for new semantics

Sequenced type-map change only when required; preserved import/AST identity and regression evidence.

### B39 — Add justified native/language/proof support

Evidence-backed desugaring/native subset/proof receipts with explicit unsupported cases.

### B40 — Investigate specialized shared-state Reflex architecture

Shared encoder/prefill, parallel typed heads, quantization/cache or RLCD-like training experiment only for a demonstrated serving/quality bottleneck; compare against B20 winners.

### B41 — Evaluate the separate bare-metal branch

Owner-approved kernel plan with actual confinement, syscall, driver and TCB obligations.

### B42 — Implement Reflex conformance fixtures and backend feature/effective-input receipts

Primitive-specific batch results keyed by QuestionId; provider/logit/sequence/generated score provenance; backend capability manifests; prompt-role/isolation/truncation/retry fixtures.

### B43 — Implement dependency/model/dataset adoption manifests

Commit/license/security/transitive-model/tokenizer/encoder/dataset/evaluator lineage; benchmark comparability review; SDK transformation and hosted-profile checks.

### B44 — Freeze MiCode↔Axon experience bridge and conformance fixtures

Versioned episode/observation/decision/knowledge schemas; authority separation; dummy producer/consumer; migration tests.

### B45 — Import one canonical MiCode coding episode into Cortex

Provenance-preserving episode import, semantic replay, eligibility/lineage and source-system attribution.

### B46 — Build external repository/history intake and knowledge-candidate extractor

Read-only normalized repo/history evidence, pattern/counterexample store, license/data-use manifests and dedupe lineage.

### B47 — Reproduce and evaluate one cross-repository knowledge candidate

Resettable local reproduction, benchmark/control evidence and held-out repository-family evaluation.

### B48 — Synthesize one guarded skill/tool from admitted knowledge/episodes

Pattern→Skill/Tool candidate with applicability, effects/capabilities, intermediate checks, fallback and full lineage.

### B49 — Demonstrate skill/tool promotion, use and deoptimization

Independently admitted artifact used on held-out tasks; injected applicability/regression case returns to previous-good path.

### B50 — Demonstrate closed self/external/generated experience loop

One report showing common evidence pipeline, source-separated ablations, failure attribution and OS-wide scorecard.

### B51 — Promote a proven capability into Axon library/compiler/runtime

Measured native/library candidate with CX-15 parity/proof/invariant evidence; refusal/fallback if unjustified.

### B52 — Map existing Axon intent/surface/approval implementation

Commit-pinned map of intent compile, AST review/approve, structured-prose surface, authority/evidence semantics and drift.

### B53 — Implement versioned Intent IR and round-trip fixtures

Typed goal/constraints/preferences/authority/budget/evidence/ambiguity schema with provenance and migrations.

### B54 — Implement ambiguity resolution and deterministic semantic renderer

Explicit interpretation alternatives/questions, stable contract renderer/diff and digest-bound approval artifact.

### B55 — Lower approved Intent IR to constrained AIR

Clause-traceable AIR graph; replanning can narrow/add checks but cannot widen authority/drop required evidence.

### B56 — Demonstrate intent-to-evidence vertical slice

Prose intent → approved contract → bounded repair/optimization → independent evidence → explanation against original clauses.

### B57 — Demonstrate self-optimization through ImprovementIntent

Measured bottleneck generates typed improvement proposal that requests authority/evidence and passes normal admission; it cannot activate/edit its own gate.

### B58 — Implement Reflex Runtime state handles and branch scheduler

Immutable/emulated StateHandle contract, shared-state scheduler, candidate packing/order manifests, per-branch accounting and cancellation.

### B59 — Run question-isolation and candidate-order conformance

Q1-alone/sibling/adversarial/100-question and permutation suite; calibration-domain drift report and stop/pivot decision per backend.

### B60 — Compare listwise and option-conditioned Reflex architectures

Independent/pointer/listwise compute-matched pilot with dynamic candidate compositions, proper-scoring learning curves and task-level evaluation.

### B61 — Implement AIR question-dependency scheduling optimization

Independent/ConditionallyRelevant/AnswerDependent validation, legal batching/speculation, and changed-policy detection for semantic fusion.

### B62 — Stand up Kev-style Axon Reflex reference pilot

Small backbone + adapter + isolated branch mask + pointer/listwise head trained with canonical encoding; reproducible learning curve.

### B63 — Freeze Reflex train/calibration/dev/locked-test/transfer suites

Repo-aware split manifest, revision hashes, inaccessible locked test and explicit transfer ladder.

### B64 — Run permutation-robustness training study

Canonical-order vs shuffle augmentation vs permutation-consistency objective under matched compute.

### B65 — Add typed candidate-absence/control examples

Missing-target/distractor fixtures with registered NONE/OBSERVE_MORE/ESCALATE outcomes and verifier labels.

### B66 — Unify canonical decision encoding across train/eval/serve/replay

Single versioned renderer/encoder API plus effective-input receipts and replay fixtures.

### B67 — Measure Coding Transfer Frontier

Same-repo to unseen-repo to unseen-family/task-family to cross-language transfer curves with selective quality/coverage/compute. Kev-style model included if B62 exists; otherwise evaluate feasible learned backends.

### B68 — Compare Kev-style transfer improvements

Representation/data/backbone/augmentation ablations focused on held-out coding transfer; no locked-test tuning.

### B69 — Define Coding Frontier task/system/run manifests and reset harness

Immutable benchmark task/suite/system/run schemas plus resettable deterministic conformance fixtures.

### B70 — Freeze first whole-system coding benchmark portfolio

Reviewed repair/debug/feature/refactor/optimization suite with matched strong-model and previous-system controls.

### B71 — Add repository/task-family transfer and contamination registry

Repository/family/task-family transfer partitions with contamination relationships, retirement and lineage rules.

### B72 — Implement Verified Coding Frontier and longitudinal regression reporting

Quality/coverage/cost/time frontier curves, Pareto views and previous-release regression matrix under registered resource envelopes.

### B73 — Integrate benchmark evidence with independent admission

Immutable CX-21 evidence bundle accepted by CX-11 without granting the benchmark controller activation authority.

### B74 — Mine recurring cognitive functions and specialization eligibility

Versioned CognitiveFunctionProfile registry with stability, frequency, evidence-quality, transfer and incumbent-cost features.

### B75 — Build specialized decision-model baselines

At least one schema-conditioned encoder/candidate-token/pointer specialization candidate plus incumbent/general Reflex comparison.

### B76 — Implement specialization ROI, applicability and fallback evaluator

Preregistered quality/resource comparison, applicability guard, OOD fallback and drift invalidation harness.

### B77 — Define neural-program ABI and immutable artifact manifest

Integrate CX-36 artifact/release contracts into the existing CX-23 owner, replacing duplicate manifest definitions; does not require a successful specialization miner.

### B78 — Local Neural Program backend candidate baseline

Local isolated learned-function backend candidate or explicitly recorded unsupported profile; hosted ProgramAsWeights integration is optional B157, never a default acquisition path.

### B79 — Implement typed neural-program runtime and output validation

Local runtime resolves immutable dependencies, validates typed/refined outputs, accounts resources and keeps generated strings outside authority.

### B80 — Implement shared-base skill cache and lifecycle

Measured hot/cold base+adapter cache, scope isolation, eviction/loading accounting and exact artifact identity across skill switches.

### B81 — Run neural-program versus specialized/general cognition bakeoff

Matched coding-function benchmark comparing rule/template, specialized model/neural program and incumbent under protected suites.

### B82 — Implement de-specialization and drift rollback

Injected drift/artifact failure suspends specialization, preserves evidence and routes future work to previous-good/general fallback.

### B83 — Connect specialized/neural skills to staged crystallization

One admitted specialized/neural skill retains lineage and enters the existing CX-18 staged crystallization path; any later native/compiler promotion remains optional under B51/CX-15.

### B84 — Build semantic perception/extraction reference baseline

Schema-conditioned local extractor with object/span provenance and deterministic baseline comparison.

### B85 — Build semantic proposition matcher and semantic-grep path

Proposition scoring over software objects with lexical/embedding baselines, uncertainty band and deterministic composition.

### B86 — Implement perception/retrieval router and disclosure policy

Measured routing among deterministic/embedding/semantic/general backends with local/remote data-use enforcement.

### B87 — Add completion critic shadow hook

Shadow stop-hook compares proposed completion against Intent/Acceptance clauses and protected verifier outcomes.

### B88 — Benchmark premature-stop critic and clause extraction

Protected premature-stop suite with false-complete/false-continue rates and clause-level missing-work evidence.

### B89 — Implement candidate model registry and isolated learner process

Content-addressed learner outputs with exact corpus/objective/code lineage and no active-serving mutation path.

### B90 — Implement shadow sampler and candidate replay

Candidate sampler processes replay/live-shadow decisions without action authority and reports matched evidence.

### B91 — Implement guarded weight/adapter transport

Versioned content-addressed candidate transport/conversion with checksum and runtime conformance refusal.

### B92 — Run mixed-policy/asynchronous learning experiment

Bounded experiment records policy lag/behavior versions and compares against fixed-policy baseline without self-promotion.

### B93 — Demonstrate admission-only activation and rollback

Mock/real admission changes active immutable sampler pointer; injected regression restores previous-good artifact with preserved lineage.

### B94 — Implement typed artifact catalogs and SELECT/PROJECT primitives

Runtime-generated immutable artifact catalogs, source-bound projection receipts and stale/authority refusal fixtures.

### B95 — Implement composition graphs, validator and dependency semantics

Typed composition graph with independent/conditional/answer-dependent scheduling, schema/cardinality checks and explicit partial/fallback states.

### B96 — Benchmark select/compose against generation

Matched protected benchmark on tasks whose needed artifacts already exist, with verified quality, model calls, latency, cost and fallback accounting.

### B97 — Pilot Intent-to-Build-Spec/task-DAG composition

Approved Intent IR lowers through registered task/evidence templates into a traceable Build Spec/task DAG; novel slots are the only generation fallback.

### B98 — Version semantic decision definitions in the Reflex Lab

Immutable question/criteria/state-projection/candidate-policy/decomposition artifacts with semantic diffs and result lineage.

### B99 — Run uncertainty-plus-random-audit semantic alignment loop

Development-pool uncertainty sampling plus random audit labels, proposed definition revisions and held-out evidence without automatic activation.

### B100 — Evaluate monolithic versus decomposed decision functions

Matched study of one broad judgment versus narrower typed questions plus deterministic composition, including transfer/calibration/latency evidence.

### B101 — Submit semantic/composition policy candidate through independent admission

Immutable semantic-definition/composition-policy candidate with protected evidence, applicability/fallback and accept/reject/rollback decision.

### B102 — Freeze semantic supervisor observation/assessment ABI

Versioned supervisor observation, assessment and decision-record schemas with evidence/state digests and probability-source provenance.

### B103 — Run shadow semantic supervisor on resettable coding episodes

Shadow-only assessments for progress/stuck/off-track/drift/verification/human/finish fixtures, compared to protected outcomes without intervention.

### B104 — Implement deterministic supervisor intervention policy

Policy mapping normalized assessments to CONTINUE/STEER/REQUEST_VERIFY/HOLD/STOP_RETRY/ESCALATE/PROPOSE_FINISH with hysteresis and attempt bounds.

### B105 — Add feature-gated steering and verification requests

Live STEER and REQUEST_VERIFY integration through existing authority paths; supervisor cannot directly execute or certify completion.

### B106 — Benchmark supervisor utility and false-intervention cost

Matched whole-system study of no-supervisor vs shadow/steer profiles with false-stop, recovery, verification, latency and cost accounting.

### B107 — Freeze semantic working-set artifact and receipt schemas

ContextArtifact, RecomputeContract and WorkingSetReceipt schemas with protected pin classes, privacy and snapshot lineage.

### B108 — Implement authorized rule/skill/map relevance selection

Selection over already-authorized versioned context candidates, with mandatory-rule bypass and pre/post authorization checks.

### B109 — Implement semantic context GC for tool history

PIN/KEEP_VERBATIM/KEEP_STRUCTURE/COMPRESS/DROP_RECOMPUTABLE decisions preserving call-result identity and durable retrieval references.

### B110 — Bind async context decisions to state/catalog freshness

Digest-bound application path that rejects stale rule/skill/GC selections after intent, files, rules or snapshots change.

### B111 — Add cache-aware cognitive model routing policy experiment

Routing experiment accounting for capability, context size, locality, warm-prefix/cache economics, quality and budgets without overriding explicit authority/user choices.

### B112 — Add semantic-predicate query planner pilot

Deterministic/authorization filter pushdown, lexical narrowing, batched semantic predicates, no-match handling and cache identity on a repository/query workload.

### B113 — Instrument cognitive cascade execution and fallback reasons

Replayable RULE→RETRIEVE→SELECT/COMPOSE→REFLEX/SPECIALIZED→GENERATE→THINK cascade with explicit abstention/fallback reasons and verifier/authority preservation.

### B114 — Run long-horizon working-set and supervisor benchmark

Matched long-horizon benchmark against full-context and generic-summary controls, measuring protected-clause retention, quality, context size, latency/cost and supervisor utility.

### B115 — Submit supervisor/working-set policy candidates through admission

Immutable candidate policies with applicability, rollback and protected evidence; no self-promotion from development metrics.

### B116 — Define cognitive-operation and ImprovementIntent protocols

Instrument a generic CognitiveOperationRecord on existing seams, including component revision, effective input and explicit omissions; do not wait for new supervisor/context implementations.

### B117 — Instrument model routing as first self-application target

Outcome-linked model-route records with effective-context/cache/cost metadata and explicit user/manual override provenance.

### B118 — Instrument working-set and semantic rule/skill selection

Passive incumbent context-selection/retention receipts on existing context owner; no new working-set policy or active control is needed.

### B119 — Build frozen replay runner for internal cognitive policies

Matched replay runner that swaps policy/component revisions while holding state/effective-input/candidate contracts fixed when possible.

### B120 — Run first offline challengers for routing and working-set policy

At least two challengers per selected low-risk policy with quality/cost/latency/applicability and failure-attribution report; no activation.

### B121 — Implement no-effect shadow challenger runtime

Shadow execution path with hard denial of tool/context/worker/completion/persistent-state effects and outcome join after live execution.

### B122 — Shadow-test first self-hosted policy challenger

Live shadow comparison for one internal policy with protected outcome evidence and no active behavior changes.

### B123 — Add low-risk self-promotion envelope and canary controls

Disabled-by-default allowlist schema, canary scope, admission token, monitoring thresholds and immediate previous-good rollback path.

### B124 — Exercise full internal observe→replay→shadow→admit→rollback lifecycle

One low-risk internal policy passes protected admission, bounded canary, monitoring and forced rollback drill with full lineage.

### B125 — Connect Cognitive Specialization Compiler to self-application episodes

Recurring internal cognitive operations become specialization candidates with ROI, applicability, fallback and protected evidence.

### B126 — Add cognitive primitive proposal workflow

PrimitiveProposal artifact, interpreter/lowering prototype path, cross-family evidence contract and governance/admission hooks.

### B127 — Enable guarded recursive optimization experiments

Research-only recursive experiments where supervisor/working-set/specialization components propose successors but cannot alter evaluator/admission roots.

### B128 — Define ProjectImprovementContract and project registry

Versioned project contracts, repository identity, protected scopes, improvement classes, evidence adapters, budgets and rollback profiles.

### B129 — Implement generic repository adapter ABI

Registered observe/build/test/benchmark/analyze/verify/rollback adapters with typed evidence and no generated shell authority.

### B130 — Create reproducible project baselines and improvement queues

ProjectBaseline artifacts, environment fingerprints, incumbent metrics and typed ProjectImprovementCandidate queues.

### B131 — Run isolated repository challengers

No-merge challengers in isolated snapshots/worktrees/sandboxes with project-contract-bound evidence.

### B132 — Add project-local replay, shadow and bounded canary lifecycle

Project-scoped replay/shadow/canary activation lifecycle with previous-good rollback and reconciliation receipts.

### B133 — Pilot repository optimization across heterogeneous projects

Run one owner-approved isolated repository experiment with no merge/deploy; report candidate, evidence and rejection/acceptance proposal. Active canary remains B132.

### B134 — Add project-family lineage and contamination metadata

Project-family/cluster identities, corpus roles, near-duplicate/fork grouping and data-use lineage for cross-project episodes.

### B135 — Implement cross-project pattern candidate miner

Pattern candidates with positive examples, counterexamples, applicability predicates, mechanism hypotheses and proposed destination.

### B136 — Build repository transfer-tier evaluation

P0–P5 repository transfer evaluation with held-out families, family-aware statistics and matched baselines.

### B137 — Connect shared capability promotion to CX-11/CX-22/CX-18

SharedCapabilityProposal path for skills, policies, specialized cognition, tools, libraries and potential native primitives with scope-qualified evidence.

### B138 — Implement de-generalization and applicability revision

ApplicabilityRevision workflow that narrows/splits/demotes shared artifacts after negative transfer while retaining contradictory evidence.

### B139 — Run end-to-end project-to-platform improvement pilot

One project-local improvement, one family-specific shared pattern and one cross-project candidate flow through discovery, transfer, admission and feedback to target repositories.

### B140 — Freeze Universal Evidence Graph node/edge/claim schemas

Versioned evidence-node, typed-edge, claim-strength, invalidation and evidence-subgraph contracts.

### B141 — Integrate existing receipts and verifier evidence into the evidence graph

Map existing observation/action/verifier events to evidence identities; adapters for future supervisor/working-set events follow as those owners land.

### B142 — Implement contradiction, invalidation and authorization-aware evidence queries

Contradiction edges, quarantine propagation and scoped why/provenance queries with disclosure enforcement.

### B143 — Bind admission and self-application evidence closure to CX-32

Implement the minimal evidence-closure/subject binding adapter; end-to-end production activation stays in B124/B166/B173.

### B144 — Freeze causal experiment and intervention schemas

ExperimentProtocol, InterventionRecord, causal-assumption, reversibility and analysis-plan contracts.

### B145 — Implement matched cognitive replay experiments

Matched incumbent/challenger replay runner holding state/candidates/effective input fixed where possible and recording confounders otherwise.

### B146 — Add active experiment selection by information gain, cost and risk

Deterministic/reference experiment planner ranking bounded observations/interventions with reversibility-aware authority.

### B147 — Run causal mechanism and transfer pilot

One mechanism hypothesis tested locally and on held-out project families with contamination-safe analysis and negative cases.

### B148 — Build Shared Capability Registry schema and lifecycle

Build the minimal deterministic capability registry for incumbent rules/tools/models, exact artifact/revision ownership and authenticated status; no learned specialization or cross-project promotion prerequisite.

### B149 — Implement hard-constraint cognitive scheduler

Scheduler enumerates authorized/type/privacy-compatible capabilities, applies hard constraints and deterministic utility routing with abstention/fallback.

### B150 — Add hardware/cache/risk-aware scheduler inputs and receipts

Hardware/load/cache/reversibility-aware utility inputs, semantic cache freshness checks and exact scheduler-decision receipts.

### B151 — Benchmark heterogeneous cognitive scheduling and rollback

Matched registry benchmark across rule/matcher/specialized/general/generative/human paths with quality/latency/cost/privacy/cache scenarios and previous-good rollback.

### B152 — Resolve Neural Program terminology and ownership

Record the CX-23 lifecycle/CX-36 format/CX-34 registry/CX-11 admission split; migrate generic PAW names without rewriting external attribution.

### B153 — Implement strict .nps source contract

Bounded strict-JSON source parser, immutable schema/validator refs and approved MUST binding; negative syntax/schema fixtures.

### B154 — Implement bounded .np inventory reader

Host-isolated production container preflight; no arbitrary extraction/code hooks; member/path/type/size/CRC/inventory rejection. Reference format tests are not this runtime implementation.

### B155 — Bind content identity and detached release

Canonical source/manifest hashes with no self-cycle; immutable dependencies and separate admission/evaluation envelope.

### B156 — Separate compiler and runtime lifecycle interfaces

Distinct credentials and negotiated capabilities; bounded compile jobs/cancellation; no unsupported fork/serialize assumptions.

### B157 — Optional ProgramAsWeights importer

Optional explicit external adapter/importer with disclosed source egress, private/public policy, target identities and no suffix-only conversion.

### B158 — Integrate typed proposal execution

Source-owned runtime returns ProposedValue plus separate structural/semantic validation; no capability witnesses minted by learned output.

### B159 — Test shared-base/adapter isolation

Pin in-flight adapter/model/runtime, isolate caches by principal/revision, refuse unsupported state sharing and incompatible hot-swap.

### B160 — Enforce applicability and bounded fallback

Known applicability hard checks, explicit unknown risk, cycle/depth/budget bound, offline no network, independent new-capability fallback receipts.

### B161 — Bind releases to existing capability registry

Use CX-34 registry and CX-11 admission, isolate candidate loads from active execution, bind exact subject/profile/evidence.

### B162 — Neural capability scheduler integration

Select only compatible admitted target profiles after fresh authority/privacy/revocation checks; unavailable reliability remains absent.

### B163 — Compile eligible experience into candidate artifacts

Freeze eligible grouped train/calibration/eval data and renderer; compile local candidate, freeze exact executable before final evaluation; bind prediction event.

### B164 — Source-bound diagnostic Neural Program pilot

Compare parser/select-copy/generative/learned candidates on source-grounded diagnostic extraction with coverage and omission metrics; admit OR reject with evidence, no forced model win.

### B165 — Reconcile generated package and gate coverage

Run document validator and negative mutation tests; reconcile source/manifests/master, complete DAGs, hashes and zero unowned gates; no product PASS promotion.

### B166 — Test admission invalidation and rollback races

Frozen subject and evidence closure; revoke between check/dispatch; reject stale or revoked previous-good; preserve stop-safe recovery.

### B167 — Calibrate named events and protect evaluator feedback

Separate labels/distributions/correctness/utility and family-held-out fits; pre-register operational thresholds and submission budgets, retain errors and abstentions.

### B168 — Bound shadow effects and fallback dispatch

No-task-effect shadow still has budgets/storage/egress; deterministic current-grant filter before fallbacks; all cancellation/timeout paths tested.

### B169 — Preserve evidence pins and bound reflexive recursion

Hash-bound pins and materialized receipts; missing/truncated/ejected evidence stays explicit; recursion/fanout budgets and change-risk escalation.

### B170 — Separate causal estimates from realized outcomes

Matched upstream state with explicit changed effective inputs; off-policy/simulated values never become realized despite calibrated validation.

### B171 — Version Axon/MiCode bridge migration fixtures

Producer/consumer fixtures for new proposal/release/effect/reliability fields; version negotiation; reject unsupported required guarantees, do not claim MiCode is updated.

### B172 — Test scoped transfer and semantic runtime profiles

Transfer dimensions and unsupported profiles explicit; no raw cross-model state or language syntax implied; classification-mode probability provenance preserved.

### B173 — Close reviewed end-to-end safety and product gates

Reconcile all review requirements against live evidence, retain NOT_RUN for unsupported targets, finish bounded release report instead of reporting all research done.

### B174 — Reconcile ACE owner contracts and live source map

Bind source precedence and all live definition/consumer/enforcement seams; retain blocked decisions, original requirement anchors and live namespace mappings. No runtime claims from document review.

### B175 — Bind ACE axes and interpreter host mapping

Implement only registered profile tuples through existing host/library seams; test type/effect parity with recorded replies and explicit refusal of unsupported mappings.

### B176 — Negotiate deployment features and typed result fidelity

Implement independent feature negotiation and lossless Choice/BinaryProbability/OrdinalDistribution; preserve producer tags, completeness, selection/ties and reliability domains; negative tests reject semantic downgrade.

### B177 — Capture physical attempts and enforce root budgets

Link root/attempt/input/selected/observed/accepted producer and all work. Exercise cancel/deadline/unknown remote outcomes, late replies, hidden egress and conservative unknown-cost reservations.

### B178 — Bind logical context and optional physical state receipts

Record observation, working-set, effective-input and reuse status without requiring raw native state. Check epoch/identity/principal/lease mismatches and preserve protected pins. Actual reuse claim deferred to B186.

### B179 — Integrate branch-aware batching and result joins

Preserve QuestionIds/visibility and actual parent inputs; test reordered results, missing active values, permitted unused failures, duplicate/unknown IDs, cyclic and stale joins. Charge speculation.

### B180 — Bind physical profiles to existing hard-filtered dispatch

Use the existing registry/scheduler for profile/feature/input/lease/reservation selection and dispatch recheck. Reject revocation races, unauthorized fallback, cycles, hidden producers and reset budgets.

### B181 — Exercise real scorer-specific conformance

For each claimed mode, test real tokenizer boundary, complete sequence convention or learned-head parameters/shape/processor/candidate conditioning. A hosted label-only core declares other features unsupported rather than claiming these tests passed.

### B182 — Close one real Axon ACE core consumer slice

Use one authorized incumbent through a real host consumer and independent downstream evidence. Test refusal, unsupported features, canceled/stale work, auth/fallback and next normal invocation. No optional model/research gate is implied.

### B183 — Reconcile and test the MiCode peer contract

Pin the owner document/projection and live Axon/MiCode revisions; exercise actual MX client/build-loop, local permissions, evidence and next-turn recovery. Peer implementation is an external prerequisite; document-only roundtrip remains NOT_RUN.

### B184 — Run optional three-family Reflex comparison

Preregister matched hosted/open-scoring/real learned-head finite-choice tasks with qualified dynamic candidates and complete costs. Missing arms block this profile only; do not claim universal quality or speed.

### B185 — Exercise reviewed Neural Program lifecycle through ACE

Use reviewed CX-36 and separate compiler/runtime grants for the real BuildLog pilot. Test data/job/artifact/detached release, ProposedValue/source checks, offline dependency closure and admission-or-rejection. No new artifact format.

### B186 — Qualify optional actual native-state reuse

Run actual engine-level compatible reuse/fork and at least two learned programs when claiming shared-base isolation. Test stale/tenant/adapter/cache/lease boundaries and account memory; handle equality fixtures are insufficient.

### B187 — Close ACE document coverage and migration checks

Verify all 86 requirements, source/owner locks, no changed reviewed NP formats, vocabulary mapping, gate ownership, fresh generated views and bounded dependency closures; preserve all product results as NOT_RUN until live tests.

### B188 — Migrate current execution naming to ACE and publish legacy aliases

Rename current profile/schema/gates/tasks/consumer exports to ACE, retain ANEA v0.2 only as historical source and provide explicit read-only alias mappings. Negative cases: no hidden current ANEA owner paths; historical files untouched.

### B189 — Implement immutable ACEExecutionPlan binding

Define plan construction and dispatch-time revalidation for operation/capability/provider/input/result/privacy/budget/deadline/fallback/policy identity. Negative cases: stale plan, revoked provider, changed privacy policy, silent provider rebinding.

### B190 — Define ACE provider logical ABI and transport mapping

Specify describe/prepare/execute/stream/cancel/reconcile/release_state/health across embedded, sidecar and remote implementations with explicit supported subsets. Negative cases: unknown feature treated supported, lossy transport mapping, provider identity drift.

### B191 — Implement typed general ACE cognitive-result union

Represent decisions, ProposedValue, predictions, evidence results, generated artifacts, ProposedAction and terminal absence/failure states without semantic coercion. Negative cases: prediction promoted to observation; generated artifact marked verified; ProposedAction executed implicitly.

### B192 — Define ACE streaming and finalization semantics

Specify tentative chunks, progress/usage, bounded ordering/backpressure, interruption/reconnect and final digest/status; only finalized accepted result can commit. Negative cases: disconnect after valid chunk, duplicate/out-of-order chunk, buffer overflow, schema failure at finalization.

### B193 — Enforce ACE cognition versus world-effect boundary

Disable hidden provider-native world effects or reify each as ProposedAction for ordinary capability/effect authorization and evidence. Negative cases: provider tool call edits file; hidden remote action; action result confused with cognitive result.

### B194 — Add provider DataHandlingProfile and scheduler hard filters

Bind retention/logging/training/residency/subprocessors/encryption/cache/telemetry/download semantics to provider identity and filter before utility ranking. Negative cases: remote allowed but training use forbidden; unknown residency; hidden model download; stale provider policy.

### B195 — Harden native-state isolation, leases and release

Bind state/cache handles to provider/model/principal/project/session/lease; deny cross-scope reuse by default and record logical release separately from physical erasure. Negative cases: cross-project KV reuse, adapter cache bleed, stale lease, false zeroization claim.

### B196 — Add determinism class and execution fingerprint

Record deterministic/seeded/best-effort/nondeterministic/unknown class plus applicable seed/sampling/precision/quantization/runtime/kernel/hardware identity. Negative cases: same model/input but different kernel; missing seed; exact replay claimed from stochastic provider.

### B197 — Standardize durable ACEExecutionHandle lifecycle

Standardize submit/attach/poll/cancel/reconcile/finalize and provider operation identity/idempotency scope for long-running work. Negative cases: timeout resubmitted blindly, remote cancellation assumed, duplicate terminal result.

### B198 — Enforce semantic-preserving fallback graph

Require fallback to satisfy requested result semantics and hard constraints; alternate semantic classes require explicit caller permission and separate typed attempts. Negative cases: GeneratedEstimate substitutes NativeOptionLogit; text guess substitutes world prediction; fallback exceeds privacy/budget.

### B199 — Run ACE provider/transport conformance slice

Use one reference in-process provider and one transport adapter against identical bounded fixtures, then pair with the real MiCode consumer when available. Preserve NOT_RUN for live interoperability until actual producer/consumer execution. Negative cases: mock-only claim, hidden tool effect, transport drops provenance, following turn retains stale provider.

### B200 — Implement calibration registry and calibrated ACE claim checks

Freeze calibration-key semantics, reference proper-score reports and ACE correctness-claim validation using existing v1 fields. Negative cases: raw softmax relabeled correctness, stale/wrong-domain artifact, ECE-only evidence, missing estimator/event provenance.

### B201 — Build pairwise and multiway Outcome-Calibrated Decision Training reference arms

Implement frozen-corpus Bradley–Terry and Luce/Plackett–Luce reference objectives plus versioned calibration, explicitly as falsifiable baselines rather than Jev-reproduction claims. Negative cases: changed corpus/event definition, hidden post-hoc calibration, vendor claim substituted for measurement.

### B202 — Prototype packed branch-isolated decision serving

Implement an optional shared-prefix packed arm and compare it with separate execution, including sibling contamination, order and position-reset probes plus memory/latency accounting. Negative cases: branch leakage, stale cache, speed-only acceptance, undeclared drift.

### B203 — Run decision falsification and consistency suite

Run semantically controlled binary/two-way, pairwise/multiway, order, candidate-set, calibration, missing-option and shortlist diagnostics. IIA is reported for relevant reference arms but is not a universal pass condition. Negative cases: wording mismatch mislabeled inconsistency, sparse-bin certainty, candidate policy omitted.

### B204 — Implement high-cardinality staged decision path

Build score/retrieve→shortlist→explicit-choice lineage with full/shortlist digests, policy reference and shortlist recall/omission evidence. Negative cases: dropped correct candidate hidden from metrics, stage-1 score renormalized as final probability, shortlist policy omitted from calibration.

### B205 — Lower Neural Program learned decisions to fan-out/join scheduling

Add host/compiler scheduling for DECIDE/FANOUT/JOIN/CALIBRATION_GATE/VERIFY/ESCALATE around unchanged CX-36 r0.2 artifacts. Negative cases: answer-dependent fusion, partial result collapsed to success, permission embedded in .np, same-model completion assertion used as verifier.

### B206 — Enforce consequence-aware calibrated execution policy

Bind matching-domain correctness estimates to external consequence thresholds and verifier/escalation paths while retaining normal authority. Negative cases: 0.999 confidence grants capability, completion from probability alone, wrong-domain calibrator accepted, fallback widens privacy/effects.

### B207 — Join production outcomes and detect calibration drift without self-promotion

Join decision events to observed outcomes, report drift and create versioned shadow recalibration/training candidates through existing admission. Negative cases: ambiguous outcome treated as truth, in-place active calibrator mutation, evaluator data exposed to candidate, mid-episode swap.

### B208 — Reconcile MiCode v0.13 calibrated-decision peer contract

Pin the MiCode v0.13 consumer and prove lossless calibration provenance, fan-out identity, shortlist lineage, local PermissionGate and independent completion semantics on bounded fixtures; real live pair remains separately gated. Negative cases: remote confidence grants permission, shortlist lineage lost, stale calibrator survives bridge.

### B209 — Close v0.19 RLCD/Jev-informed package and adversarial coverage

Run package/reference tests, owner-lock checks, generated-view freshness and adversarial requirement coverage; record that Jev internals and live product/model gates remain unverified. Negative cases: CX-36/schema drift, product PASS from unit tests, secondary hypothesis stated as vendor fact, orphan gate/task.

### B210 — Freeze schema/frontend and fixed-task qualification owner seams

Inspect actual owners; map bounded schema lowering into existing catalogs/questions/composition and exact task/label/projection identity. No new IR/ABI/runtime unless an explicit reviewed gap demands it. Negative cases: same-count changed labels, dynamic tools on a fixed head, schema compilation called admitted .np.

### B211 — Implement fail-closed normalized schema subset

Add bounded closed-object enums/consts, Boolean, small integer and optional/source-span lowering with byte/depth/candidate limits and no network discovery by default. Negative cases: duplicate/unsafe keys, remote/recursive references, arrays truncated to first item, ignored unknown keyword.

### B212 — Implement active decode, exact projection and invocation validation

Bind snapshot/schema/catalog/result, active presence/value branches, exact Unicode source spans and registered cross-field checks. Negative cases: missing false default, optional absence from missing output, typed enum mismatch, stale target, individually valid but incompatible args.

### B213 — Connect schema proposals to trusted effect and event gates

Route through existing resolver/permission/check owners and distinguish marginal/reference/whole-invocation/verified events. Negative cases: readOnlyHint grants shell, heuristic minimum called joint correctness, complete composition treated task completion.

### B214 — Build governed teacher jobs and independent fixed-task corpus

Implement scoped resumable teacher labeling, exact ID joins, human/teacher/outcome adjudication and connected-group role isolation with evaluator custody. Negative cases: partial final log, same-input test cache used for train, approval called human truth, foreign lock takeover, ambiguous paid retry.

### B215 — Compare simplest lightweight fixed-task training arms

Preregister one low-risk diagnostic/inspection task and incumbent/rule/TF-IDF/frozen-encoder-head arms; record exact tuning, seed, dataset and proper-score/per-class/coverage results. Reject/inconclusive is valid; no mandatory upstream dependency or model download.

### B216 — Qualify independent advisory thresholds and protected evaluation

Freeze trained candidate/calibration/finite-grid search, correct comparison budgets, count independent representatives, require critical-class support and use final test once under protected budget. Negative cases: 30 successes treated guarantee, null mapped zero, stale/wrong event, adaptive test tuning.

### B217 — Qualify runtime-bound lightweight artifacts and shared encoders

Reference existing .np/lifecycle or non-neural representation; bind task/labels/encoder/tokenizer/head/projection/calibration/precision/runtime/batch profile. Test isolated shared-feature lifetimes, truncation refusal, cold/warm load and parity without new CX-36 syntax.

### B218 — Run shadow, full-cost admission and drift/revocation recovery

Run bounded shadow plus independent candidate disposition. Adoption requires measured whole-cascade benefit; rejection/inconclusive retains incumbent. Exercise drift, data/release revocation, generation pinning, canceled/unknown work and authorized fallback, never in-place self-training.

### B219 — Prove paired low-risk Axon v0.20 / MiCode v0.14 use

Pair with MiCode T215 using pinned real source/provider/consumer and one authorized low-risk task. Preserve span/schema/label/runtime/threshold lineage and local authority, actual independent verification and next-turn recovery. Offline imports/fixtures are not live interop.

### B220 — Maintain v0.20 package, profile fixtures and owner export integrity

Maintain bidirectional requirement/task/gate mapping, source notes, new negative fixtures, generated views, immutable format checks and owner export/consumer locks. This package-conformance work does not require successful model training and does not activate product gates.

### B221 — Optional learned working-set relevance experiment

Preregister critical-context recall and restored-source/protected-pin outcomes; treat changed context as a treatment, compare full cascade and retain fallback. No automatic deletion or no-compaction claim.

### B222 — Optional richer learners and schema-family extension experiments

Separately propose code/trace/world encoders, MLPs, dynamic candidate scorers, agent/tool routers or arrays/other schema families only after supported-scope evidence. No new dependency on these experiments for contract/shadow closure; explicit rejection is acceptable.

### B223 — Reconcile the v0.20 contract with the uploaded source snapshot

Create a source-pinned implementation overlay before touching files. Preserve live CX-35 Reflex naming through an explicit alias map, distinguish source evidence from package NOT_RUN, and retain CX-36/ACE formats.

### B224 — Upgrade package-gate compatibility without rewriting source history

Reconcile script schema, --output flag, exclusion policy, zero orphan counts and external gate registry. Use dry-run planning; retain v0.15 vendored bytes and logs. Runtime gate results require source-executed evidence, never fixture PASS.

### B225 — Specify negotiated decision records at the existing Reflex boundary

Add a bounded research decision profile with typed outcomes, candidate identities and score semantics. Keep axon-reflex/1 old operations working; unsupported new profiles explicitly refuse. Input state and versioned inference are not implemented by the current decided(question) fixture.

### B226 — Expose a candidate view over the existing capability registry

Define immutable semantic IDs, candidate-set/version hashes, eligibility filters and existing owner exports. An action view is not a new registry. Preserve named program, skill, tool, model and role provenance and revocation.

### B227 — Build an opt-in CLM adapter and isolated embedding cache

Implement behind ReflexBackend after negotiated-profile approval. Pin encoder, tokenizer, pooling, head and runtime identity; handle upstream truncation explicitly. Enforce principal-qualified cache keys and atomic head epochs. Run real backend parity and isolation tests before any live use.

### B228 — Qualify Jev and RLCD-compatible typed decision adapters

Retain v0.19/v0.20 decision/calibration contracts. Map advertised provider capabilities to actual binary/choice/score behavior and bounded APIs, distinguishing backend errors from decisions. Never infer RLCD training or calibrated truth from TypeSafe-compatible wire shape.

### B229 — Add pijev-inspired order-robustness wrappers

Freeze bounded permutation schedules, semantic option IDs, wrapper fingerprints and full-cost evidence. Label-align each response and reject incomplete distributions. Do not permute ordinal rubric meaning. Measure question-order coupling separately.

### B230 — Freeze calibration, abstention and candidate-set qualification

Bind independent calibration to task family, candidate construction, model/head/wrapper, projection and threshold policy. Add all-bad, near-duplicate, singleton, out-of-domain and distribution-shift cases. Unknown qualification selects incumbent/refuses, not optimistic acceptance.

### B231 — Train CLM heads only from qualified and split-safe evidence

Optional activation of an actual training trial requires authorized trace lineage and separate train, validation, calibration and final-test roles. Unchosen actions are not automatic negatives. Publish immutable head candidates through existing admission; frozen encoder identity remains part of artifact compatibility.

### B232 — Add CheckEval-inspired evidence-linked checklists

Freeze criterion IDs, rubric version, applicability and required conditions before running candidate agents. Use PASS/FAIL/UNKNOWN and qualified N/A at the evidence layer. Map model judgments to evidence claims, not fabricated observed checks.

### B233 — Protect whole-task outcomes and independent evaluation

Preserve hidden-check separation, actual-process check events and final outcome semantics from source. Keep subjective scores distinct from deterministic completion. Judge agreement across shared lineage is not independent evidence.

### B234 — Build a regularized harness-mutation ledger

Implement RRSI-inspired proposal memory, bounded component edits and annealed mutation schedules under existing self-application. Store rejected hypotheses and diffs. Distinguish harness mutation, compiler rewriting and learned-weight fitting as separate intervention classes.

### B235 — Preregister leakage criticism and noise-aware comparisons

Critic screens benchmark-specific changes but never substitutes for held-out testing. Freeze task assignment, seeds, budgets, multiplicity/sequential-testing policy and noninferiority margins. Inconclusive is a first-class result; retry-until-pass is forbidden.

### B236 — Keep pruning and promotion under independent admission

Allow removal of demonstrably redundant optional mechanisms, not authority or monitoring. Candidate acceptance produces an admission request, not activation. Track rollback identity and resource commitments; meta-evolution cannot redefine the admission predicate.

### B237 — Implement a deterministic routing control with session identity

Start with existing static/rules routing over eligible registry entries. Record session/cache affinity, provider health, state compatibility and switching cost. Preserve no-model fallback and no permission inference from cortex-policy-adapter allow responses.

### B238 — Qualify TinyRouter, TRINITY and Semantic Router adapters

Treat named projects as alternatives within existing routing policy. Declare model/role/action scope and evaluate against static, cheapest eligible and best-single controls at matched task budgets. Bound routing recursion and switching. No automatic new-model activation.

### B239 — Add canonical-history context projections

Implement Cliff-inspired projection as a CX-28 policy over CX-10 canonical events, not a new memory service. Never compact a previous compaction as canonical input. Bind original event ranges and retention policy, and compare to no-compaction control.

### B240 — Preserve critical context, pair closure and recovery

Pin active obligations, constraints and tool call/result pairs. Tokenize using the consuming model and block if mandatory pins exceed the budget. Prefix mismatch recomputes a safe original projection or refuses; never fail-open into an oversized request. No automatic shell proxy installation.

### B241 — Specify bounded response-level draft/verify cascades

RLM-Cascade is a research strategy under existing decision composition. Direct and draft/verify paths use immutable candidate bytes, deadline and cost reservations. Initial support is proposals only; cheap skip requires separately qualified correctness policy, never singleton softmax.

### B242 — Isolate speculative proposals from side effects

Use existing grants and transactional executor for the one selected action after verification. No speculative tool dispatch or side-effectful patch promotion. Typed terminal states handle cancellation, timeout and unknown outcomes; commit is at most once.

### B243 — Unify whole-task evidence and priced usage

Reuse existing AI/benchmark/replay evidence. Attribute disjoint input/cache-write/cache-read/output usage, embedding/GPU overhead and attempts with stable IDs. Unknown usage is not zero. Preserve all task outcomes and cancelled/failed charges.

### B244 — Enforce shared budgets and controlled economic comparisons

Preserve the authoritative integer principal budget in axon-core. Adapt richer observed usage without double-charging or replacing it. Compare shadow treatments with equivalent resource admission and record cold/warm conditions, queueing and resource contention.

### B245 — Replay decision, route, context and cascade evidence

Record exact nondeterministic responses/projections as versioned evidence through existing recording/replay seams. Replaying a recorded episode does not call a live model. Distinguish a content digest from externally anchored event integrity.

### B246 — Register the eight research families as qualified experiments

Complete source/provenance cards, immutable backend/model pins before execution, offline fixtures and preregistered real-provider plans. Negative/inconclusive results are valid. Paper benchmarks remain author claims; no dependency on reproducing SOTA or a given savings percentage.

### B247 — Define a proposed MiCode consumer delta without asserting delivery

Supply a consumer addendum targeting a proposed MiCode support 0.15; retain v0.20/v0.14 historical requirements. Actual consumer source/pack is required for schema export locks, capability negotiation, refusal/cancellation and next-turn recovery gates.

### B248 — Exercise revocation and atomic runtime transitions

Pins bind per-request epochs while dispatch rechecks current eligibility. Revoked data/head/provider stops new requests; in-flight results cannot be promoted into a new epoch. Bounded fallback preserves principal and task authority.

### B249 — Maintain v0.21 package conformance and preserved contracts

Regenerate source-derived views, review coverage, strict schemas, offline reference fixtures, input inventories and owner export checksums. Preserve CX-36 r0.2 and ACE v1 JSON bytes; package status remains proposed regardless of fixture success.

### B250 — Run one explicitly authorized real end-to-end pilot

On the actual Rust source and authorized model endpoints, execute bounded decision/routing/context/cascade paths and the incumbent under frozen controls. Capture cargo and runtime evidence, permission denials, cancellation, rollback and independent final checks. MiCode claims additionally require B247 peer execution.

### B251 — Optional outer-loop evolution-policy experiment

Test annealing/exploration/pruning schedules only within fixed safety and evaluator boundaries. Hold out distinct tasks/time windows and treat meta-policies as separate candidates with lower iteration frequency and explicit independent admission.

### B252 — Optional large-candidate CLM retrieval experiment

Prototype ANN shortlisting as a retrieval view, not authority. Compare to exact scoring, measure critical-candidate recall and index/head compatibility. The released CLM does not by itself prove million-program routing quality.

### B253 — Optional specialized heads and learned router optimization

Compare specialized heads and routers to existing deterministic/CLM/Jev baselines. Charge training and serving economics separately; guard against novelty/model-name descriptions substituting for measured outcomes.

### B254 — Optional best-of-N and parallel response speculation

Allow only isolated response/patch candidates with reserved N-way cost, independent verification of the selected bytes and bounded scheduling. No speculative side effects or claims of target-distribution equivalence.

### B255 — Reconcile Axon, MiCode and Fabric source truth

Pin all supplied inputs and actual dirty/untracked target work; map existing CX/MX/ACF owners and legacy release profiles. Preserve B00-B254, old evidence, schemas, source CX-35 and independent Cargo versions. Resolve drift before source edits.

### B256 — Negotiate the existing CX-16 / MX-12 bridge profile

Implement a closed, opt-in sidecar profile for existing bridge artifacts. Negotiate exact schemas/features and local compatibility adapters; never infer support from document version 0.22 or rename ACE/Reflex wire versions.

### B257 — Bind identities and immutable evidence sidecars

Map Task/ExperimentArm/Trial/Attempt/Operation/Execution/Branch and policy/context/ACF receipt references without altering existing episode hashes. Bind receipts to authenticated producers and immutable artifacts; add same-episode and multi-attempt reconciliation.

### B258 — Converge compute and peer requests at existing authority

Reuse Axon supervisor/grant admission and MiCode local permission owners. Resolve executable closure/profile/approval/resource references from trusted registries, not caller text. Recheck current epochs at all effects and retirement routes.

### B259 — Preserve the truthful legacy process adapter

Retain scoped interpreter semantics and CLI parity; introduce private staging, bounded process/output handling and truthful process_scoped labels. Never translate unknown native effects into an empty effect set.

### B260 — Implement durable operation journal and aggregate reservations

Implement transactional intent-before-effect, uniqueness, fencing, outbox/reconciliation and shared budget reservations. Keep action, resource, billing and cleanup states separate. Fault-test actual persistent storage.

### B261 — Materialize immutable workspaces with per-trial isolation

Reuse MiCode WorktreeManager and Cortex observation digests through explicit projections. Publish durable content versions, fresh per-trial build/dependency caches and fenced leases; harden import and resource quotas.

### B262 — Extract axon-vm as a profiled backend without CLI drift

Refactor the existing Firecracker launcher only as required for a narrow library adapter. Preserve CLI, interpreter/browser dependency direction and attestation meaning; pin actual backend versions during implementation.

### B263 — Physically qualify the protected Linux microVM profile

On a suitable Linux/KVM host, test host/guest boundaries, dedicated UID/cgroup/process ownership, networking denial, rootfs/image pinning, crash cleanup and quotas. Keep production qualification empty until these tests run.

### B264 — Wire one real registered Cortex CheckExecutor

Introduce the narrow Runner registered-check seam with a separate fixture implementation and Fabric-backed implementation. Execute frozen checks outside subject authority against exact output bytes, keeping hidden checks outside worker-visible context.

### B265 — Join preflight, execution and independent outcome evidence

Keep ExecutionContextReceipt, Fabric ExecutionReceipt, Cortex/MiCode episode and independent verifier result as separate referenced records. Reconcile all IDs and output digests; reject altered, missing or cross-tenant evidence.

### B266 — Gate MiCode workers on observed execution context

Implement the documented ExecutionContextReceipt lifecycle and three use sites: task engine, worker-before-first-model-turn and result integrator. Observe context independently; establish exact trial base for the pilot, namespaces, role, model route and build cache.

### B267 — Enforce MiCode scope intersection and preserve trust graduation

Close or explicitly refuse unprovable delegation subset semantics at actual tool/file dispatch. Preserve PermissionGate and the independent trust-judge NoGo/graduation mechanism; no imported evaluator changes that verdict.

### B268 — Consume policies through the real MiCode bridge

Wire versioned policy import into the actual session/headless/build-loop composition, behind explicit opt-in. Enforce applicability, scope, active policy version and evidence requirements. Keep only one adoption owner.

### B269 — Export complete MiCode episodes to actual Axon intake

Produce canonical versioned episode sidecars from observed MiCode execution. Join before/after artifacts, denied/failed/cancelled/unknown outcomes, effective input, policy context and authoritative cost; preserve sensitivity and corpus roles.

### B270 — Reconcile whole-task inference and execution economics

Join price-weighted model usage and Fabric CPU/memory/disk/check costs through shared budget owners. Pin currency, price schedule, usage source, experiment cohort and pending liabilities. Separate estimates from measured costs.

### B271 — Run logical A/B branches with fenced promotion

Implement Fabric logical branches from one immutable base with independent TrialIds, AttemptIds, worktrees, mutable caches and carved shared budgets. Use CAS for workspace publication, separate from policy admission.

### B272 — Bind the 0.21 profiles to a deterministic pilot baseline

Use existing owned DEC/RTR/CVM/SPX/TEL contracts with explicit deterministic behavior: fixed model/route/context, no active speculation, known eligible candidates and full receipts. Learned providers remain separately qualified and optional.

### B273 — Generate one bounded EVO policy candidate

Use the existing hypothesis/evolution ledger to propose a tool/skill shortlist variant from allowed discovery evidence. Record mutation identity, rationale, parent, edit ceiling and all rejected hypotheses. Do not let proposer edit evaluation/admission.

### B274 — Freeze the independent experiment and admission plan

Register the task/repository split, candidate generation budget, repetitions, independent unit, fixed-horizon or valid sequential method, noninferiority/economic criteria and contamination controls before confirmation data is visible. Require an approved completed plan for live testing.

### B275 — Execute controlled real incumbent/challenger trials

Run the real MiCode path on prespecified tasks in authorized isolated Fabric jobs. Block/randomize order, bound repeats, enforce model/cache/resource parity and capture actual effective policies and workspaces for both arms.

### B276 — Evaluate exact candidates outside subject authority

Execute frozen deterministic checks and applicable evidence-linked checklists independently. Preserve whole-task conjunctive gates; CLM/Jev or worker judgments are auxiliary and cannot compensate for failed mandatory tests.

### B277 — Apply the frozen independent policy-admission decision

Use existing admission ownership to evaluate held-out confirmation evidence under frozen constraints. Record accepted/rejected/inconclusive separately from candidate ranking and require current scope/revocation checks; never autoactivate the top-ranked candidate.

### B278 — Prove fenced policy activation and future-task uptake

Atomically switch a scoped active-policy pointer using expected incumbent digest and monotonic epoch. Pin it at task start and record acknowledgements/effective digest in subsequent real MiCode tasks; do not live-edit active trials.

### B279 — Exercise rollback, revocation and safe paused states

Run real regression/failure injection against a scoped activation. Roll back by a new fenced decision only to a still-valid admitted predecessor, or pause. Drain/reconcile protected jobs and keep accounting/readers.

### B280 — Fault-test cancellation, budgets and restart across peers

Inject duplicate/out-of-order messages, process death, lost acknowledgements, worker timeout and unknown billing. Verify journals, inherited cancellation, operation reconciliation, authenticated receipt ingestion and non-duplicated resource charges.

### B281 — Enforce learning eligibility and data-retention boundaries

Route only eligible observed histories to training/retrieval with license, tenant, retention and corpus-role labels. Candidate-selected data is not unbiased counterfactual evidence; protected confirmation data stays excluded.

### B282 — Close joint adversarial and authority-bypass tests

Exercise malicious repository/tool output, receipt forgery, context echo, scope widening, candidate substitution, hidden-test exfiltration, cache cross-contamination and re-entry through fallback/replay/attach. Track every finding and limitation.

### B283 — Integrate compatible package and cross-project source gates

Update package-gate invocation only after source review of schema/CLI/count conventions; add scoped source tests and interoperability checks without hiding missing Rust/KVM prerequisites or altering historical evidence.

### B284 — Validate the complete 0.22 specification and reference pack

Maintain generated master/views, schema fixtures, parent/Fabric checksums, source pins, requirement/task/gate mappings and distinct runtime qualification ledger. Run documentation/reference tests separately from product gates.

### B285 — Qualify the bounded operational closed loop

Assemble independent nonvacuous runtime evidence for the selected dependency closure, including ACF-G00-G37. Demonstrate real proposal, trials, legitimate rejection and controlled activation/uptake/rollback within preauthorized scope. Activation mechanics may use a clearly labeled approved fixture policy, never a fabricated improvement claim.

### B286 — Report evidence for or against a real improvement claim

Execute the separately approved held-out reporting study for the selected task distribution; report uncertainty, paired outcomes, full task costs and failure classes. Evidence may support benefit, harm or inconclusive; assert improvement only if the prespecified rule and independent evidence support it.
