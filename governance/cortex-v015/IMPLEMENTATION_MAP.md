# Cortex v0.15 → live Axon: implementation map (B01 work product)

**Status of this document.** Reconnaissance only. No Cortex code has been
written. Every verdict below carries an EVIDENCE CLASS, because "a test asserts
it", "a gate runs it" and "source reads that way" are three different strengths
and conflating them is how this repository's claims drifted before.

Package integrity verified independently before any reading:
`SHA256SUMS_v0_15.json`, **113/113 match**, 0 mismatched, 0 missing.

Conflicts are filed separately in [`DISCREPANCIES.md`](DISCREPANCIES.md)
(D-001 … D-021). This file is the map; that file is the conflict register.

---

## 1. The single most decision-relevant fact

**Most of what M0 asks for already exists, under different names.** The package
describes a greenfield build. The live repository has already built — and in
several cases already GATED — the invariants M0's two evidence gates assert.

`G00-missing-gate` reads:

> simulate PASS, FAIL, SKIPPED, UNKNOWN and unavailable mandatory verifier.
> Only actual passing evidence for all requirements permits admission.

That is, almost clause for clause, the defect class this repository closed in
the session that produced this document:

| gate clause | live mechanism | evidence class |
|---|---|---|
| UNKNOWN not treated as passing | `interp.rs:1296` — a gate returning a non-bool was `Ok(_) => 0`; now exit 2 with a named reason | **a gate runs it** (`gate_verdict_is_read.sh`, unconditional) |
| unavailable verifier not passing | `main.rs:2449` — `axon verify` exited 0 with no prover linked; now exit 2 | same gate |
| SKIPPED not passing | `scripts/lib/harness_skip.sh` — positive allow-list; anything unrecognised is FAIL | verified in BOTH directions with stub binaries |
| absent gate not passing | `gates_skipped` / `gates_override` in `axon-deploy/1` | **a test asserts it** (`cli_run.rs:23139`) |
| **expired** not passing | **ABSENT** — see D-003 | — |

`G15-parity` reads:

> supported interpreter/native cases agree on semantic output, effects,
> failures and recorded model behavior; unsupported cases refuse explicitly.

Live: `parity_all.sh` (52 passed / 2 skipped / 0 failed of 54) and
`all_examples_parity.sh` ("76 examples native==interp; 41 interp-only **sound
E0910 refusal**"). E0910 IS the explicit-refusal mechanism the clause names.
Residual: **budgets are not compared across engines**, because the native engine
has no ambient ceiling to compare against — that is D-002, and it is a CONFLICT
rather than a gap.

**Consequence for sequencing.** B01's deliverable is a *"commit-pinned evidence
matrix"*. `AXON-COMPLETENESS.json` (53 rows, generated and hard-gated by
`scripts/completeness.py`, which refuses a row claiming a proof it cannot cite)
is already that artifact in all but name. **The M0 work is mapping and
extension, not construction.**

---

## 2. Ownership map — where each CX subsystem lives today

