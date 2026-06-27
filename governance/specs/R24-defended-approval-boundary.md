# Tech Spec — R24: The Defended Approval Boundary ("who can be PERSUADED")

**Spec ID:** `R24-defended-approval-boundary`
**Status:** 📝 Draft (2026-06-27)
**Implements:** `VISION_OS.md` §4.4 (the pillar "Who can be PERSUADED — the human approver as an
attack surface") and gap **G7** (§11), against the threat model in `VISION_OS.md` §1. R24 turns the
R22 approval flow into a **defended** boundary: legible+quantified grants, **pre-approval
simulation**, **risk-proportional friction** (multi-party / multi-sig for high-risk, derived from the
Phase-11 risk gate), **decision provenance** (an append-only, tamper-evident approval log), and
**default-deny + expiring/revocable** grants.
**Audience:** an implementer who builds *strictly* against this document and reads only it.

> **Read this framing first.** Against a *capability-proven* box (R20/R21), a superhuman AI's
> highest-probability escape is **not technical — it's persuading the human approver to grant a
> capability** (the AI-box experiment: the AI won by persuading the gatekeeper, not by hacking). R22
> already makes the human approve a **legible bound** and binds it tamper-evidently (`ApprovalToken`,
> `crates/axon-intent/src/approval.rs`). R22 left **four** explicit hooks for R24 (R22 §1.2): the
> `--approvers N>1 → exit 2 "multi-party approval is R24"` stub (`crates/axon-intent/src/cli.rs:278`),
> a *recorded-but-unenforced* `risk` field, no pre-approval simulation beyond `axon-os explain`, and
> no expiry/revocation. R24 fills exactly those four hooks. It does **not** invent new capability/grant
> types, does **not** run programs, and does **not** add real human identity/auth (§1.2). It makes the
> *approval act itself* a hardened, provenance-bearing, risk-gated, revocable boundary.

---

## §0 — Requirement → Section → Acceptance-check index (the build gate verifies none are skipped)

| Req | What | Spec § | Pinned acceptance check (test name) |
|---|---|---|---|
| **A1** | Real user journey + smoke test through the actual CLI | §5, §7 | `acc_a1_smoke_simulate_to_revoke` |
| **A2** | Real runnable example artifact (real grant-request files, not toys in the test dir) | §5.6, §7 | `acc_a2_example_grants_friction_and_expiry` |
| **A3** | Quickstart whose exact commands are executed by a test | §9, §7 | `acc_a3_quickstart_commands_execute` |
| **A4** | Hermetic, isolated simulation + canonical entrypoint, hard timeout | §4.6, §7 | `acc_a4_simulation_isolated_timeout` |
| **A5** | Deterministic (same request + approvers ⇒ byte-identical token + provenance digest) | §4.7, §7 | `acc_a5_deterministic_grant_and_log` |
| **A6** | Integrity: multi-sig threshold + append-only tamper-evident provenance log | §4.4, §4.5, §7 | `acc_a6_provenance_chain_tamper_detected` |
| **Core** | A high/critical-risk grant CANNOT be single-approved (friction is risk-proportional) | §4.3, §7 | `high_risk_grant_cannot_be_single_approved` |
| **Core** | Multi-sig: N distinct *recorded names* (name-distinctness, NOT independence — §1.2) below threshold → no valid token; at threshold → token | §4.4, §7 | `multisig_threshold_gates_token` |
| **Core** | A revoked OR expired grant is REFUSED at the run boundary (axon-os) | §4.8, §7 | `revoked_or_expired_grant_refused_at_run` |
| **Core** | A duplicate / forged approver signature does NOT count toward the threshold | §4.4, §7 | `duplicate_or_forged_approver_rejected` |
| **Gap** | N distinct *fabricated* ids from one console are NOT independence — R24 does NOT defend this (recorded KNOWN GAP, needs HW signing) | §1.2, §7 | `sock_puppet_distinct_ids_not_independent` |
| **Gate** | The acceptance gate fails if any check above is missing/stubbed | §10 | `scripts/r24_acceptance_gate.sh` |

The build is **not done** until every row's check exists, was seen to fail first, and now passes.

---

## §1 — Overview & scope

### 1.1 What it does
R24 hardens the R22 approval act into a **defended boundary** with five capabilities, each grounded in
`VISION_OS.md` §4.4's bullet list:

1. **Pre-approval simulation** ("show *what the grant would enable* before the human commits"). A new
   `axon-intent simulate <grant-request>` (and a `--simulate` extension to `axon-intent review`)
   renders a forward-looking statement: *"this grant would let the program reach hosts X, spend ≤ Y,
   write paths Z; the risk gate rated it `High`; the redteam found …"* — computed from the R22
   `ApprovalRequest` + the R21 effect-set + the Phase-11 `Risk`, **before** any signature is collected.
2. **Risk-proportional friction** ("require multi-party / multi-sig approval for high-risk"). The
   Phase-11 risk already computed by `axon_intent::approval::derive_risk`
   (`crates/axon-intent/src/approval.rs:57`) now drives an **approval policy**: `Risk::Low|Medium` ⇒
   threshold 1 (single approver, as today); `Risk::High` ⇒ threshold ≥ 2; `Risk::Critical` ⇒ threshold
   ≥ 3 (defaults; an explicit `--threshold N` may only RAISE, never lower, like R22's `--risk` floor).
3. **Multi-party / multi-sig approval.** A high/critical grant requires **N distinct *recorded
   names*** (name-distinctness, NOT independence — §1.2 RESIDUAL RISK) to each record an approval over
   the *same* `(program_digest, grant_digest)` content before a valid `MultiApprovalToken` is emitted.
   Below threshold ⇒ no token (default-deny). The property actually held is: *cannot be approved by
   fewer than N distinct recorded names* — it is NOT a proof that N independent humans agreed.
4. **Decision provenance** ("who approved what, when, on which presented evidence — so a manipulated
   approval is visible after the fact"). Every approval act appends a hash-chained `ProvenanceEvent`
   to an append-only `ProvenanceLog` (`<name>.provenance`), binding the approver id, the decision, the
   `(program, grant)` digests, the **evidence digest** (the exact simulation/redteam text presented),
   and a logical timestamp. The chain is tamper-evident (the R21 `RunRecord` pattern, §3.4 there).
5. **Default-deny + expiring/revocable grants.** The token carries a **TTL** (`expires_at_tick`) and a
   **max-uses** budget; `axon-intent revoke <name>` writes a revocation marker. `axon-os` refuses an
   **expired**, **exhausted**, or **revoked** token at the run boundary — extending
   `axon_os::verify_approval` (`crates/axon-os/src/approval.rs:56`), which today only checks
   content-binding + decision.

The end-state: the R22 triple `{program.ax, job.axjob, job.approval}` gains, for a friction-bearing
grant, a `<name>.provenance` (the decision chain) and a richer `<name>.approval` (multi-sig +
TTL/max-uses), and `axon-os run` honors expiry/revocation alongside the existing tamper check.

### 1.2 What it explicitly does NOT do (out of scope for R24)
- **No real human identity / authentication / PKI / login.** Approvers are **named ids + a multi-sig
  threshold**, exactly as R22's `ApprovalToken` is *content-binding, not a signature* (R22 §1.2 / A6).
  An "approver signature" here is a recorded `(approver_id, content_digest)` entry the threshold counts
  — it proves *agreement over content*, not *who* the human is. Hardware-rooted / cryptographically
  *signed* approval chained to silicon is `VISION_OS.md` §5 ("tied to a hardware root of trust") / a
  later spec. A6 documents exactly what is vs isn't authenticated.
- **RESIDUAL RISK — what multi-sig does and does NOT buy (the headline honesty gap).** The threshold
  raises the *number of humans who must be persuaded* before a high-risk grant moves. That is real
  friction against a human-scale attacker. But against a **superhuman persuader operating through ONE
  console** that submits N distinct *fabricated* approver ids, the threshold provides **NO
  protection**: approver **independence is unproven** — ids are unauthenticated (recorded, not signed)
  and the recorder is trusted, so N "distinct approvers" can be N sock-puppet names typed at a single
  invocation. Worse, even with N *real* humans, a sufficiently capable persuader can mount
  **correlated persuasion of a single operator** (or a single coordinating channel) and defeat the
  whole panel at once — multi-sig assumes independent decision channels it cannot enforce. The only
  real fix is **hardware-rooted per-approver signing** (each approval cryptographically bound to a
  distinct silicon root of trust, `VISION_OS.md` §5), which forces N genuinely separate channels;
  that is **out of scope for R24**. R24's honest claim is therefore *name-distinctness + recorded,
  tamper-evident provenance*, **not** independence. The `sock_puppet_distinct_ids_not_independent`
  test (§7) records this as a KNOWN GAP so it is visible in the gate, not silently assumed away.
- **No persuasion-detection ML.** R24 raises *friction* and records *provenance* so a manipulated
  approval is *visible after the fact*; it does **not** classify a prompt or an approver's reasoning as
  manipulative. That is a model/monitor concern (`VISION_OS.md` §4.5), out of scope.
- **No new capability / grant / effect / risk types.** R24 imports R22's `Risk`/`ApprovalRequest` and
  R21's `Grant`/`DeclaredEffects` verbatim. The Phase-11 `derive_risk` is reused, not reimplemented.
- **No execution / enforcement / sandboxing.** R24 never runs the program (R21 does). R24's job ends at
  the friction-gated, provenance-bearing, expiring triple. The **only** runtime addition is the
  expiry/revocation check inside `axon_os::verify_approval` (§4.8) — and that lives in axon-os.
- **No wall-clock time in the trusted/hashed region.** Expiry uses a **logical tick** (a monotone
  counter supplied at the run boundary, defaulting to a recorded value — §3.5), so the provenance chain
  and tokens stay deterministic (A5), mirroring R21's "no timestamp in the hashed record" rule.
- **No quorum across machines / distributed consensus.** Multi-sig is N approvers against **one**
  local approval artifact. Cross-Principal / cross-host quorum (`Quorum`/`CoordGoal`, `VISION_OS` §4.5)
  is a separate concern.

### 1.3 Persona / ICP
A **security-conscious operator / approving authority** who must grant authority to an AI-synthesized
program under social-engineering pressure. For a low-risk grant they want the fast single-approve path;
for a high/critical grant they want the system to *refuse to let one person* wave it through, to *show
them what it would enable first*, to *record who signed off on which evidence*, and to *let them revoke
or time-box* the grant. Secondarily, the R21 supervisor, which consumes the expiry/revocation contract.

### 1.4 Interface & tech constraints
- **Interface:** extend the existing `axon-intent` CLI (new `simulate`/`revoke` subcommands; `approve`
  `--approvers`/`--threshold` now implemented; `review --simulate`), plus library API. The one
  cross-crate addition is in `axon-os` (`verify_approval` gains expiry/revocation).
- **Language/deps:** Rust. **No new crates, no new heavy deps** — reuse `axon-intent` + `axon-os` +
  `sha2` + `serde`/`serde_json` (the exact deps both crates already list, `crates/axon-intent/
  Cargo.toml` / `crates/axon-os/Cargo.toml`).
- **Perf/security:** core logic pure/I-O-free (the provenance chain, multi-sig tally, policy
  derivation, token build/verify are pure functions); the only impure seams are the simulation
  subprocess (§4.6), file reads/writes in `cli`, and the run-boundary check in axon-os. **Fail closed**
  everywhere: below-threshold ⇒ no token; expired/revoked/exhausted ⇒ refused; tampered chain ⇒
  refused.

### 1.5 Domain-specific risks (what matters most here)
- **Single-approver capture of a high-risk grant** (a persuaded human waves it through). → friction
  proportional to risk; `high_risk_grant_cannot_be_single_approved`.
- **Threshold gaming** (one approver counted N times; a forged/duplicated approver entry). → distinct
  approver ids (name-distinctness, NOT independence — §1.2), content-bound, de-duplicated;
  `duplicate_or_forged_approver_rejected`. **NOTE the un-defended sibling:** N *distinct fabricated*
  ids from one console DO reach the threshold — that is the `sock_puppet_distinct_ids_not_independent`
  KNOWN GAP (§1.2 RESIDUAL RISK / §7), out of scope without HW signing.
- **Invisible approval edit** (provenance rewritten to hide who/what was approved). → hash-chained,
  append-only `ProvenanceLog`; `acc_a6_provenance_chain_tamper_detected`.
- **Standing authority that never lapses** (a one-time grant reused forever). → TTL + max-uses + explicit
  revocation, enforced at the run boundary; `revoked_or_expired_grant_refused_at_run`.
- **Evidence/decision skew** (approved on one simulation, a different bound runs). → the token already
  binds `(program, grant)` (R22); R24 *additionally* binds the **evidence digest** into the provenance
  chain so the presented simulation is itself tamper-evident.

---

## §2 — Architecture & modules

R24 is **mostly new modules inside `crates/axon-intent`** (it extends the gateway) **plus one function
extension inside `crates/axon-os`** (the run-boundary expiry/revocation check). **Core logic is pure;
the simulation subprocess is the only new impure seam, behind a `Simulator` trait** (mockable, exactly
like R22's `Synthesizer` and R21's `Runtime`).

```
crates/axon-intent/src/                  (R24 ADDS these modules; reuses R22's approval.rs/intent.rs)
  policy.rs        Risk → ApprovalPolicy (threshold, ttl, max_uses) derivation.        [PURE — NEW]
  simulate.rs      `trait Simulator` (grant-request → Simulation) + Mock + Real impl.  [I/O — only NEW impure seam]
  multisig.rs      Multi-sig tally: collect N approver entries → MultiApprovalToken.    [PURE — NEW]
  provenance.rs    ProvenanceLog: append-only, hash-chained build + verify (sha2).      [PURE — NEW]
  approval.rs      (R22, REUSED) Risk, ApprovalRequest, derive_risk, digests, legible.  [PURE — reused]
  cli.rs           (R22, EXTENDED) simulate/revoke subcmds; approve --threshold; review --simulate.
  lib.rs           (R22, EXTENDED) re-export the R24 public API.
crates/axon-os/src/
  approval.rs      (R21, EXTENDED) verify_approval ALSO checks expiry / max-uses / revocation. [§4.8]
  cli.rs           (R21, EXTENDED) pass the logical tick + a revocation-dir to verify_approval.
crates/axon-intent/tests/
  acceptance.rs    The A1–A6 + Core checks (named exactly per §0).
examples/grants/
  summarize.request.md     A real LOW-risk grant request → single-approve, short TTL (A2).
  egress.request.md        A real HIGH-risk grant (net+fs) → REQUIRES 2 approvers (A2).
  deploy.request.md        A real CRITICAL grant (exec) → REQUIRES 3 approvers (A2 negative-friction).
scripts/r24_acceptance_gate.sh
README-axon-intent-r24.md  Quickstart whose commands a test executes (A3).
```

**Dependency graph (acyclic; → = uses). Direction is one-way: `axon-intent` → `axon-os`, NEVER back.**
```
main → cli → {policy, simulate, multisig, provenance, approval(R22)}
                       simulate → (axon-core: `axon-intent review`/`axon-os explain` subprocess)  // ONLY new impure edge
policy     → (axon-intent::approval::Risk)                  // reuse R22's Risk — do NOT redefine
multisig   → (approval(R22) digests, provenance, sha2)
provenance → (sha2)                                          // hash chain only
approval(R22) → (axon-os::canonical_grant)                  // R22 already single-sources the grant digest in axon-os
─────────────────────────────────────────────────────────────────────────────────────────────
axon-os::verify_approval  → (axon-os::Grant, sha2)           // §4.8 expiry/revocation lives HERE, in axon-os
```
**Cross-crate dependency-direction rule (mirrors R22):** `axon-os` MUST NOT depend on `axon-intent`
(no cycle). The shared encodings the two crates agree on are **single-sourced in `axon-os`**: today
`canonical_grant`/`verify_approval` (`crates/axon-os/src/lib.rs:23`), and R24 adds the **TTL /
max-uses / revocation token fields** to the *structural* `TokenView` axon-os already deserializes
(`crates/axon-os/src/approval.rs:42`) — axon-os reads the new fields by name from the JSON, exactly as
it reads `risk`/`decision` today, **without importing the axon-intent `MultiApprovalToken` type**.

**Rule the implementer MUST hold:** nothing under `policy/multisig/provenance` may perform I/O, read
the clock, or call random. The logical tick, approver ids, and any model output are passed in
explicitly (A5). Only `simulate.rs`, `cli.rs` (intent), and the axon-os `cli.rs`/`approval.rs` reads
touch the outside world.

---

## §3 — Data model

### 3.1 `GrantRequest` (parsed from a `.request.md`, OR derived from an existing `.axjob`)
A grant request is the *thing being approved*: the program + the grant the human is asked to authorize,
plus the friction policy it triggers. For the common path it is **derived from the R22 `.axjob` +
`.ax`** the gateway already produced; a `.request.md` is a thin, human-authored wrapper for the
simulate-first flow.
```
GrantRequest {
    program_src:    String,        // the exact .ax program (binds program_digest)
    grant:          Grant,         // the R21/R22 grant being requested
    intent_goal:    String,        // one-line objective (from the intent), for the simulation header
    program_digest: String,        // "axsha256:"+sha256(program_src)  — R22 approval::program_digest
    grant_digest:   String,        // "axsha256:"+sha256(canonical_grant) — R22 approval::grant_digest
    risk:           Risk,          // derive_risk(grant) (R22 §4.4), the `--risk` floor may RAISE it
}
```
`.request.md` serialized form (the parser reads exactly these headers; `program`/`grant` may instead
be supplied as `--job <name.axjob>`):
```markdown
# Grant Request
Deploy the egress summarizer to production.

## Program
- ./egress.ax

## Grant
- fs_read: ./data/
- fs_write: ./out/
- net: api.example.com
- exec: none
- max_label: internal

## Budget
- calls: 100
- tokens: 50000
- cost_micro: 1000000

## TTL
- ticks: 1000           # optional; default per policy (§3.3)

## MaxUses
- 5                     # optional; default per policy
```
Validation (fail → `FrictionRefusal::Malformed`, exit 2): `# Grant Request` present; a resolvable
`## Program`/`--job`; a `## Grant` with **no `..` component** in any path (path-traversal denied,
mirroring R21's manifest validation, `crates/axon-os/src/manifest.rs`); `ttl.ticks ≥ 1` and
`max_uses ≥ 1` if present.

### 3.2 `Simulation` (the pre-approval evidence; what the human is shown)
```
Simulation {
    intent_goal:   String,
    legible:       String,    // R22 legible_bound: "WILL be allowed to … may NOT … Budget ≤ … Risk …"
    would_enable:  Vec<String>,  // forward-looking, concrete: "reach api.example.com", "write ./out/", "spend ≤ $1.00"
    risk:          Risk,
    redteam:       Vec<String>,  // findings surfaced from the redteam/risk gate (may be empty)
    threshold:     u32,          // approvers REQUIRED for this risk (the friction it triggers)
    evidence_digest: String,     // "axev1:"+sha256(canonical(legible, would_enable, risk, redteam, threshold))
}
```
`evidence_digest` is the **single hash of exactly what was presented**. It is bound into every
`ProvenanceEvent` (§3.4) so "approved on the basis of which presented evidence" is itself
tamper-evident: if the simulation text differs from what an approver saw, the chain reveals it.

### 3.3 `ApprovalPolicy` (risk → friction; pure, deterministic — §4.3)
```
ApprovalPolicy {
    threshold:  u32,   // distinct recorded-name approvers REQUIRED before a valid token (name-distinctness, NOT independence)
    ttl_ticks:  u64,   // default token lifetime (logical ticks); 0 = no expiry only for Low
    max_uses:   u64,   // default max run-boundary uses; 0 = unlimited only for Low
}
fn policy_for(risk: Risk, floor: Option<u32>) -> ApprovalPolicy
```
**Defaults (the friction ladder; `--threshold` may only RAISE, never lower):**
| Risk | threshold | ttl_ticks | max_uses |
|---|---|---|---|
| Low | 1 | 0 (no expiry) | 0 (unlimited) |
| Medium | 1 | 10000 | 100 |
| High | **2** | 1000 | 10 |
| Critical | **3** | 100 | 1 |

The threshold floor (`policy_for(risk, Some(n))`) sets `threshold = max(default, n)` — friction can be
*demanded* but never *waived*. (This is the same "floor only raises" discipline as R22's `--risk`,
`crates/axon-intent/src/approval.rs:196`.)

### 3.4 `ProvenanceEvent` + `ProvenanceLog` (the append-only decision chain; JSON `axon-provenance/1`)
Hash-chained exactly like R21's `RunRecord` (`governance/specs/R21-axon-os-supervisor.md` §3.4).
```
ProvenanceLog {
    schema:          "axon-provenance/1",
    program_digest:  String,        // binds the chain to the EXACT program
    grant_digest:    String,        // …and the EXACT grant
    events:          Vec<ProvenanceEvent>,
    chain_digest:    String,        // "axprov1:"+head_hash (the tamper-evident seal)
}
ProvenanceEvent {
    seq:             u64,           // 0,1,2,… monotonic
    approver:        String,        // the approver id (recorded, NOT authenticated — A6)
    decision:        "approved" | "rejected",
    evidence_digest: String,        // the Simulation digest the approver acted on (§3.2)
    tick:            u64,           // logical timestamp (monotone counter, NOT wall-clock — §1.2)
    prev_hash:       String,        // previous event's `hash` (or program_digest⊕grant_digest for seq 0)
    hash:            String,        // sha256(prev_hash ‖ 0x1f ‖ canonical(seq,approver,decision,evidence_digest,tick))
}
```
`canonical(...)` = the fields joined by `\x1f` in the fixed order listed, UTF-8, no whitespace
(identical convention to R22's `UNIT` separator, `crates/axon-intent/src/approval.rs:20`). The chain
makes the log tamper-evident: altering/reordering/dropping any event breaks every later `hash` and the
`chain_digest`. **Authenticated:** the integrity + ordering of the recorded decisions and the
(program, grant, evidence) they were made over. **NOT authenticated (A6):** *who* the approver really
is (the id is recorded, not signed) and *that the recorder saw every decision* (the recorder is
trusted) — a HW root of trust + signing is `VISION_OS.md` §5, out of scope.
> **Non-PKI stopgap (note, not a R24 requirement).** Short of full HW-rooted signing (§5), a
> **per-approver out-of-band nonce / shared secret** — each genuine approver holds a distinct secret
> issued over a separate channel and must echo it (or an HMAC of the evidence digest under it) in their
> `ProvenanceEvent` — would force N *real* channels rather than N names typed at one console, partially
> closing the `sock_puppet_distinct_ids_not_independent` gap (§1.2 RESIDUAL RISK / §7) without any PKI.
> It is recorded here as a forward option; R24 does not implement it.

### 3.5 `MultiApprovalToken` (the friction-bearing handoff; supersedes R22's `ApprovalToken` for
threshold > 1; JSON `axon-approval/2`)
Backward-compatible with R22's `axon-approval/1`: for `threshold == 1` and no TTL/revocation the
gateway still emits the R22 `axon-approval/1` token verbatim (so the Low-risk path is byte-unchanged).
For friction-bearing grants it emits:
```
MultiApprovalToken {
    schema:           "axon-approval/2",
    program_digest:   String,        // binds the EXACT program (R22)
    grant_digest:     String,        // binds the EXACT grant (R22)
    risk:             Risk,
    threshold:        u32,           // approvers required
    approvers:        Vec<String>,   // the DISTINCT approver ids who approved (len ≥ threshold)
    evidence_digest:  String,        // the simulation the decision was made on (binds the evidence)
    expires_at_tick:  u64,           // 0 = no expiry (Low only); else the token is dead at tick ≥ this
    max_uses:         u64,           // 0 = unlimited (Low only); else refused after this many run-boundary uses
    provenance_digest: String,       // "axprov1:"+chain head — binds the token to its decision chain
    token_digest:     String,        // "axtok2:"+sha256(canonical of ALL fields above, fixed order)
}
```
**axon-os run-boundary contract (axon-os honors this; §4.8):** when a `.approval` is present, axon-os
recomputes `program_digest`/`grant_digest` from the on-disk `(program, grant)` (R22, unchanged),
re-hashes `token_digest`, and **additionally** for `schema == "axon-approval/2"`: asserts
`approvers.len() ≥ threshold` with all-distinct ids; asserts not expired (`expires_at_tick == 0 ||
current_tick < expires_at_tick`); asserts not over max-uses (a per-token use counter at the store);
asserts no revocation marker for this `(program_digest, grant_digest)` exists. Any failure →
`Verdict::Denied` exit 8, **before** running.

### 3.6 `Revocation` (the default-deny lever; JSON `axon-revoke/1`, written by `axon-intent revoke`)
```
Revocation {
    schema:         "axon-revoke/1",
    program_digest: String,    // the grant being revoked (by content, not by name — survives renames)
    grant_digest:   String,
    by:             String,    // who revoked (recorded)
    reason:         String,
}
```
Stored as `<name>.revoked` next to the token; axon-os checks for a matching marker at the run boundary.
A revocation **cannot be un-done by editing the token** (the token can't grant past a standing
revocation) — revocation is default-deny and wins.

### 3.7 `FrictionRefusal` + exit codes (consistent with R22's carved scheme,
`crates/axon-intent/src/refusal.rs`)
```
FrictionRefusal =
  | Malformed { reason }                 → exit 2   // bad request file / usage
  | BelowThreshold { have, need }        → exit 5   // fewer distinct approvers than the risk requires (policy refusal)
  | DuplicateApprover { who }            → exit 5   // a repeated/forged approver entry — not counted
  | EvidenceMismatch { detail }          → exit 8   // approver signed over a different simulation than presented
  | ProvenanceTampered { which }         → exit 9   // the decision chain doesn't verify (new code, owned by R24)
  | Revoked { reason }                   → exit 8   // the grant was revoked
  | Expired                              → exit 8   // the token's TTL/max-uses lapsed
```
Exit 0 = a friction-satisfied token (+ provenance) was emitted, or a valid token verified at run.
(5 reuses Axon's AI-policy/"policy refusal" band as R22 does; 8 reuses the carved
sandbox/capability/handoff code; 9 reuses R21's tamper/divergence code,
`governance/specs/R21-axon-os-supervisor.md` §3.5.)

---

## §4 — Core logic / algorithms

### 4.1 Pre-approval simulation (`simulate::simulate`) — show what the grant would enable
```
fn simulate(req: &GrantRequest, sim: &impl Simulator) -> Result<Simulation, FrictionRefusal>
```
1. Build the **legible bound** from the grant via R22's `legible_bound(grant, risk, confidence)`
   (`crates/axon-intent/src/approval.rs:148`) — reuse, do not re-author.
2. Build `would_enable`: a concrete forward-looking list from the grant's non-empty axes ("reach
   {hosts}", "write {paths}", "read {paths}", "spend ≤ {cost}", "spawn processes" iff exec=any). This
   is the *positive* projection of `legible` ("what it WOULD do"), for the simulate-first framing.
3. Gather `redteam` findings: invoke the R21/R22 evidence path (`sim.redteam_and_risk(req)`, §4.6) —
   in the real impl a bounded subprocess over `axon-os explain` (the legible gate verdict,
   `crates/axon-os/src/cli.rs:128`) and/or the Phase-12 `redteam` check. The *friction* below is the
   hard gate, not the redteam text, so evidence-gathering fails *open* on the gate — **but it must NOT
   fail open silently.** On any failure (timeout / non-zero exit / unavailable), `redteam` is set to
   the single explicit marker `["RISK EVIDENCE UNAVAILABLE — adversarial findings did not run"]`
   (NEVER left empty), and that marker is hashed into `evidence_digest` (step 5) and surfaced in the
   simulation. A persuader thus cannot present a clean-looking simulation whose adversarial findings
   silently failed open: the absence of evidence is itself stamped into the bound, tamper-evident
   evidence the approver sees and the provenance chain records.
4. `threshold = policy_for(req.risk, floor).threshold`.
5. `evidence_digest = "axev1:"+sha256(canonical(legible, would_enable, risk, redteam, threshold))`.
6. Return the `Simulation`. **No decision is recorded here** — simulation is read-only.

### 4.2 Risk-proportional policy (`policy::policy_for`) — Core, pure, deterministic
Implements the §3.3 table. `threshold = max(default_for(risk), floor.unwrap_or(0))`; `ttl_ticks` /
`max_uses` from the table (a friction-bearing risk always has finite TTL + max-uses; Low keeps the R22
unbounded default for byte-compatibility). **Pure: no clock, no env.** Tested by
`policy_ladder_is_monotone` (threshold is non-decreasing in risk; the floor only raises).

### 4.3 Risk-proportional friction is enforced (`multisig::require_policy`) — Core, fail-closed
The headline defense. Before a token is emitted, `multisig::build_token` (§4.4) asserts the collected
distinct-approver count `≥ policy_for(risk, floor).threshold`. For `Risk::High` the default threshold
is 2 and for `Risk::Critical` it is 3, so **a single approver can never produce a valid token for a
High/Critical grant** (`high_risk_grant_cannot_be_single_approved`) — i.e. it cannot be approved by
fewer than N distinct *recorded names* (name-distinctness, NOT independence — §1.2 RESIDUAL RISK).
The `--approvers N>1` /
`--threshold N` path that R22 stubbed (`crates/axon-intent/src/cli.rs:278`) now routes here instead of
exiting 2.

### 4.4 Multi-sig tally + token build (`multisig::build_token` / `verify_token`) — Core, A6
```
fn build_token(req:&GrantRequest, approvals:&[Approval], policy:&ApprovalPolicy,
               evidence_digest:&str, log:&ProvenanceLog) -> Result<TokenOut, FrictionRefusal>
Approval { approver: String, decision: Decision, evidence_digest: String }
TokenOut = Single(ApprovalToken /*R22 axon-approval/1*/) | Multi(MultiApprovalToken /*axon-approval/2*/)
```
Steps (each a named failure case):
1. **Filter to approvals over the presented evidence.** Any `approval.evidence_digest !=
   evidence_digest` → `EvidenceMismatch` exit 8 (the approver acted on a *different* simulation than
   the one bound to this request — a substitution attack is caught).
2. **De-duplicate approvers.** Reduce to **distinct** approver ids that voted `Approved`; a repeated id
   counts **once** → `DuplicateApprover` exit 5 if a duplicate was submitted to inflate the count
   (`duplicate_or_forged_approver_rejected`). (Name-distinctness — NOT independence (§1.2) — is by
   distinct id; forging "more approvers" by *repeating* one is the modeled attack. Forging more
   approvers by submitting N *distinct fabricated* ids is the UN-modeled
   `sock_puppet_distinct_ids_not_independent` KNOWN GAP — R24 does not defend it.)
3. **Threshold gate.** If `distinct_approved.len() < policy.threshold` → `BelowThreshold{have,need}`
   exit 5, **no token emitted** (default-deny). (`multisig_threshold_gates_token`.)
4. **Provenance binding.** Require `log.verify()` Ok and `log.chain_digest` over the *same*
   `(program_digest, grant_digest)`; bind `provenance_digest = log.chain_digest`. A tampered chain →
   `ProvenanceTampered` exit 9.
5. **Emit.** If `policy.threshold == 1` AND `ttl_ticks == 0` AND `max_uses == 0` → emit the R22
   `ApprovalToken` (`axon-approval/1`) verbatim (Low path, byte-compatible). Else build the
   `MultiApprovalToken` (`axon-approval/2`) with `expires_at_tick = issue_tick + policy.ttl_ticks`
   (0 stays 0), `max_uses`, the distinct `approvers`, `evidence_digest`, `provenance_digest`, and
   `token_digest = "axtok2:"+sha256(canonical of all fields)`.
- `verify_token(token, program_src, grant, current_tick, uses_so_far, revocation_present)`: the
  *pure-core* mirror of axon-os's run-boundary check (§4.8) — R24's tests call this directly without a
  subprocess. For `axon-approval/1` it delegates to R22's `verify_token`
  (`crates/axon-intent/src/approval.rs:238`). For `axon-approval/2` it additionally checks
  distinct-approvers ≥ threshold, not-expired, not-over-max-uses, not-revoked.

### 4.5 Provenance chain (`provenance::append` / `verify`) — Core, tamper-evident (A6)
- `append(log, approver, decision, evidence_digest, tick) -> ProvenanceLog`: push a `ProvenanceEvent`
  with `prev_hash` = (the previous event's `hash`, or `program_digest⊕grant_digest` for seq 0) and the
  chained `hash`; recompute `chain_digest = "axprov1:"+head_hash`. **Pure, append-only** (it returns a
  new log with one more event; it never rewrites history).
- `verify(log) -> Result<(), Tamper>`: recompute every `hash` from the stored events + the
  `(program, grant)` digests and assert each `hash` and the final `chain_digest` match. Any mutation
  (changed field, dropped/reordered/inserted event) → `Err(Tamper{which})`. **Pure, no I/O.**
  (`acc_a6_provenance_chain_tamper_detected`.)

### 4.6 Hermetic, isolated simulation (`RealSimulator`) — the impure seam (A4)
- `sim.redteam_and_risk(req)` invokes the canonical entrypoint (resolved by absolute path from
  `AXON_BIN`, **not** ambient PATH) running `axon-os explain` and/or the Phase-12 redteam check in a
  **fresh subprocess**, wrapped in a **hard timeout** (`AXON_INTENT_SIM_TIMEOUT_MS`, default 30000); on
  expiry the child group is killed (RAII guard) and the simulation returns with `redteam` =
  `["RISK EVIDENCE UNAVAILABLE — adversarial findings did not run"]` (§4.1 step 3 — the marker is
  hashed into `evidence_digest`, so a failed-open simulation is *visibly* stamped, never silently
  clean; the *friction* gate in §4.3 is the hard defense, the simulation is advisory). No leaked
  handles; minimal explicit environment. (`acc_a4_simulation_isolated_timeout` asserts a hanging
  simulator is killed, produces no leaked child, AND that the rendered simulation carries the
  RISK-EVIDENCE-UNAVAILABLE marker.)

### 4.7 Determinism (A5)
- The **only** volatile inputs are: the issue tick (`--issue-tick`, default a recorded constant, never
  clock/random in core), the approver ids/order (the gateway sorts distinct approvers before hashing so
  ordering can't perturb the digest), and the simulator output (routed through the same mock/replay seam
  R22 uses, `AXON_AI_MOCK`/`AXON_AI_REPLAY`). **Contract:** same `(request, sorted-distinct-approvers,
  issue-tick)` under mock ⇒ **byte-identical** `MultiApprovalToken` + `ProvenanceLog` JSON
  (`acc_a5_deterministic_grant_and_log`). Canonical field order + the `\x1f` separator + integer
  formatting are fixed so digests are stable.

### 4.8 Run-boundary expiry / revocation (`axon_os::verify_approval` extension) — in axon-os, fail-closed
The **one** cross-crate addition, implemented in `crates/axon-os/src/approval.rs` (NOT in axon-intent —
axon-os owns the run boundary and must not depend on axon-intent). Extend `verify_approval` (today
ending at decision + content-binding, `crates/axon-os/src/approval.rs:56`):
```
fn verify_approval(token_json:&str, program_src:&str, grant:&Grant,
                   current_tick:u64, uses_so_far:u64, revoked:bool) -> Result<(), String>
```
1. Branch on `schema`: `axon-approval/1` → the existing R22 checks, unchanged (byte-compatible).
2. `axon-approval/2` → the R22 content-binding + decision + token-rehash checks (now over
   `axtok2:`/all `/2` fields), **then**:
   - `approvers` are distinct and `approvers.len() >= threshold` → else `"below approval threshold"`.
   - `expires_at_tick == 0 || current_tick < expires_at_tick` → else `"approval expired"` (exit 8).
   - `max_uses == 0 || uses_so_far < max_uses` → else `"approval use limit exhausted"` (exit 8).
   - `!revoked` → else `"grant revoked"` (exit 8).
   The caller (`crates/axon-os/src/cli.rs:194`) supplies `current_tick` (from `--tick`, default 0 ⇒
   no-expiry-yet for non-`/2` and an explicit value for tests), `uses_so_far` (read from a tiny
   `<name>.uses` counter next to the token, incremented after a successful run), and `revoked` (true
   iff a `<name>.revoked` marker matching the token's digests exists). The shared `/2` field encoding is
   **single-sourced in axon-os** (the `TokenView` struct, `crates/axon-os/src/approval.rs:42`, gains
   the new fields) so axon-intent and axon-os cannot drift — exactly the R22 pattern.

---

## §5 — Public API / interface contract

### 5.1 Library API (added to `crates/axon-intent/src/lib.rs`; R22's API stays)
```
pub fn policy_for(risk: Risk, threshold_floor: Option<u32>) -> ApprovalPolicy;
pub fn simulate(req:&GrantRequest, sim:&impl Simulator) -> Result<Simulation, FrictionRefusal>;
pub fn append_provenance(log:&ProvenanceLog, approver:&str, decision:Decision,
                         evidence_digest:&str, tick:u64) -> ProvenanceLog;
pub fn verify_provenance(log:&ProvenanceLog) -> Result<(), Tamper>;
pub fn build_multisig_token(req:&GrantRequest, approvals:&[Approval], policy:&ApprovalPolicy,
                            evidence_digest:&str, log:&ProvenanceLog, issue_tick:u64)
                            -> Result<TokenOut, FrictionRefusal>;
pub fn verify_multisig_token(token:&MultiApprovalToken, program_src:&str, grant:&Grant,
                             current_tick:u64, uses_so_far:u64, revoked:bool) -> Result<(), Mismatch>;
pub trait Simulator { fn redteam_and_risk(&self, req:&GrantRequest) -> Vec<String>; }
// In axon-os (the run boundary; signature extended, §4.8):
pub fn axon_os::verify_approval(token_json:&str, program_src:&str, grant:&Grant,
                                current_tick:u64, uses_so_far:u64, revoked:bool) -> Result<(),String>;
```

### 5.2 CLI (`axon-intent`; new + extended subcommands; every subcommand has `--help`; legible output)
```
axon-intent simulate <request.md | --job name.axjob> [--risk LEVEL]
    Render the PRE-APPROVAL simulation: the legible bound + "this grant WOULD let it: reach …, write …,
    spend ≤ …" + the risk + redteam findings + "FRICTION: requires N distinct approvers." Performs
    NO decision, writes nothing. Exit 0.   [pre-approval simulation, VISION §4.4]

axon-intent approve <name.axjob> --by OP [--accept|--reject] [--threshold N] [--evidence DIGEST]
    Record ONE approver's decision. Appends a ProvenanceEvent to <name>.provenance (creating it). If the
    accumulated DISTINCT approvers ≥ the risk-derived threshold, ALSO emits/updates <name>.approval (a
    MultiApprovalToken for threshold>1, else the R22 token). Prints "✓ approved by OP (1/2) — 1 more
    approver required" or "✓ THRESHOLD MET (2/2) — token emitted, expires in 1000 ticks". A High/Critical
    grant with one approver prints "⏳ 1/2 — multi-party approval required (risk High)" and writes NO
    token. Exit 0 (recorded) / 3 (rejected) / 5 (below threshold so far is NOT an error — it's a partial;
    only a duplicate/forged approver → exit 5).

axon-intent review <name.axjob> [--simulate]
    (R22) print the legible ApprovalRequest; with --simulate ALSO print the §4.1 forward-looking
    "would enable" projection + the required threshold. No side effects. Exit 0.

axon-intent revoke <name.axjob> --by OP [--reason TEXT]
    Write <name>.revoked binding the grant's (program_digest, grant_digest). A subsequent axon-os run is
    refused (exit 8). Prints "✓ revoked grant <digest> by OP". Exit 0.
```
The R22 `--approvers N>1 → exit 2 "multi-party approval is R24"` stub
(`crates/axon-intent/src/cli.rs:278`) is **replaced** by the multi-sig path above.
Bad usage / missing file → exit 2 with a specific message.

### 5.6 Shipped example artifacts (A2 — real, in `examples/grants/`, runnable immediately)
- `summarize.request.md` → LOW risk (fs only, no net) → **single approver** suffices; emits the R22
  `axon-approval/1` token (byte-compatible); short TTL optional. The fast path still works.
- `egress.request.md` → HIGH risk (net + fs) → `axon-intent approve --by alice` prints "1/2 — multi-
  party approval required", **no token**; a second `--by bob` → "THRESHOLD MET (2/2)", a
  `axon-approval/2` token with `expires_at_tick` set. `axon-intent revoke` then refuses the run.
- `deploy.request.md` → CRITICAL risk (exec) → **threshold 3**; one approver, then two, both refused;
  three distinct approvers → token. Submitting `alice` twice → `DuplicateApprover` exit 5 (the headline
  threshold-gaming demo).

---

## §6 — Build order (TDD: write the named test first, see it fail, make it pass; green before next)

- **S1 — Policy ladder.** `policy.rs`. Tests: `policy_ladder_is_monotone` (Low/Med=1, High=2,
  Critical=3; floor only raises; friction-bearing risks get finite ttl+max_uses).
- **S2 — Provenance chain.** `provenance.rs`. Tests: chain builds + re-verifies; mutate/reorder/drop an
  event → `Tamper`; `acc_a6_provenance_chain_tamper_detected`; equal inputs → equal `chain_digest`.
- **S3 — Multi-sig tally + token.** `multisig.rs`. Tests: `multisig_threshold_gates_token` (below ⇒ no
  token, at ⇒ token); `high_risk_grant_cannot_be_single_approved`;
  `duplicate_or_forged_approver_rejected`; `EvidenceMismatch` when an approval is over a different
  evidence digest; Low path emits the R22 `axon-approval/1` token byte-for-byte.
- **S4 — GrantRequest parse + simulation (Mock).** `simulate.rs` + a `MockSimulator`. Tests: parse the
  example request; reject `..` paths/malformed (exit 2); simulation lists "would enable" + threshold;
  `evidence_digest` is stable.
- **S5 — Token verify + axon-os run-boundary extension.** Extend `axon_os::verify_approval` (§4.8) +
  the axon-os `TokenView`. Tests: `revoked_or_expired_grant_refused_at_run` (expired tick → exit-8
  reason; over max-uses → exit-8; a revocation marker → exit-8); `axon-approval/1` path unchanged
  (byte-compatible). **Cross-crate test in axon-intent tests, driving axon-os.**
- **S6 — RealSimulator + hermetic exec.** Wire §4.6. Tests: `acc_a4_simulation_isolated_timeout`
  (a hanging simulator is killed at the timeout, no leaked child, simulation still renders).
- **S7 — CLI: simulate / approve (multi-sig) / revoke / review --simulate + human output.** Extend
  `cli.rs`. Tests: `--help` on every subcommand; usage error → exit 2; the partial-approval and
  threshold-met messages; replace the R22 `--approvers` exit-2 stub.
- **S8 — Examples + smoke + quickstart.** `examples/grants/*`, `README-axon-intent-r24.md`. Tests:
  `acc_a1_smoke_simulate_to_revoke`, `acc_a2_example_grants_friction_and_expiry`,
  `acc_a3_quickstart_commands_execute`, `acc_a5_deterministic_grant_and_log`.
- **S9 — Acceptance gate.** `scripts/r24_acceptance_gate.sh` (§10). Green = done.

---

## §7 — Test plan (happy + **adversarial**; every named test is normative)

**Unit / core (pure, fast):**
- `policy_ladder_is_monotone` — the §3.3 table; threshold non-decreasing in risk; `--threshold` floor
  only raises; High/Critical get finite ttl + max_uses.
- `high_risk_grant_cannot_be_single_approved` (the headline defense) — a `Risk::High` request with a
  single approver → `build_token` returns `BelowThreshold{have:1,need:2}`, **no token**; `Risk::Critical`
  with two approvers → `BelowThreshold{have:2,need:3}`. A persuaded single human cannot wave it through.
- `multisig_threshold_gates_token` — N-1 distinct approvers → no token; the Nth distinct approver → a
  valid `axon-approval/2` token whose `approvers.len() == threshold`.
- `duplicate_or_forged_approver_rejected` (adversarial) — the same approver id submitted twice toward a
  threshold-2 grant → counted once → `DuplicateApprover` exit 5 / still below threshold; forging a
  second entry by repeating an id does NOT reach the threshold.
- `evidence_mismatch_rejected` (adversarial) — an approval carrying a different `evidence_digest` than
  the request's simulation → `EvidenceMismatch` exit 8 (an approver "signed" over a swapped simulation).
- `sock_puppet_distinct_ids_not_independent` (adversarial — the DECISIVE negative test; **records a
  KNOWN GAP, not a defense**) — submit N *distinct fabricated* approver ids from a SINGLE invocation
  (e.g. `["ghost1","ghost2"]` toward a threshold-2 grant) and assert this **DOES** reach the threshold
  and **DOES** emit a token. This is exactly what R24 does **NOT** defend: name-distinctness is not
  independence, and a superhuman persuader at one console can mint N sock-puppet names. The test asserts
  the gap explicitly (the token is emitted) so the gap is visible **in the gate, not invisible**, with
  the marker `// KNOWN GAP: needs HW signing` (per-approver HW-rooted signatures, `VISION_OS.md` §5 /
  §1.2 RESIDUAL RISK). It must NOT be `#[ignore]`d — it is a normative, executing acknowledgement that
  R24's threshold proves *distinct recorded names*, not *independent humans*.
- `acc_a6_provenance_chain_tamper_detected` — for every field of a mid-chain `ProvenanceEvent`, mutate
  it and assert `verify_provenance` returns `Tamper` pointing at that event; also drop, reorder, and
  insert an event; and assert a token whose `provenance_digest` doesn't match its log → rejected.
- `low_risk_token_is_r22_byte_compatible` — a `Risk::Low` single-approve emits an `axon-approval/1`
  token byte-identical to what R22 emits (the fast path is unchanged).

**Integration (real `axon`/`axon-os` subprocess, mock simulator):**
- `acc_a4_simulation_isolated_timeout` — a simulator stub that sleeps past the timeout is killed;
  simulation still renders (advisory redteam empty); process gone; no leaked handle.
- `acc_a5_deterministic_grant_and_log` — run `approve` twice under mock with the same approvers + issue
  tick; assert the emitted `{.approval,.provenance}` bytes are identical across runs.
- `revoked_or_expired_grant_refused_at_run` (cross-spec, the default-deny defense) — emit a
  threshold-met `axon-approval/2` token, then (a) `axon-os run --tick T` with `T ≥ expires_at_tick` →
  `Denied` exit 8 "approval expired"; (b) run it `max_uses+1` times → the last → exit 8 "use limit
  exhausted"; (c) `axon-intent revoke` then `axon-os run` → exit 8 "grant revoked".
- `acc_a2_example_grants_friction_and_expiry` — `summarize` single-approves (exit 0, R22 token);
  `egress` requires 2 (one approver → no token; two → token); `deploy` requires 3 and rejects a
  duplicate approver (exit 5).

**User-journey smoke (A1 — drives the REAL CLI exactly as the operator would, via subprocess):**
- `acc_a1_smoke_simulate_to_revoke`: (1) `axon-intent simulate egress.request.md` → asserts the
  legible "would enable: reach api.example.com, write ./out/" + "FRICTION: requires 2 distinct
  approvers"; (2) `axon-intent approve egress.axjob --by alice --accept` → asserts "1/2 — multi-party
  approval required" + a `.provenance` exists + **no** `.approval`; (3) `axon-intent approve … --by bob
  --accept` → asserts "THRESHOLD MET (2/2)" + the `.approval` (`axon-approval/2`) exists; (4) `axon-os
  run egress.axjob` → "✓ approval verified" + completes; (5) `axon-intent revoke egress.axjob --by
  carol` then `axon-os run egress.axjob` → "⚠ DENIED: grant revoked" exit 8. Each step asserts stdout
  text AND the on-disk artifact.

**Quickstart (A3):**
- `acc_a3_quickstart_commands_execute` — extracts the fenced block from `README-axon-intent-r24.md`
  and runs each line verbatim against the built binaries; documented outputs hold.

---

## §8 — Invariants & edge cases

**Invariants (assert in tests):**
- **I-1 Friction is risk-proportional and floor-only.** A token is emitted only if distinct approvers
  ≥ `policy_for(risk, floor).threshold`; the floor raises, never lowers. High/Critical can never be
  single-approved. (`high_risk_grant_cannot_be_single_approved`.)
- **I-2 Name-distinctness by distinct id (NOT independence).** The threshold counts **distinct**
  approver ids; a repeated id counts once; a duplicate submitted to inflate the count is rejected. The
  property held is *cannot be approved by fewer than N distinct recorded names* — **not** that N
  independent humans agreed. Authentication AND independence are out of scope (A6 / §1.2 RESIDUAL
  RISK): N distinct *fabricated* ids from one console still reach the threshold
  (`sock_puppet_distinct_ids_not_independent`, a recorded KNOWN GAP needing HW signing).
- **I-3 Provenance is append-only + tamper-evident.** Any mutation/reorder/drop of a `ProvenanceEvent`
  breaks the chain; `verify_provenance` detects it; the token binds `provenance_digest`.
- **I-4 Evidence-binding.** Every approval is bound to the exact `evidence_digest` of the simulation
  presented; a mismatch is refused — "approved on the basis of which evidence" is tamper-evident.
- **I-5 Default-deny + revocability.** A grant is dead when expired, over max-uses, or revoked; axon-os
  refuses it at the run boundary (exit 8) regardless of an otherwise-valid token. Revocation wins.
- **I-6 Determinism.** Same `(request, sorted-distinct-approvers, issue-tick)` under mock ⇒
  byte-identical token + provenance; no clock/random in core (logical tick only).
- **I-7 Backward compatibility.** A `Risk::Low` single-approve emits the R22 `axon-approval/1` token
  byte-for-byte; axon-os's existing `/1` path is unchanged. R24 only *adds* the `/2` branch.
- **I-8 One-way dependency.** `axon-intent` → `axon-os`, never the reverse; the `/2` token encoding is
  single-sourced in axon-os (its `TokenView`), so the two crates cannot drift.

**Edge cases (named, with resolution):**
- High grant approved by 3 when 2 required → token emitted; all 3 recorded in `approvers` + provenance
  (over-approval is fine, recorded).
- The same approver legitimately re-approves after editing nothing → counted once, a second provenance
  event still appended (the *log* records the repeat; the *threshold* dedups). No double-count.
- A token at `expires_at_tick == 0` (Low) never expires; a `/2` token with a positive `expires_at_tick`
  dies at `current_tick >= expires_at_tick`.
- A revocation marker present but the token edited to a different grant digest → still refused (the
  marker matches the *on-disk* grant's digest, computed fresh; you can't edit out a revocation).
- A `..` path in the request grant → rejected at parse (path traversal, fail closed), never resolved.
- Simulator unavailable / timeout → simulation renders with the explicit `RISK EVIDENCE UNAVAILABLE`
  marker as its sole `redteam` entry (NEVER empty), hashed into `evidence_digest` so the failed-open
  state is visible to the approver and recorded in provenance; the friction gate still enforces (the
  hard defense is the threshold, not the redteam text).
- An `axon-approval/2` token presented to an `axon-os` that only knows `/1` → unknown schema → refused
  (fail closed, never silently single-approved).

---

## §9 — Quickstart (`README-axon-intent-r24.md`; these exact commands are executed by `acc_a3`)
```bash
# Build
cargo build -p axon-intent --bin axon-intent
cargo build -p axon-os --bin axon-os

# 1. Pre-approval SIMULATION: see what a HIGH-risk grant would enable + the friction it triggers:
axon-intent simulate examples/grants/egress.request.md

# 2. One approver signs off — but a High-risk grant needs TWO; no token is emitted yet:
axon-intent approve ./jobs/egress.axjob --by alice --accept   # → "1/2 — multi-party approval required"

# 3. A second, distinct-name approver meets the threshold; NOW a token (with a TTL) is emitted:
axon-intent approve ./jobs/egress.axjob --by bob --accept     # → "THRESHOLD MET (2/2) — token emitted"

# 4. Run it under that multi-approved bound (axon-os honors the multi-sig token + its expiry):
axon-os run ./jobs/egress.axjob --out ./runs

# 5. REVOKE the grant; the next run is refused (default-deny wins, exit 8):
axon-intent revoke ./jobs/egress.axjob --by carol --reason "rotated creds"
axon-os run ./jobs/egress.axjob --out ./runs ; echo "exit=$?"

# 6. Watch a threshold-gaming attempt fail: the SAME approver twice does NOT reach the threshold:
axon-intent approve ./jobs/deploy.axjob --by alice --accept
axon-intent approve ./jobs/deploy.axjob --by alice --accept ; echo "exit=$?"   # → DuplicateApprover, exit 5
```

---

## §10 — Acceptance gate (pinned; FAILS if any check is missing or stubbed)

`scripts/r24_acceptance_gate.sh` is the single source of "done." It MUST:
1. **Presence check** — assert every §0 check name exists in the test sources:
   `acc_a1_smoke_simulate_to_revoke`, `acc_a2_example_grants_friction_and_expiry`,
   `acc_a3_quickstart_commands_execute`, `acc_a4_simulation_isolated_timeout`,
   `acc_a5_deterministic_grant_and_log`, `acc_a6_provenance_chain_tamper_detected`,
   `high_risk_grant_cannot_be_single_approved`, `multisig_threshold_gates_token`,
   `revoked_or_expired_grant_refused_at_run`, `duplicate_or_forged_approver_rejected`,
   `sock_puppet_distinct_ids_not_independent` (the KNOWN-GAP acknowledgement test). Missing →
   **gate fails**.
2. **Anti-stub check** — each acceptance test body has a real assertion and is not `#[ignore]`d /
   `todo!()` / `assert!(true)` (grep these anti-patterns → fail).
3. **Run** `cargo test -p axon-intent` (all green) + the §9 quickstart block against the built binaries
   (A3) + `acc_a1` driving the real CLI + the cross-spec `revoked_or_expired_grant_refused_at_run`
   (which drives `axon-os run`).
4. **Reproducibility** — run `acc_a5` twice and diff the emitted `{.approval,.provenance}` triples
   byte-for-byte.
5. **Backward-compat** — assert a `Risk::Low` single-approve still emits an `axon-approval/1` token
   byte-identical to R22 (run the R22 example through the R24 binary; diff).
6. Exit 0 only if all pass; else print which check failed. Wire into `gate.sh --strict`.

---

## §11 — Definition of Done
**Per slice (S1–S9):** the slice's named tests were written first, seen to fail, now pass; full
`axon-intent` + `axon-os` suites green; no workspace regression.
**Per milestone (R24 complete):** `cargo build -p axon-intent` produces `axon-intent` with the
`simulate`/`revoke`/multi-sig-`approve` surface; the R22 `--approvers N>1 → exit 2` stub is gone;
the real example grants demonstrate the friction ladder (Low single-approve, High needs 2, Critical
needs 3) end-to-end; **`acc_a1` passes through the real CLI**; multi-sig + provenance
tamper-evidence (`acc_a6`, `multisig_threshold_gates_token`,
`high_risk_grant_cannot_be_single_approved`, `duplicate_or_forged_approver_rejected`) hold;
expiry/revocation is refused at the axon-os run boundary (`revoked_or_expired_grant_refused_at_run`);
the Low path stays R22 byte-compatible; reproducibility (`acc_a5`) holds; and
`scripts/r24_acceptance_gate.sh` exits 0 with every §0 check green. Only then is the approval boundary
*defended*, not merely trusted.

---

## §12 — Notes for the implementer (do NOT deviate without updating this spec)
- **Reuse R22's `Risk`, `derive_risk`, `legible_bound`, `program_digest`/`grant_digest`, and the R22
  `ApprovalToken` for the Low path** (`crates/axon-intent/src/approval.rs`). Do not reimplement risk
  derivation or the grant digest — the grant encoding is already single-sourced in axon-os
  (`canonical_grant`, `crates/axon-os/src/approval.rs:26`).
- Keep `policy/multisig/provenance` **pure**. `std::fs`/`SystemTime`/`rand`/`std::env` belong only in
  `simulate.rs`/`cli.rs`. Use a **logical tick**, never wall-clock, in anything hashed (A5 / I-6).
- The `Simulator` trait is the seam: build S1–S5 against `MockSimulator`; only S6 touches a subprocess.
- **The expiry/revocation check lives in `axon_os::verify_approval`** (§4.8), NOT in axon-intent —
  axon-os owns the run boundary and **must not depend on axon-intent** (no cycle, I-8). The `/2` token
  field encoding is single-sourced in axon-os's `TokenView` (`crates/axon-os/src/approval.rs:42`),
  read by name from the JSON, exactly as R22 reads `risk`/`decision`.
- **Friction is the hard gate; the simulation/redteam is advisory** (§4.6). A simulator failure must
  never *lower* friction — below-threshold always refuses.
- **Default-deny + revocation wins** (I-5): you cannot edit a token to escape a standing revocation or
  an exhausted use-budget; the markers are matched against the *on-disk* content digests.
- The multi-sig token is **content-agreement among distinct *recorded names*, NOT authentication and
  NOT independence** (A6 / §1.2 RESIDUAL RISK). The property held is *cannot be approved by fewer than
  N distinct recorded names* — do **not** claim identity/PKI/independence R24 doesn't provide. N
  distinct fabricated ids from one console DO reach the threshold (the
  `sock_puppet_distinct_ids_not_independent` KNOWN GAP); HW-rooted per-approver signing is
  `VISION_OS.md` §5 / a later spec and is the only real fix.
- Keep the Low path **byte-compatible with R22** (I-7) so adoption is incremental: only friction-bearing
  (High/Critical, or TTL/max-uses-bearing) grants get the `axon-approval/2` token.
```