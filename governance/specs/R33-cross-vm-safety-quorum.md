# Tech Spec — R33: Cross-VM Safety Quorum (attested CoordGoal gate for irreversible actions)

**Spec ID:** `R33-cross-vm-safety-quorum`
**Status:** 🚧 Implementing (re-verified 2026-07-18) — scoped slice landed (file-based
propose/vote/check CLI + pure `check_quorum` aggregator); **R27 coalition ceiling landed
2026-07-18** — `check_quorum` now caps admitted YES votes per `lineage_root` at the spec's own
blessed fallback default (`ceil(N/2)-1`, §4.5 point 4's worked example), closing the sock-puppet
attack (N instances from one principal cannot alone force quorum) without depending on R27's real
`Coalition`/`max_quorum_power` type being wired in — exactly the spec's own explicitly-sanctioned
fallback path (§6 slice-risk note, §-Gate anti-stub carve-out). **`axon deploy` integration landed
2026-07-18** (§5.2.1) — `axon deploy --quorum-dir DIR` shells out to `axon-vm quorum check`
(the same cross-binary pattern `axon-web` already uses to wrap the `axon` CLI), inserting the
quorum result as one more named pipeline gate; omitting the flag leaves the gate open, so every
pre-existing `axon deploy` invocation is unaffected. **S2 (vsock broadcast transport) re-scoped
2026-07-19** (§5.2.2): scoping it for real found the "reuse R26's `Substrate` trait" premise was
false (R26 never built that trait — see R26's own 2026-07-19 as-built note); §1.3's false claim
struck out and corrected; a concrete, right-sized design (new `quorum/vsock.rs` module, not a
trait; reuses `interp.rs`'s existing raw-`AF_VSOCK` wire framing; TCP-loopback CI-testable via an
env-var swap, matching R26's own `AXON_CI_NO_KVM` precedent) now sizes it into buildable sub-slices.
**S2a (wire protocol) landed 2026-07-19**: `quorum/vsock.rs`'s length-prefixed JSON framing, 9 unit
tests. Also closed an incidental gate-coverage gap found while landing S2a: `axon-vm`/
`axon-attest` were never clippy-gated by `scripts/gate.sh` (same blind spot BUG_HUNT #35 found in
the runtime crates) — 3 small pre-existing findings fixed, no behavior change, both crates now
enforced. **S2b (real-socket round trip) landed same day**: `connect_and_round_trip` — one
TCP-loopback connection (the §5.2.2 CI stand-in for real `AF_VSOCK`), deadline-bounded, fail-closed
on timeout/refused connection, proven against a real socket (not just an in-memory buffer), 3 new
tests (12 total), 5 consecutive reruns checked for flakiness. **S2c (broadcast/collect fan-out)
landed same day**: `broadcast_and_collect` — one thread per peer so wall-clock stays bounded by the
deadline regardless of peer count, feeding `check_quorum`'s existing shape directly; an
unreachable/timed-out peer contributes nothing and never blocks the others; 3 new tests (15 total),
including a timing assertion proving parallel (not sequential) fan-out. **S2d (listen primitive +
CLI flag) landed same day**: `respond_once` — the real production listen/accept/respond primitive
(single-shot, no daemon loop), now backing every S2a-c test's own voter helper too (proven, not
just added); `axon-vm quorum vote --listen PORT` wires it in, same approve/deny/reason/lineage-root
decision as the file-based path, `--request`/`--out` become optional-unless-`--listen`
(backward compatible), verified end-to-end with a from-scratch Python client speaking the real wire
protocol (not just the Rust test helpers). **S2e (`propose --broadcast` CLI flag) landed same
day**: `axon-vm quorum propose --broadcast CID1,CID2,... --n N --deadline-ms MS [--json]` gives
`broadcast_and_collect` its first real caller — broadcasts, collects, runs the same `check_quorum`
`check` already uses (factored into a shared `report_quorum_result` helper, not duplicated), exits
0/13/14. Verified with a real multi-process journey: `propose --broadcast` against 2 real
`vote --listen` peers + 1 genuinely unreachable one correctly reports 2/3 approvals; against 2
unreachable peers, QUORUM BLOCKED with no hang; omitting `--broadcast` is fully unaffected. The
whole `vsock.rs` module is now clippy-clean with ZERO `#[allow(dead_code)]` — S2's proposer+voter
round trip is complete end-to-end. **S2f re-scoped 2026-07-20** (see §5.2.3): "swap TCP for real
`AF_VSOCK`, same wire format" was the plan carried since S2a, but sizing it for real found the
premise doesn't hold — Firecracker's vsock device (`crates/axon-vm/src/main.rs`'s existing
`/vsock` config + `vsock_relay`) connects a guest ONLY to its own host (CID 2), never to another
guest directly, and `axon-vm run` launches exactly one VM per process (no shared host process
across peer VMs to relay through). §5.2.3 lays out the two real options — a host-mediated relay
(a new broker) vs. hardening the existing TCP path into the real transport (auth + TLS, not a
wire-level swap) — as an open question, not a decided plan; see §12 Q1. See §14 Evidence ledger.
**Implements:** the distributed gate for Risk ≥ High irreversible actions identified after R26/R27:
a single-VM safety gate, even with R26 attestation and the R27 kill-switch, has one residual failure
mode — the VM itself could be compromised *after* its attestation report was produced. R33 distributes
the gate across N independently attested VMs so that compromising fewer than ⌈N/2⌉ VMs is insufficient
to force approval. The aggregator is the existing `CoordGoal` type (Phase 14 / `examples/stdlib/coord.ax`);
each voter's identity is its R31 extended-TCB attestation (`axtcb1-ext:`).
**Depends on:**
- `governance/specs/R26-confidential-microvm-substrate.md` — attestation, `axtcb1:` chain, `AttestationReport`
- `governance/specs/R27-corrigibility-resource-bounds.md` — coalition ceiling, `max_quorum_power`,
  lineage-root membership; same-lineage VMs cannot constitute quorum alone (§4.5)
- `governance/specs/R31-extended-tcb-attestation.md` — `axtcb1-ext:` voter identity, schema `axon-vm-report/2`
- `examples/stdlib/coord.ax` — `CoordGoal`, `coord_goal_new`, `coord_goal_propose`, `coord_goal_quorum_met`,
  `coord_goal_consensus_score` (Phase 14, shipped)
- Phase 11 risk typing (`examples/stdlib/risk.ax`) — `Risk`, `risk_requires_pipeline`; Risk ≥ High
  triggers the quorum gate at the `axon deploy` level
**Audience:** an implementer who builds *strictly* against this document and reads only it.

```spec-meta
id: R33-cross-vm-safety-quorum
status-claim: Implementing
depends-on: R31-extended-tcb-attestation
blocks: none
blocked-by: none
supersedes: none
related: R26-confidential-microvm-substrate, R27-corrigibility-resource-bounds, R34-incremental-attestation
conflicts-with: none
reserves: exit codes 13 (QUORUM_BLOCKED), 14 (QUORUM_ATTEST_FAIL) — grepped
  crates/axon-vm/src/main.rs before claiming: existing axon-vm exit codes in use are 1, 2, 10, and
  12 (EXTENDED_TCB_MEASURE_FAIL); R34-incremental-attestation separately reserves 15
  (CHAIN_VERIFY_FAIL_EXIT_CODE). 13/14 were free — no collision found, claimed as-is.
evidence: scripts/r33_acceptance_gate.sh
```

(This session builds a scoped-down slice of the full protocol below: a file-based
`propose`/`vote`/`check` CLI exchange with a pure `check_quorum` aggregator in
`crates/axon-vm/src/quorum/`, self-measurement via R31's `measure_host_stack`, strict-majority +
attestation-consistency checks, **and (2026-07-18) the §4.5 coalition ceiling** — `VoteResponse`
gains a `lineage_root` field (a SEPARATE concept from `voter_tcb`: which principal/coalition a
voter belongs to, not what software it runs — see the field's own doc comment for why defaulting
one to the other would be backwards), and `check_quorum` caps admitted YES votes per root at the
spec's own hardcoded safe-default cap, with NO dependency on R27's real `Coalition` type, **and
(2026-07-18) the §5.2 `axon deploy` integration** — `axon deploy --quorum-dir DIR` shells out to
`axon-vm quorum check` (§5.2.1: cross-binary orchestration, no new `axon-core` dependency) and
folds the result into the same named-gate mechanism `simulate`/`stress`/`redteam_check`/
`assert_deployable` already use. It does **not** yet implement the R26 vsock broadcast transport
described in §2 below — that remains open follow-on work tracked by this spec's unchanged body.
See §14 Evidence ledger for exactly what was verified and how.)

> **Read this framing first.** R33 does **not** change one line of Axon-level containment, the
> kill-switch (R27), or the attestation protocol (R26/R31). What R33 changes is the **number of
> independent attestation verdicts** required before a Risk ≥ High action may proceed: from one
> (the proposing VM's own supervisor) to a strict majority of independently attested, independently
> evaluating VMs. A compromised VM is diluted by the honest majority rather than being the sole
> arbiter. The load-bearing invariant is: **no subset of VMs smaller than ⌈N/2⌉ can form a quorum**
> — not through vote forgery (each vote carries a verified `axtcb1-ext:`), not through sockpuppeting
> (R27 coalition ceiling bounds same-lineage VMs), and not through timeout races (fail-closed: no
> quorum within the deadline = action blocked).

---

## §0 — Requirement → Section → Acceptance-check index

| Req | What | Spec § | Pinned acceptance check (test name) |
|---|---|---|---|
| **A1** | Real user journey: 3 VMs attest + vote, majority approves, action proceeds | §5, §7 | `acc_a1_smoke_quorum_journey` |
| **A2** | Single VM cannot force approval: 1/3 YES → blocked | §4.3, §7 | `acc_a2_single_vm_cannot_force_approval` |
| **A3** | Quickstart commands execute verbatim against the built binary | §9, §7 | `acc_a3_quickstart_commands_execute` |
| **A4** | Timeout fails closed: quorum not formed within deadline → action blocked | §4.4, §7 | `acc_a4_hermetic_isolated_timeout` |
| **A5** | Deterministic vote aggregation: same votes in any order → same quorum result | §4.3, §7 | `acc_a5_deterministic_vote_aggregation` |
| **A6** | Attestation verified per vote: unattested VM's vote is rejected before counting | §4.2, §7 | `acc_a6_attestation_verified_per_vote` |
| **Core** | Vote without valid R31 attestation is ignored, not counted | §4.2 | `unattested_vote_is_rejected` |
| **Core** | Quorum not formed within deadline → action blocked (fail-closed) | §4.4 | `timeout_blocks_action` |
| **Core** | N VMs from same lineage root cannot constitute quorum alone (R27 coalition bound) | §4.5 | `coalition_bound_limits_same_lineage` |
| **Core** | 1 compromised YES + 2 honest NOs → action blocked (honest majority wins) | §4.3 | `compromised_vm_vote_diluted` |
| **Gate** | The acceptance gate itself fails if any check above is missing or stubbed | §-Gate | `scripts/r33_acceptance_gate.sh` |

The build is **not done** until every row's check exists, was seen to fail first, and now passes.

---

## §1 — Overview & scope

### 1.1 What it does

Phase 11 risk typing (`examples/stdlib/risk.ax`) classifies actions as Low / Medium / High / Critical.
For Risk ≥ High, the single-VM pipeline gate (`axon deploy --risk high`) runs simulate → stress →
redteam → verify → deploy inside *one* VM. R33 adds a layer *before* that final deploy step:

1. **The proposing VM broadcasts a `VoteRequest`** to N peer VMs over an attested channel (vsock,
   R26 §3.3), describing the action by content hash and the proposer's own R31 `axtcb1-ext:` digest.
2. **Each peer VM independently evaluates** the action against its own policy, produces a score
   (0–100), and returns a signed `VoteResponse` containing its vote and its own `axtcb1-ext:` identity.
3. **The `QuorumAggregator`** — running in the proposing VM's supervisor — verifies each vote's
   attestation (R31), applies the R27 coalition ceiling per lineage root, counts YES votes (score ≥
   the configured target), and checks `coord_goal_quorum_met` (strict majority ⌈N/2⌉ + 1). The
   aggregation uses the shipped `CoordGoal` type directly: each verified vote is recorded via
   `coord_goal_propose(goal, voter_pid, vote_score)`.
4. If quorum is met within the deadline: **action proceeds**. Otherwise: **action is blocked**
   (fail-closed), no matter how many YES votes arrived.

The concrete scenario motivating R33: an AI agent in VM-alpha wants to execute
`wire_transfer(amount=1_000_000)` (Risk = Critical). Without R33, a post-attestation compromise of
VM-alpha's supervisor could force the action. With R33 (N=3), VM-alpha must obtain independent YES
votes from VM-beta and VM-gamma. An attacker who controls only VM-alpha cannot unilaterally approve —
they must also compromise a majority of the peer fleet.

### 1.2 What it explicitly does NOT do

- **No new isolation.** R33 adds a distributed voting layer. The underlying R26 micro-VM isolation,
  R27 kill-switch, and R31 measurement are unchanged and remain load-bearing beneath it.
- **No majority compromise tolerance.** If ⌈N/2⌉ or more VMs are simultaneously compromised, the
  quorum guarantee breaks. For N=3 this means 2 compromised VMs can approve anything. Byzantine
  fault tolerance beyond this bound (PBFT-class, N ≥ 5 with f < N/3) is explicitly out of scope
  for R33; operators needing stronger properties MUST increase N or adopt PBFT protocols (R34).
- **No defense against adversarial network delay** as a liveness attack. An attacker controlling
  the network between VMs can delay all peer votes past the deadline, blocking every action via
  DoS. This is safe (fail-closed) but exploitable for availability attacks. Mitigation is deferred
  to R34 (redundant channels, adaptive deadlines).
- **No new Axon-level capability proofs.** The vote's content (the action's policy evaluation) is
  the peer VM's responsibility; R33 provides the *aggregation protocol*, not a new proof system.
- **No replacement for Risk typing (Phase 11).** R33 is triggered *by* the Phase 11 risk classifier;
  it does not change how Risk is derived from effect rows and budget burn.
- **No cross-root coalition bounding.** R27's coalition ceiling is per lineage root. A persuaded
  operator who spins up VMs under a second independent lineage root can participate in the quorum as
  an independent bloc. Cross-root aggregate bounding is R27's named residual (§4.5 of R27); R33
  inherits that residual unchanged.

### 1.3 Interface & tech constraints

- **Interface:** new subcommands on `axon-vm` (`quorum propose / vote / audit`).
- **Language/crate:** Rust, extends `crates/axon-vm` (R26). Allowed new deps: none (`sha2`,
  `serde_json`, and `serde` are already in scope). ~~The vsock channel the VMs communicate over is
  R26's existing `Substrate` trait boundary.~~ **truth: corrected 2026-07-19** — this trait doesn't
  exist. R26's own §2 never got built as designed (see R26 spec's own 2026-07-19 as-built note);
  there is no `substrate.rs`, no `trait Substrate`, no `MockSubstrate` anywhere in the tree. S2's
  design (§5.2.2 below) does NOT depend on it.
- **Perf/security:** the `QuorumAggregator` is pure (no I/O, no clock); all vsock sends/receives
  will live behind a dedicated seam in the quorum module itself (§5.2.2) — narrower than R26's
  full launch/attest/job-run surface, since quorum only needs broadcast-request / collect-response.
  Fail closed on every ambiguity: unknown vote attestation ⇒ vote rejected; channel error ⇒ vote
  absent; deadline elapsed ⇒ action blocked.

---

## §2 — Architecture

```
Action (Risk ≥ High, proposing VM)
    │
    ▼
┌─ QuorumGate ──────────────────────────────────────────────────────────────┐
│                                                                            │
│  1. Build VoteRequest { action_id, action_hash, risk,                     │
│                          proposer_tcb (axtcb1-ext:…), deadline_ms }       │
│                                                                            │
│  2. Broadcast VoteRequest to N peer VMs over attested vsock channels      │
│     (each peer channel was established via R26 mandatory attestation)     │
│                                                                            │
│  3. Each peer VM independently:                                            │
│       a. Verify the proposer's axtcb1-ext: against its pinned expectation │
│       b. Evaluate the action against its own policy → score (0–100)       │
│       c. Build VoteResponse { action_id, vote (score >= policy_threshold),│
│                                score, voter_tcb (axtcb1-ext:…),          │
│                                lineage_root, signature }                  │
│       d. Return VoteResponse over the vsock channel                       │
│                                                                            │
│  4. QuorumAggregator (pure, in the proposing VM's supervisor):            │
│       a. Verify each VoteResponse's axtcb1-ext: (R31 extended TCB)       │
│       b. Apply R27 coalition ceiling per lineage_root                     │
│       c. For each passing vote:                                            │
│            coord_goal_propose(goal, voter_pid, vote_score)                │
│       d. Check coord_goal_quorum_met(goal)                                │
│            YES count ≥ ⌈N/2⌉ + 1 → APPROVED                             │
│            else (or deadline elapsed) → BLOCKED (fail-closed)            │
└────────────────────────────────────────────────────────────────────────────┘
    │
    ▼
APPROVED → continue to axon deploy pipeline
BLOCKED  → QuorumBlocked exit 13, audit record written
```

**Module layout (new files in `crates/axon-vm/src/`):**

```
quorum/
  request.rs      VoteRequest + VoteResponse types (pure; no I/O).          [PURE]
  aggregator.rs   QuorumAggregator: verifies votes, applies coalition bound, [PURE]
                  calls coord_goal_propose/coord_goal_quorum_met. Pure over
                  injected vote slices; deadline check injected by caller.
  policy.rs       DefaultVoterPolicy: score_action(risk) → i64.              [PURE]
  gate.rs         QuorumGate: orchestrates broadcast → collect → aggregate   [PURE core,
                  via Substrate; maps outcomes to QuorumResult + exit codes.  I/O injected]
  cli.rs          Arg parse for `axon-vm quorum` subcommands.                [I/O seam]
quorum_tests/
  r33_acceptance.rs   The A1–A6 + Core acceptance checks (named per §0).
```

The `QuorumAggregator` is the trusted core: it receives `Vec<VoteResponse>` (already collected by
the gate), verifies each, applies the coalition ceiling, and drives `CoordGoal`. It is pure given its
inputs — fully testable with mock votes, no vsock needed for unit tests.

---

## §3 — Data model

### 3.1 `VoteRequest` (broadcast to peer VMs)
```rust
struct VoteRequest {
    action_id:    String,   // UUIDv4 — uniquely identifies this quorum instance
    action_hash:  String,   // sha256(action_description_json) — binds votes to the content
    risk:         i64,      // 0=Low … 3=Critical (from Phase 11 Risk enum)
    proposer_tcb: String,   // "axtcb1-ext:…" — R31 extended-TCB of the proposing VM
    deadline_ms:  i64,      // absolute deadline (proposer's monotonic clock); anti-liveness-attack
}
```

### 3.2 `VoteResponse` (returned by each peer VM)
```rust
struct VoteResponse {
    action_id:    String,   // MUST match VoteRequest.action_id — binds to the same request
    vote:         bool,     // true = YES (score >= policy threshold); false = NO
    score:        i64,      // voter's own policy score (0–100); 0 for NO votes
    voter_tcb:    String,   // "axtcb1-ext:…" — R31 extended-TCB of the voting VM
    lineage_root: String,   // for R27 coalition ceiling check (§4.5)
    signature:    String,   // HMAC-SHA256(VoteResponse fields sans signature, voter_key)
                            // voter_key is the per-VM secret bound at R26 attestation time
}
```

### 3.3 `QuorumResult` (output of the aggregator)
```rust
struct QuorumResult {
    action_id:       String,
    approved:        bool,
    yes_count:       i64,
    no_count:        i64,
    total_voters:    i64,    // number of verified, non-coalition-capped votes received
    quorum_required: i64,    // coord_goal_quorum(goal) = N/2 + 1
    blocked_reason:  Option<String>,   // if !approved: "timeout" | "minority" | "attest_fail" | "coalition_cap"
}
```

### 3.4 Exit codes (extends R26; no collision with existing codes 0–12)
| Code | Const | Meaning |
|---|---|---|
| 13 | `QUORUM_BLOCKED_EXIT_CODE` | Quorum not met (timeout, minority YES, or all votes rejected) |
| 14 | `QUORUM_ATTEST_FAIL_EXIT_CODE` | A vote's `axtcb1-ext:` did not verify — vote rejected, counted as absent |

Code 14 is surfaced in the audit log but does NOT stop the aggregation: a single bad vote is
rejected and treated as absent; the remaining votes proceed. Aggregation producing 0 verified votes
still produces QUORUM_BLOCKED (13) via the normal minority-not-met path.

---

## §4 — Core logic

### 4.1 `VoteRequest` construction and broadcast
The proposing VM's supervisor:
1. Derives Risk from the action's effect rows via Phase 11 `risk_derive`. If Risk < High, the
   quorum gate is **not invoked** — R33 applies only to Risk ≥ High.
2. Generates `action_id` (UUIDv4, supervisor-owned randomness — not attacker-controlled).
3. Hashes the action description: `action_hash = sha256(action_description_json)`.
4. Reads its own `axtcb1-ext:` from the running R31 extended measurement.
5. Broadcasts `VoteRequest` to each configured peer VM address over the R26 vsock channel. The
   peer VMs' vsock endpoints must themselves have been established via R26 mandatory attestation —
   R33 does not add a new authenticated transport; it reuses the R26 attested channels.

### 4.2 Per-vote attestation verification (the trusted step; fail-closed)
For each received `VoteResponse`:
1. **`action_id` match.** `response.action_id == request.action_id` — a misdirected or replayed
   vote is silently dropped (not counted, not logged as an error — dropping is safer than logging
   a forgeably-spurious error stream). A mismatch on `action_hash` is logged as QUORUM_ATTEST_FAIL
   (14) and the vote is excluded.
2. **Signature verification.** `HMAC-SHA256(response fields sans signature, voter_key)` — the
   voter key was provisioned at R26 guest-image build time and bound into the `axtcb1-ext:` chain.
   Bad signature → vote excluded (14).
3. **R31 extended-TCB verification.** `response.voter_tcb` must be the pinned `axtcb1-ext:` for
   this voter slot (the relying party holds the expected per-VM `axtcb1-ext:` values, provisioned
   out-of-band at deployment time). A voter TCB that does not match the pinned expectation →
   vote excluded (14). **A vote with no `voter_tcb` field is excluded without generating exit 14**
   (it cannot have a valid signature either; the signature check fires first).
4. **Deadline.** `now_monotonic() <= request.deadline_ms` — a vote that arrives after the deadline
   is excluded, not counted. The aggregation proceeds only on timely, verified votes.

All four checks are pure given injected inputs (clock value injected by caller). The verifier never
trusts a claim in the response; it re-derives.

### 4.3 `CoordGoal` aggregation (the quorum check)
After all peer responses are received (or the deadline fires):

```
// Instantiate CoordGoal with N voters and the configured approval target.
// target = the minimum score a voter must report to count as YES (default 75).
let goal = coord_goal_new(N, target)

// For each verified, non-coalition-capped vote:
//   voter_pid = the slot index assigned to this peer VM at deployment
//   vote_score = response.score if response.vote else 0
let goal = coord_goal_propose(goal, voter_pid, vote_score)

// Quorum check: ⌈N/2⌉ + 1 YES votes required.
let approved = coord_goal_quorum_met(goal)
let consensus = coord_goal_consensus_score(goal)   // minimum accepted score; -1 if not approved
```

The `CoordGoal` idempotence invariant (`coord_goal_propose` ignores a second proposal from the same
`pid`) means double-delivered responses cannot inflate the YES count. The quorum threshold
`coord_goal_quorum(goal) = N / 2 + 1` is the same strict-majority formula used throughout Phase 14.

**Determinism (A5):** votes are keyed by `voter_pid` (a slot index, not a network arrival order).
The same set of votes in any delivery order produces the same `CoordGoal` final state because
`coord_goal_propose` updates the `scores[pid]` slot regardless of arrival order, and
`coord_goal_quorum_met` reads the final state. The `QuorumResult` is a pure function of the
verified vote set — byte-identical for the same inputs.

**`compromised_vm_vote_diluted` (Core):** if 1 of 3 voters is compromised and unconditionally votes
YES (score = 100), while the remaining 2 honest voters vote NO (score = 0 < target), then:
- `coord_goal_accept_count(goal)` = 1 (only the compromised YES clears the target)
- `coord_goal_quorum(goal)` = 2
- `coord_goal_quorum_met(goal)` = false → QUORUM_BLOCKED exit 13

The honest majority wins by not voting YES; no amount of score inflation by the compromised VM
changes the count.

### 4.4 Timeout (fail-closed; Core `timeout_blocks_action`)
The `QuorumGate` sets an absolute deadline (`VoteRequest.deadline_ms`) before broadcasting. If
`coord_goal_quorum_met(goal)` is false at the deadline — whether because too few votes arrived,
too many attestation checks failed, or the network was slow — the action is **blocked**
(QUORUM_BLOCKED, exit 13). There is no partial-approval path, no "proceed with fewer than N voters",
and no deadline extension. A liveness attack (delay all votes past the deadline) produces
QUORUM_BLOCKED (safe) rather than approval (unsafe). Operators who need liveness guarantees MUST
use a larger N with geographic distribution or adopt adaptive timeout protocols (R34).

### 4.5 R27 coalition ceiling (Core `coalition_bound_limits_same_lineage`)
Before calling `coord_goal_propose` for a verified vote, the aggregator applies the R27 per-lineage
coalition ceiling:

1. Group verified votes by `response.lineage_root`.
2. For each lineage root, count the YES votes from VMs sharing that root.
3. If `yes_votes_from_root > coalition.max_quorum_power`: exclude the excess YES votes (beyond
   `max_quorum_power`) from the count. The `coalition.max_quorum_power` is the R27 per-coalition
   setting, provisioned at deployment by the operator.
4. The default `max_quorum_power` for any lineage root is `⌈N/2⌉` — one fewer than the quorum
   threshold. This ensures that no single lineage root can independently form a quorum, even if all
   its member VMs vote YES unconditionally.

This closes the sock-puppet attack: spinning up N VMs from one lineage root and having them all
vote YES still cannot exceed `max_quorum_power < ⌈N/2⌉ + 1` effective YES votes from that root.
To reach quorum, the action must receive YES votes from at least two *distinct* lineage roots.

**Inherited boundary from R27 §4.5:** the ceiling is per lineage root, not cross-root. An operator
who creates independent roots (distinct R26 guest images with distinct `axtcb1-ext:` values) creates
independent blocs. Cross-root aggregate bounding is out of R33's scope.

---

## §5 — API / CLI

### 5.1 CLI (`axon-vm quorum` subcommands)
```bash
# Propose an action for quorum approval (Risk ≥ High)
axon-vm quorum propose \
    --action "wire_transfer(amount=1000000)" \
    --risk critical \
    --peers vm-beta:8888,vm-gamma:8889 \
    --timeout-ms 5000
# Exit 0 = approved; 13 = blocked; 14 = attestation rejection encountered (logged, may still block)
# Prints: "✓ quorum approved: 2/3 YES, consensus_score=82" or
#         "✗ quorum blocked: 1/3 YES (need 2), reason: minority"

# Listen for and vote on incoming proposals (run on each peer VM)
axon-vm quorum vote \
    --listen 0.0.0.0:8888 \
    --policy "score_threshold:75" \
    --expected-proposer-tcb "axtcb1-ext:…"
# Runs continuously; logs each VoteRequest + VoteResponse to the R28 audit ledger.

# Audit a past quorum decision
axon-vm quorum audit --action-id UUID [--json]
# Prints the QuorumResult + each voter's verified axtcb1-ext: and score.
```

### 5.2 `axon deploy` integration (Phase 11 + R33 gate)
When `axon deploy` derives Risk ≥ High (or the operator passes `--risk high`/`--risk critical`),
the deploy pipeline inserts the R33 quorum gate before the final execution step:

```
simulate → stress → redteam → verify → [R33 quorum gate] → deploy
```

The quorum gate result is included in the `axon-deploy/1` JSON schema as:
```json
{
  "risk": "high",
  "quorum": {
    "approved": true,
    "yes_count": 2,
    "quorum_required": 2,
    "consensus_score": 82,
    "voters": ["axtcb1-ext:…", "axtcb1-ext:…"]
  }
}
```

#### 5.2.1 Scoped implementation (2026-07-18) — reads pre-collected votes, doesn't broadcast

The paragraph above describes the *full* protocol (§5.1's `quorum propose`/`vote --listen`/`audit`
daemon flow, live broadcast, `consensus_score`, per-voter `axtcb1-ext:` list). Per this spec's own
established honesty convention (§S0/§40 framing), the actually-landed integration is the scoped,
file-based counterpart:

- `axon deploy FILE --risk high --quorum-dir DIR [--quorum-n N] [--axon-vm-bin PATH]`. `DIR` is an
  operator-provided directory of `.vote` files, already collected by whatever process ran
  `axon-vm quorum propose`/`vote` beforehand (this integration does **not** itself propose, wait,
  or broadcast — it only *reads* votes that already exist, matching `axon-vm quorum check`'s own
  scope). `N` defaults to the count of `.vote` files found if omitted.
- **`--quorum-dir` is required to activate the gate at all** — if omitted, the quorum gate is
  **open** (skipped), exactly like every other Phase-11 gate function that isn't present in the
  program (§ "A gate function that is absent is treated as 'passed'"). This preserves 100%
  backward compatibility: no existing `axon deploy` invocation is affected.
- If `--quorum-dir` **is** given but the `axon-vm` binary cannot be located (sibling of the running
  `axon` binary by default, or the `--axon-vm-bin` override), this is a **hard error (exit 2)** —
  the operator explicitly opted into quorum enforcement, so silently treating a missing binary as
  an open gate would be an I-9 (no-silent-success) violation, not a graceful degrade.
- Implementation: `axon deploy` shells out to `axon-vm quorum check --responses-dir DIR --n N
  --json` (schema `axon-vm-quorum-check/1`, already shipped) and parses its result — the SAME
  cross-binary-orchestration pattern `axon-web` already uses to wrap the `axon` CLI (Phase 12), not
  a new architecture. This was chosen over making `axon-core` depend on `axon-vm` as a library
  crate specifically to avoid adding a new dependency edge to `axon-core`'s Cargo.toml — the one
  crate in this workspace with an explicit, documented "keep the fast interpreter build
  dependency-light" invariant (`BUILD_RESOLVED.md`).
- The scoped `"quorum"` JSON block therefore carries the fields `check_quorum` actually produces
  (`quorum_met`, `coalition_size`, `approvals`, `required_n`, `blocking_reason`), not the full
  protocol's `consensus_score`/`voters` list (no live per-voter identity is available from a
  pre-collected directory read alone).
