# Schema-driven decision frontend — proposed v0.20 implementation profile

**Owners:** CX-03 (trusted capability/dispatch), CX-05 (typed questions/results), CX-26 (catalog/projection/composition). **Status:** Draft, product NOT_RUN. This profile refines existing owner structures. It is neither a new execution ABI nor a Neural Program format. Its offline reference is deliberately narrower than a production MCP/WebMCP adapter.

## 1. Two different compilation operations

Schema ingestion builds an `ArtifactCatalog`, CX-05 questions and a CX-26 composition/projection plan. It does not train weights, create an admitted `.np`, confer authority or prove a task complete. A separately trained fixed-task Reflex classifier may implement some compatible questions and may be packaged under unchanged CX-36 r0.2. Deterministic rules remain a separate candidate representation. No `.reflex` format, additional registry, scheduler or admission authority is introduced.

Adapters for MCP/WebMCP must first resolve a locally registered tool identity and a versioned input schema. The initial normalized frontend accepts a closed top-level object schema; this is a bounded supported subset, not support for arbitrary JSON Schema or all tools. Every adapter declares its source protocol/revision and unsupported semantics. Parse duplicate object keys as errors before normalization. Preserve raw descriptor identity and the normalized schema digest; never silently drop an unsupported keyword to make a tool fit.

## 2. Initial supported subset and explicit exclusions

| Input | Existing question/projection structure | Required behavior |
|---|---|---|
| Registered tool set | `Choice` over stable local tool IDs plus explicit no-match | The no-match result is not an ordinary tool. Stable ID and schema/registry epoch travel together. |
| Scalar `enum` or `const` | `Choice` over opaque IDs bound to typed declared values | Do not reconstruct a value by parsing an opaque label; preserve Boolean/integer/string distinctions. |
| Boolean | `BinaryDecision` | Both true and false require an actual valid answer; missing is never false. External Noul spelling is adapter vocabulary, not new authority. |
| Integer with explicit inclusive minimum/maximum | Bounded `Choice` and typed projection | Initial reference limits the domain to 32 values. Nonintegral/out-of-range values and missing bounds are unsupported. |
| Free string with a bounded observed span catalog | `SELECT` then deterministic `PROJECT/COPY` | Preserve exact source content, Unicode code-point offsets, source digest/version and projection identity. No hallucinated string fallback. |
| Optional field | Required active presence question plus conditional value question | False presence omits the argument without injecting a default; true presence requires a valid value. A missing presence answer is invalid. |
| Required field with no suitable span | Explicit `NoSuitableCandidate` / observation request | Do not select the nearest wrong candidate or silently switch to generated text. |

The initial profile intentionally rejects nested objects, arrays (including enum sets), unions, conditionals, pattern-based maps, recursive/remote references and general unbounded numeric extraction. In particular, it must not populate only the first array element. Future adapters may qualify these semantics with new fixtures and a distinct profile revision, without changing existing `.np` syntax. OpenAPI/GraphQL/CLI grammars, code/world-state encoders, learned set/rank heads and broader dynamic extraction are optional research, not prerequisites to the initial slice.

The strict reference accepts only `type`, `properties`, `required`, `additionalProperties:false`, `title`, `description` at the object root; supported field annotations are title/description, with the scalar constraints above. Defaults, formats and other keywords remain unsupported in this profile rather than being ignored. Production adapters must either implement the exact semantics under a reviewed extension or explicitly refuse.

## 3. Resource and trust boundaries

Initial conformance limits: at most 32 tools; 32 fields per tool; 256 enum values per field; 32 candidate spans per free-string field; 64 KiB serialized tool manifest; 64 KiB source text; and 4,096 characters in any descriptor string. Limits are part of the profile identity. The reference also caps parsed nesting depth at 12 and traversed nodes at 8,192, and restricts normalized tool/property identifiers to 1–64 ASCII letters/digits/underscore/hyphen with a letter/underscore first. Other property spellings require a reviewed reversible adapter or refusal. Detect nesting/oversize before expensive recursion. Forbid dangerous property names (`__proto__`, `constructor`, `prototype`) at every parsed level. Reject references rather than fetching them. In the product adapter, impose parse depth/node-count and duplicate-key limits at the byte boundary before constructing language objects.

