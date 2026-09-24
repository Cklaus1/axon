# axon-loop

**Status: partial (v0.22).** A file-backed closed-loop store plus the
`axon-loop` binary (JSON in, JSON out).

Callers:

* `axon-fabric` is the only in-workspace library caller, and it only reads
  authority epochs.
* The binary is driven by MiCode over files and by this crate's tests.
* The paired interop harness `scripts/loop_interop_gate.sh` was invoked by
  nothing at `279da778`; since 54f41c3 `gate.sh --strict` runs it (CI does
  not). It does not pin the MiCode revision it tests against (red-team D-04).

No `axon` CLI verb reaches this crate.

## What it is

* `pointer` / `epoch`: a per-scope fenced active-policy pointer and its
  authority epoch. The pointer moves only by compare-and-swap on
  `(expected_policy_ref, expected_epoch)`, and each move is journalled before
  it is published.
* `plan`: the experiment register. A `closed-loop-pilot/1` plan is frozen by
  its `cl22:` digest.
* `candidates` / `tasks`: registered candidate lists and task manifests. Each
  is named by its list digest and stored under
  `<kind>/<tenant>/<family>/<hex>.json`. Keying the path by scope means the
  same list registered for two scopes is two files, never an overwrite
  (NS3, fixed in stage 2).
* `evo`: one bounded shortlist candidate, with its hypothesis history.
* `evl`: paired-trial evaluation of exact artifacts. Unknown is never a pass.
* `admission`: applies the frozen plan rule and returns ACCEPT, REJECT or
  INCONCLUSIVE. This is **CX-11 policy admission**.
* `tel`: whole-task economics over `Usage`. Unknown stays unknown.
* `intake`: joins a MiCode `axon.closed-loop.episode/1` sidecar to a stored
  policy and its context receipt, and records the result in the ledger.
* `ledger`: the store's hash-chained `ledger.jsonl` + `ledger.head` +
  `ledger.anchor`.

## What it does NOT do

* **No authentication.** `trusted_admitters`, `trusted_observers` and the
  verifier set are operator premises, read from the store's `config.json`.
  An `issuer_ref` names a party and proves nothing.
* **Policy admission is not effect admission.** An ACCEPT, or the active-policy
  pointer it moves, is **never a grant** and confers no effect authority. That
  is what `axon-os::gate::admit` decides (D-C6,
  `governance/cortex-v015/DISCREPANCIES.md` D-018).
* **The ledger is keyed only when the operator provides a key (D-015).**
  With `AXON_ATTEST_KEY` set (hex, at least 16 bytes: the same key and rule
  `axon-vm` attests under), each ledger entry and `ledger.head` carry an
  HMAC made with `axon_attest::hmac_sha256`, the primitive `axon-audit`'s
  keyed chain uses. Its shape is the same too: a per-entry MAC plus an
  authenticated `(count, last)` tip. Under a key, these are refused with
  exit 2:
  * a well-chained forged append with a rewritten head (F1);
  * a forged evaluation line (F2);
  * a truncation with a rewritten head (R2).

  A wrong key, a keyed store opened without its key, and an unkeyed store
  opened with a key are also refused. A malformed key value is refused
  outright rather than silently running unkeyed.

  Still out of model:
  * **No key (the default).** There is deliberately no ephemeral key, so
    F1, F2 and R2 are undetectable, as before.
  * **Anyone holding the key.**
  * **Restoring a genuine older keyed state (R1).** This needs a monotonic
    external witness and is **OPEN**.
  * **Deleting the anchor as well (R3/NS6c).**
* **Plan approval is not `axon-os` approval.** `operator_approved` is a
  self-asserted bool, and `approval_ref` is only checked for being non-null
  (D-016).
* **No cost metering of its own.** It consumes `Usage` as reported.

## Open defects

From red-team round 4, run independently against `dead41b` (operator-side
evidence, `.axon-v022/redteam/axon-loop-r4-independent.md`, **not in the
repository**). Stage 2 lane 2A fixes NS3 (scope-keyed records); the rows still listed below are open.

| id | severity | defect |
|---|---|---|
| NS4p / NS4w | MEDIUM | `evl` checks the preflight observer against `trusted_observers` and against the expecting parent, but never against the subject set (the request's `subject_issuers` plus the candidate's proposer). If an operator lists the proposer or the worker as an observer, that party can establish a verified pass over its own trials |
| NS4b | LOW | An explicit `"trusted_observers": []` in `config.json` is refused as malformed (exit 3) by every writing verb, including a fenced pause. The cause: `skip_serializing_if = "Vec::is_empty"` combined with the strict canonical round-trip. It fails closed, but "I said none" becomes unparseable, which contradicts the project's rule that absent ≠ empty |

Ownership conflicts D-C1 and D-C2 are also open. See
`governance/cortex-v015/IMPLEMENTATION_MAP.md` §4a.

## Axon ↔ MiCode

MiCode writes episode sidecars, context receipts and policy acks under
`<repo>/.micode/axon/closed-loop/`, and `axon-loop intake episode` reads them.
Neither workspace has a Cargo dependency on the other. The MiCode side of this
exchange is on branch `v022/micode`, which is based on
`checkpoint/v014-reconciled`. **It is not releasable** until MiCode `tui` is
merged into it, because it lacks `tui` commit `ed082601` (credential-to-host
isolation). This was operator decision D3, 2026-09-24. Interop results against
that branch show the two sides agree on bytes. They do not make the MiCode
side releasable.