| CX area | live owner | current behaviour | verdict |
|---|---|---|---|
| parser / AST | `crates/axon-core/src/{parser,ast}.rs` | `Expr` has no stable id; `Span` NOW carries `SourceId` (landed this session) | extend |
| type system | `infer.rs` → `checker.rs::expr_types` | inference result is **discarded** (`let _subst = …`); types re-derived 3× incl. a ~160-line codegen heuristic | extend; R2a documents the debt |
| effects / capabilities | `capabilities.rs` (static, E1001–E1005), `effects.rs` (E1310 subsumption) | static rules sound; transitive anti-laundering already present | reuse |
| interpreter | `interp.rs` + `interp/*` | reference semantics, authoritative | protected kernel |
| codegen | `codegen/*` | E0910 refusal is dense and is the explicit-refusal mechanism | reuse |
| runtime host seam | `host.rs` (`AxonHost`) | fixed method list; CX-04 §"narrow host seam" names this exact seam | **extend here, do not add an AIR crate** |
| scheduler | `kernel.rs` (`Fiber`/`Scheduler`) | cooperative, interp-only | extend |
| supervision | `axon-os/src/supervisor.rs` | pure; grant ∩ supervisor_grant | reuse |
| stores | `axon-audit` (**real hash chain**) vs `axon-ledger` (**no chain**) | see §4 | map lineage to `axon-audit` |
| model gateway | `axon-ai`, `kernel.rs::LlmGateway` | per-token metering, E1604 | extend |
| world model | `examples/stdlib/world.ax` | 128 lines, one parameter, synthetic ground truth, **no train/eval split** | the package's "distinguish a demo from a learned model" is correct |
| verification | `verify.rs::Discharged` | keys on **bare fn name**, not a digest | see §4 |
| rewrite firewall | `improve.rs` + `rewrite_dsl.rs` | G1–G3 gate, G4 advisory; 7 rules | reuse; no gate script invokes it |
| intent compilation | `axon-intent`, `main.rs cmd_intent_compile` | prose→AST; `asi-runtime` compiled by **nothing** | extend |
| approval / deploy | `main.rs:7470,7999,8127`; `axon-os/src/approval.rs` | entry-file digest bound and tested; **import graph unbound**; token is an UNKEYED hash | D-006 |
| provenance / replay | `replay.rs` + `scripts/replay_host_gate.sh` | first-divergence latch, exit 11, un-swallowable; **opt-in** via `AXON_RECORD` | reuse — the strongest subsystem here |
| tests / gates | `scripts/*.sh`, `cli_run.rs`, `AXON-COMPLETENESS.json` | see §3 | extend |

---

## 3. The two facts that change how every "gated" claim above must be read

1. **Almost every acceptance gate is `--strict` only.** `gate.sh:255`–`:526` is
   one `if [ "$STRICT" = 1 ]` block containing the parity suite, every
   per-requirement acceptance gate, and the codegen-gated tests. A default run
   prints its own notice saying it did NOT verify interp↔codegen parity. The
   unconditional exceptions worth knowing: `scripts/completeness.py` and
   `scripts/cortex_package_gate.sh`.
2. **A parity PASS is weaker than it reads.** `EXPECT_MIN_PASS=40` against 54
   harness files, so several can flip to SKIP and still pass. The
   skip-classification hole was closed this session
   (`scripts/lib/harness_skip.sh`), but the floor is still 40.

Both bear on the package's own rule: *"A missing repository or a skipped gate
is not PASS."*

---

## 4. Missing abstractions — and the existing subsystem to extend

**No new crates.** Every gap below has an existing owner.

> **SUPERSEDED 2026-09-24 (v0.22 candidate line, `v022/integration@279da778`).**
> The sentence above no longer describes the tree. Three crates were added:
> `axon-loop-contracts` (pure closed-loop contract types, strict parse, `cl22:`
> digest), `axon-loop` (fenced policy pointer + authority epoch, experiment
> register, EVO/EVL, CX-11 policy admission, TEL, MiCode intake) and
> `axon-fabric` (durable operation journal, reservations, the ACF submit path).
> **Why:** the v0.22 package's CX-11 admission, CX-29/CX-33 experiment loop
> and ACF-01 compute fabric had no code owner on `main` — they were doc-only
> (`reconciliation.md` "greenfield owners"). The crates are defensible; the
> silence was not: the candidate recorded them nowhere, and several of them
> duplicate owners this table names. Those duplications are OPEN conflicts,
> listed in §4a below and filed as `DISCREPANCIES.md` D-014 … D-021. Status of
> all three crates is **partial**; none is on an `axon` CLI verb's path.

