# Flight plan — Axon Cortex

**Mission:** build the Cortex package's own "first executable vertical slice" —
observe a broken Axon fixture, propose a patch under an explicit grant, apply
it to a copy, check it, verify independently, replay — and stop there.

**Decisions (Step 2):**
- Scope → M0+M1 only. 44 of 69 packages (M2–M9/Research/Compiler/OS-K) need
  model training, corpora and spend; a loop can scaffold them but not deliver
  them, and scaffolding reported as "built" is the failure the package warns of.
- Home → one crate `crates/axon-cortex`. Not sixteen crates for sixteen specs.
- Namespace → `CX-*` stays package-local; no new R*/E****/exit codes; gate tests
  named `cxg_<id>`.
- Reuse → extend `AxonHost`+journals, `axon_os::{grant,profile}`,
  `sandbox_create_scoped`, audit ledger, `replay.rs`. Do not build a second TCB.
- Done → C11 deterministic conformance run is the smoke test; live-model
  comparison excluded.
- ⚠ close call: C4 "partial observation" could reuse the host journal or read
  the AST directly. Chose the journal — it already has redaction and replay,
  and the spec's own warning is that observation must never be whole-world.

**needs-human (excluded, not dropped):**
- B00 — product naming + CX→repository ID mapping (owner-reviewed by its text).
- B15/B16 — authorized live-model backend: credentials + per-call spend.
- M2–M9 + Research/Compiler/OS-K — 44 packages, research program.

**[REVISED] markers from Step 1:** R1 scope→out-of-scope (44 tasks);
R2 B00 task→needs-human; R3 B15/B16 task→needs-human.

**Critical path:** C2 typed contracts. Extra gate: round-trip byte-identity +
a rejection proof per malformed shape. BLOCKED here stops the run.

**Shape:** 12 tasks · 7 tiers · longest chain 7 · 3 parallel tracks after C2 ·
TEST_CMD `cargo test --workspace` (~14 min) · RUN_BUDGET unbounded ·
REVIEW_MODEL self (adversarial) · baseline = 1 known flake, characterised.

**First 3 tasks:** C1 verify the package's existing-Axon claims against the
live repo · C2 typed event/manifest contracts · C3 resettable broken fixture.
