# `closed-loop-profile/1` — bridge-profile negotiation (B256)

**Status: Axon half implemented, MiCode half NOT implemented.** Gates
G16-r22-closed-wire and G16-r22-negotiation need BOTH real peers. This document
plus `docs/closed-loop-profile/vectors.json` is what the MiCode half implements
against. Until MiCode does, no peer can agree to anything, and every exchange
between the two is `Unsupported`.

Axon implementation: `crates/axon-loop-contracts/src/profile.rs`, schema
`crates/axon-loop-contracts/schemas/closed-loop-profile.schema.json`, tests
`crates/axon-loop-contracts/tests/profile.rs`.

## Why

Three wire families cross the Axon↔MiCode boundary, each at a fixed version,
and nothing negotiated any of them:

| Family | Version | Status under this profile |
|---|---|---|
| `cortex-policy-adapter` | `protocol_version: 1` | pinned adapter, id `cortex-policy-adapter/1` |
| MiCode `axon-bridge` | `v0` | pinned adapter, id `axon-bridge/v0` |
| closed-loop sidecars | `axon.closed-loop.{policy,transition,context,episode}/1` | negotiated schemas |

The package (B256) requires exact schemas and features to be negotiated, and
forbids inferring support from the build-pack version or from a peer's other
documents.

## The documents

One schema tag, `closed-loop-profile/1`, with two roles. Every field is
required. No other field is allowed. There are no nullable fields.

**Offer**

```json
{"schema":"closed-loop-profile/1","role":"offer","peer":"micode",
 "schemas":["axon.closed-loop.episode/1","axon.closed-loop.policy/1"],
 "features":["usage/2"],
 "adapters":["axon-bridge/v0","cortex-policy-adapter/1"],
 "required":["axon.closed-loop.episode/1"]}
```

**Accept**

```json
{"schema":"closed-loop-profile/1","role":"accept","peer":"axon",
 "offer_ref":"cl22:<64 hex>",
 "schemas":["axon.closed-loop.episode/1","axon.closed-loop.policy/1"],
 "features":[],
 "adapters":["cortex-policy-adapter/1"]}
```

## Byte-level rules

### Ingest (both roles)

These are the rules of every `axon.closed-loop.*` document (`axon-loop-contracts`
`parse`). They are applied BEFORE the document's role or content is looked at.

1. The input is UTF-8 and at most 1 048 576 bytes.
2. Object/array nesting is at most 32 levels.
3. The same key twice in one object is refused, compared as DECODED strings.
   So `"schemas"` and `"schemas"` are the same key. There is no last-wins.
4. No number has a fraction or exponent. `-0` is the integer 0. No integer has
   |n| > 2^53−1. (This profile has no numbers at all, but the rule applies
   before the shape rule.)

### Shape

5. The top level is an object.
6. `schema` is exactly `closed-loop-profile/1`.
7. `role` is exactly `offer` or `accept`. An offer has exactly the keys
   `schema role peer schemas features adapters required`. An accept has exactly
   `schema role peer offer_ref schemas features adapters`. Any other key is
   refused as an unknown field.
8. `peer` matches `^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$`. It is a label and
   authenticates nothing.
9. Every entry of `schemas`, `features`, `adapters` and `required` is an **id**
   matching `^[a-z][a-z0-9.-]{0,95}/v?(0|[1-9][0-9]{0,3})$`: lowercase ASCII,
   one `/`, a version with no leading zero.
10. Each list has at most 64 entries, and entries within a list are unique.
    `schemas` has at least 1 entry. The other lists may be empty but must be
    present.
11. `offer_ref` is `cl22:` followed by 64 lowercase hex digits. The schema
    pattern also admits `acf1:` and `sha256:`, but those are refused at the
    semantic step.

### Semantic

12. An id appears in at most one of `schemas`, `features` and `adapters`. The
    three vocabularies are separate: an id offered as a schema never matches
    the same id held locally as an adapter.
13. No entry of `schemas` starts with `closed-loop-profile/`. The profile
    document is not itself negotiable.
14. (Offer) Every `required` entry appears in one of the offer's three lists.

### Ids are compared as bytes

15. Two ids match only if they are byte-identical. There is no case folding,
    no version range, and no "`/2` implies `/1`".

## The rule

### Responder: `negotiate(offer, local) → Accept | Unsupported`

`local` is this side's own configuration: `peer`, `schemas`, `features`,
`adapters` and `required`, under the same rules as an offer. It is never
derived from a peer document.

1. For each vocabulary V in {schemas, features, adapters}:
   `selected[V] = { id ∈ offer[V] : id ∈ local[V] }`, in ascending byte order.
   An id that only ONE side lists is never selected.
2. If `selected.schemas` is empty, the result is `Unsupported(no_common_schema)`.
   It does not matter whether any feature or adapter is shared: a pinned
   adapter is not a fallback.
3. Each id in `offer.required` (in offer order), then each id in `local.required` (ascending),
   must be in some `selected[V]`. The first one that is not gives
   `Unsupported(required_not_agreed, side, id)`.
