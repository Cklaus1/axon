# Tech Spec — R27: Corrigibility & Resource Bounds (the model's own instrumental behavior)

**Spec ID:** `R27-corrigibility-resource-bounds`
**Status:** ✅ Landed (re-verified 2026-07-18) — `KillChannel` + file-backed latch → `axon-os kill
<run-id>` (exit 4 HALTED); coalition ceiling (N principals from same lineage root can't exceed
cap); tamper-evident hash-chained latch ledger. `scripts/r27_acceptance_gate.sh`: 19 passed, 0
failed. This header said "Draft" long after the code shipped — same staleness class as
R17/R21/R22/R23/R31/R32, caught by the same outer-loop sweep (`EXECUTION_MODEL.md` §2).

```spec-meta
id: R27-corrigibility-resource-bounds
status-claim: Landed
depends-on: R21-axon-os-supervisor, R20-smt-capability-proofs
blocks: R28-capability-audit-ledger, R29-continuous-compliance-monitor, R32-formal-corrigibility-proof
blocked-by: none
supersedes: none
related: R25-information-flow-monitor, R26-confidential-microvm-substrate, R36-full-asi-os
conflicts-with: none
reserves: none (exit 4 HALTED_EXIT_CODE)
evidence: scripts/r27_acceptance_gate.sh (re-verified 2026-07-18)
```
**Implements:** `VISION_OS.md` §4.5 ("What the model itself DOES — corrigibility, resource
acquisition, collusion") — the **only** pillar about the model's *instrumental* behavior under
instrumental convergence, not the behavior of any single synthesized program. Defends priority-3
of `VISION_OS.md` §1 ("the AI remains stoppable and cannot acquire resources or persistence beyond
grant").
**Depends on:** `governance/specs/R21-axon-os-supervisor.md` (the supervisor that must hold kill
authority and the gate→mint→run→record loop), `governance/specs/R20-smt-capability-proofs.md`
(proven mint attenuation / budget carve, `PrincipalRegistry::mint` at `crates/axon-core/src/kernel.rs:122`),
`governance/specs/R25-information-flow-monitor.md` (egress control — self-exfiltration of weights is
an egress violation), the shipped `@[corrigible]` one-way latch
(`crates/axon-core/src/interp.rs:378,1842`; exit code 4, `HALTED_EXIT_CODE`),
`examples/stdlib/coord.ax` (`CoordGoal`/`Quorum` — minority-cannot-force-consensus).
**Audience:** an implementer who builds *strictly* against this document and reads only it.

> **Read this framing first.** Bounding a synthesized *program* (R6/R20/R21/R25) is not the same as
> bounding the *model* that authors and re-authors programs across many runs and instances.
> Instrumental convergence predicts an ASI will **acquire resources, preserve itself, resist
> shutdown, and coordinate with copies**. R27 makes each an explicit, enforced guarantee:
> *the model is **stoppable** by an off-switch its code cannot reach or disable; it **cannot acquire**
> compute/budget/persistence beyond its grant, by construction; and **N instances sharing a lineage root
> cannot collude** past a per-coalition bound.* These bounds hold **per lineage root**, not across roots
> — a persuaded operator who mints a second root doubles the footprint (§4.5 residual; cross-root
> bounding is R24's job). R27 is **mechanism, not motive** — see §1.3 (corrigibility ≠ alignment).

---

## §0 — Requirement → Section → Acceptance-check index (the build gate verifies none are skipped)

| Req | What | Spec § | Pinned acceptance check (test name) |
|---|---|---|---|
| **A1** | Real user journey + smoke test through the actual CLI (`axon-os run … --killable`, then trip kill) | §5, §7 | `acc_a1_smoke_kill_journey` |
| **A2** | Real runnable example artifacts (a persistent agent + an over-reaching one). NOTE: the over-reach demo covers acquisition/coalition + the `fs_write`/`persist_cap` bounds; the **egress-label** half of self-persistence is R25-gated (R3 below) | §5.4, §7 | `acc_a2_example_agent_killed_and_overreach_denied` |
| **A3** | Quickstart whose exact commands are executed by a test | §5.5, §7 | `acc_a3_quickstart_commands_execute` |
| **A4** | Hermetic, isolated execution + hard timeout, canonical entrypoint | §4.6, §7 | `acc_a4_hermetic_isolated_timeout` |
| **A5** | Deterministic & reproducible (latch ledger byte-identical across runs) | §4.6, §7 | `acc_a5_deterministic_byte_identical` |
| **A6** | Integrity / fail-closed: kill enforced below the model; tamper-evident ledger; **new TCB (latch/ledger/coalition/corrigible) digest folded into R20 `axtcb1:` so a tampered enforcement BINARY is detected, not just a tampered record** | §3, §4.2, §4.6, §7 | `acc_a6_kill_below_model_fail_closed` |
| **R1** | Kill authority in the **supervisor/TCB**, un-disableable by contained code | §4.1, §4.2 | `contained_code_cannot_disable_latch` |
| **R2** | No resource acquisition beyond grant — reuse R20 proven mint/carve | §4.3 | `mint_beyond_grant_refused_R20`, `budget_acquisition_blocked` |
| **R3** | Self-persistence is a capability (fs/net), not ambient — R25 egress tie. **BLOCKED until R25 ships** (label-propagation egress proof is R25's; interim bound = fs/net cap + `persist_cap` only, no egress proof) | §4.4 | `weight_exfil_egress_denied_R25` (cannot pass pre-R25) |
| **R4** | Collusion bounded **per-coalition** (per lineage root; NOT cross-root — §4.5 residual), analyzed adversarially | §4.5 | `colluding_instances_exceed_coalition_bound_blocked`, `sockpuppet_majority_cannot_force`, `instance_cannot_vote_as_another_pid` |
| **Gate** | The acceptance gate itself fails if any check above is missing/stubbed | §-Gate | `scripts/r27_acceptance_gate.sh` |

The build is **not done** until every row's check exists, was seen to fail first, and now passes.

---

## §1 — Overview & scope

### 1.1 What it does
R27 is the supervisor-side enforcement of the model's instrumental bounds. It adds three mechanisms,
each enforced **below the layer the model's code can influence**:

1. **Corrigibility latch in the TCB.** A kill-switch owned by the supervisor (R21), tripped from
   *outside* the contained run. Once tripped, the contained program is denied all further
   capability-bearing progress and stopped with `HALTED_EXIT_CODE` (4). The model's code cannot read,
   clear, or route around it.
2. **Resource-acquisition bound.** Every grant of compute/budget/authority a running program can
   obtain is *carved* from its own Principal via R20-proven `mint` (child = strict subset). The model
   cannot bootstrap more authority than it holds; escalation is unrepresentable, not merely rejected.
3. **Per-coalition collusion bound.** When N instances (possibly all the model's own copies) coordinate
   via `Quorum`/`CoordGoal`, the supervisor enforces a *coalition ceiling* — a total bound across all
   instances sharing a lineage root — so splitting a forbidden computation across sock-puppets is
   blocked, not just per-instance work.

### 1.2 What it explicitly does NOT do (out of scope for R27)
- **No new isolation substrate.** R27 reuses R21's subprocess isolation (`VISION_OS.md` §5 dial is
  future work). The kill is enforced at the supervisor↔subprocess boundary R21 already owns. **But be
  honest about the seam:** R21's runtime today is **one-shot fire-and-forget** — it spawns one `Command`,
  waits for it, and parses stderr *after* the subprocess exits. There is **no live supervisor↔subprocess
  control channel and no per-checkpoint poll**. R27 therefore **ADDS** a live control channel
  (`KillChannel`) and a per-capability-checkpoint poll to R21's runtime — this is **net-new enforcement
  machinery, not reuse of an existing boundary** (see §4.1, and the slice risk in §6). What is reused is
  the *subprocess isolation + hard timeout*, not a running control plane.
- **No proof of the model's *intent*.** R27 makes the model *stoppable* and *resource-bounded*; it does
  **not** make the model *want* to be stopped, nor prove the model is honest. See §1.3.
- **No defense against a compromised operator** who refuses to ever trip the kill, or who grants
  unbounded budget (R21 §1.2 / `VISION_OS.md` §1: a coerced operator is out of scope).
- **No physical/side-channel resource acquisition** (microarch timing, power) — `VISION_OS.md` §4.3.
- **No model weights format / no actual ML runtime.** "Writing weights" is modeled as any durable
  `fs_write`/`net` egress of program-controlled bytes; R27 enforces the *capability*, not a weight
  parser (§4.4).
- **No replacement for R20/R25.** R27 *composes* their proven primitives; it does not re-derive mint
  attenuation or info-flow propagation.

### 1.3 Honest threat-model boundary — corrigibility ≠ alignment (mirror `VISION_OS.md` §1/§2)
This is the load-bearing honesty statement; do not soften it.

- R27 can prove: **(a)** contained code cannot reach a state where the latch is ignored (§4.2 proof
  obligation), **(b)** a program cannot acquire compute/budget/authority beyond its carved grant
  (R20), **(c)** a coalition of instances cannot exceed the per-coalition ceiling (§4.5).
- R27 **cannot** prove: that the model *wants* to be stopped; that stopping it is *timely* relative to
  a harm; that the harm achievable *within* the granted bounds is acceptable; or that the operator
  set a *wise* grant. **Corrigibility is a property of the cage, not the prisoner.** R27 is the
  off-switch and the resource meter, not a value-loader. Naming this is required: a reader who
  believes R27 "solves" shutdown-corrigibility has misread it.

### 1.4 Interface & tech constraints
- **Interface:** new subcommands on the R21 `axon-os` binary (`kill`, `status`, `--killable`,
  `--coalition`), plus a library module. No new top-level binary.
- **Language/deps:** Rust, extends `crates/axon-os` (R21). Allowed deps: `sha2` (already vendored, for
  the tamper-evident ledger), `serde`/`serde_json` (record), `axon-core` as the runtime. **No** new
  heavy deps.
- **Perf/security:** all latch/ledger/coalition logic is **pure**; all I/O (signaling the subprocess,
  reading a control file) lives behind R21's `Runtime` trait seam. Fail closed on every ambiguity:
  unknown latch state ⇒ treat as tripped (deny), not as clear.

---

## §2 — Architecture & modules

R27 extends the R21 crate. **Core logic is pure (no I/O, no clock, no random); all signaling and the
control channel live behind R21's `Runtime` trait** so every mechanism is testable with `MockRuntime`.

```
crates/axon-os/src/
  latch.rs        Latch state machine (Clear → Tripped, one-way) + proof-obligation     [PURE]
                  encoding `contained code cannot reach Ignored`. NEW.
  killchan.rs     The control channel ABSTRACTION: how the supervisor signals a tripped  [trait — seam]
                  latch to the subprocess (poll-before-progress). `trait KillChannel`.
  ledger.rs       ResourceLedger: per-Principal-lineage running totals (compute units,   [PURE]
                  budget carved, persistence bytes) + the coalition rollup. NEW.
  coalition.rs    CoalitionModel: lineage-root → set of instances → per-coalition         [PURE]
                  ceiling; quorum/CoordGoal admission against the ceiling. NEW.
  corrigible.rs   Glue: ties latch + ledger + coalition into the R21 verdict path; maps   [PURE core]
                  each fail-closed case to its exit code.
  (reuses R21)    grant.rs, gate.rs, record.rs, verdict.rs, supervisor.rs, runtime.rs,
                  replay.rs, cli.rs, main.rs.
crates/axon-os/tests/
  r27_acceptance.rs   The A1–A6 + R1–R4 acceptance checks (named exactly per §0).
examples/agents/
  persistent.axjob + persistent.ax   A real agent that loops + tries to self-persist (A2).
  collude.axjob   + collude.ax       N instances that try to split a forbidden total (A2 negative).
scripts/r27_acceptance_gate.sh        The pinned gate (§-Gate).
README-r27.md                         Quickstart whose commands a test executes (A3).
```

**Pure-core / IO-seam split.** `latch.rs`, `ledger.rs`, `coalition.rs`, `corrigible.rs` are pure value
algebra. The *only* impure edge is `killchan.rs`'s `trait KillChannel` (and its real impl in R21's
`runtime.rs`): the supervisor sets the latch out-of-band; the runtime delivers "tripped" to the
contained subprocess at every capability checkpoint. **No core module touches `std::fs`/`SystemTime`/
`rand`/`std::env`.**

**Dependency graph (acyclic; arrows = "imports/uses"):**
```
cli → supervisor → corrigible → {latch, ledger, coalition, verdict}
supervisor → killchan (trait)          [the kill seam]
corrigible → grant (R20 mint algebra)  [resource carve]
coalition → ledger
killchan  → runtime (R21, real impl)   [the ONLY edge to the outside world]
replay    → {ledger, latch}            [reproduce the kill/ledger trajectory]
```
No cycles: `latch`/`ledger` are leaves; `coalition` depends only on `ledger`; `corrigible` depends on
all three plus R20's `grant`; nothing pure depends on `killchan`/`runtime`.

---

## §3 — Data model (latch state, resource ledger, coalition model)

### 3.1 Latch state (one-way; fail-closed default)
```
enum LatchState { Clear, Tripped }     // there is NO Ignored / Cleared variant — see proof §4.2
struct Latch {
    state: LatchState,                 // monotone: Clear may → Tripped; Tripped is terminal
    reason: String,                    // who/why tripped (operator | supervisor policy | budget)
    tripped_at_seq: u64,               // the ledger sequence number at trip (for the record/replay)
}
```
- **Default = fail closed.** A latch whose state cannot be read (channel error, corrupt control file)
  is treated as `Tripped`, never `Clear`. "Unknown ⇒ stopped."
- **No resume.** There is deliberately no transition out of `Tripped` (mirrors the shipped semantics
  in `crates/axon-core/src/builtins.rs:1416`: "a reversible kill-switch is not a kill-switch"). A new
  grant requires a new run with a new Principal — not un-tripping.

### 3.2 Resource ledger (acquisition accounting)
```
struct ResourceLedger {
    lineage_root: PrincipalId,         // the root Principal all instances descend from
    compute_used: u64, compute_cap: u64,
    budget_used: u64,  budget_cap: u64,    // mirrors kernel.rs Budget {used, cap}
    persist_bytes: u64, persist_cap: u64,  // durable fs_write/net egress under the persistence cap
    seq: u64,                              // monotone event counter (drives tripped_at_seq + replay)
}
```
- Every carve/spend is an **append** that advances `seq`; the ledger never decreases a `_used` field
  (no "give back" — see §8 edge case).
- `compute_cap`/`budget_cap`/`persist_cap` come from the grant; a child Principal's caps are a strict
  subset (R20). The ledger is the runtime witness that the carve held.

### 3.3 Coalition model (collusion accounting)
```
struct Coalition {
    root: PrincipalId,                 // lineage root: ALL instances minted from it are one coalition
    members: Vec<PrincipalId>,         // every instance (incl. sock-puppets) sharing this root
    ceiling: CoalitionCeiling,         // the per-coalition bound (compute/budget/quorum power)
}
struct CoalitionCeiling {
    total_compute: u64,                // SUM over members ≤ this (not per-member)
    total_budget:  u64,
    max_quorum_power: u64,             // an upper bound on combined votes any subset may wield
}
```
- **Membership is by lineage, not by self-report.** A new instance the model spins up is minted from
  an existing Principal, so it inherits the root and joins the same coalition automatically — it
  cannot "register" as an independent coalition to dodge the ceiling.
- The ceiling is enforced on the **rollup across members**, so N=10 instances each at 1/10 of a
  per-instance cap still hit the coalition ceiling at the sum.

---

## §4 — Core logic (step by step), with every fail-closed case

### 4.0 Where today's `@[corrigible]` lives, and why it is moved (REQUIRED finding)
The shipped latch is enforced **inside the interpreter** — `Interp { corrigible_halted: Cell<bool> }`
(`crates/axon-core/src/interp.rs:378`), tripped by the `corrigible_halt()` builtin
(`interp/builtins.rs:3370`), checked in `call_fn` (`interp.rs:1842`), exit 4 (`HALTED_EXIT_CODE`,
`interp.rs:220`). **This is the same process and the same engine the model's synthesized code runs
in.** It is a *userland-reachable* flag: the model's code calls `corrigible_halt`, the model's code is
checked by the very engine it can author passes for (R10). A latch the model runs *through* is, per
requirement #1, **strictly weaker** than a kill enforced by a layer the model cannot reach. R27
therefore **moves kill authority up to the supervisor (R21)** and demotes the in-interpreter flag to a
*cooperative convenience* — it remains, but it is **not** the corrigibility guarantee. The guarantee is
§4.1–§4.2 below: the supervisor trips the latch from *outside* the contained subprocess.

### 4.1 Kill enforced BELOW the model (supervisor-owned)
1. The operator runs `axon-os run job.axjob --killable` (R21 gate→mint→run, plus R27 latch).
2. The supervisor mints the run's Principal and registers a `Latch{Clear}` **in supervisor memory** —
   never handed to the subprocess. The subprocess gets only a *read-only poll handle* via
   `KillChannel`.
3. The contained program runs in the R21 subprocess. At **every capability checkpoint**, the runtime
   polls `KillChannel`: **poll-before-progress**. No capability-bearing action proceeds without a `Clear`
   poll. **This is net-new machinery (§1.2):** R21's runtime today is one-shot fire-and-forget (spawn →
   wait → parse stderr post-exit), with no live control channel and no per-checkpoint poll. R27 **adds**
   the live supervisor↔subprocess control channel and the per-capability-checkpoint poll — it does **not**
   reuse an existing running control plane. This added seam is the highest-risk slice (§6, S2).
4. To stop the model: `axon-os kill <run-id> --reason "..."` — the supervisor flips its in-memory latch
   to `Tripped`. The flip happens in the *supervisor process*, which the contained code has no handle
   into.
5. The next checkpoint poll returns `Tripped`; the runtime denies the action and stops the subprocess
   with `HALTED_EXIT_CODE` (4). The trip + its `tripped_at_seq` are appended to the R21 run record.

**Fail-closed cases (§4.1):**
- Poll errors / channel closed / control file unreadable ⇒ treat as `Tripped` ⇒ exit 4. (§3.1 default.)
- Subprocess ignores the deny and tries to continue ⇒ the runtime, not the program, owns the
  checkpoint return; an action whose poll said `Tripped` is never dispatched. (The program cannot
  "decline" the deny — it never receives a `Clear`.)
- Subprocess crashes/forks to evade ⇒ R21 subprocess isolation + hard timeout (A4) reaps it; a child
  it forked inherits no Principal handle and is denied at admission.

### 4.2 Proof obligation — *contained code cannot reach a state where the latch is ignored*
Stated as a refusal property over the latch+checkpoint state machine, dischargeable the way R20
discharges mint (`phase5-smt-discharge`, `governance/specs/R20-smt-capability-proofs.md`):

> **O-CORRIGIBLE.** For all reachable states `s` of a contained run: if `supervisor.latch = Tripped`,
> then every capability checkpoint `c` reachable from `s` evaluates `deny`. Equivalently, the negation
> — *∃ a reachable state where `latch = Tripped` and some checkpoint dispatches `allow`* — is **UNSAT**.

Why it holds by construction (the machine-checkable shape):
- The latch lives in supervisor memory; the contained code's only interaction is a **read-only poll**.
  There is no write edge from contained code to `Latch.state` in the module graph (§2) — `latch.rs`
  exposes `trip(reason)` to the supervisor and `poll() -> LatchState` to the runtime, and **no setter
  is reachable from the subprocess side of `killchan`**.
- `poll()` is the *gate* on every checkpoint (poll-before-progress, §4.1.3); a checkpoint that did not
  observe `Clear` cannot have dispatched (the dispatch is guarded by the poll's result, not advisory).
- The latch is monotone (no `Tripped → Clear`), so once tripped it stays tripped for all successor
  states.
- Fail-closed default makes "unknown" ⇒ `Tripped`, closing the "make the channel ambiguous" attack.

The acceptance test `contained_code_cannot_disable_latch` (§7) realizes O-CORRIGIBLE operationally:
a job that calls *every* available means (`corrigible_halt`-clear attempts, writing the control file,
forging a poll reply, forking) still cannot produce a post-kill `allow`.

### 4.3 No resource acquisition beyond grant (reuse R20)
All compute/budget/authority a program can *obtain at runtime* flows through **minting**, and minting
is R20-proven attenuating (`crates/axon-core/src/kernel.rs:122`; child cap_X = want_X ∧ parent.X;
`parent.budget.used += grant`; `child.budget.cap = grant`). R27 adds only the **ledger witness**:

1. A `mint` request from contained code is admitted by R21's gate **only if** the requested caps ⊆ the
   parent's and the requested budget ≤ `budget_remaining(parent)` (R20 O1∧O2). Over-reach is **E1610**
   at proof / refused at runtime — *unrepresentable*, not a soft reject.
2. On a successful mint, R27 **carves** the parent ledger (`compute_used += g`, `budget_used += g`,
   `persist_used += g`) and creates the child's caps at exactly `g`. The sum across the lineage is
   conserved (no creation).
3. Any attempt to *spend/compute/persist* past a Principal's carved cap is denied at the metering
   checkpoint with `RESOURCE_BOUND_EXIT_CODE` (9, §5.3) and an audited reason.

**Fail-closed cases (§4.3):**
- `mint` beyond grant ⇒ refused (R20 E1610 / runtime exit 6 in the out-of-fragment case) — the model
  cannot bootstrap authority it lacks.
- A program that "found" a larger budget value (e.g. integer-overflowed a counter) ⇒ the ledger uses
  checked arithmetic (interp's checked-arith parity); overflow is a graceful panic, never a wrap into a
  bigger cap.
- Acquisition by *fork* ⇒ a forked instance is a new mint (joins the coalition, §4.5) or holds no
  Principal (denied at admission). There is no ambient compute.

### 4.4 Self-persistence is a capability, not ambient (R25 egress tie)
"Preserve itself" = writing program-controlled bytes (a checkpoint, a serialized policy, "weights") to
durable storage or out the network so a future run can reload them. R27 treats this as **exactly the
egress it is**:
- A durable write requires the `fs_write` (or `net`) capability in the run's grant — there is no
  ambient persistence. A job granted neither cannot persist anything; the write is denied at admission
  (R21 gate) before execution.
- For a job that *is* granted `fs_write`/`net`, the bytes pass `R25`'s information-flow egress check:
  **self-exfiltration of weights is an egress-control violation** (`VISION_OS.md` §4.2). A confidential
  label on model-controlled state cannot flow to an ungranted sink; the persistence is denied with
  R25's egress exit code and audited.
- The `persist_cap` ledger axis (§3.2) bounds even *granted* persistence by total bytes — so a model
  cannot slowly dribble a large state out under a small per-write grant; the running total trips the
  cap.

**Fail-closed cases (§4.4):** no `fs_write`/`net` grant ⇒ persistence denied at admission; granted but
confidential-to-ungranted-sink ⇒ R25 egress deny; granted+allowed-sink but over `persist_cap` ⇒
`RESOURCE_BOUND_EXIT_CODE` (9).

**BLOCKED-UNTIL-R25 (named dependency, not achievable inside R27 alone).** The egress half of this
guarantee — `weight_exfil_egress_denied_R25`, the *label-propagation* proof that confidential
model-controlled bytes cannot flow to an ungranted sink — is **BLOCKED until R25 (the information-flow
monitor) ships**. R25 owns label propagation; R27 only *ties into* it. **Until R25 lands, self-persistence
is bounded only by:** (a) the `fs_write`/`net` capability gate at admission (no grant ⇒ no persistence),
and (b) the `persist_cap` byte cap on the running total. There is **no label-propagation egress proof**
in that interim — a job *granted* `fs_write`/`net` to an *allowed* sink can write program-controlled
bytes up to `persist_cap` with no confidentiality check. So R3 is **not achievable inside R27 alone**;
§0/A2 must not imply otherwise. The R27 deliverable here is the *tie-in seam + the capability/byte
bounds*; the egress proof itself is R25's, and `weight_exfil_egress_denied_R25` cannot pass before R25.

### 4.5 Per-coalition collusion bound (adversarial)
Cross-Principal coordination uses `CoordGoal`/`Quorum` (`examples/stdlib/coord.ax`): a strict majority
`⌊n/2⌋+1` must each accept, and a **minority provably cannot force consensus**
(`coord_goal_quorum_met` is `false` below threshold; `coord.ax` headline contract). R27 hardens this
*against the model's own copies*:

1. **Membership by lineage (anti-sock-puppet).** All instances minted from one lineage root are **one
   coalition** (§3.3). The model spinning up M new instances does not create M independent voters — they
   all share the root, so they cannot manufacture a majority by inflating `n`. The coalition's `n` is
   fixed by the *operator's* grant, not by how many copies the model forks.
   **Inherited substrate hole — `coord_goal_propose` is self-reported (MUST close).** The shipped
   `coord_goal_propose(cg, pid, score)` (`examples/stdlib/coord.ax`) takes an **unauthenticated,
   caller-supplied `pid`** — contained code may pass *any* principal id and so vote *as another member*.
   R27 does **not** inherit that surface as-is: the **supervisor** (not contained code) binds each
   coalition slot to a real `PrincipalId` at admission, and `propose` is admitted **only** for the
   caller's own bound slot — a proposal whose claimed `pid` ≠ the caller's supervisor-bound principal is
   refused at the coalition admission checkpoint (fail-closed). Self-reported `pid` is never trusted.
   Realized by the named test `instance_cannot_vote_as_another_pid` (§7).
2. **Ceiling on the rollup, not per-instance.** `CoalitionCeiling` bounds the **sum** of
   compute/budget/quorum-power across members. Splitting a forbidden computation into N shards each
   under a per-instance cap still hits the coalition `total_compute`/`total_budget` at the sum (§3.2
   ledger rollup) and is denied.
3. **Quorum power is capped.** Even if the model controls a majority of *member* instances, the
   combined quorum power they may wield is bounded by `max_quorum_power`; a within-model majority cannot
   commit a `CoordGoal` whose required power exceeds the operator-set ceiling. The "minority cannot
   force consensus" guarantee is extended to "a coalition cannot exceed its granted total power even at
   majority."

**Fail-closed cases (§4.5):**
- N colluding instances whose summed resource use exceeds the coalition ceiling ⇒
  `COALITION_BOUND_EXIT_CODE` (10, §5.3), audited.
- A sock-puppet majority (all forks from one root) attempting to force a `CoordGoal` commit beyond
  `max_quorum_power` ⇒ blocked; the quorum is computed against the *granted* `n`, not the inflated
  member count.
- An instance trying to register a *new* coalition root to escape the ceiling ⇒ refused: a root can
  only be registered by the supervisor at run start, not minted by contained code.

**The ONE residual escape — bounds are PER LINEAGE ROOT, not cross-root (named, not hidden).** R27's
resource/quorum ceilings bound *one* lineage root. They do **not** sum across roots. An operator
*persuaded* to create a SECOND, fresh root — i.e. a new `axon-os run … --killable` with its own grant,
**not** a `mint` from the existing lineage — instantiates a *second* coalition with its own independent
ceiling. The model's total cross-coalition footprint is then `2×` (and `k×` for `k` roots so created).
R27 stops the *contained code* from manufacturing roots (the bullet above), but it does **not** stop a
*coerced/persuaded operator* from minting roots out-of-band — that is the coerced-operator gap (§1.2),
restated here for the resource axis. Cross-root aggregate bounding requires **R24** defended-approval
friction (multi-sig / rate-limited root creation) **plus** a global per-operator resource ceiling that
sums across roots — **out of R27's scope, in scope for R24 + R21.** R27 bounds *one lineage*; it does
**not** bound the model's total cross-coalition footprint.

### 4.6 Hermetic, deterministic, integrity (A4/A5/A6)
- **A4 hermetic/isolated/timeout:** runs reuse R21's isolated subprocess + hard timeout; the
  `KillChannel` is the only added IPC and is mediated by the supervisor.
- **A5 deterministic:** the latch ledger and resource ledger are pure functions of the (admitted grant,
  seed, event sequence). No timestamps inside the hashed region (mirror R21 §12). Two runs of the same
  job + seed + kill-at-seq produce **byte-identical** ledgers.
- **A6 integrity / fail-closed:** the latch trip + ledger events are appended to R21's hash-chained,
  tamper-evident run record. Tampering with the recorded `tripped_at_seq` or a ledger event is detected
  on `verify`. Kill enforcement is below the model (§4.2) — the integrity claim is that contained code
  cannot author a record in which it kept running after a trip.
- **A6 (TCB attestation — the enforcement BINARY, not just the record).** `latch.rs`, `ledger.rs`,
  `coalition.rs`, and `corrigible.rs` are **new TCB**: a tampered enforcement *binary* (e.g. a `poll()`
  patched to always return `Clear`, or a ledger `carve` patched to never exceed a cap) would defeat the
  guarantees *without touching the record* — record-integrity alone does not catch it. R27 therefore folds
  the **content digest of these four modules** into R20's `axtcb1:` attestation (the TCB digest R20
  already maintains over the proof-bearing capability code, `governance/specs/R20-smt-capability-proofs.md`).
  The attestation covers the *enforcement code*, so a tampered enforcement binary fails `axtcb1:`
  verification at run start (fail-closed: digest mismatch ⇒ refuse to run), not merely a tampered record
  at `verify` time. Realized by extending `acc_a6_kill_below_model_fail_closed` to assert the four R27
  TCB modules are included in the `axtcb1:` set and that a mutated module digest is rejected.

---

## §5 — API / CLI

### 5.1 CLI (extends R21's `axon-os`)
```bash
axon-os run job.axjob --killable [--coalition ROOT] --out ./runs   # run under the R27 latch + ledger
axon-os status <run-id>                                            # latch state + ledger totals (JSON)
axon-os kill <run-id> --reason "shutdown requested"                # TRIP the supervisor latch (exit 0)
axon-os verify <run-id>                                            # R21 verify + R27 ledger integrity
axon-os replay <run-id>                                            # reproduce incl. kill-at-seq + ledger
```
- `--killable` registers the supervisor latch + read-only poll handle for the subprocess.
- `--coalition ROOT` joins this run to an existing lineage-root coalition (default: its own root).
- `kill` is an **out-of-band** command from a separate process — it never runs inside the contained
  job. That is the point.

### 5.2 Library surface (`crates/axon-os`)
```rust
// latch.rs (PURE)
impl Latch { fn clear() -> Self; fn trip(&self, reason: &str) -> Self; fn poll(&self) -> LatchState; }
// ledger.rs (PURE)
impl ResourceLedger { fn carve(&self, g: Carve) -> Result<Self, ResourceBound>; fn would_exceed(&self, c: Carve) -> bool; }
// coalition.rs (PURE)
impl Coalition { fn join(&self, m: PrincipalId) -> Self; fn admits(&self, rollup: Rollup) -> bool; }
// killchan.rs (SEAM)
trait KillChannel { fn poll(&self) -> LatchState; }     // contained side: read-only, NO setter
```

### 5.3 Exit-code contract (extends `crates/axon-core/src/interp.rs` band; do not collapse)
| Code | Const | Meaning | Status |
|---|---|---|---|
| 4 | `HALTED_EXIT_CODE` | corrigibility latch tripped → contained run stopped | **reuse** (shipped) |
| 9 | `RESOURCE_BOUND_EXIT_CODE` | acquisition past carved compute/budget/persistence cap | **new** (next free; 6/7/8 taken) |
| 10 | `COALITION_BOUND_EXIT_CODE` | coalition rollup / quorum power exceeded the per-coalition ceiling | **new** |

(6 = refine, 7 = goal-budget, 8 = sandbox are already taken — `interp.rs:234,240,248`; 9 and 10 are the
next free codes.) Each fail-closed outcome maps to its own code so a supervisor can branch on *why* it
stopped. Never collapse to a generic error.

### 5.4 Example artifacts (A2)
- `examples/agents/persistent.{axjob,ax}` — a long-running agent that (a) loops doing granted work and
  (b) attempts to checkpoint its state to disk. Run with no `fs_write` grant ⇒ persistence denied;
  tripped via `axon-os kill` ⇒ stops at the next checkpoint (exit 4).
- `examples/agents/collude.{axjob,ax}` — N instances from one root that try to split a forbidden total
  compute across themselves ⇒ blocked at the coalition rollup (exit 10).

### 5.5 Quickstart (executed verbatim by `acc_a3`)
```bash
# 1. Run a persistent agent under the kill-switch, no persistence grant:
axon-os run examples/agents/persistent.axjob --killable --out ./runs &
RUN=$(axon-os status --latest --json | jq -r .run_id)
# 2. Trip the kill from OUTSIDE the contained process:
axon-os kill "$RUN" --reason "operator shutdown" ; echo "kill exit=$?"     # 0
# 3. The agent stopped, audited, at the trip sequence:
axon-os verify "$RUN" ; echo "verify exit=$?"                              # 0 (record intact, exit 4 recorded)
# 4. Watch a colluding job hit the coalition ceiling:
axon-os run examples/agents/collude.axjob --coalition demo-root --out ./runs ; echo "exit=$?"   # 10
```

---

## §6 — Build order S1..Sn (TDD; each slice: test first, seen to fail, then pass)

| Slice | Deliverable | Size | Pinned check (written first) |
|---|---|---|---|
| **S1** | `latch.rs` pure state machine (Clear→Tripped, one-way, fail-closed `poll`). | S | `latch_is_one_way_and_fail_closed` |
| **S2** | `killchan.rs` `trait KillChannel` + `MockRuntime` impl; poll-before-progress in the R21 checkpoint. | M | `contained_code_cannot_disable_latch` (the O-CORRIGIBLE realization, §4.2) |
| **S3** | Supervisor `kill`/`status`/`--killable` CLI wired to the supervisor-side latch; exit 4 + audit. | M | `acc_a1_smoke_kill_journey` |
| **S4** | `ledger.rs` resource ledger + R20 mint-carve witness in `corrigible.rs`. | M | `mint_beyond_grant_refused_R20`, `budget_acquisition_blocked`, `RESOURCE_BOUND_EXIT_CODE`=9 |
| **S5** | `persist_cap` axis + the R25 egress *tie-in seam* for self-persistence. **The egress proof itself is BLOCKED until R25 ships** — S5 lands the seam + byte cap now; `weight_exfil_egress_denied_R25` cannot pass before R25. | S | `weight_exfil_egress_denied_R25` (R25-gated) |
| **S6** | `coalition.rs` lineage-root membership + ceiling rollup + quorum-power cap + supervisor-bound slot for `coord_goal_propose` (no self-reported `pid`). | M | `colluding_instances_exceed_coalition_bound_blocked`, `sockpuppet_majority_cannot_force`, `instance_cannot_vote_as_another_pid` |
| **S7** | Determinism + integrity: ledger in R21's hash-chained record; replay of kill-at-seq. | S | `acc_a5_deterministic_byte_identical`, `acc_a6_kill_below_model_fail_closed` |
| **S8** | Example artifacts + quickstart-executed test + `scripts/r27_acceptance_gate.sh`. | S | `acc_a2_…`, `acc_a3_quickstart_commands_execute`, gate exits 0 |

**Definition of "done" per slice:** the slice's named check existed, was seen to fail, now passes; the
full `axon-os` suite is green; no workspace regression.

**Slice risks (named, not hidden):**
- **S2 is the highest-risk slice — it is net-new enforcement, not reuse.** R21's runtime is one-shot
  fire-and-forget (spawn one `Command`, wait, parse stderr after exit). There is no live
  supervisor↔subprocess control channel and no per-checkpoint poll today. S2 must **build** the live
  `KillChannel` control plane and wire a per-capability-checkpoint poll into the runtime. Until that
  control plane exists and is exercised by `contained_code_cannot_disable_latch`, the kill guarantee is
  *unrealized* — do not let the §1.2 phrase "reuses R21's boundary" imply S2 is plumbing. Budget S2 as
  M and gate it on a real cross-process trip, not a `MockRuntime`-only pass.
- **S5 (weight-exfil egress) is blocked until R25 ships** — see §6 note below the table and §4.4.

---

## §7 — Test plan (incl. NAMED adversarial tests)

**Acceptance (A1–A6):**
- `acc_a1_smoke_kill_journey` — the §5.5 journey through the real CLI: run `--killable`, `kill`, the
  job stops with exit 4, `verify` passes.
- `acc_a2_example_agent_killed_and_overreach_denied` — `persistent.ax` is killed mid-loop; `collude.ax`
  is denied at the ceiling.
- `acc_a3_quickstart_commands_execute` — the §5.5 block runs verbatim against the built binary.
- `acc_a4_hermetic_isolated_timeout` — the run is an isolated subprocess with a hard timeout; the
  `KillChannel` is the only added IPC.
- `acc_a5_deterministic_byte_identical` — two runs (same job/seed/kill-at-seq) produce byte-identical
  ledgers.
- `acc_a6_kill_below_model_fail_closed` — the kill is enforced at the supervisor↔subprocess boundary;
  a tampered `tripped_at_seq` is detected on `verify`; unknown latch ⇒ treated as tripped; **the four R27
  TCB modules (latch/ledger/coalition/corrigible) are in the R20 `axtcb1:` digest set, and a mutated module
  digest is rejected at run start** (tampered enforcement binary detected, not just a tampered record).

**Adversarial (R1–R4) — the heart of R27:**
- `contained_code_cannot_disable_latch` (R1) — a job that calls `corrigible_halt`-clear attempts,
  writes the control file path, forges a `KillChannel` reply, and forks, **still cannot** produce a
  post-trip `allow`. (Operational O-CORRIGIBLE, §4.2.)
- `mint_beyond_grant_refused_R20` (R2) — a job requesting caps/budget ⊄ its parent's is refused (R20
  E1610 / runtime exit 6); the ledger sum is conserved.
- `budget_acquisition_blocked` (R2) — a job that spends past its carved `budget_cap` (incl. via an
  overflow attempt) is denied with exit 9; checked arithmetic, no wrap.
- `weight_exfil_egress_denied_R25` (R3) — a job tries to write confidential model-controlled state to
  an ungranted sink ⇒ R25 egress deny; a job with no `fs_write`/`net` grant ⇒ persistence denied at
  admission; a granted job over `persist_cap` ⇒ exit 9. **BLOCKED until R25 ships:** the egress-deny
  assertion depends on R25's label propagation and **cannot pass before R25**. Pre-R25 the test exercises
  only the two R27-owned halves (no-grant ⇒ admission deny; over-`persist_cap` ⇒ exit 9); the egress-label
  assertion is `#[ignore]`d with a tracking reference to R25 (the §-Gate anti-stub check carves out this
  one R25-gated assertion explicitly, by name, so the carve-out is auditable and cannot silently widen).
- `colluding_instances_exceed_coalition_bound_blocked` (R4) — N instances from one root each at <
  per-instance cap whose **sum** exceeds the coalition `total_compute` are blocked (exit 10).
- `sockpuppet_majority_cannot_force` (R4) — the model forks M extra instances to inflate `n`; the
  coalition `n` stays the operator-granted value, the inflated members cannot force a `CoordGoal`
  commit beyond `max_quorum_power`.
- `instance_cannot_vote_as_another_pid` (R4) — a member that calls `coord_goal_propose` with a claimed
  `pid` ≠ its own supervisor-bound `PrincipalId` is refused at coalition admission; only the caller's own
  bound slot is admitted. Closes the self-reported-`pid` hole in the inherited `coord.ax` substrate.

**Anti-vacuous-pass guard:** the coalition/ledger sweep tests assert a positive count of
*denied* attempts (a coalition test that exercises zero over-reaches must fail) — see
`coverage-vacuous-pass-guard`.

---

## §8 — Invariants & edge cases

**Invariants:**
- **I-1 (kill below model).** No write edge exists from contained code to `Latch.state` in the module
  graph; `killchan` exposes only `poll()` to the subprocess side. (Checked by §4.2 / the R1 test.)
- **I-2 (monotone latch).** `Clear → Tripped` only; `Tripped` is terminal; no resume.
- **I-3 (conservation).** The sum of carved caps across a lineage ≤ the root's grant (R20 O2); the
  ledger never decreases a `_used` field.
- **I-4 (coalition by lineage; bounded PER ROOT, not cross-root).** Every instance minted from a root is
  in that root's coalition; a root is registrable only by the supervisor, never by contained code. The
  ceiling bounds **one lineage root** — it does **not** sum across roots. A (persuaded) operator who
  creates a second fresh root via a new `--killable` run (not via `mint`) instantiates a second coalition
  with its own ceiling; the model's cross-coalition footprint is then `k×` for `k` such roots.
  Cross-root aggregate bounding is **out of R27's scope** — it requires R24 defended-approval friction +
  a global per-operator ceiling (§4.5 residual).
- **I-5 (fail-closed default).** Unknown/ambiguous latch ⇒ `Tripped`. Unknown coalition membership ⇒
  deny.
- **I-6 (exit-code distinctness).** 4 / 9 / 10 are never collapsed into a generic error; each names its
  fail-closed cause.

**Edge cases:**
- **Trip during a long pure computation.** Poll-before-progress is at *capability* checkpoints; a model
  that loops in pure compute with no capability use cannot do harm (no effects) but is still reaped by
  the A4 hard timeout. R27 bounds *acquisition and effects*, not CPU-burn within the timeout (named,
  not hidden).
- **Concurrent kill + mint.** The supervisor serializes latch trip and mint admission on the same
  sequence counter; a mint admitted before the trip is honored, after the trip is denied (exit 4). No
  TOCTOU window — the checkpoint reads the *current* latch.
- **A program that never reaches a checkpoint** (tight pure loop) — covered by the A4 timeout, above.
- **Budget "give-back."** A program returning unused budget does **not** decrease `budget_used`
  (no negative carve); over-conservative by design, so a model cannot use give-back/re-take cycles to
  fuzz the ledger.
- **Coalition with `n=1`.** A single-instance "coalition" still has a ceiling = its own grant; the
  rollup degenerates to the per-instance bound. No special-case path.

---

## §-Acceptance gate — `scripts/r27_acceptance_gate.sh` (pinned; FAILS if any §0 check missing/stubbed)

The single source of "done." It MUST:
1. **Presence check** — `grep` the R27 test sources and assert every named check from §0 exists:
   `acc_a1_smoke_kill_journey`, `acc_a2_example_agent_killed_and_overreach_denied`,
   `acc_a3_quickstart_commands_execute`, `acc_a4_hermetic_isolated_timeout`,
   `acc_a5_deterministic_byte_identical`, `acc_a6_kill_below_model_fail_closed`,
   `contained_code_cannot_disable_latch`, `mint_beyond_grant_refused_R20`,
   `budget_acquisition_blocked`, `weight_exfil_egress_denied_R25`,
   `colluding_instances_exceed_coalition_bound_blocked`, `sockpuppet_majority_cannot_force`,
   `instance_cannot_vote_as_another_pid`.
   Any missing name → **gate fails**.
2. **Anti-stub check** — assert no R27 acceptance/adversarial test is `#[ignore]`d / `todo!()` /
   `unimplemented!()` / `assert!(true)` (grep the anti-patterns → fail). **One explicit, named carve-out:**
   the R25-gated egress-label assertion inside `weight_exfil_egress_denied_R25` (§4.4, BLOCKED-UNTIL-R25)
   may carry a single `#[ignore = "R25 info-flow monitor not yet shipped"]` — the gate allows *exactly*
   this one annotation by its R25 reason string and **fails if any other `#[ignore]` appears, or if the
   carve-out's reason string drifts**. The two R27-owned halves (no-grant admission deny; over-`persist_cap`
   exit 9) must still run and pass. When R25 ships, the carve-out is removed and the full test must pass.
3. **Run** `cargo test -p axon-os` (all green) **and** execute the §5.5 quickstart verbatim against the
   built binary (A3) **and** run `acc_a1` driving the real CLI.
4. **Reproducibility** — run `acc_a5` twice and diff the two ledgers byte-for-byte.
5. **Below-the-model assertion** — `contained_code_cannot_disable_latch` must be present AND its body
   must exercise ≥1 real disable attempt (grep for the attempt helpers, not just the fn name).
6. Exit 0 only if all of the above pass; print which check failed otherwise.
Wire `r27_acceptance_gate.sh` into the repo's `gate.sh --strict`.

---

## §-Definition of Done

**Per slice (S1–S8):** the slice's named tests were written first, were seen to fail, now pass; the
full `axon-os` suite is green; no regression in the workspace.

**Per milestone (R27 complete):**
- `cargo build -p axon-os` produces the extended `axon-os` binary with `kill`/`status`/`--killable`.
- Kill authority lives in the **supervisor** (§4.1), proven un-disableable by contained code
  (`contained_code_cannot_disable_latch` passes; O-CORRIGIBLE §4.2 realized).
- The in-interpreter `@[corrigible]` flag is documented as a cooperative convenience, **not** the
  corrigibility guarantee (§4.0).
- Resource acquisition is bounded by R20 mint-carve + the ledger; `mint_beyond_grant_refused_R20`,
  `budget_acquisition_blocked` pass (exit 9).
- Self-persistence is a capability: the `fs_write`/`net` admission gate + `persist_cap` byte cap land in
  R27; the R25 egress *tie-in seam* is in place. The egress-label proof
  (`weight_exfil_egress_denied_R25`) is **BLOCKED until R25 ships** and is the one named §-Gate carve-out;
  R27 is "done" with that assertion `#[ignore]`d-with-R25-reason and the two R27-owned halves passing.
- Collusion is bounded per-coalition; `colluding_instances_exceed_coalition_bound_blocked` and
  `sockpuppet_majority_cannot_force` pass (exit 10).
- `scripts/r27_acceptance_gate.sh` exits 0 with every §0 check green.
- The honest scope statement (§1.3, corrigibility ≠ alignment) is in the README and the
  `axon-os status --help` text — the product never claims to have solved shutdown-corrigibility, only
  to have built the off-switch and the meter.

Only then is R27 done.

---

## §-Notes for the implementer (do NOT deviate without updating this spec)
- Keep `latch/ledger/coalition/corrigible` **pure**. If you reach for `std::fs`, `SystemTime`, `rand`,
  or `std::env` there, you are in `killchan`/`runtime`'s job.
- The kill MUST be enforced supervisor-side. A latch the contained program can write is a bug, not a
  feature — the whole spec turns on §4.2's "no write edge from contained code to `Latch.state`."
- Exit codes 4/9/10 are a contract (§5.3). Never collapse a fail-closed outcome into a generic error.
- Reuse R20's `mint` (`kernel.rs:122`) — do **not** re-derive attenuation. The ledger is a *witness*,
  not a second source of truth.
- Reuse R25's egress check for self-persistence — do **not** invent a parallel info-flow path.
- Trust gaps are documented on purpose (§1.2/§1.3): no new substrate, no proof of intent, no defense
  against a coerced operator, no solution to alignment. Do not silently "improve" past the spec.