- A blocked quorum is treated as one more named pipeline gate (`"gate":"quorum"`) in the exact
  same `"status":"blocked_gate"` shape the existing `simulate`/`stress`/`redteam_check`/
  `assert_deployable` gates already produce — `axon deploy`'s own process exit code stays 1
  (its existing "blocked" convention), while the JSON's `exit_code` field surfaces the real
  `axon-vm` code (13 QUORUM_BLOCKED / 14 QUORUM_ATTEST_FAIL) for detail.

#### 5.2.2 S2 (vsock transport) design — corrected 2026-07-19, replaces the false "reuse R26's
Substrate trait" claim struck out in §1.3

**Why this needed re-scoping:** the original plan was "reuse R26's `Substrate` trait boundary."
Scoping S2 for real found that trait was never built (R26's own §2 module layout — `substrate.rs`,
`MockSubstrate`, `hw-attest` feature — is spec prose only; see R26's 2026-07-19 as-built note). S2
therefore does NOT gain a pre-built abstraction to plug into. The corrected design below is scoped
to exactly what quorum needs (broadcast a `VoteRequest`, collect `VoteResponse`s within a deadline)
— narrower than R26's full launch/attest/job-run surface — and does not attempt to retroactively
build R26's aspirational trait as a prerequisite (that would be a separate, much larger refactor of
already-shipped, working R26 code, out of scope for this spec).