| missing | extend | not |
|---|---|---|
| AIR graph + validator (CX-04) | `host.rs` seam + `checker.rs` categories + reserved E-codes in `error.rs` | a new `axon-air` crate |
| Evidence-class taxonomy (CX-00 R5) | the `legend` of `AXON-COMPLETENESS.json` | a second evidence matrix |
| Lineage chain (CX-10) | `axon-audit`'s `Ledger` — prev-hash chain, keyed tip, truncation detection, all built and tested | `axon-ledger`, which has no chain |
| Episode chain integrity | same — `axon-cortex`'s episode digest is a **snapshot hash, not a chain**; truncate the last event and re-digest and nothing detects it | — |
| Proof receipts (CX-15 P-1) | `verify.rs::Discharged`, keyed on a **program digest** instead of a bare fn name | a proof-receipt crate |
| Epoch / version pinning (CX-00 R7) | `axon-os::manifest::JobManifest` + `record::RunRecord` | a `RunManifest` type — same artifact, new name |
| CX gate execution records | `governance/cortex_gate_execution_registry.json` — exists, empty, unconditionally gated, rejects rows naming orphan scripts | a fifth registry, or edits to the SHA-pinned vendored manifest |


### 4a. Ownership conflicts opened by the v0.22 candidate (all OPEN)

Evidence for every row: `git show v022/integration@279da778:<path>`, plus
operator-side analysis in `.axon-v022/analysis/D_architecture.json` (untracked,
**not in the repository** — cite it as operator evidence, not as a repo file).
None of these is fixed on the Stage-1 honesty branch; Stage 1 only records them.

| id | conflict | existing owner it bypasses | stage | discrepancy |
|---|---|---|---|---|
| D-C1 | `axon-loop/src/ledger.rs` is a second hash-chained ledger, **unkeyed** sha256 in the same directory as its data; its own doc lists forged-append, consistent truncation and whole-store rollback (red-team F1/F2/R1/R2) as out of model | `axon-audit` `Ledger::open_keyed` + authenticated `compute_tip` — the row above ("Lineage chain (CX-10) → extend axon-audit's Ledger") | 2 | D-015 |
| D-C2 | a fourth authority vocabulary: contract `principal_ref`/`grant_ref` are opaque strings used only to format `authority_ref`; Fabric admission runs `supervise_requiring` over a no-op `AdmissionProbe` with a HARD-CODED `Profile::Restricted` grant and `require_approval: false` (`axon-fabric/src/submit.rs`, `supervisor_admits`), so the grant admitted is not the grant enforced; `cortex --fabric-journal` hard-codes principal `cortex:repair` and `policy_digest acf1:000…` (`axon-cortex/src/bin/cortex.rs`) | `axon-os::grant::Grant` + `gate::admit` + `approval.rs` (§5 risk 2 predicted exactly this) | 2 | D-016 |
| D-C3 | two `acf1:` canonicalisers either side of the cortex→fabric process seam: `axon-fabric/src/submit.rs` `executable_digest`/`workspace_digest` (via `axon_loop_contracts::canonical_bytes`) and `axon-cortex/src/runner.rs` `fabric_*_digest` (serde_json sorted-map order; equality asserted only in tests) | one canonicaliser | 2 | D-017 |
| D-C5 | a second reservation / budget algebra: `axon-fabric/src/journal.rs` `reserve` | `axon-os::ledger::ResourceLedger::carve` (checked, never-decreasing; itself still without a production caller) | 2 | D-017 |
| D-C6 | name collision: `axon-loop/src/admission.rs` is CX-11 **policy** admission; `axon-os::gate::admit` / `axon-intent` `admit.rs` are **effect** admission. An ACCEPT, or the active-policy pointer it moves, is **never a grant** and must never be read as one | — (distinct concerns; keep both, keep them named apart) | invariant | D-018 |

Also open, and not ownership conflicts: the `axon-vm` library entry point
bypasses `cmd_run`'s pre-launch gates (D-019); the `linux-microvm-protected`
profile is enclosure-only and its Fabric eligibility check ignores BLOCKED
rows (D-020); red-team r4 NS3/NS4p/NS4b and G6 (Fabric ignores the requested
architecture / checkpoint kind) are Stage-2 defects, see the crate READMEs.
The v0.22 package's own `EXISTING_AXON_MAP.md` repeats two stale claims
(D-021).

