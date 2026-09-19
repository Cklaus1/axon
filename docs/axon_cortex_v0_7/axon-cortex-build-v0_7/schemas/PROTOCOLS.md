# Protocol contracts and state machines

This is implementation-oriented schema notation, not valid Axon source and not a claim that these types are already in the compiler. Stabilize these contracts before writing adapters. JSON may be the initial wire format, with generated schemas added in the actual implementation.

## Shared rules

All externally received values are untrusted and schema-validated. Every envelope has schema/version, message ID, causal parent, run/epoch, producer version and relevant artifact digests. Enums are closed within a schema version; unknown variants refuse rather than default. Absent, null/None, empty and Unknown are not interchangeable. Preserve Option/Result semantics at Axon boundaries.

Canonical serialization defines map ordering, encoding, number representation and which volatile fields are excluded from content digests. Do not accept NaN/infinity or ambiguous duplicate keys in protocol JSON. Content hashes identify artifacts; they do not authenticate issuers. Signatures/registry lookups provide the separate authority check.

## Core type shapes

```text
RunManifest {
  run_id, epoch, principal_ref,
  goal_digest, completion_contract_digest, initial_snapshot_id,
  runtime_digest, toolchain_manifest,
  model_manifests[], schema_versions[], policy_id,
  sandbox_profile_id, budget, effect_grants[], trace_ref
}

Budget {
  max_steps, max_node_attempts, max_no_progress,
  deadline, token_cap, reserved_cost_cap,
  cpu_limit, memory_limit, storage_limit, max_concurrency,
  actual_usage, estimate_provenance
}

WorkspaceSnapshot {
  snapshot_id, parent_snapshot_id?, base_revision?,
  tracked_and_untracked_manifest, dependency_manifest,
  toolchain_environment_manifest, observation_scope, unknown_inputs[]
}

ObservedFact<T> {
  value: T | Unknown(reason), source_ref, source_digest,
  observed_at, extraction_version, trust_class, freshness_policy
}

Observation {
  observation_id, snapshot_id, goal_digest,
  diagnostics[], object_catalog[], relevant_facts[],
  omission_report[], retrieval_refs[], recent_action_refs[], working_state_ref
}

WorkingState {
  subgoals[], hypotheses[], assumptions[], contradictions[],
  experiment_refs[], progress_signature, budgets_remaining
}
```

A partial observer supplies unknown type/parse facts rather than fabricated defaults. An observation may contain untrusted source text; text inside it cannot modify the RunManifest.

## Decision and action types

