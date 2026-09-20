# Cortex v0.15 ↔ live Axon — discrepancy records

The build package states its own status as **"documented, not rerun"** and
instructs: *"Repository paths are taken from the attachment and must be
confirmed before editing"* and *"For conflicting documentation, add a
discrepancy record with both passages, observed implementation, proposed
resolution, owner, and linked invariant change."*

This file is that register. One record per conflict. A record is only added
after the live behaviour has been checked — a disagreement between two
documents is not a discrepancy until one of them is measured against code.

Package integrity was re-verified independently before any of this:
`SHA256SUMS_v0_15.json`, 113/113 match, 0 mismatched, 0 missing.

---

## D-001 — `EXISTING_AXON_MAP.md`: "Empty scoped list is not deny-all" is STALE

**Package passage** (`EXISTING_AXON_MAP.md`, row "Sandbox APIs", column
*Required check / new work*):

> Empty scoped list is not deny-all; untrusted native tools need process
> isolation outside interpreter checks.

**Live implementation.** The opposite now holds, and it was a deliberate,
documented inversion. Three independent sources in the repository agree:

* `crates/axon-core/src/builtins.rs` — the `sandbox_create_scoped` doc string:
  *"`\"\"` DENIES everything on that axis; `\"*\"` is unrestricted; a list
  means every path/host must match an entry. Matches the `AXON_ALLOWED_EFFECTS`
  convention, where an empty value denies."*
* `crates/axon-core/src/interp/builtins.rs:4064` — *"`Some([])` = nothing
  matches, i.e. deny-all."*
* `CLAUDE.md` (capability policy) records the change WITH its measurement:
  before the inversion, *"a scope of `\"\"` let a call to an arbitrary host
  through even when the principal did not grant net."*

**Status of this check.** Verified by reading three agreeing sources in the
live tree, NOT by an end-to-end run. The builtin is interp-only and its
harness was not executed for this record. That is a weaker class of evidence
than the package asks for and is stated rather than glossed.

**Why it matters.** The package's sentence is a WARNING to an implementer
("do not assume empty means deny"). Following it against today's code would
produce the inverse of the intended policy: an implementer would add a
belt-and-braces deny that is already there, or worse, treat an empty list as
permissive somewhere new — reintroducing precisely the hole CLAUDE.md records
as measured and closed.

**Proposed resolution.** GOVERNED UPDATE to the package row, not a change to
live code. Replace the passage with: *"An empty scoped list IS deny-all
(`\"\"` denies, `\"*\"` is unrestricted) as of the capability-profile
inversion; confirm the convention still holds at the call site before relying
on it. Untrusted native tools still need process isolation outside interpreter
checks — that half of the row stands."*

**Invariant link.** Touches the protected-kernel item *capability/effect
enforcement*. The live semantics are the STRICTER of the two readings, so
adopting them narrows authority and does not require a migration.

**Owner.** Repository owner (package is third-party input).
**Blocks.** B01 (audit Axon seams) must not copy this row forward unrevised.
