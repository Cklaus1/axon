# Proposed closed-loop sidecar profile — v0.22

**Profile family:** `axon.closed-loop.* /1`. **Status:** proposed, not deployed/qualified. Negotiation is required; build-pack 0.22 is not a wire protocol handshake. Source types and original ACE/Reflex/ACF/neural digest rules are unchanged.

## Closed projections

| Schema | Meaning | Does not establish |
|---|---|---|
| `closed-loop-policy` | Versioned shortlist over an eligible candidate view and fixed control digest. | Permission, correctness or activation. |
| `closed-loop-context` | Expected/independently observed context and time/epoch bindings. | Authenticated observation merely from a JSON issuer field. |
| `closed-loop-episode` | Exact identities joining original episode, context and Fabric request/receipt plus independent verification and usage. | Independent verifier identity, unchanged artifact bytes or accurate billing without trusted resolution. |
| `closed-loop-transition` | Scoped expected-policy/epoch CAS intent for activate/rollback/pause. | Authority to change active state. |
| `closed-loop-pilot` | Prespecified plan; nullable owner fields intentionally block runtime readiness. | A powered study or a measured result. |
| `closed-loop-evidence` | Structural projection of externally supplied source/host/peer test receipts. | Authenticity or runtime qualification. |

All objects are closed. Unknown required semantics are refused or explicitly negotiated; no ignored fields. IDs are opaque scoped labels, not grants or content identities. References identify immutable content and must be resolved through the current tenant/data-use/issuer policy. ACF fields that use `acf1:` retain that scheme; a generic sidecar reference does not reinterpret their canonical bytes.

`cl22:` is a domain-separated **reference-profile example digest** over strict JSON UTF-8 bytes (sorted object keys, compact separators, original strings without silent Unicode normalization). The digest is outside the hashed object's own content, avoiding self-hash recursion. It is not an existing Axon production digest, signature or stable ABI claim. JSON strings with invalid Unicode scalar values, floats/non-finite numbers, duplicate keys, unsafe integers and excessive nesting/size are refused by the executable reference. Boolean is not an integer budget.

## Semantic checks after syntax

Schemas enforce shape; the reference functions additionally check candidate subset/control stability, pilot exact-context equality, unique independent observer, expiry/current epoch, role/write-set restrictions, episode-policy/context identity, exact output binding, finalized costs, and current-fence policy transition semantics. Production must independently authenticate the external observer/verifier/admitter and resolve their evidence; passing dictionaries into these reference functions is not authentication.

For post-preflight episodes, context and operation identifiers are mandatory. A failure before preflight is recorded by the existing task/refusal owner as TASK_NOT_STARTED; it must not fabricate a started execution/episode. General implementation-worker ancestry requirements are not globally replaced: exact equality is the bounded paired-trial profile only.

Fabric request/receipt schemas remain in the byte-preserved reference pack. The sidecar carries additional arm/context/policy references rather than adding unknown fields to `acf-compute-request/1`. A concrete production mapping from policy sidecar digest to the supervisor's ACF `policy_digest` needs an authenticated, versioned projection, not a string replacement between schemes.

## Unknowns and evidence roles

A supervisor's process completion can be unverified. A verifier needs nonzero matched required checks, a current registered profile and exact candidate output. Neither high confidence nor JSON `verification=passed` authenticates that result. Unknown cost is null with reserved liability, never zero; estimated usage is not final comparative evidence.

Shortlist scoring is not authorization. A policy is evaluated before filtering only when the input candidate view has already been authorized; every actual effect rechecks authority afterward. Unprovable pattern subset relations refuse. CLM relative probabilities remain separate from calibrated correctness or admission confidence.

## No test-to-production shortcut

Fixture bundles label all data synthetic. The in-memory lifecycle/budget/CAS model has no database, process, sandbox, model, network, signature verifier or production policy pointer. It tests intended invariants only. Runtime qualification stays false even if every fixture passes. The evidence checker reports missing/invalid records or requires external verification; it never prints a qualified PASS for a real deployment.