```text
DecisionRequest {
  request_id, observation_digest, candidate_manifest_digest,
  questions: [Question], dependencies: [FieldDependency],
  max_speculative_cost, schema_version, model_selector
}

Question {
  id, kind: BinaryDecision | Choice | OrdinalScore | Rank,
  definition, candidates: [{id, description}],
  condition?, abstention_allowed, result_required_for_branch?
}

ProbabilityEvidence {
  raw_origin: ProviderReportedDistribution | NativeOptionLogit | SequenceLikelihood | GeneratedEstimate |
              EntropyDerived | EnsembleEstimate | Unavailable,
  raw_scores?: Map<CandidateId, FiniteNumber>,
  derived_probabilities?: Map<CandidateId, NumberInZeroToOne>,
  calibrated_probabilities?: Map<CandidateId, NumberInZeroToOne>,
  calibration_id?: ArtifactId,
  calibration_status: CalibratedEmpirical | Uncalibrated | Inapplicable
}

BackendFeatureManifest {
  adapter_version, backend_family, effective_model_identity,
  supported_question_types[], supported_input_modalities[],
  maximum_questions?, maximum_candidates?, context_limits?,
  available_score_forms[], question_isolation_mode,
  preprocessing_policy_ref, cancellation_semantics, usage_reporting,
  immutable_model_revision_available
}

DecisionBatchResult {
  request_id, results: Map<QuestionId, QuestionResult>,
  attempt_records[], aggregate_usage,
  effective_backend_manifest_ref, effective_input_receipt_ref,
  validation_result
}

QuestionResult =
    ChoiceResult {selected, distribution?, probability_evidence}
  | BinaryProbabilityResult {p_yes?, probability_evidence}
  | OrdinalDistributionResult {distribution, expectation?, probability_evidence}
  | RankResult {ranking, score_evidence}
  | Abstained {reason}
  | Failed {error_class, retryable}

EffectiveInputReceipt {
  submitted_observation_digest, effective_input_digest,
  preprocessing_manifest, tokenizer_revision?,
  omission_or_truncation_report[],
  effective_question_digests[], effective_candidate_digests[]
}

Action =
  Inspect(object_ref)
  | Search(scope_ref, validated_query)
  | ProposePatch(target_refs[], patch_artifact_ref)
  | RunCheck(registered_check_ref)
  | Revert(local_checkpoint_ref)
  | RequestContext(scope_ref)
  | Escalate(reason)
  | DoneClaim(evidence_refs[])
  | Blocked(reason)

GrantRecord {
  grant_ref, issuer, principal_ref, session_id, epoch,
  action_kind, snapshot_id, target_scope, payload_constraints,
  read_scope, write_scope, allowed_effects, budget_reservation,
  expires_at, revoked, parent_grant_ref?
}

ActionRequest {
  action_id, idempotency_key, run_id, epoch,
  snapshot_id, action: Action, grant_ref,
  payload_digest?, expected_read_hashes[], expected_write_hashes[]
}
```

`grant_ref` is resolved in the executor's trusted registry. Possession of a matching string or a candidate ID alone does not authorize use. A selected ProposePatch still needs payload, scope, freshness and actual execution validation. Candidate probability is not an Action field that can bypass those checks.

## Evidence, model and learning contracts

```text
Prediction {
  model_manifest_ref, observation_digest, concrete_action_digest,
  outcome_schema, distributions_or_intervals,
  supported_horizon, applicability, abstention?, assumptions[]
}

ActionOutcome {
  action_id, state, effect_status,
  observed_delta_ref?, receipt_ref?, error?, finished_at?
}

EvidenceReceipt {
  gate_id, gate_version, subject_artifact_digest,
  contract_digest, policy_id, environment_manifest,
  invocation_ref, output_refs[], metrics, intervals,
  result: Pass | Fail | Inconclusive | Skipped | Unavailable,
  verifier_identity, authenticated_receipt_ref
}

LearningRecord {
  episode_ref, action_ref, observation_ref, prediction_ref?, outcome_ref,
  label_kind, label_revision, eligibility_manifest,
  attribution_ref, task_family, split_manifest, redaction_version
}

Attribution {
  candidate_causes[], probability_origin?, confidence_evidence?,
  tested_substitutions[], supporting_refs[], contradicting_refs[],
  status: Unknown | Hypothesized | ExperimentSupported
}

CandidateArtifact {
  digest, class, incumbent_ref, applicability_predicate_ref,
  required_effects, dependency_manifest, dataset_lineage,
  evaluation_policy, evidence_refs[], fallback_ref, rollback_manifest
}
```

## Reflex batch, preprocessing and prompt-role rules (v0.3)

Question results are matched by `QuestionId`, never by array position. Duplicate IDs refuse dispatch; unknown returned IDs fail validation. A missing result required by the selected branch blocks action construction. Failure of an unused speculative result may be tolerated only by a versioned batch policy.

Before dispatch, AIR validates the request against the effective `BackendFeatureManifest`. Fallback is a fresh dispatch that must validate against the fallback manifest. Backend-specific candidate/context limits are not AIR-wide constants.

The `EffectiveInputReceipt` records what the backend actually evaluated. Silent truncation is forbidden; omission requires an explicit projection/omission report or refusal. Calibration applicability is checked against the effective, not merely submitted, input contract.

