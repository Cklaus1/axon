# Flight plan — the HIGH tier

**Mission.** Close the two security/gate defects that need no policy decision,
take the one measurement that decides the diagnostics question, and hand back
seven decisions instead of guessing them.

**Decisions (Step 2)**
- E1 S1a mechanism — stored count / sidecar / **compare against process memory** → chosen: the other two are forgeable or deletable by the same adversary; memory is what the adversary does not have. Also removes the on-disk format change entirely.
- E2 S2 mechanism — spawn `cargo test` per name / **parse one suite run** → chosen: nested cargo contends on the build lock O036 is about, so the honest gate would be flaky for the same reason.
- E3 S2 placement — `--strict` / **default gate** → chosen: a gate moved to strict because it became honest is a gate that stops running.
- E5 O023 — measure / fix / **leave alone** → chosen: its own entry says LEAD-not-defect and records a prior fix that deadlocked.
- ⚠ close call: **S1a's scope.** It defends the threat O-RLM-05 names and nothing wider. A reader who wants "the ledger is now tamper-evident" will not get it, and the commit must say so plainly.

**needs-human — 7 excluded, vs 4 built**
S1b (post-hoc ledger authenticity) · H1 O026 unmanifested-guest default · H2 O031
native-vs-interp AI capability · H3 O032 budget scope (a spec amendment) · H4
O035 quorum signing key · H5 O030 SQL dialect contract · H6 O036 harness-strict
default. S1b and H4 want the same signing-identity decision and should be taken
together.

**Step 1 `[REVISED]` markers** — S1 split, because `compute_entry_hash` is an
UNKEYED SHA-256 and an in-file anchor is forgeable by the same adversary who
truncates · S2 is three gates (r31/r33/r34), not one, plus a nested-cargo hazard
the source entry does not mention.

**Critical path — S2.** Gate: it must fail when a required test is `#[ignore]`d,
which a name-grep provably cannot detect.

**Shape.** 4 tasks · 2 tiers · longest chain 2 · 2 repos · baseline **1773/0
adopted** · budget unbounded · full suite ~7m30s. Tier 2 is expected to be
BLOCKED: the atlas tree is on `main` holding another session's untracked files.

**First three tasks**
1. **S1a** — live ledger detects truncation of its own file; no format change.
2. **S2** — r31/r33/r34 parse a real suite run; `#[ignore]` must fail them.
3. **S3** — re-measure, if the atlas tree is free; otherwise log blocked.