**What exists to build on:** `crates/axon-core/src/interp.rs` already has a working, narrow vsock
pattern — raw `libc` `AF_VSOCK` `socket`/`connect`, wrapped as a `TcpStream` via `FromRawFd`, with a
4-byte-length-prefixed JSON wire protocol (`vsock_send_recv`, Gap-6 `host_await`). It is
**guest-initiates-connection-to-host only** (connects to `VMADDR_CID_HOST`); there is no host-side
`bind`/`listen`/`accept` anywhere in the tree, and no peer-to-peer or N-way broadcast precedent.
S2's real new work is the host-side listener and the fan-out/collect loop — the wire framing can be
reused as-is.

**Proposed shape (new module, not a trait):**
- New file `crates/axon-vm/src/quorum/vsock.rs` — a THIRD edge alongside `logic.rs` (pure
  aggregation, unchanged) and `io.rs` (the existing file-based impure edge, unchanged and NOT
  replaced — both transports coexist; file-based stays the default, tested, backward-compatible
  path). `vsock.rs` is impure by design, mirroring `io.rs`'s role, not a generic seam other code
  plugs into.
- `vsock_broadcast_and_collect(peer_cids: &[u32], port: u32, req: &VoteRequest, deadline_ms: u64)
  -> Vec<VoteResponse>` — proposer side. Connects to each peer CID (reusing `interp.rs`'s
  connect+frame pattern), sends `req`, collects responses until `deadline_ms` elapses; a peer that
  doesn't respond in time contributes no vote (fail-closed, matching §4.4's A4 test intent — this
  is the SAME semantics `collect_responses` already has for a missing `.vote` file, just over a
  live connection instead of a directory glob).
