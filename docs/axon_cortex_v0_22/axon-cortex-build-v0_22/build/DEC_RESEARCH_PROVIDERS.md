# DEC — research providers through Axon Reflex

**Owners:** CX-05, with CX-06 calibration, CX-24 perception, CX-25 learning, CX-26 composition and CX-34 registry. **Tasks:** B225–B231; optional B252/B253. **References:** RES-CLM, RES-JEV, RES-PIJEV.

## Preserve the existing serving seam

The reviewed source already implements `ReflexBackend`, principal-bound state handles and `axon-reflex/1` error handling. Its `ReflexCore::encode_state` ignores the input and its decision returns `decided(question)`. This is a useful deterministic transport/isolation fixture, not a learned decision implementation. Preserve it as a compatibility/test backend; build adapters behind the same ownership boundary.

New research capabilities require explicit profile negotiation and typed schema validation. Do not silently change the old `Decision { choice, principal }` meaning, send new fields through strict legacy requests, or treat an unsupported profile as a negative answer. No new top-level ACE wire tag is required by this pack. A reviewed profile extension needs old-client/new-server and new-client/old-server tests.

## Candidate identity and permission

Derive the action catalog from the existing CX-34 registry. Immutable semantic IDs are independent of display order. A candidate includes versioned description, source artifact, runtime compatibility, task scope and current eligibility. Filter authority/privacy/host/budget restrictions before exposing candidate text to an external provider. Empty eligible sets block. A `choice` is a proposal, never an effect grant.

Hash both the candidate set and the ordered presentation. Include the rendering template/instructions, question family and effective input projection. Duplicate semantic IDs refuse; duplicate text across different IDs is either intentionally modeled with documented semantics or rejected by policy. One candidate yields conditional mass one without establishing correctness. Candidate addition or duplication changes the softmax denominator and must not silently preserve an old correctness threshold.

## CLM adapter

CLM independently encodes state and candidate text, projects them and ranks compatibility. It is not a generator and does not inspect all candidates jointly as a generative judge would. The released head depends on the compatible Qwen3 encoder and pooling recipe; pin the actual encoder/model, tokenizer, last-token pooling configuration, normalization, projection head, numeric precision and runtime revision before real evaluation.

The inspected upstream embedder defaults to `max_tokens=2048` and passes `truncate_prompt_tokens`. Axon must not inherit silent truncation. Tokenize/check using the actual provider tokenizer, reject excessive input or apply a separately qualified projection, and record its digest and token count. A head trained on a different context/embedding recipe needs new qualification. Long-trajectory verifier results from a fine-tuned head cannot qualify the generic default head automatically.

Cache keys must bind principal/tenant and authorized retention scope, exact rendered text/effective input, registry/candidate version, question/projection identity and all encoder/head/runtime fingerprints. Share only explicitly approved public immutable candidates; private state embeddings remain scope-isolated. Plain text equality is insufficient for cross-principal reuse.

A request pins one immutable head epoch. Atomic replacement publishes a new epoch and prevents mixed state/action projections. Old in-flight requests keep their epoch only within authorization and retention limits; revocation stops new dispatch and disqualifies subsequent admission. Bound cache memory and account for misses, batch sizes, cold-start and GPU contention. Tests must cover concurrent reload, eviction, repeated identical text in different principals and cancellation cleanup.

For fixed independent candidate scores, reordering should merely permute the vector. Validate this property within explicit floating tolerance. Choose a stable semantic-ID tie rule, not first-in-input argmax. This does not prove calibration, candidate-set robustness or instruction/state-field order invariance. ANN retrieval is optional B252 and must measure shortlist recall against exact scoring.

## Jev/RLCD adapter

Preserve v0.19/v0.20 typed decision and calibration contracts. Capabilities advertise tested binary/choice/ordinal scope, model/revision and any probability support. Wire compatibility is not evidence that a backend was trained with RLCD or that returned numbers are correctness probabilities. Partial responses and malformed distributions refuse; transport failure is neither abstention nor a false answer.

For an ordinal rubric, scores are indexed by frozen semantic levels. Renumbering those levels is a different question, not an innocent categorical permutation. Noul/binary semantics also require exact positive/negative definitions and orientation. Existing source errors remain explicit across adapters.

## pijev robustness profile

Freeze a bounded, reproducible permutation schedule. For each scheduled call, require the identical semantic candidate set and a complete valid distribution. Align by ID before averaging. Record the exact schedule, seed, question grouping, model and wrapper revision; count every subcall and retry in the task ledger.

A finite subset of permutations is not exact symmetrization. Full symmetrization is often factorial and unaffordable. A convex proper-loss benefit relative to the average included predictions is not dominance over the best ordering and is not a calibration theorem. Question ordering/grouping effects are a separate variable. A failed subcall does not silently reduce the schedule or renormalize its missing labels; explicitly refuse or use a prequalified alternative wrapper with a different identity.

Do not automatically apply permutation averaging to CLM's independent scorer. Audit its invariance and tie behavior first. Any changed wrapper invalidates old calibration binding until requalified. Ordinal levels retain their meaning rather than being permuted as unordered choices.

## Results and training

Separate `raw_scores`, `conditional_probabilities`, `calibrated_correctness`, disposition and provider failure. Record calibration identity binding the entire pipeline: model, head, wrapper, candidate construction, projection, task domain and threshold policy. All-bad options, singleton sets, near-duplicates and out-of-domain inputs are required stress cases. A score cannot override a hard deterministic failure.

Train heads only on authorized, provenance-complete evidence. Unchosen is not incorrect; observed counterfactuals or suitably qualified labels are needed for hard negatives. Split by task/repository before fitting; keep calibration and final tests separate. Publish through existing CX-25/CX-11 artifact lifecycle, not in-place self-updates while decisions are running. Source/code pinning does not pin downloaded weights: model revision and digest remain required before live qualification.
