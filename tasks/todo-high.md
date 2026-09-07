# TODO — the HIGH tier spec

Derived from commit history; attempt counts in `tasks/attempts.log`.

## Tier 1 — built
- [x] **S1a** — a live ledger detects truncation of its own file — `bf95e27`
  - [x] mechanism compares against process memory, so no key and no on-disk format change
  - [x] the honest limit asserted by test: a post-hoc reader still cannot detect it
  - [x] caller check caught it unwired; `flush_ledger` verifies and the CLI reads the result
  - [x] both halves mutation-verified
- [x] **S2** — r31/r33/r34 run their named tests instead of grepping for them — `39580ed`
  - [x] three gates, not one
  - [x] one suite run parsed per gate (no nested cargo-per-name)
  - [x] discriminating mutation: `#[ignore]` now fails the gate, exit 1
  - [x] r31 ✅ (10 names) · r33 ✅ (21 names) · r34 ❌ **pre-existing**, HEAD fails identically

## Tier 2 — BLOCKED (not failed)
- [ ] **S3** — re-measure with the corrected char-literal advice
- [ ] **S4** — add character literals to `LANGUAGE_CARD`, measured alone (A2b)

  **Why:** the atlas working tree is on `main` holding another session's
  untracked files, and `bin/axon_card.rs` is not in it. Checking my branch out
  would disturb a dirty tree this run did not create. The work itself is
  committed and pushed (`35ac5f4`); only the *measurement* is blocked. S4
  depends on S3, so it is transitively blocked. Neither is critical-path, so the
  run completed rather than halting.

## needs-human — 6 (was 7; one was already closed)
- [ ] **S1b** — post-hoc ledger authenticity. Needs a MAC/signature or an
      append-only medium. **Decide with H4** — one signing identity answers both.
- [ ] **H2 (O031)** — native links `axon-ai` unconditionally (`link.rs:334`,
      confirmed still live), so `axon build` can make AI calls `axon run` refuses.
- [ ] **H3 (O032)** — AI budget uses the *current* fn (`current_ai_budget`,
      confirmed still live). Changing it amends `R3c-ai-budget-meter.md` §3.
- [ ] **H4 (O035)** — quorum votes unsigned (the code's own comments confirm).
- [ ] **H5 (O030)** — SQL dialect contract.
- [ ] **H6 (O036)** — should `AXON_HARNESS_STRICT=1` be the default?
- [x] ~~H1 (O026)~~ — **ALREADY CLOSED** by AUDIT T48 (`axon-vm/src/main.rs:1114`).
      Carried into this spec from a stale opportunity entry. See O-HI-02.

## Out of scope
- [ ] **O023** — untouched, on its own entry's instruction (LEAD not defect; a
      prior flock attempt deadlocked).
