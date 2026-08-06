# Spec — the HIGH tier: security/soundness, gate integrity, and two decisive measurements

**Status:** DRAFT — written 2026-08-06 from `tasks/opportunities.md`.
**Source entries:** O-RLM-05, O026, O031, O032, O035, O030 (security/soundness) ·
O037, O036, O023 (gate integrity) · O-RLM-11, O-RLM-10 (research).
**Risk class:** Structural — one item changes a persisted on-disk format, and
six are policy decisions rather than code.

---

## The finding that shapes this spec

Reading the eleven source entries in full — rather than the one-line summaries
they were ranked by — **six say "decision needed" in their own text**, and a
seventh explicitly warns against fixing it:

| entry | its own words |
|---|---|
| O026 | "which is a call for the operator, not a bug fix to slip into an unrelated commit" |
| O031 | "Needs a decision; not fixed." |
| O032 | "Spec-blessed today" — implemented to `R3c-ai-budget-meter.md` §3 as written |
| O035 | "**Decision needed.**" — needs key material that does not exist yet |
| O030 | "left as a decision rather than patched over" |
| O036 | "**Decision needed.**" |
| O023 | "**LEAD, not a confirmed defect**… Worth measuring before changing" — and records a prior fix attempt that **deadlocked** |

So the HIGH tier is mostly not a work queue. Treating it as one would produce
exactly the failure the build loop is meant to prevent: a run that picks a
security default nobody chose, or "fixes" a lead that a previous attempt already
broke the build with.

This spec therefore has a small buildable core and a large, explicit
`needs-human` set. That ratio is the deliverable, not a shortfall.

---

## Tier 1 — buildable now, no decision required

### S1 — the audit ledger must detect tail truncation (O-RLM-05)

`crates/axon-audit/src/lib.rs:349`. `verify_chain` walks entries checking
`seq`/`prev_hash`/`entry_hash`; nothing anchors the tip or the count. `Ledger::open`
reads whatever lines exist, so deleting the last N lines verifies **clean**. An
audited program can erase its own trailing FS/Exec records. Independently
reproduced 3× by the 2026-08-01 triage (P6-COV-02), still open.

**Constraint that makes this Structural:** the ledger is a JSONL file that
outlives the process. The inner-loop "delete the old path" rule explicitly does
*not* apply to persisted formats — an existing ledger must still verify after
this change, or the fix destroys the audit history it exists to protect. A
migration or a version-tagged record is part of the work, not an alternative to it.

**Acceptance:** a test that writes N entries, truncates the last K lines, and
requires `verify()` to fail naming truncation; plus a test that a ledger written
before the change still verifies.

### S2 — `r31_acceptance_gate.sh` must run its tests, not grep for their names (O037)

The gate's loop is `grep -q "$name" "$LIB_SRC" "$VM_SRC"` — satisfied by the name
appearing in a comment, a docstring, or an `#[ignore]`d body. This is how
P4-OS-11 survived: the gate was green while `--extended-tcb` gated nothing.

The entry states the fix: run the tests and assert the result
(`cargo test … -- --exact <name>`, or one run whose output is parsed for each
name reporting `ok`). It also names the sweep: `grep -l REQUIRED_NAMES scripts/*.sh`
finds the other gates with the same shape.

**Acceptance:** the gate fails when a required test is `#[ignore]`d or renamed to
a comment — a name-grep cannot distinguish those, so that is the discriminating
test. Every other `REQUIRED_NAMES` gate audited and either fixed or listed.

### S3 — re-measure the fluency gate with the corrected advice (O-RLM-11)

The "diagnostics did not repair" result was measured with the **wrong** hint (it
claimed `char_at` returns a `str`). The corrected hint is unmeasured. One run of
`atlas/spikes/rlm-engine`'s `axon_card` bin, three trials per D5.

**Known blocker, not a surprise:** the atlas working tree was moved to `main` by
another session and holds that session's uncommitted work. This run must **not**
check out a branch over it. If the tree is still occupied, S3 is `blocked`, and
that is a legitimate outcome rather than a failure.

### S4 — add character literals to the language card (O-RLM-10)

One line in `LANGUAGE_CARD`. Constraint A2b: it changes what the model is shown,
so it must be measured **separately from S3**, or neither result is attributable.
Sequenced strictly after S3, and blocked by the same tree availability.

---

## Tier 2 — `needs-human`. Recommendations recorded; nothing built.

Each of these is written up with options and a recommendation, and **none is
adopted**. Specifically none is built behind a default-off flag: a flag nobody
has decided to turn on is built-but-uncalled code, not a partial win.

