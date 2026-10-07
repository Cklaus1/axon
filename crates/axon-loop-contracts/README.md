# axon-loop-contracts

**Status: partial (v0.22).** Pure contract types. Callers are `axon-loop`,
`axon-fabric` and `axon-reflex` (`shortlist.rs`). It is not on the path of
any `axon` CLI verb.

## What it is

* Closed serde types for the v0.22 package's `axon.closed-loop.*/1` family
  (policy, transition, context, episode). It also carries the ACF
  `acf-compute-request/1` and `acf-execution-receipt/1` contracts, with their
  bytes preserved.
* `parse`: strict ingest.
  * It refuses duplicate keys and escaped-alias keys, via
    `axon_cortex::parse_strict`.
  * It rejects floats, integers with |n| > 2^53−1, nesting deeper than 32,
    and input over 1 MiB.
  * `deny_unknown_fields` applies.
  * A required-nullable field must be present.
  * Each document is validated against its checked-in JSON Schema (`schemas/`)
    before serde runs.
* `digest`: the package's `cl22:` sorted-key canonical digest. It is
  deliberately **not** `axc1` or `acf1`, and not the frozen episode digest.
* `profile`: `closed-loop-profile/1`, the B256 offer/accept negotiation of
  exact schema ids, feature flags (`usage/2`) and pinned adapters
  (`cortex-policy-adapter/1`, `axon-bridge/v0`). `negotiate` is pure. An
  absent or old peer, no common schema, or an unmet requirement is an explicit
  `Unsupported` and never a fallback. **Not wired**: no transport calls it yet,
  and the MiCode half does not exist. Rules:
  `docs/CLOSED_LOOP_PROFILE_NEGOTIATION.md`.
* `checks`: semantic checks that need no I/O (`bind_acf`, `bind_episode`,
  `check_paired_trial_context`, `check_shortlist`, …).

Dependency: `axon-cortex`, for `parse_strict` and `ContractError` only.
`axon-cortex` must never depend on this crate. As a result, every consumer of
these contracts links `axon-cortex` (D-C4, accepted for v0.22).

## What it does NOT do

* **No authentication.** A JSON `issuer_ref` names a party. It does not prove
  that party wrote the document.
* **No I/O, no storage, no budget arithmetic, no admission.** Those belong to
  `axon-loop` and `axon-fabric`.
* **No authority.** `principal_ref` and `grant_ref` in `ComputeRequest` are
  `OpaqueRef` strings. Nothing here resolves them to an `axon_os::Grant`. That
  gap is the fourth authority vocabulary (D-C2,
  `governance/cortex-v015/DISCREPANCIES.md` D-016).
* **Passing a check is never runtime qualification.**
* **`architecture` and `checkpoint_kind` are parsed but not honoured.** The
  Fabric ignores them (G6, see `crates/axon-fabric/README.md`).

## Evidence location

The red-team reports against this crate live in the operator's **untracked**
`.axon-v022/redteam/` directory (`contracts.md`, D1–D4, fixed by `6f06444`).
They are **not in the repository**.
