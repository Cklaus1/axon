# Research decision record — provisional v0.21 reference profile

The closed [JSON schema](json/research-decision.schema.json) and [fixture](../fixtures/research_v021/decision.json) demonstrate additional decision semantics. They do not change existing Reflex or ACE wire formats. A real adapter must negotiate a separately reviewed capability/profile and enforce existing runtime admission and principal authentication.

Serialized fixture numbers are integers: probabilities/correctness use parts-per-million and raw scores use a declared micro-unit representation. Canonical digests use the existing `axon.cjson/1` implementation, not a newly invented canonical JSON encoding. Provider-native floating-point values must be retained as exact source evidence and converted under a qualified numeric policy; the integer reference is not proof a conversion preserves calibration.

`candidate_set_sha256` binds the sorted semantic candidate records; `presentation_sha256` binds input order. Model/encoder/tokenizer/pooling/head/wrapper/projection/precision/epoch fields bind the actual scorer. A conservative fixed-candidate-domain calibration binding is illustrated; dynamic candidate policies require their own qualified contract rather than claiming the fixed fixture generalizes.

`DECIDED` requires total normalized candidate distributions and a stable semantic-ID tie rule. `ABSTAIN`, `REFUSED`, `TRANSPORT_ERROR`, `CANCELLED` and `UNSUPPORTED` carry no fabricated choice/confidence. Unknown usage carries null observed cost and a positive reserved ceiling. Calibration, when present, is an identified external qualification record—not a number inferred from softmax.

The reference sets `fixture_only=true` and `product_result=NOT_RUN`. These fields deliberately prevent it masquerading as live evidence. Runtime schemas and evidence authenticity are separate implementation work.
