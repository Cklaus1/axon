# Build harness — the HIGH tier spec

**Placeholders:** `<TEST_CMD>`=`cargo test --workspace` · `<RUN_CMD>`=`./target/debug/axon`
· `<FULL_SUITE_BUDGET>`≈7m30s · `<RUN_BUDGET>` unbounded · `<REVIEW_MODEL>`=self.

**Baseline — adopted.** `tasks/rlm2-final.txt`: **1773 pass / 0 fail**,
`BASELINE_FAILURES = { }`, at `e715ddb`. Delta to now is two markdown files
(`git diff --stat e715ddb..HEAD`), which cannot affect a test. Empty failure set
⇒ the gate is exact.

## DAG

```
 TIER 1 — code, no decision needed
   S1a  live ledger detects truncation of its own file     (axon-audit)
   S2   r31 + r33 + r34 gates RUN their tests              (scripts/)   ★CRITICAL PATH★
        └─ disjoint file scopes; S2 is critical because two other gates
           currently make the same false claim and nothing else in the repo
           would catch it.

 TIER 2 — measurement, blocked on a tree this run does not own
   S3   re-measure with the corrected char-literal advice   (atlas)
   S4   add character literals to LANGUAGE_CARD, measured ALONE (A2b)
        └─ S3 → S4 strictly. Both blocked if the atlas tree is occupied.

 PRUNED — needs-human (7): S1b, H1 O026, H2 O031, H3 O032, H4 O035, H5 O030, H6 O036
 OUT OF SCOPE (1): O023, on its own entry's instruction
```

Topological order: S1a, S2, S3, S4. Four nodes, one edge, acyclic. Coverage:
every one of the eleven source entries is either a Tier-1/2 node, a pruned
`needs-human` item, or O023 with its reason recorded.

## Critical path — S2, and its extra gate

Not S1a: S1a is one crate with no dependents. S2 is critical because r33 and r34
carry the identical name-grep loop, so a fix to r31 alone leaves two gates
asserting a property they never check — and a vacuous gate is the failure mode
that let P4-OS-11 ship.

**Extra gate (beyond a regression test):** the discriminating test is that the
gate must fail when a required test is `#[ignore]`d. A name-grep cannot tell an
ignored test from a passing one; a result-parser can. That single case proves
the gate changed kind, not just wording.

## Loops

Inner / outer / meta / regression / smoke as in `tasks/build-loop-rlm.md`
§Loops, unchanged. One addition:

- **Claim-honesty check (S1a only).** Before committing, re-read what the code
  now *claims* in its doc comments and error strings, and confirm the claim
  matches what the mechanism can actually enforce without a key. This spec
  exists because the obvious version of S1 would have claimed more than it could
  deliver; the check is what stops that recurring one layer down.

## Smoke

Concrete signal, red today: append 3 entries to a ledger, truncate the last line
underneath the live `Ledger`, and require the next `verify()`/append to report
truncation naming the expected vs actual count. Plus `bash scripts/r31_acceptance_gate.sh`
must fail when a required test is `#[ignore]`d.

## Stop condition

```
DONE = S1a, S2 DONE or blocked-and-logged
   AND S3, S4 measured or blocked (atlas tree occupied)
   AND the 7 needs-human items written up and NOT built
   AND O023 untouched, reason recorded
   AND cargo test --workspace shows no new failures vs { }
```