### H1 — O026: what does an unmanifested guest run get?

Today: every effect (`0xff`), because the chain ends in `None` and the guest
reads `None` as unrestricted. Worst where it matters most — a program the
compiler *refuses* cannot have a manifest at all, so the least trustworthy
programs arrive with the widest grant.

Options: (a) deny-all, (b) IO-only, (c) refuse to run unmanifested.
**Recommendation: (c) refuse**, with (a) behind an explicit opt-in flag — a
capability ceiling should never be inferred from absence. Changes the behaviour
of every unmanifested run, which is why it is the operator's call.

### H2 — O031: should `axon build` be able to make AI calls `axon run` refuses?

Native codegen links `axon-ai` unconditionally, so the same compiler binary
produces an AOT program that dials the real model in a configuration where
`axon run` fails closed with E1300. Reproduced end-to-end in the entry.

Options: (a) gate the native link on `asi-runtime` so both paths agree,
(b) make the interpreter live too, (c) document the divergence.
**Recommendation: (a)** — agreement between the two execution paths is invariant
I-2's whole premise, and this is a capability difference, not a performance one.

### H3 — O032: should an AI budget cover callees?

Implemented to `R3c-ai-budget-meter.md` §3 as written, so changing it is a **spec
amendment**, not a bug fix. But R4's agent action-log made the opposite choice
deliberately (`enclosing_agent`) precisely so indirection cannot escape the
audit. A ceiling an `Extract Function` refactor removes is not a ceiling.
**Recommendation: amend R3c to the enclosing-fn semantics**, matching R4.

### H4 — O035: what key signs a quorum vote?

Anyone who can write the responses directory is the entire quorum. T49 closed
what needed no key material (run-id binding, `--expect-tcb`); authenticity needs
signing. **Recommendation: reuse the R26/R31 per-guest attestation identity**
rather than minting a second key hierarchy — but that binds quorum to the
attestation TCB, which is a real coupling someone must accept.

### H5 — O030: which SQL dialect does `sql_query` target?

Currently trades a MySQL injection for Postgres/SQLite data corruption on any
backslash-containing parameter. The entry's own framing is the right one:
**rendering a query string is the unsafe pattern**, and real safety is driver-side
binding, which Axon cannot do because it has no database sink.
**Recommendation: take a dialect argument and refuse parameters it cannot encode
safely for that dialect** — an API change, hence a decision.

### H6 — O036: should `AXON_HARNESS_STRICT=1` be the default?

The parity harnesses run or skip depending on which command last wrote
`target/debug/axon` — a shared mutable artifact. Both outcomes were observed in
one session, and **this spec's author hit it again during the previous loop and
logged it as a fresh lesson (L014) before noticing it was already O036** — which
is the argument for closing it rather than documenting it a third time.

Options: (a) make strict the default (a green suite then means the guard ran),
(b) have the harness build the codegen binary itself, (c) leave it.
**Recommendation: (a) plus (b)** — but (a) turns every environment without LLVM
into a hard failure, which is a CI-policy call.

---

## Explicitly NOT in scope

**O023 — harness lock coverage (14 of 56).** Its own entry classes it a *lead,
not a confirmed defect*, notes that cargo's target-dir lock already serialises
these builds so adding `flock` changes queueing rather than correctness, and
records that an earlier attempt to add a second flock **deadlocked and left a
stale holder blocking later runs**. The entry's instruction is to *measure the
correlation first*. Building it would be acting against the source's explicit
warning; measuring it is a separate task nobody has scoped.

---

## Open questions for Step 2

1. **S1's compatibility story** — version-tag new records, or write a sidecar
   anchor file? A sidecar is deletable by the same attacker who truncates; a
   version tag keeps everything in one file that old readers can still parse.
   Resolve before building.
2. **S1's threat model boundary** — an attacker who can truncate the file can
   also rewrite it wholesale. What does the anchor actually buy? State the
   answer explicitly, or S1 risks being security theatre.
3. **S2's blast radius** — running the required tests inside a gate script makes
   the gate much slower. Does it stay in the default gate or move to `--strict`?

## Stop condition

```
DONE = S1, S2 each DONE or blocked-and-logged
   AND S3, S4 measured, or blocked because the atlas tree is occupied
   AND H1–H6 written up with recommendations and NOT built
   AND O023 left alone, with its reason recorded
   AND cargo test --workspace shows no new failures vs the Step 0 baseline
```
