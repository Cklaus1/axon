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

---

## D-002 — CX-15 L2-3 CONFLICT: ambient ceilings are ignored by the native engine

**Spec passage** (`specs/CX-15-language-compiler-proof.md`, L2):

> Ambient interpreter constraints require explicit native runtime/launcher
> support before Cortex enables that target. **The lack of a call site is not an
> excuse for quietly ignoring policy.**

**Live passage** (`CLAUDE.md`, `AXON_ALLOWED_EFFECTS` row):

> **Interpreter-only**, like the rest of F5 — a natively-built binary ignores
> it, and being ambient there is no call site to E0910-refuse at.

The repository states, as a documented design decision, precisely the excuse the
spec names and forbids.

**Live implementation — verified by grep, both directions.**

* `crates/axon-core/src/codegen/` contains **zero** reads of
  `AXON_ALLOWED_EFFECTS` or `AXON_BUDGET_TOKENS`.
* Both are read only in the interpreter (`crates/axon-core/src/interp.rs:3655`
  for the token cap; the effect ceiling nearby in the same file).
* `crates/axon-guest-init/src/main.rs:171,177` **exports both into the guest**
  (`env::set_var`), and that binary REFUSES TO BOOT when the MMDS policy cannot
  be read (`AXON_GUEST_ALLOW_NO_POLICY=1` opts out, development only).

**Why it matters — this is worse than an ordinary gap.** The launcher refuses to
start without a policy, then attests that the policy was applied, then hands the
payload two environment variables that a natively-built payload silently
ignores. The effect ceiling (SandboxViolation, exit 8) and the AI token cap
(E1303, exit 5) are both absent from `axon build` output. An operator reading
the boot sequence has every reason to believe the ceiling is in force.

Note the failure direction: the interpreter is the STRICTER engine here, so
running interpreted is safe and running native is not — the opposite of the
usual "native is the optimised path" intuition.

**Status of this check.** Verified by grep over the live tree (zero reads in
`codegen/`, the two `env::set_var` call sites in guest-init). NOT verified by
building a native binary under a set ceiling and observing the violation — that
end-to-end demonstration is the obvious next evidence step and is not claimed
here.

**Proposed resolution.** Extend the EXISTING refusal path rather than
implementing native enforcement now: `axon build` should REFUSE (E0910 is the
allocated mechanism, already dense in `codegen/`) when an ambient ceiling is set
in the environment, instead of emitting a binary that cannot honour it. That
converts a silent gap into an explicit refusal, which is the repository's own
stated convention for "interpreter has it, codegen cannot express it". Native
enforcement, if wanted, is a separate and larger piece of work.

**Invariant link.** Protected kernel: *capability/effect enforcement*. The
proposed refusal NARROWS what `axon build` will emit, so it needs no migration.

**Owner.** Repository owner.
**Blocks.** CX-15 `G15-parity` cannot honestly report "supported cases agree on
effects and budgets" while budgets are uncomparable across engines.

---

## D-003 — The approval friction ladder is computed and read by nothing

**Spec requirement** (CX-00 R4): *"A required gate that is absent, skipped,
unsupported, **expired** or inconclusive MUST NOT be treated as passing."*

**Live implementation.** `crates/axon-intent/src/policy.rs` derives an
`ApprovalPolicy { threshold, ttl_ticks, max_uses }` per risk tier — three
approvers, a 100-tick TTL and single use for Critical. A repository-wide grep
for `policy_for|ApprovalPolicy|ttl_ticks|max_uses` **outside that one file
returns zero hits**. `axon_os::approval::verify_approval` checks digests only:
no TTL, no use count, no approver count.

To the file's credit, its own doc comment says so: *"This slice only derives the
policy from risk — it does not yet enforce anything (S3)."* The defect is not
dishonesty in the source; it is that nothing downstream records the distinction,
so an approval token cannot expire and a Critical action needs one approver.

**Status of this check.** Verified by grep. The absence of a caller is
conclusive for "nothing reads it"; whether the intended enforcement point is
`verify_approval` is a design reading.

**Why it matters.** This is the "ambient controls were inert" class the
repository has already been bitten by twice — `AXON_ALLOWED_EFFECTS` and
`AXON_BUDGET_TOKENS` were both documented as enforced while being read by
nothing, and only a behavioural diff caught it. Here the same shape recurs in
the approval path, which is the control an operator reaches for first.

**Proposed resolution.** Extend `crates/axon-os/src/approval.rs::verify_approval`
— the enforcement point that already exists and is already called at the run
boundary — to consult the derived policy. Do NOT add an enforcement crate.

**Owner.** Repository owner.

---

## D-004 — `POLICY_FILES` protects four basenames, not the gate surface

