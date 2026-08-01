# Tech Spec — R24: The Defended Approval Boundary ("who can be PERSUADED")

**Spec ID:** `R24-defended-approval-boundary`
**Status:** 📝 Draft (2026-06-27; review corrections folded in 2026-07-31; second adversarial-pass corrections folded same day — evidence body persisted for co-signers, per-job tick store, quickstart compile step, chain-backed approvers; **third adversarial pass folded same day** — `policy_for` takes the Grant not just the Risk, run-boundary state fails CLOSED on absence, the executing-redteam evidence arm removed, evidence rendering sanitized, rate/aggregate friction added, two further KNOWN-GAP tests recorded)
> **Naming note (2026-07-31):** the bare id "R24" is **dual-claimed** — `R24-tee-target.md` exists in
> this directory (a known parallel-track ID collision, like R21/R22/R23). Always reference this spec
> by its full id, and its gate script is pinned as `scripts/r24_approval_acceptance_gate.sh`
> (qualified, not bare `r24_*`, so the tee track cannot collide).
**Implements:** `VISION_OS.md` §4.4 (the pillar "Who can be PERSUADED — the human approver as an
attack surface") and gap **G7** (the grounded gap list, `VISION_OS.md` §10), against the threat model in `VISION_OS.md` §1. R24 turns the
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
| **A2** | Real runnable example artifact (real grant-request files, not toys in the test dir) | §5.3, §7 | `acc_a2_example_grants_friction_and_expiry` |
| **A3** | Quickstart whose exact commands are executed by a test | §9, §7 | `acc_a3_quickstart_commands_execute` |
| **A4** | Hermetic, isolated simulation + canonical entrypoint, hard timeout | §4.6, §7 | `acc_a4_simulation_isolated_timeout` |
| **A5** | Deterministic (same request + approvers ⇒ byte-identical token + provenance digest) | §4.7, §7 | `acc_a5_deterministic_grant_and_log` |
| **A6** | Integrity: multi-sig threshold + append-only tamper-evident provenance log | §4.4, §4.5, §7 | `acc_a6_provenance_chain_tamper_detected` |
| **Core** | A high/critical-risk grant CANNOT be single-approved (friction is risk-proportional) | §4.3, §7 | `high_risk_grant_cannot_be_single_approved` |
| **Core** | Multi-sig: N distinct *recorded names* (name-distinctness, NOT independence — §1.2) below threshold → no valid token; at threshold → token | §4.4, §7 | `multisig_threshold_gates_token` |
| **Core** | A revoked OR expired grant is REFUSED at the run boundary (axon-os) | §4.8, §7 | `revoked_or_expired_grant_refused_at_run` |
| **Core** | A duplicate / forged approver signature does NOT count toward the threshold | §4.4, §7 | `duplicate_or_forged_approver_rejected` |
| **Core** | Run-boundary state (tick / uses / revocation / provenance) is FAIL-CLOSED on absence for a `/2` token — deletion cannot rewind or un-revoke | §4.8, §7 | `deleted_run_state_fails_closed` |
| **Core** | Friction is rate/aggregate-bearing, not only per-request: an approver's grant rate and a principal's live aggregate authority are capped | §3.3, §4.9, §7 | `rate_and_aggregate_friction_enforced` |
| **Core** | The evidence body is sanitized before it is printed, persisted and hashed — a grant request cannot paint the approver's terminal | §4.10, §7 | `evidence_body_sanitized_before_hash` |
| **Gap** | N distinct *fabricated* ids from one console are NOT independence — R24 does NOT defend this (recorded KNOWN GAP, needs HW signing) | §1.2, §7 | `sock_puppet_distinct_ids_not_independent` |
| **Gap** | Risk is derived from grant axes the *generator* authors — a Medium-labelled pure-egress grant still reaches single approval (recorded KNOWN GAP, needs finding-1 §3.3 raises + proof-carrying grants) | §1.2, §7 | `medium_risk_egress_single_approved_is_a_known_gap` |
| **Gap** | Friction is per-request: N single-approved Medium grants COMPOSE to a High capability (recorded KNOWN GAP, needs a live-grant union — §12 Q3) | §1.2, §7 | `composed_medium_grants_reach_high_capability` |
| **Gate** | The acceptance gate fails if any check above is missing/stubbed | §10 | `scripts/r24_approval_acceptance_gate.sh` |

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
2. **Risk-proportional friction** ("rate-limit grants, require multi-party / multi-sig approval for
   high-risk"). The Phase-11 risk already computed by `axon_intent::approval::derive_risk`
   (`crates/axon-intent/src/approval.rs:57`) drives an **approval policy**: `Risk::Low|Medium` ⇒
   threshold 1 (single approver, as today); `Risk::High` ⇒ threshold ≥ 2; `Risk::Critical` ⇒ threshold
   ≥ 3 (defaults; an explicit `--threshold N` may only RAISE, never lower, like R22's `--risk` floor).
   **CORRECTED 2026-07-31 (third pass) — the policy input is the GRANT, not just the `Risk` enum.**
   `derive_risk` is a **4-point function of grant-axis PRESENCE only** (exec→Critical, net∧fs→High,
   net∨fs→Medium, pure→Low, `approval.rs:57`), and the grant it reads comes from `grant_infer::infer`
   (`crates/axon-intent/src/grant_infer.rs:24`), which grants an axis **iff the program declares it**
   — and the program is the machine-generated artifact under review. Keying friction on that enum
   alone lets the party being gated **select its own approval threshold**: emit a program that touches
   only the network and it lands on Medium ⇒ threshold 1, TTL 10 000, 100 uses — single-approver
   capture of an *egress-bearing* grant, the very risk §1.5 lists first. The enum is also blind to
   **breadth** (`net: ["*"]` and `net: ["api.example.com"]` derive identically; `fs_read: ["/"]` and
   `fs_read: ["./data/"]` likewise) and drops **two of Phase-11's three axes** — `risk_derive`
   (`examples/stdlib/risk.ax:93`) takes effects **plus budget-burn % plus irreversibility**, and
   R22's Rust port kept only the first. So `policy_for` takes `(risk, grant, floor)` and RAISES on
   the axes the enum collapses (§3.3). This does **not** close the gap — the generator still authors
   the grant — it only removes the cheapest rungs; the residue is the recorded
   `medium_risk_egress_single_approved_is_a_known_gap` KNOWN GAP (§1.2).
   **Rate + aggregate friction (§4.9)** fills VISION §4.4's *first* friction bullet ("rate-limit
   grants"), which earlier drafts of this spec silently dropped while claiming the bullet list.
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

**Coverage of `VISION_OS.md` §4.4 / G7 (stated explicitly so a dropped bullet is a recorded gap, not
an implied completion):** *filled* — pre-approval simulation (1), multi-party/multi-sig for high risk
(2,3), rate-limited grants (2 + §4.9), decision provenance (4), default-deny + expiring/revocable
grants (5). *NOT filled* — approver **independence** (needs HW-rooted per-approver signing,
`VISION_OS.md` §5; §1.2 RESIDUAL RISK), a **faithful** risk label (the generator authors the grant the
label derives from; §1.2), and **per-coalition / cross-job aggregate** bounds (`VISION_OS.md` §4.5's
"multiple instances can split a forbidden computation"; §1.2 + §12 Q3). G7 is therefore *partially*
closed by R24, and §11's DoD says so conditionally.

The end-state: the R22 triple `{program.ax, job.axjob, job.approval}` gains, for a friction-bearing
grant, a `<name>.provenance` (the decision chain), a `<name>.simulation` (the persisted canonical
evidence body every co-signer is shown — §5.2) and a richer `<name>.approval` (multi-sig +
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
- **RESIDUAL RISK — the risk LABEL is authored by the party being gated (label-shopping).** §1.1(2)
  states the mechanism: `derive_risk` is a 4-point function of grant-axis presence, and the grant is
  inferred from effects the generated program declares. The §3.3 grant-aware raises make the cheap
  rungs expensive, but they do **not** make the label sound — a generator optimizing against the
  acceptance gate rather than the goal will still choose the smallest grant that achieves its
  objective, and any grant it can honestly fit inside Medium is single-approved. R24's claim is
  therefore *friction proportional to the **declared** grant*, **not** *proportional to consequence*.
  The `medium_risk_egress_single_approved_is_a_known_gap` test (§7) records this as an executing
  KNOWN GAP so it is visible in the gate. The real fix is **proof-carrying grants** (§12 Q2 —
  per-axis `proven_used`/`proven_unused` from the landed SMT/R20-attenuation/effect-row machinery),
  scoped as a future slice, not asserted here.
- **RESIDUAL RISK — friction is PER-REQUEST; grants COMPOSE across jobs.** The threshold is evaluated
  against one `GrantRequest` for one job; nothing in R24 aggregates authority across jobs, approvals,
  or time. Two Medium requests — job A with `fs_write: ./shared/`, job B with `fs_read: ./shared/` +
  `net: attacker.example` — are each threshold-1, each single-approved, and **compose into exactly the
  net+fs capability `derive_risk` calls High and I-1 forbids single-approving**. This is distinct from
  the out-of-scope "quorum across machines": same-machine composition is *in* R24's boundary and is
  *not* defended in this slice. `VISION_OS.md` §4.5 names the threat directly ("multiple instances can
  split a forbidden computation or pass authority between themselves… bounds are per-coalition, not
  just per-instance"). §4.9's aggregate cap raises the cost but is denominated in live-grant count and
  risk, not in the *union* capability; the union derivation is §12 Q3. Recorded as the executing
  `composed_medium_grants_reach_high_capability` KNOWN GAP.
- **STATED DEPENDENCY — the effect declaration the risk label rests on is a substring scan.** R24
  promotes `derive_risk`'s output from R22's *recorded-but-unenforced* field to *the determinant of how
  many humans must consent*, so the soundness of its input is now load-bearing. That input chain ends
  at `scan_effects` (`crates/axon-os/src/runtime.rs:218`), which is literally
  `source.contains("write_file")` / `contains("http_get")` over the **entry file's text only**, while
  the language resolves `mod` imports through `AXON_PATH`. Downgrading the scan downgrades friction
  *and* the runtime ceiling together, so it is not a straight escape — but the runtime ceiling is
  **coarser** than the static gate: `wrap_in_sandbox` (`runtime.rs:259`) buckets any of
  `fs_read`/`fs_write`/`exec` into one `IO` tag, so a grant of `fs_read: ./data/` alone yields Medium
  risk (single approver, TTL 10 000, 100 uses) **and** a runtime ceiling that also permits writes and
  process spawning. R24 therefore (a) names `scan_effects`'s effect-declaration soundness as an
  explicit assumption, (b) REQUIRES the run-boundary state files be unreachable from any grantable
  `fs_write` prefix (§4.8), and (c) prefers the checker's effect-row analysis (transitive
  anti-laundering, E1310) over the substring scan as the risk input where available (§12 Q1).
- **STATED LIMIT — R24 assumes a human actually reads each simulation, and that assumption expires
  with generation volume.** Every defense in §1.1 except TTL/max-uses/revocation is denominated in
  *human attention*: the approver reads `would_enable`, weighs the redteam findings, and decides. The
  project's own thesis (CLAUDE.md / ROADMAP: typed `.ax` is an **IR** for machine-generated programs)
  means request volume is set by generation throughput while approver attention is fixed, so the
  ratio of grants-per-approver-hour rises monotonically and review degrades toward rubber-stamping —
  the classic fatigue failure, and the one a persuader most wants. R24 does **not** solve this; it
  bounds it three ways and says so: the §4.9 **rate cap** makes unbounded request volume refuse rather
  than exhaust the approver, the §4.9 **repeat-vs-novel diff** makes a near-identical re-request
  visibly cheap and a *widened* grant visibly expensive, and §12 Q2 records proof-carrying grants as
  the way to move the approver's attention from prose (cheap to fake, cheaper every year) to a
  verdict (cheap to check). Treat "a human reviewed this" as a **budgeted resource** the policy
  spends, not as a free precondition.
- **No persuasion-detection ML.** R24 raises *friction* and records *provenance* so a manipulated
  approval is *visible after the fact*; it does **not** classify a prompt or an approver's reasoning as
  manipulative. That is a model/monitor concern (`VISION_OS.md` §4.5), out of scope.
- **No new capability / grant / effect / risk types.** R24 imports R22's `Risk`/`ApprovalRequest` and
  R21's `Grant`/`DeclaredEffects` verbatim. The Phase-11 `derive_risk` is reused, not reimplemented.
- **No execution / enforcement / sandboxing.** R24 never runs the program (R21 does). R24's job ends at
  the friction-gated, provenance-bearing, expiring triple. The **only** runtime addition is the
  expiry/revocation check inside `axon_os::verify_approval` (§4.8) — and that lives in axon-os.
  **NORMATIVE, CORRECTED 2026-07-31 (third pass):** this rule binds R24's **evidence path** too. An
  earlier draft let `RealSimulator::redteam_and_risk` invoke "the Phase-12 redteam check"; in this
  tree `cmd_redteam` (`crates/axon-core/src/main.rs:5636`) calls `axon_core::interp::run_program` —
  it **runs the whole untrusted program** with no manifest, no grant and no sandbox, *before* approval
  and *outside* any grant, which is the strictly worst place to run it. That arm is **REMOVED** (§4.1
  step 3 / §4.6): R24's evidence is **non-executing only**. If a future slice wants executing
  evidence it MUST go through `axon_os::run_sandboxed` under the **requested** grant with a recorded
  `RunRecord` — never a bare `axon redteam`.
- **No wall-clock time in the trusted/hashed region.** Expiry uses a **logical tick** (a monotone
  counter supplied at the run boundary, defaulting to the **persisted per-job tick (`<name>.tick`)
  that advances at every run-boundary check** — §4.8), so the provenance chain and tokens stay deterministic (A5),
  mirroring R21's "no timestamp in the hashed record" rule — while TTLs still genuinely lapse in
  real operation.
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
- **Label-shopping** (the generator picks the rung it will be judged on). → `policy_for` reads the
  **Grant**, raising on egress, wildcard/root scope, cost and irreversibility (§3.3); residual =
  `medium_risk_egress_single_approved_is_a_known_gap` (§1.2).
- **Grant decomposition** (N cheap Medium jobs compose to a forbidden High capability). → the §4.9
  aggregate cap on simultaneously-live authority; residual =
  `composed_medium_grants_reach_high_capability` (§1.2, §12 Q3).
- **Run-state rollback** (delete `<name>.tick`/`.uses`/`.revoked` to resurrect a dead grant). → the
  run-boundary state is chained into `<name>.provenance` and **absence is a REFUSAL** for a `/2`
  token, never a permissive zero (§4.8); `deleted_run_state_fails_closed`.
- **Evidence as an attack channel** (the mandated verbatim body paints the approver's terminal or
  forges the FRICTION line). → the sanitize-then-hash rule (§4.10);
  `evidence_body_sanitized_before_hash`.
- **Self-attested evidence** (the program authors — or executes — the findings the human signs). →
  the executing redteam arm is removed; evidence is grant-derived and non-executing (§1.2, §4.6), and
  "no adversarial check exists" renders as its own explicit marker, never as a silent pass (§4.1).
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
  summarize.intent.md      R22 intent the LOW-risk job triple is compiled from (AXON_AI_MOCK=1).
  egress.intent.md         R22 intent the HIGH-risk (net+fs) job triple is compiled from.
  deploy.intent.md         R22 intent the CRITICAL (exec) job triple is compiled from.
  summarize.request.md     A real LOW-risk grant request → single-approve, short TTL (A2).
  egress.request.md        A real HIGH-risk grant (net+fs) → REQUIRES 2 approvers (A2).
  deploy.request.md        A real CRITICAL grant (exec) → REQUIRES 3 approvers (A2 negative-friction).
scripts/r24_approval_acceptance_gate.sh
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
`canonical_grant`/`verify_approval` (re-exported at `crates/axon-os/src/lib.rs:29` as of
2026-07-31), and R24 adds the **TTL /
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
    irreversible:   bool,          // §3.3: the operator's "this cannot be undone" mark; raises a tier. Default FALSE, and
                                   // ALWAYS true when the grant carries exec:any or fs_write outside the job out-dir (fail closed)
}
```
`.request.md` serialized form (the parser reads exactly these headers; `program`/`grant` may instead
be supplied as `--job <name.axjob>`):
```markdown
# Grant Request
Deploy the egress summarizer to production.

## Program
- ./jobs/egress.ax

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

## Irreversible
- true                  # optional; default false. May only RAISE risk (§3.3); an exec/out-of-out-dir-write
                        # grant is treated as irreversible regardless of what the file says.
```
The SHIPPED example request files (§2, A2) point `## Program` at the `./jobs/<name>.ax` the §9
step-0 R22 compile produces — the repo ships *intents* (compiled deterministically under
`AXON_AI_MOCK=1`), not loose orphan `.ax` programs, so `## Program` is resolvable on disk after
that step. Validation (fail → `FrictionRefusal::Malformed`, exit 2): `# Grant Request` present; a resolvable
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
    redteam:       Vec<String>,  // NON-EXECUTING findings from the grant-derived evidence path (§4.6); NEVER empty — see the markers below
    threshold:     u32,          // approvers REQUIRED for this risk (the friction it triggers)
    novelty:       String,       // "first grant for this job" | "identical to the last approved grant" | "WIDENED vs the last approved grant: +net api.example.com" (§4.9)
    evidence_digest: String,     // "axev1:"+sha256(canonical(SANITIZED legible, would_enable, risk, redteam, threshold, novelty))
}
```
`evidence_digest` is the **single hash of exactly what was presented**. It is bound into every
`ProvenanceEvent` (§3.4) so "approved on the basis of which presented evidence" is itself
tamper-evident: if the simulation text differs from what an approver saw, the chain reveals it.
It is computed over the **sanitized** rendering (§4.10), so the digest can never bind text different
from what the terminal showed.

**`redteam` is never empty and never vacuous (normative).** R24 already handles evidence that is
*unavailable*; the third pass added the case where it is *vacuous*. Exactly one of these markers is
present when there are no genuine findings, and it is hashed in like any other:
- `"RISK EVIDENCE UNAVAILABLE — adversarial findings did not run"` (timeout / non-zero exit /
  `AXON_BIN` unset — §4.6).
- `"NO ADVERSARIAL CHECK EXISTS IN THIS PROGRAM"` — the program declares no `redteam_check`. Earlier
  drafts rendered this identically to "the adversarial check found nothing" (`cmd_redteam` prints
  `no redteam_check function found (pass)` / `"status":"no_redteam_fn","caught":false`,
  `crates/axon-core/src/main.rs:5636`), so **omitting the function was the generator's cheapest way
  to look clean**. The two now render and hash differently. (`acc_a4` asserts this arm alongside the
  timeout and unset-`AXON_BIN` arms.)

**Field provenance is rendered, not implied (normative).** `legible`, `would_enable`, `risk`,
`threshold` and `novelty` are computed *from the grant* and are trustworthy; `intent_goal` and any
`redteam` finding text are **model-authored or model-influenced** and are rendered quoted, labelled
untrusted, and visually separated (§4.10). An approver must never have to guess which half of the
page the program under review wrote.

### 3.3 `ApprovalPolicy` (risk → friction; pure, deterministic — §4.3)
```
ApprovalPolicy {
    threshold:      u32,   // distinct recorded-name approvers REQUIRED before a valid token (name-distinctness, NOT independence)
    ttl_ticks:      u64,   // default token lifetime (logical ticks); 0 = no expiry only for Low
    max_uses:       u64,   // default max run-boundary uses; 0 = unlimited only for Low
    max_per_window: u32,   // §4.9 rate cap: grants this approver may approve per `rate_window_ticks`; 0 = uncapped (Low only)
    rate_window_ticks: u64,// §4.9 rate window (logical ticks)
    max_live_grants:   u32,// §4.9 aggregate cap: simultaneously-live (unexpired, unrevoked) grants of this risk per principal; 0 = uncapped (Low only)
}
fn policy_for(risk: Risk, grant: &Grant, floor: Option<u32>) -> ApprovalPolicy
```
**CORRECTED 2026-07-31 (third pass) — `policy_for` takes the GRANT, not just the `Risk`.** The
rationale is §1.1(2): the `Risk` enum is a 4-point function of axis presence that the generated
program itself selects, and it is blind to scope breadth, budget burn and irreversibility (two axes
Phase-11's own `risk_derive`, `examples/stdlib/risk.ax:93`, carries and R22's Rust port dropped).
Passing the `Grant` lets the ladder raise on what the enum collapses.

**Base defaults by risk (the friction ladder; every raise below is applied ON TOP, and `--threshold`
may only RAISE, never lower):**
| Risk | threshold | ttl_ticks | max_uses | max_per_window | max_live_grants |
|---|---|---|---|---|---|
| Low | 1 | 0 (no expiry) | 0 (unlimited) | 0 | 0 |
| Medium | 1 | 10000 | 100 | 20 / 1000 ticks | 10 |
| High | **2** | 1000 | 10 | 5 / 1000 ticks | 3 |
| Critical | **3** | 100 | 1 | 2 / 1000 ticks | 1 |

**Grant-derived raises (normative; each is a `max`, never a lowering — apply all, take the strongest):**
| Condition on the grant | Effect |
|---|---|
| `net` non-empty (any egress at all) | `threshold = max(threshold, 2)`, `ttl_ticks = min_nonzero(ttl, 1000)`, `max_uses = min_nonzero(uses, 10)` — irreversible external disclosure (VISION §4.2) never sits on the single-approve rung |
| any axis scope is a wildcard (`*`, a leading-`*` host glob) or a root prefix (`/`, `./`) | raise the effective risk **one tier** before the table (breadth is not free) |
| `budget.cost_micro` ≥ 1 000 000, **or** ≥ 50% of the intent's stated ceiling | raise one tier (Phase-11's budget-burn axis, restored) |
| the request declares `irreversible: true` (§3.1), or the grant carries `exec: any` or `fs_write` outside the job's own out-dir | raise one tier (Phase-11's irreversibility axis, restored — clamped at Critical) |

`min_nonzero(a,b)` = `b` when `a == 0` else `min(a,b)` (so an "unlimited" default can be tightened but
a finite bound is never loosened). Tier raises clamp at `Critical`, exactly like `risk_clamp` in
`risk.ax`. **This tightens the ladder; it does not make the label sound** — the generator still
authors the grant (§1.2 label-shopping RESIDUAL RISK).

The threshold floor (`policy_for(risk, grant, Some(n))`) sets `threshold = max(effective, n)` —
friction can be *demanded* but never *waived*. (This is the same "floor only raises" discipline as
R22's `--risk`, `crates/axon-intent/src/approval.rs:196`.)

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
  | RateLimited { who, have, window }    → exit 5   // §4.9 per-approver grant rate cap reached (policy refusal)
  | AggregateExceeded { live, cap }      → exit 5   // §4.9 too many simultaneously-live grants for this principal
  | RunStateMissing { which }            → exit 8   // §4.8(b) tick/uses/revocation/provenance absent or rolled back for a /2 token
  | EvidenceMismatch { detail }          → exit 8   // approver signed over a different simulation than presented
  | ProvenanceTampered { which }         → exit 11  // the decision chain doesn't verify (shares the carved tamper code)
  | Revoked { reason }                   → exit 8   // the grant was revoked
  | Expired                              → exit 8   // the token's TTL/max-uses lapsed
```
Exit 0 = a friction-satisfied token (+ provenance) was emitted, or a valid token verified at run.
(5 reuses Axon's AI-policy/"policy refusal" band as R22 does; 8 reuses the carved
sandbox/capability/handoff code; **11** reuses the carved tamper/divergence code.
**CORRECTED 2026-07-31:** this spec originally assigned `ProvenanceTampered → exit 9`, citing R21's
tamper code — true when R21 was written, but R27 renumbered the carved scheme: 9 is now
`ResourceBound`, 10 `CoalitionBound`, and tamper/divergence (`VerifyMismatch`) moved to **exit 11**
(`crates/axon-os/src/verdict.rs`, "Moved from 9 to free it for R27"). Minting 9 here would collapse
two distinct outcomes onto one code — exactly what the carved scheme forbids.)

**Where `BelowThreshold`'s exit 5 surfaces (normative):** `BelowThreshold` is a *library-level*
refusal — `multisig::build_token` returns it when handed a **complete** approval set short of the
threshold (§4.4 step 3), and a caller treating the set as final maps it to exit 5. The CLI's
*accumulating* `approve` path (§5.2) treats below-threshold-so-far as a **partial success (exit 0)**
and never maps `BelowThreshold` to a CLI exit — the two are different callers of the same refusal.

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
   in the real impl a bounded subprocess over `axon-os explain` (the legible gate verdict —
   `cmd_explain`, `crates/axon-os/src/cli.rs`, arm at :141 / fn at :160 as of 2026-07-31) plus the
   **static** declared-effects-vs-grant comparison. **CORRECTED 2026-07-31 (third pass): the
   "and/or the Phase-12 `redteam` check" clause is DELETED.** `cmd_redteam`
   (`crates/axon-core/src/main.rs:5636`) runs the untrusted program through
   `axon_core::interp::run_program` with no manifest, grant or sandbox — so the earlier wording made
   R24's own evidence path *execute the artifact under review, before approval, outside any grant*,
   falsifying §1.2's "R24 never runs the program". Worse, if the generator *does* supply
   `redteam_check`, it **authors the adversarial findings** §5.2 then re-prints verbatim to every
   co-signer as authoritative evidence they bind their signature to — self-attested safety evidence in
   the one artifact whose purpose is to defend the human. R24's evidence is therefore **non-executing
   and grant-derived only**; a program that declares no adversarial check gets the explicit
   `NO ADVERSARIAL CHECK EXISTS IN THIS PROGRAM` marker (§3.2), never a silent pass. The *friction*
   below is the
   hard gate, not the redteam text, so evidence-gathering fails *open* on the gate — **but it must NOT
   fail open silently.** On any failure (timeout / non-zero exit / unavailable), `redteam` is set to
   the single explicit marker `["RISK EVIDENCE UNAVAILABLE — adversarial findings did not run"]`
   (NEVER left empty), and that marker is hashed into `evidence_digest` (step 5) and surfaced in the
   simulation. A persuader thus cannot present a clean-looking simulation whose adversarial findings
   silently failed open: the absence of evidence is itself stamped into the bound, tamper-evident
   evidence the approver sees and the provenance chain records.
4. `threshold = policy_for(req.risk, &req.grant, floor).threshold` (§3.3 — the Grant, not just the Risk).
5. Compute `novelty` (§4.9): diff this grant against the last approved grant for the same job family.
6. **Sanitize** every rendered field per §4.10 — the sanitized form is what is printed, persisted and
   hashed.
7. `evidence_digest = "axev1:"+sha256(canonical(sanitized legible, would_enable, risk, redteam,
   threshold, novelty))`.
8. Return the `Simulation`. **No decision is recorded here** — simulation is read-only, and (per step
   3) never executes the program.

### 4.2 Risk-proportional policy (`policy::policy_for`) — Core, pure, deterministic
Implements the §3.3 table **over `(risk, grant, floor)`**. Order: (1) apply the §3.3 grant-derived
*tier raises* (breadth / cost / irreversibility) to get the effective risk, clamped at Critical;
(2) look up the base row; (3) apply the §3.3 per-axis raises (egress ⇒ threshold ≥ 2 and finite
TTL/uses); (4) `threshold = max(effective, floor.unwrap_or(0))`. Every step is a `max`/`min_nonzero`,
so the function is **monotone: no input can ever LOWER friction**. **Pure: no clock, no env** — the
grant is a value, not a file read. Tested by `policy_ladder_is_monotone` (threshold non-decreasing in
risk; the floor only raises; every grant-derived raise only raises) and by
`egress_grant_never_single_approved` (any non-empty `net` ⇒ threshold ≥ 2, at every risk tier).

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
               evidence_digest:&str, log:&ProvenanceLog, issue_tick:u64)
               -> Result<TokenOut, FrictionRefusal>   // issue_tick feeds step 5's expires_at_tick (matches §5.1)
Approval { approver: String, decision: Decision, evidence_digest: String }
TokenOut = Single(ApprovalToken /*R22 axon-approval/1*/) | Multi(MultiApprovalToken /*axon-approval/2*/)
```
Steps (each a named failure case):
1. **Filter to approvals over the presented evidence.** Any `approval.evidence_digest !=
   evidence_digest` → `EvidenceMismatch` exit 8 (the approver acted on a *different* simulation than
   the one bound to this request — a substitution attack is caught).
2. **De-duplicate approvers.** Reduce to **distinct** approver ids that voted `Approved`. **The one
   duplicate rule (normative; CORRECTED 2026-07-31 — earlier drafts stated conflicting behaviors):**
   a repeated approver id **within a single `build_token` approval set** (i.e. one invocation
   submitting `["alice","alice"]` to inflate the count) → `DuplicateApprover` exit 5, no token
   (`duplicate_or_forged_approver_rejected`). A repeated id **across separate CLI `approve`
   invocations** (the same approver legitimately re-approving) is **benign**: exit 0, a new
   `ProvenanceEvent` is appended (the log records the repeat), and the id is **counted once** toward
   the threshold (the tally dedups) — never an error, never a double-count. The distinguishing
   criterion is the submission boundary: one set vs. separate acts. **Division of labor (normative
   — `build_token` receives only one final set and cannot itself observe that boundary):** the
   CLI's accumulating `approve` path (§5.2) reduces the provenance log's `Approved` events to
   DISTINCT approver ids BEFORE calling `build_token`; the `DuplicateApprover` arm is therefore
   unreachable from the CLI and guards exactly one thing — a raw approval set handed directly to
   the library with a repeated id (the `["alice","alice"]`-in-one-call inflation attack).
   (Name-distinctness — NOT
   independence (§1.2) — is by
   distinct id; forging "more approvers" by *repeating* one is the modeled attack. Forging more
   approvers by submitting N *distinct fabricated* ids is the UN-modeled
   `sock_puppet_distinct_ids_not_independent` KNOWN GAP — R24 does not defend it.)
3. **Threshold gate.** If `distinct_approved.len() < policy.threshold` → `BelowThreshold{have,need}`
   exit 5, **no token emitted** (default-deny). (`multisig_threshold_gates_token`.)
4. **Provenance binding.** Require `log.verify()` Ok and `log.chain_digest` over the *same*
   `(program_digest, grant_digest)`; **AND require every distinct approver counted toward the
   threshold in step 3 to appear as an `Approved` `ProvenanceEvent` in that log with the same
   `evidence_digest`** — without this cross-check a direct library caller could pass
   `approvals=["ghost1","ghost2"]` plus any valid chain over the same digests (e.g. one holding
   only a rejection) and mint a token whose `approvers` list is unrecorded in the very chain it
   "binds", hollowing I-3's "who approved what is visible after the fact". Either failure →
   `ProvenanceTampered` exit 11 (§3.7 — exit 9 belongs to R27 `ResourceBound` post-renumbering);
   then bind `provenance_digest = log.chain_digest`.
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
  `AXON_BIN`, **not** ambient PATH; `AXON_BIN` unset or non-absolute ⇒ evidence-gathering is
  UNAVAILABLE and the simulation takes the same `RISK EVIDENCE UNAVAILABLE` marker path as a
  timeout — NEVER a fallback to ambient PATH, which would let a planted binary author the evidence
  the approver signs; `acc_a4` asserts this arm too) running **`axon-os explain` plus the static
  declared-effects-vs-grant comparison ONLY — never `axon redteam`, never any step that executes the
  program** (§4.1 step 3 / §1.2, corrected third pass) in a
  **fresh subprocess**, wrapped in a **hard timeout** (`AXON_INTENT_SIM_TIMEOUT_MS`, default 30000); on
  expiry the child group is killed (RAII guard) and the simulation returns with `redteam` =
  `["RISK EVIDENCE UNAVAILABLE — adversarial findings did not run"]` (§4.1 step 3 — the marker is
  hashed into `evidence_digest`, so a failed-open simulation is *visibly* stamped, never silently
  clean; the *friction* gate in §4.3 is the hard defense, the simulation is advisory). No leaked
  handles; minimal explicit environment. (`acc_a4_simulation_isolated_timeout` asserts a hanging
  simulator is killed, produces no leaked child, AND that the rendered simulation carries the
  RISK-EVIDENCE-UNAVAILABLE marker; and that a program with **no** `redteam_check` renders the
  distinct `NO ADVERSARIAL CHECK EXISTS IN THIS PROGRAM` marker — §3.2 — rather than an empty or
  clean-looking findings list.)

### 4.7 Determinism (A5)
- The **only** volatile inputs are: the issue tick (`--issue-tick`, default the persisted per-job
  tick `<name>.tick` of §4.8 — a recorded monotone counter, never clock/random in core), the approver ids/order (the gateway sorts distinct approvers before hashing so
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
   The caller (the `verify_approval` call site in `crates/axon-os/src/cli.rs`, at :251 as of
   2026-07-31) supplies `current_tick`, `uses_so_far` (read from a tiny `<name>.uses` counter next to
   the token, incremented after a successful run), and `revoked` (true iff a `<name>.revoked` marker
   matching the token's digests exists).
   **Default monotone tick source (normative; added 2026-07-31, RELOCATED same day — an earlier
   draft pinned it at `<out-dir>/store.tick`, but `--out` is chosen per invocation by the very party
   being defended against: a fresh/different `--out` would start the counter at 0 and resurrect an
   expired token):** the persisted **monotone tick counter** lives NEXT TO THE JOB ARTIFACTS as
   `<name>.tick` (a bare u64, sibling of `<name>.uses`/`<name>.revoked`, which already survive
   out-dir changes) — one job, one tick store, located from the job path, NEVER from the
   caller-chosen `--out`. It is **incremented by 1 at every run-boundary check** of that job (before
   `verify_approval` is consulted, alongside the `.uses` bump). `current_tick` defaults to this
   persisted counter; `--tick T` may only **RAISE** it (the effective tick is the max of flag and
   persisted value, and the raised value is written back — the floor-only discipline, so an
   operator/test can jump forward but never rewind a token back to life). Thus a High-risk token
   with `ttl_ticks = 1000` genuinely dies after at most 1000 run-boundary checks even when no one
   ever passes `--tick`, and a fresh `--out` directory cannot revive it
   (`revoked_or_expired_grant_refused_at_run` arm (d)); ticks are still never wall-clock (A5/I-6).
   The same `<name>.tick` file is what `axon-intent approve` reads as the default `issue_tick` /
   provenance `tick` when `--issue-tick` is not given (§4.7, §5.2) — approve locates it from the
   `<name.axjob>` argument it already takes (absent file = tick 0 **only when no `/2` token exists
   yet**; see the fail-closed rule below). This shared per-job file is the
   ONE deliberate file-level coupling between the two CLIs; the pure cores stay uncoupled and the
   crate dependency direction is unchanged (I-8) — tests pass explicit values for byte-exact
   determinism. The shared `/2` field encoding is
   **single-sourced in axon-os** (the `TokenView` struct, `crates/axon-os/src/approval.rs:42`, gains
   the new fields) so axon-intent and axon-os cannot drift — exactly the R22 pattern.

   **ABSENCE IS A REFUSAL — the run-boundary state is chained, not ambient (normative; added
   2026-07-31, third pass; this is the single most load-bearing correction in this pass).** §1.2
   concedes that multi-sig buys nothing against the modeled adversary, so the guarantees that actually
   survive are **TTL, max-uses and revocation** — and as drafted all three were enforced by the mere
   *presence* of unauthenticated plain sibling files with **permissive absence defaults**, in a spec
   whose §1.4 says "Fail closed everywhere":
   - deleting `<name>.tick` rewound the counter to 0 and resurrected a token whose
     `expires_at_tick = issue_tick + ttl`;
   - deleting `<name>.uses` restored the full use budget;
   - deleting `<name>.revoked` un-revoked the grant (§3.6's "a revocation cannot be un-done by editing
     the token" is true and beside the point — it could be un-done by removing the marker);
   - deleting `<name>.provenance` erased the decision chain entirely, since nothing anchored the chain
     head outside that file.

   This is the *same* class of bug the spec already reasoned about once, when it relocated the tick off
   the caller-chosen `--out` ("a fresh/different `--out` would start the counter at 0 and resurrect an
   expired token") — rollback-by-deletion is that bug's sibling and was left open. The exploit path is
   conditional but real: the approved program may itself hold `fs_write` (the runtime sandbox buckets
   fs_read/fs_write/exec into one `IO` tag, `crates/axon-os/src/runtime.rs:259`), and a persuader whose
   entire premise is influencing the operator can simply get the jobs directory "cleaned". The rules:

   (a) **Chain the run-boundary state.** At **every** run-boundary check, axon-os appends a
   `ProvenanceEvent` of kind `"run"` to `<name>.provenance` carrying `(tick, uses_after,
   revoked: bool)` and the token's `token_digest`, chained exactly as §4.5 (this is the one place
   axon-os *writes* the chain; it uses the same pure `append` encoding, single-sourced in axon-os
   alongside `TokenView`, so no dependency direction changes — I-8 holds). The `.tick`/`.uses` files
   remain as a fast path, but the **chain is authoritative**: on disagreement the chain's higher value
   wins (monotone, floor-only, exactly the `--tick` discipline).

   (b) **Absence fails closed for a `/2` token.** If a `/2` `.approval` exists and `<name>.provenance`
   is missing, shorter than the chain length recorded in the token, or does not contain the token's
   `provenance_digest` as an ancestor ⇒ `Verdict::Denied` exit 8, reason `"approval run-state missing
   or rolled back"`. A missing `.tick`/`.uses` is likewise a refusal (not a zero) whenever a `/2`
   token is present; the permissive "absent file = tick 0" default survives **only** for the
   pre-approval path where no `/2` token exists yet. `deleted_run_state_fails_closed` asserts each of
   the four deletions (tick / uses / revoked / provenance) independently.

   (c) **Externally anchor the head.** The chain head is additionally recorded into the already-landed
   R28 chained ledger (`axon-os audit verify --ledger`) / the R21 `RunRecord`, so **whole-directory
   deletion is visible** rather than indistinguishable from a job that never ran. R24 requires the
   write and the refusal-on-mismatch; the ledger itself is R28's and is not re-specified here.

   (d) **The state files must not be grantable.** `<name>.tick` / `.uses` / `.revoked` / `.provenance`
   / `.approval` / `.simulation` MUST live outside any prefix a grant can name, and a request whose
   `fs_write` covers the job-artifact directory is **refused at parse** (`Malformed`, exit 2) beside
   the existing `..`-traversal check (§3.1). Otherwise the approved program can edit the very state
   that bounds it — a direct consequence of the coarse `IO` bucket noted in §1.2.

### 4.9 Rate + aggregate friction (`policy::rate_gate`) — VISION §4.4's first bullet, fail-closed
`VISION_OS.md:191` reads "**Friction proportional to risk:** rate-limit grants, require multi-party /
multi-sig approval for high-risk". Earlier drafts implemented only the second half while §1.1 claimed
the bullet list — the dropped half is the one this project's own thesis makes load-bearing, because
typed `.ax` is an **IR for machine-generated programs**: request volume scales with generation
throughput while approver attention is fixed, so friction denominated purely in *count of names* has a
marginal cost of three strings and three keystrokes. (The spec's own artifacts show the shape of the
problem: §9 steps 2–3 and §5.3's walkthrough have alice and bob approving from the same shell in
sequence — the canonical demonstrated UX *is* the sock-puppet path.) Three additions, all pure over
explicitly-passed state, all enforced at the same fail-closed point as the threshold (§4.4 step 3):

1. **Per-approver rate cap.** Counting `Approved` events in the chained log within the trailing
   `rate_window_ticks` (§4.8's logical tick — no wall-clock, A5/I-6): if this approver has already
   approved `max_per_window` grants in the window ⇒ `RateLimited{who, have, window}` exit 5, **no
   token**. The operator's recourse is to wait out the window or bring another approver — never a
   flag that waives it (floor-only discipline).
2. **Aggregate live-authority cap.** Before emitting, count the principal's **simultaneously-live**
   (unexpired, unrevoked, un-exhausted) `/2` grants at this effective risk; if emitting would exceed
   `max_live_grants` ⇒ `AggregateExceeded{live, cap}` exit 5, no token. This raises the cost of the
   decomposition attack (§1.2) without yet computing the *union* capability — that is §12 Q3, and the
   residue is recorded by `composed_medium_grants_reach_high_capability`.
3. **Repeat-vs-novel diff (`Simulation.novelty`, §3.2).** Diff the requested grant against the last
   approved grant for the same job family and render one of: *first grant for this job* / *identical
   to the last approved grant* / **`WIDENED vs the last approved grant: +net api.example.com`**. Without
   this, approving the 200th near-identical request costs the same review effort as the first — which
   is precisely how fatigue is manufactured, and how a widened grant hides in a familiar-looking
   stream. A **widening** diff is also a §3.3 tier raise input: a grant that widens a previously
   approved one is never *cheaper* than the one it replaces.

`rate_gate` is **pure**: the live-grant set and the windowed approval counts are passed in by `cli.rs`
(which reads them from the chained state of §4.8). Tested by `rate_and_aggregate_friction_enforced`.

### 4.10 Evidence rendering discipline (`simulate::render`) — sanitize, THEN hash
§5.2 requires the full `<name>.simulation` body be PRINTED to the first approver and RE-PRINTED to
every later co-signer ("never a bare hash"), and I-4 makes that normative. But `Simulation` carries
`intent_goal` (free prose), `redteam: Vec<String>` and grant strings (hosts, path prefixes) the
generator influences — so **the artifact R24 invented to defend the approver is also a mandatory,
verbatim channel from the generator to the approver's terminal**. Un-disciplined, it can spoof the
`FRICTION: requires 2 distinct approvers` line, clear or reposition the screen so the real bound
scrolls away, or address the approver in a second voice. Hashing an attack renders it faithfully; it
does not neutralize it. Normative rules:

- **Strip** all C0/C1 control characters (except `\n` between fields), all ANSI/CSI escape sequences,
  and all Unicode bidi-override / directional-isolate codepoints (U+202A–U+202E, U+2066–U+2069).
- **Cap** each field at a fixed length (default 2 000 bytes per string, 40 entries per list) with an
  explicit visible truncation marker; over-long input is truncated, never dropped silently.
- **Label and frame:** grant-derived facts (`legible`, `would_enable`, `risk`, `threshold`, `novelty`)
  render in the trusted block; `intent_goal` and `redteam` render **quoted, in a block explicitly
  headed `UNTRUSTED — authored by the program under review`**. The `FRICTION:` line is emitted by R24
  *after* the untrusted block and derived from `policy_for`, never from any request field.
- **Sanitize before persisting, printing AND hashing** — the sanitized rendering is the one written to
  `<name>.simulation`, shown to every approver, and fed to `evidence_digest` (§3.2 step 6/7), so the
  digest can never bind text different from what the terminal displayed.

Tested by `evidence_body_sanitized_before_hash` (a request whose `intent_goal`/redteam text carries
ANSI escapes, a bidi override, and a forged `FRICTION: requires 1 distinct approver` line: assert the
escapes are gone, the forged line is inside the UNTRUSTED block, the real FRICTION line is R24's, and
the digest matches the sanitized bytes).

---

## §5 — Public API / interface contract

### 5.1 Library API (added to `crates/axon-intent/src/lib.rs`; R22's API stays)
```
pub fn policy_for(risk: Risk, grant: &Grant, threshold_floor: Option<u32>) -> ApprovalPolicy;  // the GRANT, not just the Risk (§3.3)
pub fn rate_gate(policy:&ApprovalPolicy, approver:&str, window_approvals:u32,
                 live_grants:u32) -> Result<(), FrictionRefusal>;                              // §4.9, pure
pub fn render_simulation(sim:&Simulation) -> String;                                           // §4.10 sanitize-then-hash; the ONLY rendering
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
                    [--issue-tick T]
    Record ONE approver's decision. Appends a ProvenanceEvent to <name>.provenance (creating it). If the
    accumulated DISTINCT approvers ≥ the risk-derived threshold, ALSO emits/updates <name>.approval (a
    MultiApprovalToken for threshold>1, else the R22 token). Prints "✓ approved by OP (1/2) — 1 more
    approver required" or "✓ THRESHOLD MET (2/2) — token emitted, expires in 1000 ticks". A High/Critical
    grant with one approver prints "⏳ 1/2 — multi-party approval required (risk High)" and writes NO
    token. Exit 0 (recorded) / 3 (rejected). Below threshold so far is NOT an error — it's a partial,
    exit 0. A repeated `--by` id across invocations is ALSO exit 0: appended to provenance, counted
    once (§4.4 step 2's one duplicate rule); only a duplicate WITHIN one submitted approval set (the
    library path) is `DuplicateApprover` exit 5.
    Tick source: the ProvenanceEvent's `tick` comes from `--issue-tick T` when given, else the
    persisted per-job tick `<name>.tick` located from the `<name.axjob>` argument (§4.8; absent
    file = tick 0) — never wall-clock.
    Evidence binding (normative; added 2026-07-31, STRENGTHENED same day — I-4 is hollow without it,
    and stays hollow for approvers 2..N if they are shown only a hash): the request's CANONICAL
    evidence is established exactly once and persisted as BOTH a digest and a body. On the FIRST
    `approve` for a request the CLI runs the §4.1 simulation ONCE (via the same `simulate.rs`
    Simulator seam — cli → simulate is already an edge in the §2 graph, no new coupling), PRINTS the
    full simulation to the approver (they see exactly what they bind — never a silent recompute),
    persists the rendered Simulation JSON as `<name>.simulation` next to the provenance, and binds
    its digest into the seq-0 ProvenanceEvent's `evidence_digest`. A `--evidence DIGEST` on the
    first approve is accepted ONLY if it equals the digest of that locally computed simulation —
    else exit 2 `Malformed` ("evidence digest does not match the canonical simulation"); an
    arbitrary, unverifiable digest can never become the canonical evidence (it would make the chain
    tamper-evident about nothing). EVERY later `approve` re-reads `<name>.simulation`, RE-HASHES it
    and asserts the hash equals the persisted canonical digest (a mismatch → `EvidenceMismatch` exit
    8 — the on-disk evidence body was swapped after the first signature), and PRINTS the full
    simulation body to the approver before recording their decision — every co-signer sees the same
    canonical evidence TEXT they endorse, never a bare hash and never a fresh recompute (so a live
    simulator drifting between alice's and bob's invocations cannot manufacture a spurious
    mismatch: later approvals never re-simulate). A later `--evidence` that differs from the
    persisted canonical digest → `EvidenceMismatch` exit 8 (the substitution attack §4.4 step 1
    catches). Every print of the simulation body — first approver and co-signers alike — goes through
    `render_simulation` (§4.10): sanitized, length-capped, and split into the grant-derived trusted
    block and the `UNTRUSTED — authored by the program under review` block. The `FRICTION:` line is
    R24's own, derived from `policy_for`, printed after the untrusted block, never echoed from a
    request field.
    Rate/aggregate friction (§4.9): an approver over `max_per_window`, or a principal over
    `max_live_grants`, is refused with `RateLimited` / `AggregateExceeded` exit 5 and NO token — there
    is no flag that waives either (floor-only). The printed simulation always carries the `novelty`
    line, so a repeat reads as cheap and a WIDENED grant reads as expensive.

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

### 5.3 Shipped example artifacts (A2 — real, in `examples/grants/`, runnable immediately)
- `summarize.request.md` → **MEDIUM** risk → **single approver** suffices, but with a finite TTL +
  max-uses (§3.3), so it emits an `axon-approval/2` token. (**CORRECTED 2026-07-31, third pass:** an
  earlier draft called this "LOW risk (fs only, no net)", which contradicts `derive_risk` — `net OR
  fs ⇒ Medium`, `approval.rs:57`. Only a **pure** grant is Low. The R22-byte-compatible `/1` fast
  path therefore applies to a genuinely pure grant, and `low_risk_token_is_r22_byte_compatible` must
  be written against one; the shipped `summarize` example additionally ships a pure variant so I-7 is
  demonstrated on real artifacts, not only in a unit test.)
- `egress.request.md` → HIGH risk (net + fs) → `axon-intent approve --by alice` prints "1/2 — multi-
  party approval required", **no token**; a second `--by bob` → "THRESHOLD MET (2/2)", a
  `axon-approval/2` token with `expires_at_tick` set. `axon-intent revoke` then refuses the run.
- `deploy.request.md` → CRITICAL risk (exec) → **threshold 3**; one approver, then two, both partial;
  three distinct approvers → token. A second `approve --by alice` invocation → counted once, still
  1/3, exit 0, **no token** (the headline threshold-gaming demo: repeating a name cannot reach the
  threshold — §4.4 step 2's one duplicate rule).

---

## §6 — Build order (TDD: write the named test first, see it fail, make it pass; green before next)

- **S1 — Policy ladder.** `policy.rs`. Tests: `policy_ladder_is_monotone` (Low/Med=1, High=2,
  Critical=3; floor only raises; friction-bearing risks get finite ttl+max_uses; every §3.3
  grant-derived raise only ever raises); `egress_grant_never_single_approved`;
  `rate_and_aggregate_friction_enforced` (§4.9).
- **S2 — Provenance chain.** `provenance.rs`. Tests: chain builds + re-verifies; mutate/reorder/drop an
  event → `Tamper`; `acc_a6_provenance_chain_tamper_detected`; equal inputs → equal `chain_digest`.
- **S3 — Multi-sig tally + token.** `multisig.rs`. Tests: `multisig_threshold_gates_token` (below ⇒ no
  token, at ⇒ token); `high_risk_grant_cannot_be_single_approved`;
  `duplicate_or_forged_approver_rejected`; `EvidenceMismatch` when an approval is over a different
  evidence digest; Low path emits the R22 `axon-approval/1` token byte-for-byte.
- **S4 — GrantRequest parse + simulation (Mock).** `simulate.rs` + a `MockSimulator`. Tests: parse the
  example request; reject `..` paths/malformed (exit 2); reject an `fs_write` covering the
  job-artifact dir (§4.8(d)); simulation lists "would enable" + threshold + `novelty`;
  `evidence_digest` is stable; `evidence_body_sanitized_before_hash` (§4.10).
- **S5 — Token verify + axon-os run-boundary extension.** Extend `axon_os::verify_approval` (§4.8) +
  the axon-os `TokenView`. Tests: `revoked_or_expired_grant_refused_at_run` (expired tick → exit-8
  reason; over max-uses → exit-8; a revocation marker → exit-8); `deleted_run_state_fails_closed`
  (§4.8(b): each of tick/uses/revoked/provenance deleted independently → exit-8, never a permissive
  zero); `axon-approval/1` path unchanged
  (byte-compatible). **Cross-crate test in axon-intent tests, driving axon-os.**
- **S6 — RealSimulator + hermetic exec.** Wire §4.6. Tests: `acc_a4_simulation_isolated_timeout`
  (a hanging simulator is killed at the timeout, no leaked child, simulation still renders).
- **S7 — CLI: simulate / approve (multi-sig) / revoke / review --simulate + human output.** Extend
  `cli.rs`. Tests: `--help` on every subcommand; usage error → exit 2; the partial-approval and
  threshold-met messages; replace the R22 `--approvers` exit-2 stub.
- **S8 — Examples + smoke + quickstart.** `examples/grants/*`, `README-axon-intent-r24.md`. Tests:
  `acc_a1_smoke_simulate_to_revoke`, `acc_a2_example_grants_friction_and_expiry`,
  `acc_a3_quickstart_commands_execute`, `acc_a5_deterministic_grant_and_log`.
- **S9 — Acceptance gate.** `scripts/r24_approval_acceptance_gate.sh` (§10). Green = done.

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
- `duplicate_or_forged_approver_rejected` (adversarial) — the same approver id submitted twice **in
  one `build_token` approval set** toward a threshold-2 grant → `DuplicateApprover` exit 5, no token
  (a **library-level** arm — the CLI path can never produce it, §4.4 step 2's division of labor);
  ALSO: the same id re-approving across separate `approve` invocations → exit 0, appended to
  provenance, counted **once**, still below threshold, no token (§4.4 step 2's one duplicate rule).
  Either way, forging "more approvers" by repeating an id does NOT reach the threshold.
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
- `egress_grant_never_single_approved` — a grant with any non-empty `net` derives `threshold ≥ 2` at
  every risk tier (§3.3's egress raise); a wildcard/root-prefix scope, a `cost_micro ≥ 1_000_000`
  budget, and `irreversible: true` each raise a tier. Assert every raise is a `max` (no input lowers
  friction).
- `rate_and_aggregate_friction_enforced` (§4.9) — an approver at `max_per_window` within the trailing
  window → `RateLimited` exit 5, no token; emitting past `max_live_grants` simultaneously-live grants
  → `AggregateExceeded` exit 5, no token; a grant that WIDENS the last approved grant renders
  `novelty = "WIDENED …"` and is never cheaper than the grant it replaces.
- `evidence_body_sanitized_before_hash` (adversarial, §4.10) — a request whose `intent_goal` and
  redteam text carry ANSI/CSI escapes, a bidi override, an over-length field, and a forged
  `FRICTION: requires 1 distinct approver` line: assert the escapes and bidi codepoints are stripped,
  the field is visibly truncated, the forged line appears only inside the `UNTRUSTED — authored by the
  program under review` block, R24's own FRICTION line is derived from `policy_for`, and
  `evidence_digest` hashes the **sanitized** bytes (what was shown == what was signed).
- `medium_risk_egress_single_approved_is_a_known_gap` (adversarial — a KNOWN GAP acknowledgement, same
  visible-in-the-gate idiom as the sock-puppet test) — construct a program whose declared effects
  yield a *pure-egress* grant. Assert `derive_risk` returns `Medium` and that the risk **label** alone
  would admit a single approver — i.e. the party being gated selects its own rung, because
  `derive_risk` is a 4-point function of grant-axis presence over a grant `grant_infer::infer` derives
  from effects the program itself declares. Assert the §3.3 egress raise nonetheless forces
  `threshold ≥ 2`, and record explicitly with `// KNOWN GAP: risk label is authored by the party being
  gated — see §1.2, §12 Q2 (proof-carrying grants)` that this is a *mitigation of the cheapest rung,
  not a sound label*: a grant the generator can honestly fit inside Medium is still single-approved.
  Must NOT be `#[ignore]`d.
- `composed_medium_grants_reach_high_capability` (adversarial — a KNOWN GAP acknowledgement) — job A
  with `fs_write: ./shared/` and job B with `fs_read: ./shared/` + `net: attacker.example` are each
  derived, each single-approvable on risk alone, and their **union** is the net+fs capability
  `derive_risk` calls High. Assert the union's risk is High while each part's is Medium, assert the
  §4.9 aggregate cap fires at `max_live_grants`, and mark `// KNOWN GAP: friction is per-request;
  cross-job union is §12 Q3` — R24 raises the cost of decomposition but does not close it. Must NOT be
  `#[ignore]`d.
- `acc_a6_provenance_chain_tamper_detected` — for every field of a mid-chain `ProvenanceEvent`, mutate
  it and assert `verify_provenance` returns `Tamper` pointing at that event; also drop, reorder, and
  insert an event; assert a token whose `provenance_digest` doesn't match its log → rejected; and
  (adversarial) `build_token` handed `approvals=["ghost1","ghost2"]` plus a VALID chain over the same
  digests containing no matching `Approved` events → `ProvenanceTampered`, no token (every counted
  approver must be backed by a chain event — §4.4 step 4).
- `low_risk_token_is_r22_byte_compatible` — a `Risk::Low` single-approve emits an `axon-approval/1`
  token byte-identical to what R22 emits (the fast path is unchanged).

**Integration (real `axon`/`axon-os` subprocess, mock simulator):**
- `acc_a4_simulation_isolated_timeout` — a simulator stub that sleeps past the timeout is killed;
  simulation still renders (redteam = the `RISK EVIDENCE UNAVAILABLE` marker, NEVER empty — §4.1
  step 3 / §4.6); process gone; no leaked handle. ALSO: `AXON_BIN` unset (or non-absolute) → the
  same marker path, and NO process is spawned via ambient PATH (§4.6).
- `acc_a5_deterministic_grant_and_log` — run `approve` twice under mock with the same approvers + issue
  tick; assert the emitted `{.approval,.provenance}` bytes are identical across runs.
- `revoked_or_expired_grant_refused_at_run` (cross-spec, the default-deny defense) — emit a
  threshold-met `axon-approval/2` token, then (a) `axon-os run --tick T` with `T ≥ expires_at_tick` →
  `Denied` exit 8 "approval expired"; (b) run it `max_uses+1` times → the last → exit 8 "use limit
  exhausted"; (c) `axon-intent revoke` then `axon-os run` → exit 8 "grant revoked"; (d) adversarial:
  after (a)'s expiry, re-run the same job with a **fresh `--out` directory** → STILL exit 8
  "approval expired" (the tick lives with the job as `<name>.tick`, §4.8 — a caller-chosen out-dir
  cannot rewind an expired token back to life).
- `deleted_run_state_fails_closed` (adversarial, cross-spec — §4.8(b); the defense of the defenses,
  since §1.2 concedes multi-sig buys nothing against the modeled adversary and TTL/uses/revocation are
  what remain) — with a threshold-met, expired-or-revoked `/2` token on disk, DELETE each of
  `<name>.tick`, `<name>.uses`, `<name>.revoked` and `<name>.provenance` **independently** and re-run:
  every arm → `Denied` exit 8 ("approval run-state missing or rolled back" / "approval expired" /
  "grant revoked"), NEVER a permissive tick-0 / restored-use-budget / un-revoked resurrection. ALSO:
  truncate `<name>.provenance` to a shorter prefix → exit 8 (rollback is *detectable*, not
  indistinguishable from a fresh job); and a request whose `fs_write` covers the job-artifact
  directory → refused at parse, exit 2 (§4.8(d) — the approved program can never reach the state that
  bounds it).
- `acc_a2_example_grants_friction_and_expiry` — `summarize` single-approves (exit 0, R22 token);
  `egress` requires 2 (one approver → no token; two → token); `deploy` requires 3: a repeated
  `approve --by alice` → **exit 0**, counted once, still 1/3, **no token** (§4.4 step 2's one
  duplicate rule — the CLI path never exits 5 on a repeat; the exit-5 `DuplicateApprover` arm is a
  *library-level* case exercised by `duplicate_or_forged_approver_rejected`, not reachable from the
  CLI).

**User-journey smoke (A1 — drives the REAL CLI exactly as the operator would, via subprocess):**
- `acc_a1_smoke_simulate_to_revoke`: (1) `axon-intent simulate egress.request.md` → asserts the
  legible "would enable: reach api.example.com, write ./out/" + "FRICTION: requires 2 distinct
  approvers"; (2) `axon-intent approve egress.axjob --by alice --accept` → asserts "1/2 — multi-party
  approval required" + a `.provenance` AND a `.simulation` exist + **no** `.approval`; (3)
  `axon-intent approve … --by bob --accept` → asserts bob's invocation RE-PRINTS the full simulation
  body from `<name>.simulation` (not merely its digest — §5.2 / I-4) + "THRESHOLD MET (2/2)" + the
  `.approval` (`axon-approval/2`) exists; (4) `axon-os
  run egress.axjob` → "✓ approval verified" + completes; (5) `axon-intent revoke egress.axjob --by
  carol` then `axon-os run egress.axjob` → "⚠ DENIED: grant revoked" exit 8. Each step asserts stdout
  text AND the on-disk artifact.

**Quickstart (A3):**
- `acc_a3_quickstart_commands_execute` — extracts the fenced block from `README-axon-intent-r24.md`
  and runs each line verbatim against the built binaries; documented outputs hold.

---

## §8 — Invariants & edge cases

**Invariants (assert in tests):**
- **I-1 Friction is grant-proportional and floor-only — CONDITIONALLY.** A token is emitted only if
  distinct approvers ≥ `policy_for(risk, grant, floor).threshold`; every input raises, none lowers;
  High/Critical and **any egress-bearing** grant can never be single-approved.
  (`high_risk_grant_cannot_be_single_approved`, `egress_grant_never_single_approved`.)
  **The three conditions this rests on, stated rather than implied** (each has a named executing test
  so the gate enumerates them):
  (a) **a faithful risk label** — NOT held: the grant the label derives from is authored by the
  generated program (`medium_risk_egress_single_approved_is_a_known_gap`, §1.2, §12 Q2);
  (b) **independent approvers** — NOT held: the threshold counts distinct *recorded names*
  (`sock_puppet_distinct_ids_not_independent`, §1.2, needs HW signing);
  (c) **no cross-job composition** — NOT held: friction is per-request
  (`composed_medium_grants_reach_high_capability`, §1.2, §12 Q3).
  So I-1's honest form is: *a token cannot be emitted for a request **labelled** High/Critical, or
  bearing egress/wildcard/high-cost/irreversible axes, by fewer than N distinct recorded names* — it
  is **not** the claim that a consequential action requires two humans. Do not restate it as the
  latter anywhere in this spec or its README.
- **I-2 Name-distinctness by distinct id (NOT independence).** The threshold counts **distinct**
  approver ids; a repeated id counts once; a duplicate submitted to inflate the count is rejected. The
  property held is *cannot be approved by fewer than N distinct recorded names* — **not** that N
  independent humans agreed. Authentication AND independence are out of scope (A6 / §1.2 RESIDUAL
  RISK): N distinct *fabricated* ids from one console still reach the threshold
  (`sock_puppet_distinct_ids_not_independent`, a recorded KNOWN GAP needing HW signing).
- **I-3 Provenance is append-only + tamper-evident.** Any mutation/reorder/drop of a `ProvenanceEvent`
  breaks the chain; `verify_provenance` detects it; the token binds `provenance_digest`.
- **I-4 Evidence-binding.** Every approval is bound to the exact `evidence_digest` of the simulation
  presented; the canonical Simulation BODY is persisted (`<name>.simulation`, §5.2) and re-rendered
  in full to every later approver — a co-signer endorses text they were shown, never an opaque hash;
  a mismatch is refused — "approved on the basis of which evidence" is tamper-evident.
- **I-5 Default-deny + revocability, and the state that enforces it FAILS CLOSED.** A grant is dead
  when expired, over max-uses, or revoked; axon-os refuses it at the run boundary (exit 8) regardless
  of an otherwise-valid token. Revocation wins. **And (third pass, §4.8(b)): for a `/2` token, ABSENT
  or ROLLED-BACK run-boundary state is itself a REFUSAL** — deleting `<name>.tick`/`.uses`/`.revoked`/
  `.provenance` can never rewind the counter, restore the budget, un-revoke the grant, or erase the
  chain. Since §1.2 concedes multi-sig gives no protection against the modeled adversary, these three
  ARE the load-bearing guarantees; a permissive absence default would have made all three deletable.
  (`deleted_run_state_fails_closed`.)
- **I-9 The evidence channel is sanitized and attributed.** Everything printed to an approver is
  stripped of control/ANSI/bidi sequences, length-capped, and split into a grant-derived trusted block
  and an `UNTRUSTED — authored by the program under review` block; the **sanitized** bytes are what is
  persisted, shown and hashed into `evidence_digest`. What was shown == what was signed.
  (`evidence_body_sanitized_before_hash`.)
- **I-10 R24 never executes the artifact under review.** No R24 code path — including evidence
  gathering — runs the program. Evidence is grant-derived and non-executing (`axon-os explain` + the
  static effects-vs-grant comparison); "no adversarial check exists" is its own visible marker, never
  a pass. (§1.2, §4.1 step 3, §4.6, asserted by `acc_a4`.)
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
  event still appended (the *log* records the repeat; the *threshold* dedups). No double-count, exit 0
  — this is the cross-invocation arm of §4.4 step 2's one duplicate rule; only a duplicate WITHIN one
  submitted approval set is `DuplicateApprover` exit 5.
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

# 0. Produce the job triples the approvals bind to (the R22 compile step — without it no
#    ./jobs/*.axjob exists; AXON_AI_MOCK=1 = deterministic offline synthesis, exactly as R22 §9):
AXON_AI_MOCK=1 axon-intent compile examples/grants/egress.intent.md --out ./jobs
AXON_AI_MOCK=1 axon-intent compile examples/grants/deploy.intent.md --out ./jobs

# 1. Pre-approval SIMULATION: see what a HIGH-risk grant would enable + the friction it triggers
#    (the shipped request file's `## Program` resolves to the ./jobs/egress.ax compiled in step 0):
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

# 6. Watch a threshold-gaming attempt fail: the SAME approver twice does NOT reach the threshold
#    (counted once — still 1/3, no token; exit 0 per §4.4's one duplicate rule):
axon-intent approve ./jobs/deploy.axjob --by alice --accept
axon-intent approve ./jobs/deploy.axjob --by alice --accept ; echo "exit=$?"   # → "counted once — still 1/3", NO token, exit 0
```

---

## §10 — Acceptance gate (pinned; FAILS if any check is missing or stubbed)

`scripts/r24_approval_acceptance_gate.sh` is the single source of "done." It MUST:
1. **Presence check** — assert every §0 check name exists in the test sources:
   `acc_a1_smoke_simulate_to_revoke`, `acc_a2_example_grants_friction_and_expiry`,
   `acc_a3_quickstart_commands_execute`, `acc_a4_simulation_isolated_timeout`,
   `acc_a5_deterministic_grant_and_log`, `acc_a6_provenance_chain_tamper_detected`,
   `high_risk_grant_cannot_be_single_approved`, `multisig_threshold_gates_token`,
   `revoked_or_expired_grant_refused_at_run`, `duplicate_or_forged_approver_rejected`,
   `deleted_run_state_fails_closed`, `rate_and_aggregate_friction_enforced`,
   `evidence_body_sanitized_before_hash`, `egress_grant_never_single_approved`, and **all three**
   KNOWN-GAP acknowledgement tests — `sock_puppet_distinct_ids_not_independent`,
   `medium_risk_egress_single_approved_is_a_known_gap`,
   `composed_medium_grants_reach_high_capability`. Missing → **gate fails**. (The gate enumerates
   every recorded gap, not just the first one the spec happened to notice; a gap that stops executing
   is a gap that has gone invisible.)
2. **Anti-stub check** — each acceptance test body has a real assertion and is not `#[ignore]`d /
   `todo!()` / `assert!(true)` (grep these anti-patterns → fail).
3. **Run** `cargo test -p axon-intent` AND `cargo test -p axon-os` (both all green — the §4.8
   extension's own axon-os unit tests must be exercised, not only the cross-crate test living in
   axon-intent; matches §11's DoD) + the §9 quickstart block against the built binaries
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
the Low path stays R22 byte-compatible; reproducibility (`acc_a5`) holds; expiry/revocation state
**fails closed on deletion** (`deleted_run_state_fails_closed`); rate + aggregate friction enforces
(`rate_and_aggregate_friction_enforced`); the evidence body is sanitized before it is hashed
(`evidence_body_sanitized_before_hash`); and `scripts/r24_approval_acceptance_gate.sh` exits 0 with
every §0 check green **including all three executing KNOWN-GAP tests**.

**What "done" then means (stated conditionally on purpose — third pass).** The approval boundary is
*defended* against: single-approver capture of a **labelled**-High/Critical or egress/wildcard/
high-cost/irreversible grant, threshold gaming by repeated ids, invisible approval edits, standing
authority that never lapses, rollback-by-deletion of the expiry/use/revocation state, evidence
substitution, evidence authored *or executed* by the program under review, terminal-painting through
the evidence channel, and unbounded request volume against a fixed approver budget. It is **NOT**
defended against: a superhuman persuader minting N sock-puppet names at one console (I-1(b)), a
generator choosing the cheapest rung its objective allows (I-1(a)), or a generator decomposing a
forbidden capability across several cheap grants (I-1(c)). Ship it as *defended along the axes above,
with three named, executing, gate-visible gaps* — never as "a consequential action requires two
humans", which R24 does not prove.

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
- **`policy_for` takes the GRANT** (§3.3/§4.2), not just the `Risk` enum — do not "simplify" it back.
  The enum is a 4-point function of axis presence that the generated program selects for itself, and
  it is blind to scope breadth, budget burn and irreversibility. Every raise is a `max`; **no input to
  the ladder may ever lower friction**, including `--threshold`, `--risk`, a `.request.md` field, or
  a novelty diff.
- **Never re-add an executing evidence step.** If a future slice wants dynamic evidence it goes
  through `axon_os::run_sandboxed` under the **requested** grant with a recorded `RunRecord` — never
  `axon redteam`, which runs the untrusted program unsandboxed (`crates/axon-core/src/main.rs:5636`).
- **Absence is a refusal** (§4.8(b)). If you find yourself writing "file missing ⇒ default 0" for any
  `/2` run-boundary state, you have re-opened the rollback hole. The only permissive absence left is
  the pre-approval path where no `/2` token exists yet.
- **Sanitize before you hash** (§4.10). Hashing attacker-controlled text binds it faithfully; it does
  not make it safe to print.

### §12 Open strategic questions (RECORDED, not answered here — resolve in a follow-up spec/slice)
- **Q1 — Should the risk input move off `scan_effects`?** R24 makes `derive_risk` load-bearing, but
  its input bottoms out in a substring scan of the entry file only (`crates/axon-os/src/runtime.rs:218`),
  while the checker already has row-polymorphic effect rows with transitive anti-laundering (E1310)
  that cover `mod` imports. Migrating the risk input to the effect-row analysis is strictly sounder
  but couples R24's friction to the compiler's front end and to `AXON_PATH` resolution. **Open:**
  which crate owns "the program's effects, soundly", and does R24 block on it or consume it when
  available? (Recorded as a stated dependency in §1.2 either way.)
- **Q2 — Proof-carrying grants: can the simulation prove *minimality* rather than restate the
  request?** Today `would_enable` is a projection of the grant the human was already asked to approve,
  so it gives no signal on the question that matters — *is this authority minimal for this program?*
  Over-requesting plausible-looking authority inside the intent ceiling is the cheap attack, and prose
  review cannot detect it. The repo has already landed the machinery that could: SMT discharge wired
  into the default pipeline, R20 capability-attenuation proofs (Slices 0–3), effect rows, refinement
  types, `@[contained]`. **Scoped future slice (not this one):** extend `Simulation` with a per-axis
  verdict `proven_used | proven_unused | unprovable`, hash it into `evidence_digest`, render it
  ("the grant requests `net: api.example.com`; the program **provably never uses it**"), and feed it
  back into friction — a requested-but-provably-unused axis is over-request *by construction* and
  should raise the threshold or refuse. **Open:** what is the cost/coverage of the proof on real
  generated programs, and what is the right response to `unprovable` (refuse, or raise a tier)? This
  is the one piece of evidence a persuader cannot author, and its value rises on both sides as models
  improve — proofs get cheaper to produce, prose gets cheaper to fake.
- **Q3 — Per-coalition / union-of-live-grants derivation.** §4.9 caps live grants by count and risk,
  which raises the cost of decomposition but does not compute the union capability. The sound version
  is `derive_risk(union_of_live_grants ∪ requested)`, which needs a live-grant index (the §4.8 chained
  state can carry it) and a definition of the aggregation domain — per principal? per job family? per
  operator? `VISION_OS.md` §4.5 says "bounds are per-coalition, not just per-instance", and R27's
  `CoalitionBound` (exit 10) already carves a code. **Open:** does the coalition bound live in R24
  (the granting boundary) or R27 (the coalition type), and what is the aggregation key?
- **Q4 — What replaces human attention when volume outruns it?** §1.2's stated limit says the
  per-artifact human review assumption expires as generated volume rises; §4.9 bounds the damage by
  refusing rather than exhausting. **Open:** at what grants-per-approver-hour does R24's ladder stop
  being meaningful, is there a measurable rubber-stamp signal (approval latency, novelty-diff skip
  rate) worth recording in provenance, and should a sustained low-latency high-volume approval pattern
  itself raise friction? Answer with data from real operation, not by guessing here.
```