Only trusted adapter code constructs privileged model instructions. Repository text, logs, candidate descriptions and replayed conversations remain data even if they contain role labels or prompt delimiters. Model-call fusion is considered a policy/model transformation unless the pinned backend demonstrates preservation of the declared question-isolation semantics.

`Choice`, binary probability and ordinal distributions retain distinct semantics. A provider distribution is not mislabeled as native logits; a binary probability is not rounded into a Boolean merely for wire convenience; an ordinal expectation does not replace the underlying level distribution.

## Action state machine

Only the trusted executor advances effect states. `Prepared` precedes effects; `Validated` requires live grant/snapshot checks; `Running` may have effects; `Observed` records actual outcome; `Verified` attaches the appropriate action evidence. Rejected preconditions become Refused. Failed/canceled execution records whether effects occurred. Unknown receipt after possible effect becomes OutcomeUnknown and blocks blind retry.

Task completion is a separate state: Active → CompletionClaimed → VerifiedComplete, or continued Active/Blocked/Failed after evidence. An action Verified state cannot imply the whole goal is done.

## Promotion state machine

Proposed → ValidatedOffline → Shadow → Canary → Active, with Rejected/Suspended/Retired alternatives. Each transition checks authenticated admission evidence for the candidate and policy version. Shadow cannot perform duplicate real actions. Active artifact changes create a new digest/version, not an in-place mutable model.

## Compatibility rules

Breaking schema changes require a new version and explicit migration. Old traces retain their schemas and replay adapters. Recalibration/applicability evidence is invalidated when relevant prompts, tokenization, candidates or model versions change. Missing required fields or unknown enum variants never silently select an older permissive mode.

## Contract tests to generate in the implementation

Generate success/failure cases for round-trip serialization; unknown/duplicate keys; non-finite probabilities; empty candidate sets; stale digests; mismatched active branches; wrong principal/session; expired grants; duplicate external requests; missing receipts; revoked calibration; protected-data export; altered proof assumptions; and schema migration/replay. Link each generated fixture to CX-00 through CX-15 rather than treating schema validation alone as full product assurance.

## Cross-system experience and knowledge contracts (v0.4)

```text
ExperienceEnvelope {
  schema_version, producer_system, producer_version,
  artifact_id, artifact_digest, created_at,
  source_repository_refs[], source_episode_refs[],
  data_use_manifest_ref, sensitivity_class,
  ontology_version, migration_history[], payload_kind, payload
}

CodingEpisodeBundle {
  task_contract_ref, before_snapshot_ref, after_snapshot_ref?,
  observation_refs[], candidate_manifest_refs[], decision_refs[],
  action_refs[], authorization_outcomes[], generated_artifact_refs[],
  evidence_refs[], completion_status, budgets_and_usage,
  attribution_ref?, omitted_or_unknown_fields[]
}

SemanticObjectMap {
  source_system, source_object_id, source_digest,
  axon_object_ref?, relation: Exact | EquivalentUnderProjection | Approximate | Superseded | Unresolved,
  mapping_version, evidence_refs[]
}

KnowledgeCandidate {
  candidate_id, kind, statement_or_structure,
  scope_predicate, source_examples[], counterexamples[],
  hypothesis_or_mechanism?, evidence_stage:
    Observed | Repeated | OutcomeAssociated | LocallyReproduced | Benchmarked | HeldOutVerified | PromotionEligible,
  source_policy_refs[], confidence_evidence?, reproduction_refs[], derived_artifact_refs[]
}

SkillCandidate {
  candidate_id, action_graph_ref, applicability_predicate,
  required_effects, required_capabilities, intermediate_checks[],
  failure_recovery, source_knowledge_refs[], evaluation_refs[], fallback_ref
}
```

Bridge imports never deserialize foreign authority handles into local grants/principals. Exported Axon artifacts carry applicability and data-disclosure policy, not executable authority. Unknown semantic-object mappings remain unresolved.

