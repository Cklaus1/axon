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

> **[REVISED — S1 SPLIT. Open question 2 was not a question; it is decisive, and
> most of S1 as written would have been security theatre.]**
>
> `compute_entry_hash` (`axon-audit/src/lib.rs:156`) is an **unkeyed SHA-256**.
> There is no secret anywhere in the chain. So an adversary who can write the
> ledger file can recompute a perfectly valid chain of any length — and
> truncation does not even require that much, since a prefix of a valid chain is
> itself a valid chain.
>
> The consequence for the proposed fix: **an in-file anchor buys nothing.** A
> stored entry-count or tip-hash is written by the same process, into the same
> file, protected by the same absent key. Any adversary who can delete the last
> K lines can also edit the count. Shipping that and calling the truncation hole
> closed would be worse than leaving it open, because the ledger would then
> carry an explicit claim it cannot support.
>
> What the chain actually provides today is narrower than "tamper-evident": it
> detects **inconsistent** edits — someone who changes a line without
> recomputing the hashes after it. That is real, and it is all there is.
>
> So S1 splits along exactly the line the key falls on:
>
> - **S1a — buildable, key-free, and it defends the threat O-RLM-05 names.**
>   The named threat is "an audited program erases its own trailing FS/Exec
>   records" — the program is running *under the auditor*, which holds the
>   `Ledger` in memory with `entries` it knows it appended. The live ledger can
>   detect that the file no longer matches what it wrote, without any key,
>   because it is comparing against its own memory rather than against the file.
>   That is a genuine, provable improvement and it is what S1 becomes.
> - **S1b — `needs-human`, and it is the same decision as H4 (O035).**
>   Post-hoc authenticity against an adversary who rewrites the file needs a MAC
>   or a signature, or an append-only medium the auditee cannot write. Both are
>   key-management decisions. **S1b and H4 should be decided together** — they
>   both want a signing identity, and answering one answers most of the other.
>
> The acceptance above applies to S1a and is amended: the truncation must be
> detected **by the running ledger**, and the test must state that a post-hoc
> `verify()` on a truncated file still passes, because it does and pretending
> otherwise is the failure mode this review exists to prevent.

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

> **[REVISED — the sweep is three gates, not a hypothetical.]** `grep -l
> REQUIRED_NAMES scripts/*.sh` returns **`r31`, `r33` and `r34`**, all with the
> same `grep -q "$name" "$LIB_SRC" "$VM_SRC"` loop. Fixing only r31 would leave
> two gates making the identical false claim, so all three are in scope and the
> task is sized accordingly.
>
> One caution the entry does not mention: these gates are invoked from
> `cargo test` wrappers, and making them *run* `cargo test` means a cargo
> invocation nested inside a cargo invocation — which is how O036's build-lock
> contention arises in the first place. Prefer parsing one already-running
> suite's output over spawning a new `cargo test` per required name; if a spawn
> is unavoidable, it must tolerate the parent's build lock rather than reporting
> a lock timeout as a missing test. Resolved in Step 2 as E2.

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

---

## Decisions — build-loop Step 2, 2026-08-06

### E1 — S1a's mechanism: compare against memory, not against a stored count (engineering)

Options: (a) store an entry-count in the file; (b) sidecar anchor file;
(c) **the live ledger re-reads the file and compares against `self.entries`**.

(a) is forgeable by the same adversary — see the Step 1 revision. (b) is
deletable by that adversary and adds a second file to keep in sync. (c) needs no
key and no format change at all, because the comparison is against process
memory the adversary does not have. **Chosen: (c).**

That also resolves open question 1 (version-tag vs sidecar): **neither.** The
on-disk format does not change, so the compatibility problem that made this
Structural disappears, and an existing ledger keeps verifying because nothing
about how it is written or read has changed.

### E2 — S2 parses one suite run; it does not spawn cargo per name (engineering)

Spawning `cargo test` per required name from inside a script that `cargo test`
invoked is nested cargo, which contends on the same build lock O036 is about —
the gate would then be flaky for the same reason the parity harnesses are.

**Chosen:** run the crate's suite **once**, capture the output, and require each
required name to appear with an `ok` result. That catches `#[ignore]` (which
reports `ignored`, not `ok`) and a renamed-to-comment (which reports nothing at
all), which is precisely what name-grepping cannot do.

### E3 — open question 3: does S2 stay in the default gate? (engineering)

The gates already shell out to real work; the added cost is one suite run for
the crates involved, not the workspace. **Chosen: stays in the default gate.** A
gate moved to `--strict` because it became honest is a gate that stops running.

### E4 — `needs-human`, not built, no default-off flags

**H1 (O026)**, **H2 (O031)**, **H3 (O032)**, **H4 (O035)**, **H5 (O030)**,
**H6 (O036)** — each carries options and a recommendation in Tier 2 above, and
none is adopted. Added by Step 1: **S1b**, which is the post-hoc-authenticity
half of O-RLM-05 and belongs with H4, since both need one signing-identity
decision.

That is **seven** `needs-human` items against **four** buildable ones.

### E5 — O023 stays out, on its own entry's instruction

Its source says LEAD not defect, notes cargo's target-dir lock already
serialises these builds, and records a prior flock attempt that deadlocked and
left a stale holder. The instruction is to measure the correlation first. Not
built, and not "measured" either — that measurement is unscoped work, not a
task this spec can carry.