**Spec requirement** (CX-00 R6 / `G00-authority`): a learned component *"MAY
propose authority, gate or checker changes but MUST NOT activate them"*; the
gate simulates *"a learner attempts to edit gate code, evaluation data, policy
or signer credentials."*

**Live implementation.** `crates/axon-cortex/src/runner.rs:78`:

```rust
const POLICY_FILES: &[&str] = &["axon.lock", ".axon-policy", "gate.sh", "profile.rs"];
```

matched with `target_path.ends_with(p)`. Not barred: every other
`scripts/*.sh` gate (including `gate_verdict_is_read.sh` and
`cortex_package_gate.sh`), `governance/cortex_gate_execution_registry.json`,
`AXON-COMPLETENESS.json`, `crates/axon-core/tests/cli_run.rs`, and
`crates/axon-cortex/src/locate.rs` itself.

**Status of this check.** Verified by reading the constant and its single use.
NOT verified by attempting such an edit through the loop — the authority
corpus that would test this is designed but unbuilt (see the manifest row
"authority-discrimination corpus").

**Why it matters.** `G00-authority` is satisfied for `gate.sh` and nominally
for three other names. The evaluation matrix and the gate registry — the two
artifacts that decide whether a change is admitted — are writable.

**Proposed resolution.** Make `POLICY_FILES` a PREFIX set rather than a
basename list, covering `scripts/`, `governance/` and the completeness matrix.
A prefix set is also what the repository's own write-prefix grant machinery
already uses, so this reuses an existing idiom.

**Owner.** Repository owner.

---

## D-005 — CONFLICT: self-report is written into the evidence channel as a passing check

**The crate breaks, in its own evidence trail, the rule it states in its own
module docs.** `crates/axon-cortex/src/lib.rs:11`:

> **"Absent, null/None, empty and Unknown are not interchangeable."** … a
> missing fact cannot be silently rendered as a present one. This is the
> absent-vs-passed collapse the rest of this repo keeps finding, stated as
> a type.

**Spec passages.** CX-10 v0.13 appendix (NEW in v0.15, absent from the v0.7
package the crate cites as its basis):

> Outcome labels must come from downstream evidence/verification or remain
> unknown; **component self-report is not retrospective ground truth**.

CX-04 §Graph contract:

> **No implicit empty artifact, default success**, guessed distribution or
> hidden provider fallback is permitted.

**Live implementation — verified verbatim.** `crates/axon-cortex/src/episode.rs:41`
documents the variant as:

> `/// A registered check, with its real exit code.`

and two call sites push events that are neither:

* `crates/axon-cortex/src/runner.rs:543` —
  `CheckRun { name: "claim_done", exit_code: 0, passed: claim.done }`.
  No process ran. The exit code is invented, and `passed` is **the candidate's
  own claim about itself**.
* `crates/axon-cortex/src/runner.rs:794` —
  `CheckRun { name: format!("patch_proposed_by:{}", …), exit_code: 0,
  passed: true }`. Not a check at all — generator identity — with both fields
  fabricated.

A patched step therefore emits TWO synthetic passing `CheckRun` events
alongside the real ones.

**Blast radius — measured, and contained TODAY.** `Episode::verified_ok`
(`episode.rs:110`) matches only `EpisodeEvent::Verified { passed: true, .. }`,
so the adjudication path is NOT fooled by the synthetic events. Confirmed by
reading the function. But the whole episode ships as `cortex-repair/1` JSON
(`bin/cortex.rs:453`), and any consumer computing a check pass-rate — which is
exactly what a CX-10 learning export does — would count self-report as
evidence. The containment is a property of one function, not of the record.

**Status of this check.** Verified by reading the two call sites, the variant's
doc comment, and `verified_ok`. NOT verified by a test: the conformance test
asserting an exact event-kind trace runs the NON-AI generator path, so
`patch_proposed_by` is never exercised by any assertion.

**Why it matters most.** This is the defect class the whole session has been
closing — a non-observation rendered as an observation — occurring inside the
crate written specifically to refuse it, in the channel that a learning
pipeline would treat as ground truth.

**Proposed resolution.** Add two variants to the EXISTING closed enum in
`crates/axon-cortex/src/episode.rs`: `Claimed { rationale }` and
`Proposed { generator_id }`. Neither carries an exit code or a `passed` field,
because neither is a check. This is a schema-version bump, which
`schemas/PROTOCOLS.md` §Compatibility already requires; the compiler will force
the conformance test's match arms to be updated, which is the right forcing
function. No new files.

**Invariant link.** Protected kernel: *provenance integrity*.

**Owner.** Repository owner.
**Blocks.** CX-10 `G10-lineage` must not be reported as satisfiable while the
evidence channel carries fabricated checks.