All page/tool descriptions and annotations are untrusted data. Screening can annotate suspicion but cannot remove taint or establish safety. `readOnlyHint`, destructive/consequential hints and confidence cannot set effective permissions. The trusted local registry resolves effects, principals, targets, grant requirements and confirmation policy, and the executor checks them again immediately before use. Schema discovery alone never invokes a tool, starts training, downloads weights or sends context to a provider.

Speculative questions have the same disclosure/compute budget obligations as other model calls. Only Independent/ConditionallyRelevant questions sharing the correct state may fan out. AnswerDependent values require sequencing or an explicitly bounded equivalent branch expansion. Account for unused questions and canceled work; no one-pass or constant-time guarantee is made.

## 4. Identity and source binding

The frontend retains: source adapter/profile revision; raw manifest digest; normalized tool/schema identities; local registry epoch; state/snapshot digest; catalog/candidate-order digest; typed value map; question semantics/dependencies; exact input projection; and optional approved Intent clause references. These populate or reference existing CX-03/CX-05/CX-26 fields, not a parallel production IR.

A span projection is `(source_ref, source_digest, source_version, offset_unit, start, end, projection_rule_id)`. The initial reference uses Unicode code-point half-open offsets and refuses empty/out-of-range spans. UTF-8 byte or JavaScript UTF-16 offsets need explicit conversion before this boundary. Do not normalize whitespace, case, punctuation, paths or Unicode after selecting a span while claiming exact copying. If normalized variants are useful, register a separate deterministic projection and retain the original source.

Revalidate state, current tool registry/schema and source at decode/prepare and again at dispatch. Same-named replacements, changed descriptions/criteria, candidate order, new fields and changed semantics are not silently compatible. Candidate generation is outside the model's authority. A missing correct entity/span produces absence or further observation, not an invented candidate.

## 5. Decode and complete-invocation validation

A valid result envelope matches the plan/state/catalog identities. Validate branch tag, active output presence, selected-ID membership, finite distributions and normalization before deriving any value. Initial reference supports complete distributions only as a synthetic test shape; production label-only backends retain CX-05 `Unavailable` probability semantics and cannot invent distributions.

Only active branch values become arguments. Inactive speculative outputs never dispatch. Missing active Boolean, member or optional-presence results cannot become false, absent, zero or a successful empty response. Unknown output IDs and stale envelopes are rejected. A valid omission is distinct from a missing answer. An optional value failure after a positive presence answer remains an invalid composition.

Run the final registered input validator and deterministic cross-field/precondition checks on the complete invocation, not merely each argument independently. Registered constraints may require another argument, enforce a valid combination, or rule out an action in current state. Externally supplied instructions cannot install executable validators. Unsupported validation semantics yield refusal, not an executable approximation.

The output is a proposed invocation or the existing CX-26 explicit partial/absence/refusal state. A `CompleteComposition` is not an executed action, authorization, verified outcome or `DONE`. Preserve provenance through the consumer so the local executor can resolve a current target and independently check permission.

## 6. Confidence is not joint correctness

Marginal class probability, calibration for a named event, complete-invocation validity, permission, execution outcome and verified completion are distinct. Minimum/product/average of component probabilities may be diagnostic heuristics only, never a calibrated estimate of complete-call correctness. Where autonomous routing depends on complete-call risk, qualify that event on matching complete invocations under CX-06; otherwise abstain/escalate under local policy. A label matching the teacher is not the same event as a tool achieving the user's objective.

## 7. Conformance and implementation limits

`tools/schema_reflex_reference.py` and `fixtures/schema_reflex/` implement a model-free miniature normalizer/decoder and synthetic training-policy checks. Their JSON digests are explicitly fixture-local, not claims to implement `axon.cjson/1` or production wire encoding. Tests cannot satisfy browser security, host isolation, learned quality, calibration representativeness, live interoperability or product acceptance. The profile remains Draft until mapped to actual owner/consumer call sites and its product gates execute.

See [schema build loop](../build/SCHEMA_DECISION_FRONTEND.md), [distillation profile](LIGHTWEIGHT_REFLEX_PROFILE.md), [review](../review/JIMOTHY_WEBMCP_DESIGN_REVIEW.md) and CX-03/CX-05/CX-26 for the owning boundaries.