4. `offer_ref` = `"cl22:" + lowercase hex SHA-256` of the offer's canonical
   bytes. The canonical bytes are the parsed offer re-serialised with object
   keys sorted by code point, no whitespace, strings escaped as in the `cl22`
   rule (`crates/axon-loop-contracts/src/canonical.rs`), and ARRAY ORDER
   PRESERVED. This is Python's
   `json.dumps(v, sort_keys=True, separators=(',',':'), ensure_ascii=False)`.
5. The accept is `{schema, role:"accept", peer: local.peer, offer_ref,
   schemas, features, adapters}` built from `selected`. The result is a pure
   function of `(offer, local)`.

### Over the wire: `negotiate_wire(bytes?, local)`

Checks are applied in this order, and the first failure wins:

| Input | Result |
|---|---|
| nothing (the peer sent no profile) | `no_offer` |
| fails ingest rules 1–4 | `refused` (e.g. `duplicate_key`), even for an old peer's document |
| not a JSON object | `refused` / `shape` |
| `schema` absent, not a string, or not `closed-loop-profile/*` | `not_a_profile`. This is an OLD PEER: a `cortex-policy-adapter` v1 request or an `axon-bridge/v0` document. It is NOT read as support for that adapter. |
| `schema` = `closed-loop-profile/N`, N ≠ 1 | `unknown_profile_version` |
| `role` ≠ `offer` | `refused` / `shape` |
| fails rules 5–14 | `refused` with class `unknown_field` / `shape` / `semantic` |
| otherwise | `negotiate` |

### Offerer: `confirm(offer, accept, local) → Agreement | Unsupported`

1. The accept is parsed under the ingest, shape and semantic rules above.
2. `accept.offer_ref` must equal the `cl22` digest of the offer that was SENT,
   otherwise `offer_mismatch`. This stops an accept for one offer from being
   replayed against another.
3. Every accepted id must be in the SAME vocabulary of the sent offer AND of
   `local`. Otherwise the result is `not_offered`. This is where a one-sided
   `usage/2` is refused on the offerer side.
4. `accept.schemas` must not be empty (`no_common_schema`).
5. Every id in `offer.required` and `local.required` must be accepted
   (`required_not_agreed`).

A responder may accept a strict subset of the intersection. That is allowed,
provided rule 5 still holds.

## `usage/2`

`usage/2` is the feature for exact integer **micro-cents** (1e-8 of the
currency unit, no rounding). `axon.closed-loop.episode/1` carries Usage/1
instead: `cost_micro`, at 1e-6, converted with round-up
(`axon-loop` `cost_micro_from_micro_cents`).

* Rule 1 selects `usage/2` only when BOTH the offer and `local` list it.
* The only way code can obtain a `Usage2Permit` is from an `Agreement` that
  contains `usage/2`. Future Usage/2 producers and consumers must take that
  permit.
* `LocalCapabilities::axon_v022()` lists NO features. Usage/2 is not
  implemented anywhere in Axon today, so Axon does not claim it.

## Pinned adapters

`cortex-policy-adapter/1` and `axon-bridge/v0` keep their own wires unchanged.
The profile may list them, and they are agreed exactly like any other id. They
are NEVER INFERRED:

* An old peer's document is `not_a_profile`, not "speaks the adapter".
* A shared adapter does not rescue an empty schema intersection.
* Axon lists only `cortex-policy-adapter/1`. `axon-bridge/v0` is MiCode's wire,
  so Axon does not claim it.

What a caller does after `not_a_profile` or `no_offer` is the caller's choice.
It may run the incumbent pinned adapter under its existing, retained authority,
which is the G16-r22-negotiation "retained-authority incumbent operation". That
is an explicit local decision. The profile never makes that decision on the
caller's behalf.

## Stable result codes

The vectors' `expect.unsupported` values are: `no_offer`, `not_a_profile`,
`unknown_profile_version`, `refused`, `invalid_local`, `no_common_schema`,
`required_not_agreed` (with `side` = `offerer`|`local` and `id`),
`offer_mismatch`, `not_offered` and `feature_not_agreed`.

For `refused`, `expect.refusal` is one of `duplicate_key` (which includes
escaped-alias keys), `unknown_field`, `shape`, `semantic`, `float`,
`unsafe_integer`, `too_deep`, `too_large` and `malformed`.

Each vector contains exactly ONE fault, so its class is determined. For a
document with several faults, an implementation must refuse it, but the class
it reports is not specified.

## Vectors

`docs/closed-loop-profile/vectors.json` contains the following:

* `negotiate[]`: `{name, why, local, offer, expect}`. `offer` is RAW TEXT, or
  `null` for an absent peer. `expect` is either `{accept: <document>}` or
  `{unsupported, refusal?, side?, id?}`.
* `confirm[]`: `{name, why, local, offer, accept, expect}`. `expect` is either
  `{agreement: {schemas, features, adapters}}` or an unsupported result.

Axon runs every vector in `cross_language_vectors`. An accept is compared by
value AND by canonical bytes.

## What this does NOT establish

* **No transport interoperability.** G16-r22-closed-wire says so itself: "a
  syntactic fixture does not prove transport interoperability". Nothing sends
  these documents yet.
* **No wiring.** `cortex-policy-adapter` and the `axon-loop` intake do not call
  `negotiate`. Existing closed-loop documents still flow unnegotiated.
* **No authentication.** `peer` names a party. Nothing proves that party sent
  the document.
