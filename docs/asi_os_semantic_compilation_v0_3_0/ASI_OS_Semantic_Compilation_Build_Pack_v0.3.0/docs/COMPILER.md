# Semantic Compiler Contract and Diagnostics

## Inputs
An authenticated control plane provides the normalized intent, allowed capability registry snapshot, approved evaluation contract, task-scoped context/artifact references, optimization profile and remaining compile budget. Neither a prompt nor a model-produced field can elevate permissions.

## Stages and artifacts
Normalize requirements and label inferred assumptions. Resolve approved capabilities. Select a bounded task template or decompose into a DAG. Lower to core operations. Bind all argument sources and output types. Propose additional verification without changing required criteria. Estimate resource use. Apply structural/semantic checks. Optionally rank or repair a candidate within the compile budget. Revalidate the exact selected result and emit an immutable candidate.

Persist normalized intent, candidate plans, selected plan, rejected-candidate diagnostics and compiler provenance. Candidate generation failures count as trial costs. Do not expose hidden test labels to the compiler through examples or reviewer summaries.

## Output result
The transport response contains a status, candidate references, assumptions, diagnostics, estimated usage, requested permissions, unresolved dependencies and provenance reference. `CANDIDATE` means structurally and semantically admissible as far as the validator checked; authorization and independent task correctness remain separate.

| Code | Meaning | Action |
|---|---|---|
| C001 | Ambiguous material requirement | Clarify before effectful work |
| C002 | No permitted capability | Return unsupported; do not invent a tool |
| C003 | Protected criterion cannot be evaluated | Ask for a criterion/evidence decision |
| C004 | Budget cannot bound the plan | Narrow task or reject |
| C005 | Invalid output contract or graph | Bounded compiler repair |
| C006 | Context unavailable or not authorized | Do not fabricate content |
| C007 | Required runtime feature absent | Reject instead of silently interpreting it |

## Planning quality
Prefer existing deterministic operations for parsing, arithmetic, schema checks and formatting with a fully specified algorithm. Use model calls for genuine semantic uncertainty. Do not rewrite a task's meaning to fit a cheap primitive. A deterministic replacement is eligible only if its semantics satisfy the requested task, not merely because it returns the same JSON shape.

## Evaluation-plan generation
The compiler can suggest a test harness, but the control plane combines that suggestion with the owner-approved contract. A repository task must preserve the independent regression gate and exact tested patch. A research task must check support for claims rather than just presence of links. A UI task needs behavior and human preference evidence, not only a screenshot score.

## First implementation
One generator, one normalized prompt template, one candidate and no learned graph rewrite are sufficient. Add candidate search only after compiler overhead and task-quality baselines are measured. The P1 adapter must return real provider usage and explicitly mark unresolvable model revisions.