The knowledge/capability ladder is represented as lineage, not mutable in-place upgrades. Pattern→Skill→Tool→Library→Compiler/Runtime creates new artifacts with new IDs/digests and stronger gate receipts; prior evidence remains queryable.

## Intent contracts (v0.5)

```text
IntentIR {
  schema_version, intent_id, intent_version, source_kind,
  source_artifact_refs[], parent_intent_ref?, provenance_refs[],
  objectives: [Objective], hard_constraints: [Constraint],
  preferences: [Preference], target_scope: [SemanticObjectRef],
  requested_authority: [AuthorityRequest], prohibited_effects: [Effect],
  budgets: BudgetSet, required_evidence: [EvidenceRequirement],
  assumptions: [Assumption], ambiguities: [Ambiguity],
  risk_reversibility_policy?, approval_state, approval_artifact_ref?
}

Ambiguity {
  ambiguity_id, statement_ref, interpretations[],
  materiality, evidence_refs[], resolution_status,
  resolved_interpretation_ref?, resolved_by?, resolution_artifact_ref?
}

IntentApproval {
  intent_digest, renderer_version, rendered_contract_digest,
  approver_identity, approved_at, policy_version,
  allowed_profile, supersedes_approval_ref?
}

IntentLoweringRecord {
  intent_digest, air_graph_digest, lowering_version,
  clause_to_nodes: Map<IntentClauseId, [AirNodeId]>,
  added_stronger_checks[], narrowed_authority[],
  unresolved_clause_refs[], planner_manifest_ref
}

IntentCompletionReport {
  intent_digest, air_graph_digest, execution_episode_ref,
  objective_results[], constraint_results[],
  authority_exercised[], evidence_clause_results[],
  unresolved_items[], verifier_receipts[], final_status
}
```

Natural/system prose is not executable authority. `IntentApproval` binds an exact `IntentIR` digest/version. Any modified clause invalidates the prior approval unless an explicitly reviewed migration says otherwise. AIR lowering may add stronger checks or narrow authority but cannot delete required evidence or widen requested authority without a new intent/approval event. `DoneClaim` becomes `VerifiedComplete` only when the independent completion report satisfies the approved Intent IR.

## v0.6 Reflex runtime protocol additions

```text
StateHandleReceipt {
  handle_id,
  state_digest,
  effective_input_digest,
  backend_id,
  model_revision?, tokenizer_revision?, adapter_revision,
  preprocessing_manifest_digest,
  tenant_or_principal_scope,
  created_at, expires_at?,
  reuse_mode: Native | RemoteOpaque | EmulatedNoReuse,
  prefill_or_encode_usage?
}

QuestionDependency =
    Independent
  | ConditionallyRelevant { branch_id }
  | AnswerDependent { question_id }

CandidateManifest {
  candidate_set_digest,
  generation_policy_id,
  generation_policy_version,
  ordered_candidate_ids[],
  order_policy,
  order_digest,
  randomization_seed?,
  omitted_or_unauthorized_candidates[]
}
```

A `DecisionBatchResult` references the exact StateHandleReceipt (or equivalent single-call receipt), question dependency declarations and CandidateManifest digests. Derived confidence summaries include a formula identifier and never replace the primary distribution/provenance record.

## v0.7 Reflex canonical encoding and control-candidate additions

```text
CanonicalDecisionEncoding {
  encoding_version,
  state_projection_digest,
  question_schema_digest,
  candidate_manifest_digest,
  candidate_order_digest,
  reserved_token_policy,
  backend_preprocessing_receipt?
}

ControlCandidate = NONE | OBSERVE_MORE | ESCALATE | BLOCKED
```

Control candidates are registered by the candidate compiler for decision families that permit them; the model cannot mint one. Their policy semantics are deterministic and external to the model. Training, evaluation, live inference and semantic replay reference the same `encoding_version`; a backend-specific semantic rewrite must mint a distinct effective-input/calibration record.