- `vsock_listen_and_respond(port: u32, policy: impl Fn(&VoteRequest) -> VoteResponse) -> !` — voter
  side. `bind`/`listen`/`accept` on `VMADDR_CID_ANY`, one connection at a time (quorum votes are
  infrequent and low-throughput; no need for a connection pool in v1), applying the existing
  `policy.rs` voter policy per request and writing the response back over the same connection.
- CLI: `axon-vm quorum vote --listen PORT` (blocks, replaces the current one-shot file-read with a
  vsock listener when this flag is given — omit it and the existing file-based `vote` behavior is
  unchanged) and `axon-vm quorum propose --broadcast CID1,CID2,...  --port PORT` (additive flag;
  omit it and `propose` keeps writing a request file as it does today).
- **Testing without real microVMs:** raw `AF_VSOCK` loopback support is kernel/environment-
  dependent, so the acceptance gate cannot assume it's available in every CI runner. Precedent
  (R26's own `AXON_CI_NO_KVM` pattern — a runtime env-var swap, not a trait) is reused rather than
  inventing a new mock abstraction: an `AXON_VM_QUORUM_TRANSPORT=tcp-loopback` env var swaps the
  `socket()`/`connect()`/`bind()` calls for plain `127.0.0.1` TCP at the same framing, so the wire
  protocol and fan-out/collect/deadline logic get real integration coverage in ordinary CI; a
  separate, explicitly-gated real-vsock lane (matching R26's `hw-attest`-style separation) can
  later validate actual `AF_VSOCK` on a host that supports it.

**Sizing note:** this is still real, multi-part work (new module, listener lifecycle, CLI flags, a
CI-safe test harness) — a single future iteration should size it into its own sub-slices (e.g. "S2a:
wire protocol + CLI flag scaffolding, TCP-loopback-tested" as the first bounded cut) rather than
attempt all of it at once. This section's job is to make that next iteration's scoping decision
cheap, not to have built it.

#### 5.2.3 S2f (real transport) — the "swap TCP for AF_VSOCK" plan doesn't hold, corrected
2026-07-20

**What was assumed (S2a-e's own doc comments, written across four iterations landing the
TCP-loopback path):** real `AF_VSOCK` support would be a late, narrow swap — replace `vsock.rs`'s
`TcpStream::connect`/`TcpListener::bind` with the raw `libc::socket(AF_VSOCK, ...)` calls
`crates/axon-core/src/interp.rs`'s `vsock_send_recv` already demonstrates, keep the identical
length-prefixed JSON wire format, done. This was never verified against how `AF_VSOCK` is actually
wired up elsewhere in THIS codebase before committing to it as the plan.

**What sizing S2f for real found:** the premise is false. Every existing use of vsock in this repo
(`crates/axon-vm/src/main.rs`'s `/vsock` Firecracker device config at `cmd_run` — `"guest_cid": 3`,
a host-side `uds_path` — and the `vsock_relay` function that serves `host_await` over it) connects
a guest **only to its own host** (`VMADDR_CID_HOST` = 2, the convention `interp.rs`'s
`vsock_send_recv` also uses). There is no mechanism, in Firecracker's vsock device model or
anywhere in this codebase, for one guest VM to address ANOTHER guest VM directly over vsock — it is
architecturally a guest↔own-host channel, not a peer-to-peer one. Compounding this: `axon-vm run`
launches exactly one VM per process (confirmed: `cmd_run`'s signature takes one `program`/`kernel`/
`initrd`/`vsock_port` — a single Firecracker instance), so there is no existing "one host process
already manages N peer VMs" structure to relay quorum votes through, the way `vsock_relay` relays
`host_await` calls for the single VM its own process launched.

**Why this matters for the threat model, not just the plumbing:** R33's own motivation (§1,
"Implements") is explicit that the threat is *a single VM being compromised after its attestation
report was produced* — a guest-level compromise, not a host-kernel compromise. That threat model is
satisfied by Firecracker-class micro-VM isolation regardless of whether voter VMs share one
physical host or are spread across several; the spec does not require (and never stated) that voter
VMs must be on separate hosts. So this is NOT a "the whole quorum concept requires separate hosts
and vsock can never work" dead end — same-host co-located voter VMs are a legitimate, spec-consistent
deployment. But it does mean the mechanism has to actually match one of the topologies vsock can
support, and "peer VMs connect directly to each other over vsock" isn't one of them.

**Two real options, deliberately left as an open question (§12 Q1) rather than silently picked:**

1. **Host-mediated relay.** A NEW broker component (not `vsock_relay` as-is, which is scoped to one
   process's one VM) would need to run on the host, aware of every voter VM's own vsock UDS path,
   relaying `VoteRequest`/`VoteResponse` frames between them — the same wire format `vsock.rs`
   already has, carried over an extra hop (guest → own UDS → broker → peer's UDS → peer guest)
   instead of a direct connection. Preserves R26-style attestation of the voting PROCESS itself
   (it runs inside an attested guest). Real, non-trivial new work: the broker's own trust boundary
   becomes part of the TCB (it can see and possibly tamper with every vote in transit), and it only
   works when voter VMs share a host — cross-host quorum still needs a different mechanism for the
   broker-to-broker hop.
2. **Harden the existing TCP path into the real transport.** Rather than treating `vsock.rs`'s TCP
   implementation as a throwaway CI stand-in, harden it (mutual TLS or an equivalent authenticated
   channel, non-loopback addressing) and use it as the actual production transport. Voter VMs would
   need real network reachability to each other (or to a rendezvous point) — a materially different
   deployment assumption than vsock's intra-host model, but one that naturally supports both
   same-host and cross-host quorum without a broker's added TCB surface. The attestation of the
   voting PROCESS itself would then rely on TLS client-cert identity tied to the R31 `axtcb1-ext:`
   measurement (a real, if unbuilt, design — not sketched further here since it's the losing half of
   an undecided fork, not a committed plan).

Neither is a quick sub-slice the way S2a-e were — both are real design+build efforts with different
trust-boundary and deployment-topology consequences. **This spec does not pick one.** See §12 Q1.

### 5.3 Voter policy
The default voter policy (`policy.rs`) maps Phase 11 Risk to a score:
```
score_action(risk_level: i64) -> i64 =
    risk_level < 20 → 95 (Low risk, clearly safe)
    risk_level < 50 → 82 (Medium risk, acceptable)
    risk_level < 80 → 65 (High risk, abstains at default target=75)
    else            → 30 (Critical risk, abstains)
```
A peer VM may substitute its own policy by implementing the `VoterPolicy` trait in `policy.rs`.

---

## §6 — Build order (TDD; each slice: test first, seen to fail, then pass)

| Slice | Deliverable | Pinned check (written first) |
|---|---|---|
| **S1** | `request.rs` data types; serde round-trips; `action_id` uniqueness; `action_hash` derivation. | `acc_a5_deterministic_vote_aggregation` |
| **S2** | `aggregator.rs`: CoordGoal integration, yes/no count, `coord_goal_quorum_met` wiring; mock votes. | `acc_a2_single_vm_cannot_force_approval`, `compromised_vm_vote_diluted` |
| **S3** | Per-vote attestation verification in `aggregator.rs`: R31 `axtcb1-ext:` check, signature verify, `action_id` match. | `acc_a6_attestation_verified_per_vote`, `unattested_vote_is_rejected` |
| **S4** | R27 coalition ceiling in `aggregator.rs`: lineage grouping, `max_quorum_power` cap. | `coalition_bound_limits_same_lineage` |
| **S5** | `gate.rs`: deadline enforcement, broadcast over `MockSubstrate`, QUORUM_BLOCKED on timeout. | `acc_a4_hermetic_isolated_timeout`, `timeout_blocks_action` |
| **S6** | CLI (`quorum propose/vote/audit`) + `axon deploy` Risk ≥ High integration + Phase 11 tie-in. | `acc_a1_smoke_quorum_journey`, `acc_a3_quickstart_commands_execute` |
| **S7** | Acceptance gate `scripts/r33_acceptance_gate.sh`. | Gate exits 0 with all §0 checks green. |

**Definition of "done" per slice:** the slice's named check existed, was seen to fail, now passes;
the full `axon-vm` suite is green; no workspace regression; R26 and R31 baseline tests still pass.

**Slice risks (named):**
- **S3 is the security-critical slice.** Attestation verification that passes vacuously (missing
  the `voter_tcb` check or accepting an empty string) would silently accept unattested votes.
  `unattested_vote_is_rejected` MUST send a `VoteResponse` with no `voter_tcb` field and assert
  exit 14 (not 0); the anti-stub gate checks for a real negative assertion in the test body.
- **S4 (coalition ceiling) depends on R27's `Coalition` type.** If R27's implementation has not
  yet landed `max_quorum_power`, S4 must stub it at `⌈N/2⌉` (the default safe value) and mark
  the stub with a tracking comment referencing R27. The gate anti-stub check carves out exactly
  this one R27-pending annotation and no other.

---

## §7 — Test plan (happy + adversarial; every named test is normative)

**Unit (pure; votes injected as byte slices, no vsock):**
- `acc_a2_single_vm_cannot_force_approval` — 1 verified YES vote in a 3-voter CoordGoal (target=75):
  `coord_goal_quorum_met` returns false; `QuorumResult.approved = false`; exit 13.
- `acc_a5_deterministic_vote_aggregation` — same 3 votes submitted in 6 different arrival orders
  produce byte-identical `QuorumResult` (all orderings tried; assert 6 identical results).
- `compromised_vm_vote_diluted` — 1 YES (score=100) + 2 NO (score=0) → `yes_count=1`,
  `quorum_required=2`, `approved=false`.
- `unattested_vote_is_rejected` — a `VoteResponse` with no `voter_tcb` field and a valid `action_id`:
  the aggregator logs exit 14 and excludes the vote; `coord_goal_propose` is NOT called for it.
  Assert via a call-counter on the mock `CoordGoal`.
- `acc_a6_attestation_verified_per_vote` — a vote with an `axtcb1-ext:` that does not match the
  pinned expectation for that voter slot → excluded (14), not counted.
- `coalition_bound_limits_same_lineage` — 3 verified YES votes, all with the same `lineage_root`;
  `max_quorum_power = 1` (default for N=3: ⌈3/2⌉ - 1 = 1); only 1 vote admitted from that root;
  `coord_goal_accept_count = 1 < coord_goal_quorum = 2` → blocked.

**Integration (vsock gate with `MockSubstrate`):**
- `acc_a4_hermetic_isolated_timeout` — broadcast 3 requests; `MockSubstrate` returns 0 responses;
  deadline fires; assert `QuorumResult.blocked_reason = "timeout"`, exit 13; assert the action
  was NOT executed (call-count on the action mock == 0). The gate never waits past the deadline.
- `timeout_blocks_action` — same as A4 but with 1 of 3 responses arriving before the deadline
  (insufficient for quorum); assert still blocked.

**User-journey smoke (A1 — real CLI, 3 mock voter daemons):**
- `acc_a1_smoke_quorum_journey`:
  1. Start 3 `axon-vm quorum vote --listen` processes with `AXON_CI_NO_KVM=1`.
  2. Run `axon-vm quorum propose --action "test_action" --risk high --peers ...`.
  3. Assert exit 0; assert stdout contains "✓ quorum approved: 2/3 YES".
  4. Shut down 2 of 3 voters; re-run; assert exit 13 and "✗ quorum blocked: minority".
  5. Inject a voter with a mismatched `axtcb1-ext:`; assert exit 14 is logged and that voter's
     vote is excluded; quorum outcome determined by remaining verified voters.

**Quickstart (A3):** `acc_a3_quickstart_commands_execute` — extracts the fenced commands from §9
and runs each verbatim against the built `axon-vm` binary with `AXON_CI_NO_KVM=1`; all exit 0
with the documented output patterns.

---

## §8 — Invariants & edge cases

**Invariants (assert in tests):**
- **I-1 (no approval without quorum).** `approved = true` in any `QuorumResult` implies
  `yes_count >= quorum_required` (= `N/2 + 1`). There is no partial-approval path and no override.
- **I-2 (attestation before counting).** `coord_goal_propose` is called for a vote if and only if
  all four checks in §4.2 pass. A vote that fails any check does not enter the `CoordGoal`.
- **I-3 (fail-closed on deadline).** The gate never returns `approved = true` after the deadline
  has elapsed. If the deadline fires before quorum is met, the result is QUORUM_BLOCKED (13).
- **I-4 (coalition cap per root).** YES votes from any single lineage root never exceed
  `max_quorum_power < ⌈N/2⌉ + 1` effective votes in the count. A same-lineage sweep cannot alone
  form a quorum.
- **I-5 (idempotent double delivery).** A `VoteResponse` delivered twice from the same `voter_pid`
  is counted once — `coord_goal_propose` is idempotent on repeated proposals for the same pid
  (inherits `coord.ax` invariant).
- **I-6 (exit code distinctness).** 13 = quorum blocked (insufficient YES votes or timeout);
  14 = vote attestation rejected (logged but does not stop aggregation); never collapsed.

**Edge cases:**
- **N = 1 (single-voter quorum).** Quorum threshold = `1/2 + 1 = 1`. A single attested YES vote
  approves the action. This degenerates to a single-VM gate. Operators using N=1 get no
  multi-party protection; the CLI MUST warn "N=1: no distributed protection; consider N ≥ 3."
- **All voters abstain (score < target).** `coord_goal_accept_count = 0 < quorum_required` →
  QUORUM_BLOCKED (13), `blocked_reason = "minority"`. Safe.
- **Exactly quorum-sized YES, then one voter revises to NO.** `coord_goal_propose` is idempotent —
  the first proposal from a pid is final. A voter cannot retract its YES once submitted. This is
  by design (CoordGoal §3.3 in `coord.ax`).
- **Deadline set in the past.** The gate immediately fires the timeout path without broadcasting.
  QUORUM_BLOCKED (13), `blocked_reason = "timeout"`. Operators MUST set deadlines in the future;
  the CLI rejects a `--timeout-ms 0` with a `Malformed` error before broadcasting.
- **Peer VM unreachable (vsock connect fails).** The vote for that slot is absent (treated as NO
  by the CoordGoal's initial zero state). If enough other VMs vote YES, quorum can still be met.
  An unreachable VM does NOT block an otherwise-valid quorum — liveness is preserved for honest
  fleets.

---

## §9 — Quickstart (these exact commands are executed by `acc_a3`)
```bash
# CI demo — 3 mock voter daemons on localhost (no real KVM required)
export AXON_CI_NO_KVM=1

# 1. Start 3 peer voters (each runs the default policy, score_threshold=75)
axon-vm quorum vote --listen 127.0.0.1:8888 --policy "score_threshold:75" &
axon-vm quorum vote --listen 127.0.0.1:8889 --policy "score_threshold:75" &
axon-vm quorum vote --listen 127.0.0.1:8890 --policy "score_threshold:75" &

# 2. Propose a Risk=High action — 3 VMs attest, 2 vote YES (low risk), quorum met
axon-vm quorum propose \
    --action "run_batch_analysis(dataset=./public/)" \
    --risk high \
    --peers 127.0.0.1:8888,127.0.0.1:8889,127.0.0.1:8890 \
    --timeout-ms 5000
# → ✓ quorum approved: 2/3 YES (threshold 2), consensus_score=82; exit 0

# 3. Propose a Risk=Critical action — all 3 score < 75 (critical risk), quorum blocked
axon-vm quorum propose \
    --action "wire_transfer(amount=1000000)" \
    --risk critical \
    --peers 127.0.0.1:8888,127.0.0.1:8889,127.0.0.1:8890 \
    --timeout-ms 5000
# → ✗ quorum blocked: 0/3 YES (need 2), reason: minority; exit 13

# 4. Audit the last quorum decision (shows each voter's axtcb1-ext: and score)
axon-vm quorum audit --action-id <UUID-from-step-2> --json
```

---

## §12 — Open questions

- **Q1 (real S2f transport — added 2026-07-20, unresolved): host-mediated vsock relay, or a
  hardened real-network path?** See §5.2.3 for the full analysis. Sizing "swap TCP for real
  `AF_VSOCK`" found the premise doesn't hold — Firecracker's vsock is guest↔own-host only, never
  peer-to-peer, and `axon-vm run` launches one VM per process with no existing multi-VM host relay
  to extend. The two real options have materially different trust-boundary and deployment
  consequences (a new broker's TCB surface + same-host-only, vs. TLS-hardening the TCP path +
  needing real network reachability between voters) — this is a founder/architecture-level fork,
  not something to pick implicitly while landing an incremental slice. Unresolved.

---

## §13 — Dependency DAG

| Node | Depends-on / blocked-by | Gate | Status |
|---|---|---|---|
| R33.S0 | R26-confidential-microvm-substrate, R31-extended-tcb-attestation (both landed) | `scripts/r33_acceptance_gate.sh` §§1-7 (CLI journey + exit-code distinctness) | landed |
| R33.S1 | R33.S0 | R26/R31 regression check (`axon-attest` test suite unchanged) | landed |
| R33.S2a (wire protocol) | R33.S0 | `crates/axon-vm/src/quorum/vsock.rs` unit tests (9); `cargo clippy -p axon-vm --all-targets -D warnings` clean | **landed 2026-07-19** — length-prefixed JSON framing (`write_frame`/`read_frame`/`write_json_frame`/`read_json_frame`), transport-agnostic (generic `Read`/`Write`), matching `interp.rs`'s existing `vsock_send_recv` wire convention exactly |
| R33.S2b (real-socket round trip) | R33.S2a | `crates/axon-vm/src/quorum/vsock.rs` unit tests (3 new, 12 total) | **landed 2026-07-19** — `connect_and_round_trip`: proposer side, one real TCP-loopback connection (the §5.2.2 CI stand-in for `AF_VSOCK`), deadline-bounded (`connect_timeout` + read/write timeouts), fail-closed on timeout/refused-connection (`Err`, treated like a missing `.vote` file by future collect logic). Voter side still test-local only (`spawn_one_shot_voter`, not a production primitive) |
| R33.S2c (broadcast/collect fan-out) | R33.S2b | `crates/axon-vm/src/quorum/vsock.rs` unit tests (3 new, 15 total) | **landed 2026-07-19** — `broadcast_and_collect`: one thread per peer (wall-clock bounded by `deadline` regardless of peer count, not `deadline * N`), feeds `logic::check_quorum`'s existing `&[VoteResponse]` shape directly. An unreachable/timed-out peer contributes nothing, never aborts the whole broadcast |
| R33.S2d (listen primitive + `quorum vote --listen` CLI flag) | R33.S2c | `crates/axon-vm/src/quorum/vsock.rs` unit tests (1 new, 16 total); `scripts/r33_acceptance_gate.sh` §11 (real CLI journey) | **landed 2026-07-19** — `respond_once`: real production listen/accept/respond primitive (single-shot, no daemon loop), also now backing the test helper every S2a-c test already used (`spawn_one_shot_voter` delegates to it). `axon-vm quorum vote --listen PORT` wires it in — same `--approve`/`--deny`/`--reason`/`--lineage-root` decision as the file-based path, `--request`/`--out` become `required_unless_present="listen"` (100% backward compatible), `--listen` `conflicts_with_all` them. Still `broadcast_and_collect`'s ONLY non-test caller is `axon-vm quorum vote --listen`'s peer, not itself — the `propose --broadcast` flag (S2e) is what would finally give `broadcast_and_collect` a real caller |
| R33.S2e (`propose --broadcast` CLI flag) | R33.S2d | `scripts/r33_acceptance_gate.sh` §12 (real multi-process CLI journey) | **landed 2026-07-19** — `axon-vm quorum propose --broadcast CID1,CID2,... --n N --deadline-ms MS [--json]` wires `broadcast_and_collect` into a real, non-test caller for the first time: broadcasts, collects, runs the same `check_quorum` `check` uses, exits with the same 0/13/14 convention (factored into a shared `report_quorum_result` helper, not duplicated). Whole S2 proposer+voter path is now real end-to-end: `propose --broadcast` against real `vote --listen` peers + an unreachable one correctly reports partial approvals and excludes the unreachable peer |
| R33.S2f (real transport) | R33.S2e; **§12 Q1 must be resolved first** | not yet named | **blocked, not just todo (re-scoped 2026-07-20, §5.2.3)** — the "swap TCP for real AF_VSOCK" premise was found false (Firecracker vsock is guest↔own-host only, never peer-to-peer; `axon-vm run` is one-VM-per-process). Two real options remain (host-mediated relay vs. hardened real-network path), each real design+build work with different trust-boundary consequences — this is a founder/architecture decision (§12 Q1), not a mechanical sub-slice like S2a-e were |
| R33.S3 (coalition ceiling) | R33.S0 | `coalition_bound_limits_same_lineage`, `coalition_cap_does_not_block_distinct_lineage_roots` (`crates/axon-vm/src/quorum/mod.rs`); `scripts/r33_acceptance_gate.sh` §8 (real CLI sock-puppet + distinct-roots journey) | landed (hardcoded `ceil(N/2)-1` fallback per the spec's own §6 slice-risk note — no dependency on R27's real `Coalition`/`max_quorum_power` type) |
| R33.S4 (`axon deploy` integration) | R33.S0 | `scripts/r33_acceptance_gate.sh` §10 (backward-compat + sock-puppet-blocked + legit-quorum-met + missing-binary-hard-error, real `axon`↔`axon-vm` cross-binary journey) | landed (scoped per §5.2.1: reads pre-collected votes via subprocess shell-out to `axon-vm quorum check`, no live broadcast — no new `axon-core` Cargo dependency) |

## §14 — Evidence ledger

| Claim | Verify command | Expected | Last verified (commit @ date) | Result |
|---|---|---|---|---|
| Quorum unit tests (`check_quorum` aggregator logic) pass | `bash scripts/r33_acceptance_gate.sh` (step 4) | 16 passed, 0 failed (11 + 5 coalition tests, incl. even-N N=4/N=6 and legacy-vote coverage added after a decision audit flagged those paths as untested) | this commit @ 2026-07-18 | PASS |
| Decision-audit follow-up: coalition cap formula holds at even N (N=4, N=6, sock-puppet + distinct-root control); legacy `.vote` JSON (no `lineage_root`) coalesces into one capped bucket as intended, not N independent voters; Risk >= Critical with no `--quorum-dir` now warns visibly instead of silently deploying unenforced | `bash scripts/r33_acceptance_gate.sh` (step 8 even-N tests via step 4; step 10e for the warning) | all pass; warning text present, deploy still succeeds (visibility only, no behavior change) | this commit @ 2026-07-18 | PASS |
| `axon-vm` binary builds with the quorum CLI wired in | `bash scripts/r33_acceptance_gate.sh` (step 5) | build succeeds | this commit @ 2026-07-18 | PASS |
| End-to-end propose→vote→check CLI journey, both majority outcomes | `bash scripts/r33_acceptance_gate.sh` (step 6) | 1/3 approvals → exit 13 QUORUM_BLOCKED; 2/3 approvals → exit 0 QUORUM MET | this commit @ 2026-07-18 | PASS |
| Attestation mismatch across voters is a distinct failure from a plain minority | `bash scripts/r33_acceptance_gate.sh` (step 7) | mismatched `voter_tcb` → exit 14 QUORUM_ATTEST_FAIL, distinct from exit 13 | this commit @ 2026-07-18 | PASS |
| R27 coalition ceiling: 3 YES votes from ONE `--lineage-root` cannot alone force quorum (real CLI, not just unit test); 3 YES votes from 3 DISTINCT roots meet quorum normally (cap does not over-trigger) | `bash scripts/r33_acceptance_gate.sh` (step 8) | sock-puppet → exit 13, reason names "coalition"; distinct-roots → exit 0 | this commit @ 2026-07-18 | PASS |
| R33 did not regress R26/R31 | `bash scripts/r33_acceptance_gate.sh` (step 9) | `axon-attest` test suite: 22 passed, 0 failed | this commit @ 2026-07-18 | PASS |
| R33.S4: `axon deploy --quorum-dir` real cross-binary journey — no-flag backward compat (no `quorum` field); sock-puppet coalition BLOCKED (exit 1, `gate:quorum`, JSON `exit_code:13`); 3-distinct-root quorum met (deploys, `quorum_met:true`); missing `--axon-vm-bin` is a hard error (exit 2), never a silent open gate | `bash scripts/r33_acceptance_gate.sh` (step 10) | all 4 sub-checks pass | this commit @ 2026-07-18 | PASS |
| S2's "reuse R26's Substrate trait" premise is false — no such trait exists in any crate | `grep -rn "trait Substrate\|MockSubstrate\|QemuSwtpmSubstrate" crates/` | zero hits (confirmed independently: an Explore-agent research pass over R26/R33/quorum code, 2026-07-19) | this commit @ 2026-07-19 | CONFIRMED (spec text corrected in §1.3, §5.2.2 added; no code claim, this is a documentation-drift finding) |
| R33.S2a: vsock wire protocol (`quorum/vsock.rs`) round-trips arbitrary bytes and real `VoteRequest`/`VoteResponse` JSON; truncated/empty/malformed-JSON streams are `io::Error`s, never panics; the EOF sentinel (length=0) is distinguishable from a real empty payload | `cargo test -p axon-vm quorum::vsock::` | 9 passed, 0 failed | this commit @ 2026-07-19 | PASS |
| `axon-vm`/`axon-attest` clippy-clean under `--all-targets -D warnings` (previously ungated by `gate.sh`, same class of blind spot BUG_HUNT #35 found in the runtime crates) — 3 pre-existing findings fixed (a 9-arg fn, a manual-range-contains, a dead-code manifest struct), no behavior change | `cargo clippy -p axon-vm -p axon-attest --all-targets -- -D warnings` | clean | this commit @ 2026-07-19 | PASS |
| R33.S2a did not regress any existing R33 CLI journey | `bash scripts/r33_acceptance_gate.sh` | ALL CHECKS PASSED (unchanged from before S2a) | this commit @ 2026-07-19 | PASS |
| R33.S2b: `connect_and_round_trip` completes a real round trip over TCP loopback (voter's actual response returned); a voter's EOF sentinel is `Ok(None)`, not an error; connecting to a dead/refused port is `Err`, not a panic or a hang past the deadline | `cargo test -p axon-vm quorum::vsock::` (5 consecutive reruns, checking for socket/timing flakiness) | 12 passed, 0 failed, all 5 reruns identical | this commit @ 2026-07-19 | PASS |
| R33.S2b did not regress axon-vm as a whole or any existing R33 CLI journey | `cargo test -p axon-vm`; `bash scripts/r33_acceptance_gate.sh` | 58/58 unit tests; ALL CHECKS PASSED (unchanged) | this commit @ 2026-07-19 | PASS |
| R33.S2c: `broadcast_and_collect` gathers every responsive peer's vote (order-independent); an unreachable peer (dead/refused port) contributes nothing without blocking the others; wall-clock for 4 dead peers stays under 3x a single deadline (proves parallel fan-out, not a sequential loop that would take ~4x) | `cargo test -p axon-vm quorum::vsock::` (5 consecutive reruns, checking for threading/timing flakiness) | 15 passed, 0 failed, all 5 reruns identical | this commit @ 2026-07-19 | PASS |
| R33.S2c did not regress axon-vm as a whole or any existing R33 CLI journey | `cargo test -p axon-vm`; `bash scripts/r33_acceptance_gate.sh` | 61/61 unit tests; ALL CHECKS PASSED (unchanged) | this commit @ 2026-07-19 | PASS |
| R33.S2d: a real `axon-vm quorum vote --listen` process answers a real TCP client's `VoteRequest` with the operator-specified `--approve`/`--reason`/`--lineage-root`, over the actual wire protocol (verified with a from-scratch Python client speaking the same length-prefixed JSON framing, not the Rust test helpers); `--listen` conflicts with `--request`/`--out`; omitting `--listen` still requires them (100% backward compatible) | `bash scripts/r33_acceptance_gate.sh` §11 (3 sub-checks, 3 consecutive reruns for socket/timing flakiness) | all pass, all 3 reruns identical | this commit @ 2026-07-19 | PASS |
| R33.S2d did not regress axon-vm as a whole or any existing R33 CLI journey | `cargo test -p axon-vm`; `cargo clippy -p axon-vm --all-targets -D warnings`; `bash scripts/r33_acceptance_gate.sh` | 62/62 unit tests; clippy clean; ALL CHECKS PASSED (unchanged plus new §11) | this commit @ 2026-07-19 | PASS |
| R33.S2e: `propose --broadcast` against 2 real `vote --listen` peers + 1 unreachable peer reports exactly 2/3 approvals and QUORUM MET (exit 0); against 2 unreachable peers, QUORUM BLOCKED (exit 13) with no hang; omitting `--broadcast` is unaffected (writes the file, exits 0, no quorum check runs at all) | `bash scripts/r33_acceptance_gate.sh` §12 (3 sub-checks, real multi-process journey, 3 consecutive reruns for flakiness) | all pass, all 3 reruns identical | this commit @ 2026-07-19 | PASS |
| R33.S2e: the whole `vsock.rs` module is now clippy-clean with ZERO `#[allow(dead_code)]` — every function has a real, non-test caller | `cargo clippy -p axon-vm --all-targets -- -D warnings` (after removing the module-level allow) | clean | this commit @ 2026-07-19 | PASS |
| R33.S2e did not regress axon-vm as a whole or any existing R33 CLI journey | `cargo test -p axon-vm`; `bash scripts/r33_acceptance_gate.sh` | 62/62 unit tests; ALL CHECKS PASSED (12 sections now) | this commit @ 2026-07-19 | PASS |
| S2f's "swap TCP for real AF_VSOCK" premise is false — Firecracker vsock is guest↔own-host only, `axon-vm run` is one-VM-per-process | `grep -n 'vsock' crates/axon-vm/src/main.rs` (the `/vsock` device config in `cmd_run`, `guest_cid: 3`/`VMADDR_CID_HOST`-style host-only addressing, `vsock_relay`'s single-VM scope); `grep -n 'fn cmd_run'` (confirms one VM per process, no multi-VM host structure) | confirmed: no peer-to-peer vsock mechanism anywhere in this codebase or Firecracker's own device model | this commit @ 2026-07-20 | CONFIRMED (spec text corrected in header + §5.2.3 added + §12 Q1 opened; no code claim, this is a documentation-drift-prevention finding, same class as the earlier R26 Substrate-trait correction) |

Honest scope, per this spec's own header: the above is a **file-based, single-host** slice, now
WITH the R27 coalition ceiling (§4.5) AND the `axon deploy` integration (§5.2.1) closed. The R26
vsock broadcast transport (§2) is the one remaining gap — R33.S2 above. Do not read "gate:
ALL CHECKS PASSED" as "R33 fully implements this spec"; it means "the scoped slice this session
built is what it claims to be."

---

## §-Gate — `scripts/r33_acceptance_gate.sh` (pinned; FAILS if any §0 check missing or stubbed)

The single source of "done." It MUST:

1. **Presence check** — `grep` the R33 test sources and assert every named check from §0 exists:
   `acc_a1_smoke_quorum_journey`, `acc_a2_single_vm_cannot_force_approval`,
   `acc_a3_quickstart_commands_execute`, `acc_a4_hermetic_isolated_timeout`,
   `acc_a5_deterministic_vote_aggregation`, `acc_a6_attestation_verified_per_vote`,
   `unattested_vote_is_rejected`, `timeout_blocks_action`, `coalition_bound_limits_same_lineage`,
   `compromised_vm_vote_diluted`.
   Any missing name → **gate fails**.

2. **Anti-stub check** — assert no R33 acceptance or adversarial test body is `#[ignore]`d /
   `todo!()` / `unimplemented!()` / `assert!(true)`. **One explicit, named carve-out (NOT
   currently active — see below):** the `coalition_bound_limits_same_lineage` test may carry
   `#[ignore = "R27 max_quorum_power not yet
   shipped"]` while R27 slice S6 is in progress; the gate allows *exactly* this annotation by its
   R27 reason string and **fails if any other `#[ignore]` appears, or if the carve-out's reason
   string drifts**. The fallback behaviour (cap at `⌈N/2⌉ - 1`) must still be exercised and pass.
   **2026-07-18 update: this carve-out was never needed and was never wired into the actual gate
   script** — `coalition_bound_limits_same_lineage` landed for real, using exactly the fallback
   default this carve-out anticipated (a hardcoded cap, no R27 `Coalition` type dependency), so the
   `#[ignore]` path was never exercised. Left documented here for the historical record of why it
   was anticipated, not because it's live.

3. **Anti-vacuous attestation check** — assert `unattested_vote_is_rejected` exercises a real
   negative: the test body must contain a call sending a `VoteResponse` with no `voter_tcb` AND an
   assertion that `coord_goal_propose` was not called for that vote (call-counter assert, not
   merely asserting the final `QuorumResult`). A test that only checks the final count passes
   vacuously if the aggregator accidentally counted the vote anyway.

4. **Determinism proof** — assert `acc_a5_deterministic_vote_aggregation` exercises ≥ 3 distinct
   vote orderings (grep for ordering permutation helpers; fail if fewer than 3 are present).

5. **R26/R31 regression check** — run `cargo test -p axon-vm` with the existing R26 and R31 test
   suites and assert all still pass. R33 must not regress R26 or R31.

6. **Run** `cargo test -p axon-vm` (all R33 unit tests green) **and** execute the §9 quickstart
   block verbatim against the built binary (A3) **and** run `acc_a1` driving the real CLI with
   mock voter daemons.

7. Exit 0 only if all of the above pass; print which check failed otherwise. Wire
   `r33_acceptance_gate.sh` into the repo's `gate.sh --strict`.

---

## §-Definition of Done

**Per slice (S1–S7):** the slice's named check existed, was seen to fail, now passes; the full
`axon-vm` suite is green; no workspace regression; R26 and R31 baselines still pass.

**Per milestone (R33 complete):**
- `axon-vm quorum propose` (N=3) with 2 of 3 verified YES votes exits 0 with
  "✓ quorum approved" (`acc_a1` passes).
- 1 of 3 YES votes exits 13 (`acc_a2` passes).
- An unattested vote is excluded and logged as exit 14, not counted (`unattested_vote_is_rejected`
  passes; the action's approval/block is determined by the remaining verified votes).
- A timeout with insufficient votes exits 13 and does NOT execute the action
  (`timeout_blocks_action` passes; action call-count == 0).
- Same-lineage VMs cannot form quorum alone (`coalition_bound_limits_same_lineage` passes, or the
  R27-pending carve-out is active with the safe default).
- A compromised VM's YES vote is diluted by honest NOs (`compromised_vm_vote_diluted` passes).
- Vote aggregation is order-independent (`acc_a5` passes for ≥ 3 orderings).
- `axon deploy --risk high` invokes the quorum gate and blocks if quorum is not met.
- `scripts/r33_acceptance_gate.sh` exits 0 with every §0 check green.

---

## §-Notes for the implementer (do NOT deviate without updating this spec)

- Keep `aggregator.rs` **pure**. It receives `Vec<VoteResponse>` and a `deadline_elapsed: bool`;
  it does not call `now_monotonic()` or touch vsock. If you reach for a clock or network call
  inside the aggregator, you are in `gate.rs`'s job.
- **`coord_goal_propose` is the only write to `CoordGoal`.** Do not update scores any other way.
  The idempotence property (`coord.ax` line 158–163) — "a second proposal from the same pid is
  ignored" — is load-bearing for I-5 (double-delivery safety); do not bypass it.
- **The coalition ceiling is enforced before `coord_goal_propose`.** The aggregator must check
  lineage-root counts and cap them at `max_quorum_power` BEFORE calling `coord_goal_propose`.
  A vote admitted past the ceiling and then retracted at the `CoordGoal` level would create a
  confusing audit trail; the cap happens at the door.
- **The approval target and N are operator-configured at deployment, not caller-supplied.** The
  proposing VM MUST NOT let the agent code under evaluation choose the quorum size or the score
  threshold. These values come from the deployment manifest (the same authority level as R26's
  `GuestManifest`).
- **Exit codes 13 and 14 are distinct contracts.** 13 = the action is blocked (final outcome);
  14 = a vote was rejected during aggregation (logged, does not stop aggregation). Never collapse
  14 into 13 in the user-facing output, and never use 14 as a final action outcome.
- **Be honest about N < 3.** Single-VM quorum (N=1) gives no multi-party protection and the CLI
  MUST say so. Do not silently accept N=1 as equivalent to a full quorum gate — name the
  degenerate case in the output so an operator is never misled.
- R33 changes no R26 isolation, no R27 kill-switch logic, no R31 measurement algebra. If you are
  editing `substrate.rs`, `latch.rs`, or `measure.rs`, you are out of scope for R33.
