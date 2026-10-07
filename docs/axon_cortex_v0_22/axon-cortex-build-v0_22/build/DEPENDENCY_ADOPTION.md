# Cortex dependency, model and dataset adoption

**Status:** Draft normative build contract for v0.3.

## Purpose

System-One/Jev ecosystem repositories are implementation leads, not pre-approved production dependencies. Cortex must be able to reproduce what it adopted, identify transitive artifacts, understand license/security consequences, and distinguish upstream benchmark claims from Axon evidence.

## Adoption manifest

Every external repository, package, model, tokenizer, dataset, evaluator or hosted endpoint used beyond disposable exploration gets an `AdoptionManifest`.

```text
AdoptionManifest {
  artifact_class,
  source_url, source_commit_or_version,
  integrity_digest,
  license_and_notice_refs[],
  dependency_lock_digest?,
  transitive_model_refs[], tokenizer_refs[], encoder_refs[],
  dataset_refs[], preprocessing_refs[], evaluator_refs[],
  security_profile, network_endpoints[], credential_requirements[],
  local_reproduction_commands[], local_reproduction_receipts[],
  approved_use: research | benchmark | dev | protected_runtime,
  owner, review_date, supersedes?
}
```

Pin the artifacts that determine behavior, not only the top-level repo. If a scorer loads an external encoder by name, that encoder revision is part of the effective model identity. If a dataset or model has separate terms, record them separately.

## Intake sequence

1. Pin source commit/version and archive integrity.
2. Read license/notice and terms for code, model weights, datasets and hosted services separately.
3. Enumerate transitive executable/model artifacts and network dependencies.
4. Reproduce minimal upstream example in an isolated environment.
5. Re-run a Cortex-owned conformance fixture rather than accepting README output.
6. Record security defaults; insecure example defaults do not become protected deployment defaults.
7. Add the artifact to SOURCES with claim boundaries.
8. Permit wider use only through owner review and, where appropriate, admission gates.

## Benchmark comparability review

Imported results must state whether systems received the same:

- state/evidence;
- candidate set and candidate ordering;
- output obligation;
- reasoning/generation budget;
- retries/fallbacks;
- tools and permissions;
- cache/warmup conditions;
- completion verifier;
- failure accounting.

If these differ, the result may still motivate an experiment but is not presented as a direct performance comparison. Cached responses are labeled separately from live inference. Injected-failure benchmarks are labeled separately from naturally occurring failures.

## Hosted service profile

A sample endpoint being unauthenticated or broadly network-accessible is not an Axon deployment recommendation. Protected-runtime adoption requires authenticated principals, transport security where applicable, tenant/cache isolation, budgets, observability, revocation, and the host policies required by CX-13.

## SDK transformations

Official/community SDKs may retry, normalize probabilities, coerce outputs, truncate prompts, remap candidates, or select fallback models. Cortex wraps these transformations in explicit adapter events and budget accounting. Convenience behavior that cannot be observed or disabled is evaluated as part of that backend, not treated as invisible plumbing.

## Update policy

Dependency/model updates create a new effective identity and trigger the relevant conformance, calibration and protected-task checks. Floating aliases are allowed only for explicitly non-reproducible research profiles; protected evidence records the effective resolved model identity returned by the provider when available.

## Initial ecosystem adoption categories

The following categories are suitable for research intake, subject to repository-level verification at implementation time:

- official TypeSafe SDKs and System-One adapter for interface semantics;
- OpenJev/openjev-sglang/MLX/sequence-scoring implementations for bounded-option inference techniques;
- learned option scorers for variable candidate-set research;
- Jev benchmarks/calibration projects for fixture ideas;
- browser/computer-use agents for dynamic action-space patterns;
- guard/triage/search/tree projects for decomposition and hierarchical candidate search ideas.

This list conveys research relevance, not license clearance, security approval or benchmark reproduction.

## Acceptance cases

- Changing a transitive encoder revision changes effective identity.
- An unpinned/floating model used in protected evidence is refused or explicitly marked non-reproducible.
- A probability-normalizing SDK records the pre/post transformation.
- Corrective retries consume the same parent budget and remain visible.
- An unauthenticated demo endpoint fails the protected-runtime host profile.
- A benchmark with easier candidate information is accepted as an implementation lead but rejected as a direct quality comparison.