**Axon ↔ MiCode.** The MiCode v0.22 consumer (`v022/micode`, based on
`checkpoint/v014-reconciled`) is **not releasable** until MiCode `tui` —
specifically `ed082601`, credential-to-provider-host isolation — is merged
(operator decision D3, 2026-09-24).

---

## 5. Risks

1. **247 `NOT_RUN` gates entering a status table.** The prior intake review named
   this as the intake risk and it stands: the execution class does not travel
   with a gate ID. Mitigation exists — `cortex_gate_execution_registry.json`
   carries `implementation_status` and `product_result` per row and its gate
   rejects a row citing a script nothing invokes.
2. **A fourth authority vocabulary.** Three exist:
   `axon-core::kernel::PrincipalRegistry`, `axon-os::grant::Grant`, and
   `axon-cortex::EditGrant`/`Authorized`. The package's own v0.7 note already
   said to build on `axon_os::grant::Grant`.
3. **Self-report in the evidence channel (D-005).** Contained today only because
   one function ignores the synthetic events. A CX-10 learning export is exactly
   the consumer that would not.
4. **`POLICY_FILES` is four basenames (D-004).** The evaluation matrix and the
   gate registry are writable by any grant.

---

## 6. Smallest foundational milestone that unlocks the most

Computed from `task_manifest.json` (152 tasks, transitive dependents):

```
B00  unlocks 151   M0/contracts   Import and reconcile the package
B01  unlocks 150   M0/contracts   Audit Axon seams and engine guarantees
B02  unlocks 148   M0/evaluation  Freeze task, safety and evaluation policy
B04  unlocks 145   M0/contracts   Implement common manifests and event schemas
B03  unlocks 142   M0/evaluation  Resettable fixtures and simple controls
```

Exactly one task has no dependencies. **M0 is the whole bottleneck**, and B00/B01
are reconciliation, not code — which is what this document and the discrepancy
register are.

**Recommended first implementation slice, once reconnaissance is accepted:
D-005.** It is small (two variants on an existing closed enum), it is inside the
crate Cortex will build on, it removes fabricated evidence from the channel a
learning pipeline would treat as ground truth, and the compiler forces the
conformance test to be updated. It is also the only proposed change that makes
a protected-kernel property (*provenance integrity*) strictly stronger with no
migration.

---

## 7. What this document does NOT establish

No Cortex gate, benchmark or model evaluation was executed. The 35 specs'
internal claims are not independently checked — only their reconciliation
against live code. Several verdicts are "source reads that way" and say so.
D-002 is verified by grep, not by building a native binary under a set ceiling;
D-004 by reading a constant, not by attempting the edit through the loop; D-008
by reading two call sites, not by spawning a grandchild.

The package says the same about itself, and that agreement should not be
mistaken for independent confirmation.

## CX numbering spans two locations

`CX-00` … `CX-34` are **vendored** inside the v0.15 package and pinned by
`SHA256SUMS_v0_15.json`. `cortex_package_gate.sh` verifies that manifest in
BOTH directions, so a file added under the package root without a digest entry
fails the gate, and editing `INDEX.md` or `spec_manifest.json` to register one
changes the digest of a pinned file. Either way, a new spec cannot be added to
the package without breaking its integrity check.

`CX-35`+ therefore live **beside** the package, in `governance/cortex-v015/`,
outside the SHA-pinned set. Verified rather than assumed: the package gate
passes with `CX-35-reflex-serving.md` present.

Consequence for readers: the CX series is no longer a single directory listing.
Vendored specs are the frozen v0.15 record; live specs are the ones this
repository is currently authoring and may revise. `implementation_evidence`
stays empty for live specs until something executes, and their gates get rows
in `governance/cortex_gate_execution_registry.json`, never in the vendored
manifest.
