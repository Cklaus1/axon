# Cortex v0.15 ↔ live Axon — discrepancy records

## Resolution status

| id | subject | state | branch |
|---|---|---|---|
| D-001 | "empty scoped list is not deny-all" is stale | package update proposed | — |
| **D-002** | ambient ceiling ignored by native | **FIXED** — `axon build` refuses (E0910); mutation-verified | `fix/native-ceiling-parity` |
| D-003 | approval TTL / max-uses read by nothing | open | — |
| **D-004** | policy surface was four basenames | **FIXED** — prefix set; mutation-verified | `fix/cortex-evidence-channel` |
| **D-005** | self-report written as a passing check | **FIXED** — `Claimed`/`Proposed`; mutation-verified | `fix/cortex-evidence-channel` |
| D-006 | approval row: required work already landed | package update proposed | — |
| D-007 | four governance registries already exist | consolidation proposed | — |
| **D-008** | cancellation killed only the direct child | **FIXED** — `process_group` + `killpg`; differential-verified | `fix/kill-the-process-group` |
| D-014 | v0.22 added three crates; "No new crates" and the crate table said otherwise | recorded (docs corrected on `v022/stage1-c`) | `v022/stage1-c` |
| D-015 | D-C1: `axon-loop` keeps a second, unkeyed ledger beside `axon-audit`'s keyed chain | **partly resolved** (keyed under `AXON_ATTEST_KEY`: F1/F2/R2 detected); R1 + no-key default **open** | `v022/stage2-2a` |
| D-016 | D-C2: Fabric admits under a hard-coded grant; cortex hard-codes principal + zero policy digest | **fixed for the Fabric** (Stage 2 lane 2B); `axon-loop` plan approval still open | `crates/axon-fabric/tests/grant_authority.rs` |
| D-017 | D-C3 / D-C5: duplicate `acf1:` canonicaliser; second reservation algebra | **fixed** (Stage 2 lane 2B) | `crates/axon-cortex/tests/fabric_acf1.rs`, `crates/axon-fabric/tests/journal.rs` |
| D-018 | D-C6: "admission" names two different things; an ACCEPT is never a grant | invariant recorded; **open** until a test pins it | — |
| **D-019** | `axon-vm` library entry bypassed `cmd_run`'s pre-launch gates | **FIXED** — `axon_vm::admit` holds the four gates; `LaunchSpec` requires an `AdmittedLaunch` only `admit()` can build (compile_fail doctest); quorum/chain remain CLI-only (`80a46767`); ACF-G22 cleanup (`aafd3f8f`) | `crates/axon-vm/src/admit.rs`, `tests/launch_cleanup.rs` |
| D-020 | `linux-microvm-protected` is enclosure-only; eligibility ignores BLOCKED | **part fixed** — `HardwareIsolated` no longer accepts an unqualified VM (S3-7, `0f21a9fd`, mutation-checked); qualification now requires issuer-signed, unblocked-or-waived, fresh, clean evidence (S3-1, `5fbf44d3`); the in-guest policy channel (S3-5), the x3 waiver (D7) and signed re-qualification (S3-6) still open — Stage 3 | `crates/axon-os/tests/hardware_isolation.rs` |
| D-021 | v0.22 package `EXISTING_AXON_MAP.md` repeats two stale claims | package update proposed | — |
| **D-022** | interp stamped AI-sourced `Uncertain` as user-constructed, at the constructor and through arithmetic (both engines) | **FIXED** — ported; mutation-verified | ported from `D-014@upgrade/cortex-v0_20` |
| **D-023** | reflex reply parse was last-wins; an id-less request was served as id 0 | **FIXED** — ported; mutation-verified | ported from `D-015@upgrade/cortex-v0_20` |
| D-024 | CX-35 claimed by the repo (axon-reflex) and reserved by the package; CX-37 overlaps it | open — owner decision | ported from `D-016@upgrade/cortex-v0_20` |
| D-025 | model self-reported confidence gates `@[verify]` / `deploy --gate verify` | open — owner decision | ported from `D-017@upgrade/cortex-v0_20` |
| D-026 | typed approval/record structs silently drop unknown fields | open — fold into R24's `axon-approval/2` | ported from `D-018@upgrade/cortex-v0_20` |
| D-027 | `axon trace --replay` re-runs LIVE AI calls under a "replaying" banner | open | ported from `D-019@upgrade/cortex-v0_20` |
| D-028 | estimated token usage is indistinguishable from reported | open | ported from `D-020@upgrade/cortex-v0_20` |
| D-029 | package fixture bundle carries the G10 price-schedule mismatch B270 now refuses | package — file upstream | `crates/axon-loop/tests/tel_price.rs` |

None of the four fixes is on `main` yet: a `gate.sh --strict` run is in flight
and several parity harnesses rebuild, so integrating mid-run would invalidate
it. Each branch is one coherent slice and carries its own evidence.

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

**Status of this check — UPGRADED to end-to-end execution.** Originally
recorded as grep-only. Now demonstrated with the same program under the same
ceiling, one engine each:

```
$ AXON_ALLOWED_EFFECTS=Pure axon run c.ax
axon: sandbox violation: builtin `println` requires effect `IO` which is not in
      the active sandbox's allowed set {"Pure"} (principal handle 0)
exit 8

$ AXON_ALLOWED_EFFECTS=Pure axon build c.ax -o cbin
Compiling c.ax...
Binary: cbin (1196ms)
exit 0                       <-- no refusal; the binary is emitted

$ AXON_ALLOWED_EFFECTS=Pure ./cbin
IO HAPPENED
exit 0                       <-- the ceiling is not enforced
```

The program is three lines and does nothing but `println`. The interpreter
refuses it under the ceiling; the native binary performs the effect and exits
clean. `axon build` does not refuse to emit a binary it knows cannot honour the
ambient policy, which is the specific remedy this record proposes.

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

---

## D-006 — `EXISTING_AXON_MAP.md` approval row: the required work has LANDED; the real hole is invisible

**Package passage** (row "CLI/web approval and risk gate", *Required check /
new work*):

> Bind approval to final content digest; missing required gate becomes
> rejection in Cortex profile.

**Live implementation — BOTH halves already exist, and are tested.**

* Content-digest binding: `crates/axon-core/src/main.rs:7470` (`cmd_ast_approve`
  writes an `axsha256:` digest) and `:7999-8044` (deploy recomputes the CURRENT
  source's digest and compares; approved-then-edited is exit 8,
  `status:"blocked_approval"`). The header comment records the measured pre-fix
  defect: approval bound a FILENAME, so one could "approve a benign file,
  rewrite it to exfiltrate /etc/passwd, and deploy still reported
  approved:true".
* Missing gate → rejection: `main.rs:8127-8150`. At Risk ≥ High a missing gate
  fn becomes `failed_gate = missing:…` and blocks unless
  `--allow-missing-gates` is passed, which is surfaced as `gates_override`.
* Evidence class: **a test asserts it** —
  `cli_run.rs:2878 deploy_approval_binds_the_program_text_not_the_filename`,
  plus `:23139/:23160/:23175` for the gate fields. Plain `cargo test`, so
  unconditional.

**What the row does NOT say, and should.** The genuine hole is that approval
binds ONE FILE of a program:

* `cmd_ast_approve` hashes only the entry file's bytes. Demonstrated earlier in
  this session: approve `main.ax`, rewrite an imported module to
  `write:["/"] net:["*"] exec:any`, and the approval still validates.
* A transitive mechanism EXISTS — `axon lock` / `verify-lock`
  (`main.rs:1211-1320`, transitive closure, `axh1:` hashes) — and **no approval
  or deploy path calls it**.
* `ast review` showed imported fns as the entry file's own with no origin
  marker. FIXED this session (impl methods and origin now rendered), but the
  digest gap remains.
* Import resolution is itself order-dependent with no record of which file was
  chosen (see D-001's sibling finding, recorded in the completeness matrix), so
  no digest can distinguish two meanings of the same source.

**Why it matters.** An implementer following this row rebuilds digest binding
and gate-rejection from scratch — both already exist with subtle correct
behaviour they would likely lose (the legacy-hash non-blocking arm at
`main.rs:8019-8027` exists because changing the hash algorithm without it told
upgrading users "source changed since approval" about files nobody had edited).
Meanwhile the real defect is invisible in the row, so a Cortex profile inherits
it.

**Proposed resolution.** Rewrite the row to: entry-file binding and Risk≥High
missing-gate rejection ARE landed and tested at the cited lines; the OPEN work
is binding the IMPORT GRAPH — reuse `axon lock`'s existing transitive hash
rather than building a second one — and deciding whether a MISSING approval
should reject (today it warns by design, which is the genuine Cortex-profile
delta).

**Owner.** Repository owner.

---

## D-007 — The "do not build a second governance registry" row is already violated four times

**Package passage** (row "R39 typed governance/evidence graph"):

> Import CX specs after namespace reconciliation; **do not build a second
> governance registry**.

**Live implementation.** FOUR registries exist, all carrying a completion claim
plus evidence paths, at three different granularities, for overlapping subjects:

| registry | schema | read by | gated |
|---|---|---|---|
| `AXON-COMPLETENESS.json` | `axon-completeness/1` | `scripts/completeness.py` | YES, unconditional (`gate.sh:159`), fails on a nonexistent evidence path |
| `governance/state/specs.jsonl` | `axon-gov-spec/1` | R39 slice gates | yes, `--strict` only |
| `governance/REQUIREMENTS.md` | hand-maintained markdown | **nothing parses it** | **no** |
| `governance/cortex_gate_execution_registry.json` | `cortex-gate-execution/1` | `scripts/cortex_package_gate.sh` | YES, unconditional (`gate.sh:92`) |

**Why it matters.** The row reads as PREVENTION. The live job is
CONSOLIDATION across four that already exist — and specifically deciding which
is authoritative, given that the UNGATED one (`REQUIREMENTS.md`) is the one
`gate.sh`'s own comments cite as the evidence index.

**Proposed resolution.** Do not add a fifth for CX gates.
`governance/cortex_gate_execution_registry.json` already exists for exactly
this purpose, is validated by a gate that runs unconditionally, and rejects a
row naming a script nothing invokes. Its `gates` and `tasks` arrays are empty —
that is where CX gate execution belongs, NOT in the vendored
`gate_manifest.json`, which is SHA-pinned and whose own validator hard-fails on
a non-`NOT_RUN` result.

**Owner.** Repository owner.

---

## D-008 — live-code defect, not a package discrepancy: cancellation kills only the direct child

**Live implementation.** `crates/axon-os/src/runtime.rs:187` (timeout path) and
`:196` (kill-file latch path) call `child.kill()` — SIGKILL to the DIRECT CHILD
only, not the process group. Under `exec: any`, which is the `developer`
profile default, any grandchild the job spawned survives both paths.

**Why it matters.** The operator kill switch (R27) and the compliance monitor
(R29) both terminate through this path. A job that spawned a subprocess is not
stopped by either.

**Status of this check — CONFIRMED BY EXECUTION.**

```
$ ps ... sleep 4002                                    -> 0
$ AXON_OS_TIMEOUT_MS=8000 axon-os run gc.axjob
  DENIED: timed out after 8000 ms (axis: time)         <-- supervisor killed it
$ ps ... sleep 4002                                    -> 1   <-- SURVIVED
```

The job `exec`s `sh -c "nohup sleep 4002 &"`, prints, then loops until the
supervisor kills it. The grandchild outlives the kill.

**Four probe iterations, each broken for its own reason — recorded because the
third nearly produced a false NEGATIVE:**

1. `axon-os` could not find the interpreter (`AXON_BIN` is an absolute path, not
   a PATH search) — the job never ran.
2. `pgrep -fc "$MARK"` matched ITS OWN command line, and its multi-line output
   broke the numeric comparison. It reported "2 grandchildren" before anything
   had run.
3. The marker was passed as an extra `sleep` operand — `sleep 400 MARKER` is an
   INVALID invocation, so the grandchild exited immediately and never existed.
   A count of zero then looked like "cancellation works".
4. Correct: a unique DURATION as the marker (`sleep 4002`) plus a
   `ps -eo comm,args | awk '$1=="sleep"'` filter, which cannot match the probing
   shell. Validated standalone first — the grandchild survives a CLEAN exit —
   before being run under the supervisor.

A probe that fails for its own reasons reports "fine" on broken code. Three of
the four here would have done exactly that.

**Proposed resolution.** Kill the process GROUP (`setsid` at spawn +
`killpg`), in the existing `run_bounded`. This is the repository's own
documented lesson from a different context — "kill the job, not a PID".

**Owner.** Repository owner.

---

## D-009 — the ceiling refusal is BUILD-time; the guest path is RUN-time and stays open

**This record exists because my own fix for D-002 named the case it does not
close, in its own comment, and I did not notice until an audit quoted it back.**

**What D-002's fix does.** `refuse_if_ambient_ceiling` is called from every
codegen entry point and refuses to emit an artifact when
`AXON_ALLOWED_EFFECTS` or `AXON_BUDGET_TOKENS` is set IN THE BUILD ENVIRONMENT.
Verified across `axon build` and `axon target build --engine codegen`, with
controls, and mutation-verified.

**What it does not.** The guest sequence is:

1. the image is built on a machine where **no ceiling is set** — so the build
   succeeds, correctly;
2. `axon-guest-init` refuses to boot without an MMDS policy, then SETS
   `AXON_ALLOWED_EFFECTS` and `AXON_BUDGET_TOKENS` at boot
   (`crates/axon-guest-init/src/main.rs:171,177`);
3. it `exec`s the payload. **Nothing on that path re-checks.**

A natively built payload therefore receives both variables and ignores them, in
the one deployment where a policy has been attested as applied. `guest-init`'s
own header still asserts they are "the guest's ENFORCED policy, not just
labels" — true only when the payload is `axon run`.

**Status of this check.** The build-time half is verified by execution. The
run-time half is verified by reading `guest-init`'s `set_var`/`exec` sequence
plus the absence of any ceiling read in `codegen/` and `axon-rt`. NOT verified
by booting a guest with a native payload.

**Proposed resolution — an OWNER DECISION, deliberately not taken here.**
Three options, and they differ in where the authority lives:

* **(a) `guest-init` refuses to exec a payload it cannot constrain.** Consistent
  with what that binary already does — it refuses to boot without a policy — and
  keeps enforcement in the launcher. Needs a way to tell an interpreter payload
  from a native one.
* **(b) the native runtime checks the ceiling at startup and refuses.** Closes
  it everywhere, but puts a policy check inside `axon-rt`, which the brief's
  reviewer explicitly questioned: *"probably should not be 'native binary checks
  the environment variable at runtime' unless that is already the intended Axon
  security model."*
* **(c) native actually ENFORCES the ceiling.** The largest piece of work and
  the only one that makes the capability — not merely the boundary — equivalent.

**Owner.** Repository owner.
**Invariant.** Protected kernel: *capability/effect enforcement*.

---

## CORRECTION (post-audit): D-002 was marked FIXED on a build-time-only remedy

`refuse_if_ambient_ceiling` keys on the environment **at build time**. It stops
`axon build` from emitting a binary while a ceiling is set. It does nothing
about the case that actually occurs in production:

```
axon build c.ax -o cbin                 # no ceiling set — emits happily
AXON_ALLOWED_EFFECTS=Pure ./cbin        # every effect performed, exit 0
AXON_ALLOWED_EFFECTS=Pure axon run c.ax # exit 8
```

Same program, same variable, two engines, opposite answers. The guest path is
exactly this shape: `axon-guest-init` sets the ceiling from MMDS and then
`exec`s a payload built earlier.

The remedy is not new machinery. `__axon_rt_refuse_interp_only_env`
(`axon-rt/src/lib.rs`) already runs in every native binary's prologue and
already refuses three sibling vars at run time. `AXON_ALLOWED_EFFECTS` and
`AXON_BUDGET_TOKENS` are absent from its list — which is why the class stayed
open after being declared closed. This also subsumes D-009, which recorded the
build-time/run-time gap as an owner decision; the audit shows it is broader
than the guest case and the refusal mechanism already exists.

What I got wrong, recorded because the shape recurs: I verified the fix against
the failure I had *reproduced* (build under a ceiling) rather than against the
control's *stated scope* (a run-wide ceiling the program cannot raise). The
test passed, the mutation was caught, and the control was still open.

### Newly found by the same audit (static, not yet executed)

| id | control | engine | shape |
|---|---|---|---|
| D-010 | `AXON_AI_REPLAY` | native | promises "no live call"; native makes a live billed call |
| D-011 | `AXON_PRINCIPAL` | native | no `ai_call` records emitted; audit trail silently empty |
| D-012 | `AXON_SEED` + 3 refusals | wasm | `target_is_wasm` early-returns skip both prologue inits |
| D-013 | `AXON_TEE_ENCLAVE` / `_MEASUREMENT` | registry gate | read via the host seam, which `vars_read()` does not scan — no registry row, absent from `AXON_REFERENCE.md`, while the both-directions gate reports full coverage |

D-013 is the one that undercuts the others: the enumeration tool this whole
matrix is built on cannot see a var read through `with_host`.

#### D-013 verified by inspection (not taken on the audit's word)

`env_registry::vars_read()` scans for exactly two literal forms,
`env::var("` and `env::var_os("`, plus a `const NAME: &str = "AXON_…"` shape.
`crate::host::with_host(|h| h.env_var("AXON_TEE_ENCLAVE"))`
(`crates/axon-core/src/interp/builtins.rs:3531`, `:3538`) matches none of them.

- registry rows for `AXON_TEE_*`: **0**
- env-var rows in `AXON_REFERENCE.md`: **0** — the two matches there are prose
  inside the `tee_in_enclave` / `tee_attest_measurement` doc strings, which is
  documentation of a builtin, not enumeration of a control

So CLAUDE.md's stated guarantee — "a var read without a registry row fails the
build" — does not hold for any variable read through the host seam. The
guarantee is real for `std::env` reads and silently absent for the seam, and
the seam is the better-behaved path (those reads are recorded and replayed).
The gate is not wrong about what it checks; it is wrong about what it claims
to cover, which is the same absent-vs-passed shape as everything else here.

### Post-audit closures

| id | status | how |
|---|---|---|
| D-002 residual | **CLOSED** | both ceilings added to `axon-rt`'s run-time refusal table; build-with-no-ceiling-then-run-under-one now exits 2. Mutation-verified, with an unconstrained-run control. |
| D-009 | **SUBSUMED** by the above — it recorded the build-time/run-time gap as an owner decision; the audit showed it was broader than the guest case and the refusal mechanism already existed. |
| D-010 | **CLOSED** | `AXON_AI_REPLAY` refused on native. Proven first: interp served from cache with no key; native ignored the cache and reached for the live API. |
| D-011 | open | `AXON_PRINCIPAL` — native emits no `ai_call` records at all, so attribution is absent rather than wrong. Refusal is the wrong remedy here: the var does not CAUSE the gap, native simply has no AI audit trail. Needs the trail, not a refusal. |
| D-012 | open | `AXON_SEED` and the three refusals on wasm — both prologue inits early-return on `target_is_wasm`, so a wasm artifact honours no seed and refuses nothing. Note this is the SAME early return that, left ungated in `axon-rt`, broke every wasm build (see commit `94c69b1`): emission is guarded, compilation was not. |
| D-013 | open | the registry's literal scan cannot see a var read through the host seam; verified by inspection, see above. |

**A note on what the refusals do and do not buy.** Native now refuses five
controls it cannot honour. That makes the safety BOUNDARY equivalent across
engines — neither engine performs an effect the interpreter would forbid — and
it does not make the CAPABILITY equivalent. A natively built binary still
cannot enforce an effect ceiling, record a journal, or replay an AI call. Every
matrix row says `explicitly-refused`, not `enforced`, for exactly that reason.

---

# v0.22 candidate records (D-014 … D-021)

Filed 2026-09-24 against `v022/integration@279da778` on branch
`v022/stage1-c` (Stage 1, "candidate honesty"). Every live-code citation is
`git show 279da778:<path>`. Supporting analysis lives in the operator's
untracked `.axon-v022/` directory — `analysis/D_architecture.json`,
`analysis/E_hardening.json`, `analysis/F_guest_vm.json`,
`redteam/axon-loop-r4-independent.md`, `evidence/b263/*.json`,
`integration/gate_279da77.log`. **Those files are NOT in the repository**;
they are cited as operator-side evidence and a reader without that directory
cannot re-check them from this tree. Evidence class for each record: verified
by reading the named source at `279da778` unless stated otherwise. Stage 1
fixes none of the code defects below; it records them so the candidate cannot
read greener than it is.

## D-014 — "No new crates" and the crate table are false on the v0.22 line

**Document passages.** `governance/cortex-v015/IMPLEMENTATION_MAP.md` §4:
*"No new crates. Every gap below has an existing owner."* `ARCHITECTURE.md` §4
listed no `axon-loop-contracts` / `axon-loop` / `axon-fabric`, and said of
`axon-cortex`: *"no CLI verb calls it yet, so nothing here is on a user's
path"*.

**Live implementation.** Workspace `Cargo.toml` at `279da778` has three more
members: `crates/axon-loop-contracts`, `crates/axon-loop`,
`crates/axon-fabric`. The `axon-cortex` sentence was already false on `main`:
the crate's own `cortex` binary (`src/bin/cortex.rs`, commit `d50d6af`, an
ancestor of `main`) is its production caller; what remains true is that no
`axon` CLI verb calls it. The candidate also records the crates in neither
`AXON-COMPLETENESS.json` nor `governance/release-verification.json`, so
`gate.sh` fails at "a completeness claim is not backed by evidence" (3 UNBACKED
crates, `integration/gate_279da77.log`) — those two files are owned by another
Stage-1 lane and are not changed here.

**Resolution.** Docs corrected on this branch: `ARCHITECTURE.md` §4 rows +
"Cortex family on the v0.22 candidate line"; `IMPLEMENTATION_MAP.md` carries a
dated SUPERSEDED note and §4a. The crates are defensible (CX-11, CX-29/CX-33
and ACF-01 had no code owner on `main`); the duplications they introduce are
D-015 … D-018.

**Owner.** Repository owner. **State.** Recorded.

## D-015 — D-C1: a second, unkeyed hash-chained ledger

**Document passage.** `IMPLEMENTATION_MAP.md` §4: *"Lineage chain (CX-10) →
`axon-audit`'s `Ledger` — prev-hash chain, keyed tip, truncation detection, all
built and tested"*, and *not* a new chain.

**Live implementation.** `crates/axon-loop/src/ledger.rs:1-45` is a new
`cl22:`-chained `ledger.jsonl` + `ledger.head` whose doc states the chain is
*"UNKEYED sha256 in the same directory as the data"* and lists as OUT OF MODEL
a consistent truncate-and-rewrite-head (R2), whole-store rollback (R1) and a
well-chained forged append (F1/F2). `crates/axon-audit/src/lib.rs:237`
`open_keyed` (HMAC) and `compute_tip` already close F1/F2/R2 for a holder of the
key. Every "trusted" set in `axon-loop` is an operator premise from
`config.json`, and the ledger stamps no writer identity from trusted runtime
context — weaker than the repository's own `axon-ledger` rule (claim is not
authority; `authenticated_admins`).

**Why it matters.** `axon-loop`'s ledger is described as the store's ONE source
of authority. An authority ledger whose forgery resistance is declared out of
model, next to one that has it, is a regression of an invariant the repository
already paid for.

**Proposed resolution.** Keep the typed event log, anchor its head on
`axon-audit`'s keyed primitives (or witness each entry's ref in an
`axon-audit` keyed chain). **Stage 2.**

**Resolution (Stage 2, lane 2A): partly resolved.** The typed event log is
kept, and it is now keyed with `axon-audit`'s mechanism. The key comes from
the operator key `axon-vm` already uses, `AXON_ATTEST_KEY` (hex, at least 16
bytes). A malformed value is refused rather than dropping to unkeyed. The
MACs reuse `axon-audit`'s own primitive and shape:

* Each entry carries `mac = HMAC(k, digest(entry without mac))`. This is
  `axon-audit`'s keyed `entry_hash`.
* `ledger.head` carries `mac = HMAC(k, seq ‖ entry_ref)`. This is
  `compute_tip`.

The MACs use `axon_attest::hmac_sha256`, and `k` is domain-separated from the
operator key. The key decides how the store is verified, never the files: a
keyed opener refuses an unauthenticated line or head, and an unkeyed opener
refuses a keyed ledger.

**Measured.** This used `tests/keyed_ledger.rs` plus the independent rt4i
harness, regenerated keyed. Under a key:

| attack | result |
|---|---|
| F1 (forged well-chained append + recomputed head + projection) | exit 2 |
| F1 with the head left alone | exit 2. Without the per-entry MAC it would have been rolled forward as a crash tail |
| F2 (forged evaluation line + head, then the real admit) | admit exit 2 |
| R2 (truncate + rewrite head) | exit 2 |

Unkeyed controls of the same attacks still succeed. Two mutations were run:

* Removing the head-MAC check fails the R2 test.
* Removing the entry-MAC check fails the F1 roll-forward test.

**Still open:**

* **No key.** This is the default. There is deliberately no ephemeral key,
  because a per-process key cannot verify what the previous process wrote.
  F1, F2 and R2 stay undetectable.
* **A key holder.** Anyone holding the key can mint any ledger.
* **R1.** Restoring a genuine older keyed state needs a monotonic external
  witness. Witnessing each head in an `axon-audit` `Ledger` was considered and
  not done: that `Ledger` takes no inter-process lock, so concurrent
  `axon-loop` processes would corrupt the witness chain. The OPEN state is
  pinned by `r1_restoring_a_genuine_older_keyed_state_is_still_out_of_model`.
* **R3/NS6c and PF1.** Unchanged.
* **Writer identity.** No writer identity is stamped from trusted runtime
  context (the `axon-ledger` `authenticated_admins` rule). This part is
  **open**.

**Related, same lane (not D-015).** red-team r4 NS3a/b/c is fixed.
Candidate lists and task manifests are now stored under
`<kind>/<tenant>/<family>/<hex>.json`, so registering the same list for a
second scope cannot overwrite the first scope's record. A re-put repairs a
store that the old flat layout bricked. NS4p/NS4w are fixed: EVL never accepts a subject issuer as the preflight
observer of its own trials. NS4b is fixed: an explicit empty
`trusted_observers` is valid, round-trips, and fails closed.

## D-016 — D-C2: a fourth authority vocabulary; admission under a grant that is not enforced

**Document passage.** `IMPLEMENTATION_MAP.md` §5 risk 2: *"A fourth authority
vocabulary. Three exist … build on `axon_os::grant::Grant`."* ACF-01 (v0.22
package) forbids a second identity/approval system.

**Live implementation.**
* `crates/axon-loop-contracts/src/compute.rs:96-97` — `principal_ref` /
  `grant_ref` are `OpaqueRef` strings; `crates/axon-fabric/src/submit.rs` uses
  them only to format `authority_ref`.
* `crates/axon-fabric/src/submit.rs` `supervisor_admits` / `AdmissionProbe`
  (≈ lines 315-398) — admission is `axon_os::supervise_requiring` over a runtime
  that declares an EMPTY effect row and runs nothing, under
  `Profile::Restricted.default_grant(..)` with `require_approval: false`,
  whatever the request says. The check actually executed later is bounded only
  by an OPTIONAL `effect_ceiling` (`SubmitConfig.effect_ceiling: Option<String>`
  → `AXON_ALLOWED_EFFECTS`); `None` means no ceiling. The grant admitted is not
  the grant enforced.
* `crates/axon-cortex/src/bin/cortex.rs:187-189` — under `--fabric-journal`,
  `principal_ref: "cortex:repair"` and `policy_digest: acf1:000…0` are
  hard-coded.
* `crates/axon-loop/src/plan.rs:52,77,166` — plan approval is a self-asserted
  `operator_approved: bool` plus a non-null `approval_ref`; it does not reuse
  `axon-os` approval verification.

**Proposed resolution.** Resolve `grant_ref` to an `axon_os::grant::Grant` and
pass THAT to `supervise_requiring`; enforce the same grant at dispatch; make the
policy digest a required operator input or refuse.

**Resolution (Stage 2, lane 2B).** `crates/axon-fabric/src/grants.rs`: an
operator `axon-fabric-grant-registry/1` file maps `grant_ref` → a grant file
pinned by sha256 and bound to one `principal_ref`; the file is parsed by
`axon_os::parse_manifest` (no second grant parser). Unknown ref / unbound
principal / edited file → `SubmitError::Unauthorized` (exit 7) before the
journal is opened. `supervise_requiring` now runs under THAT grant with its
`require_approval` policy (token = the grant file's `.approval` sibling,
verified by axon-os) over a probe declaring the program's scanned effect row;
`limits.max_cost_micro` must fit `grant.budget.cost_micro`. The executed check's
`AXON_ALLOWED_EFFECTS` is derived from the admitted grant (`--effect-ceiling`
removed); path-scoped and reproducible grants are `unsupported` (no backend can
enforce them), and the Linux profile is eligible only for a grant withholding
nothing (in-guest enforcement is Stage 3, B263 x1 — not claimed). The all-zero
`policy_digest` is refused by the Fabric and by `FabricSubmitExecutor::new`;
`cortex --fabric-journal` requires `--fabric-principal`, `--fabric-grant-ref`,
`--fabric-grant-registry`, `--fabric-policy-digest` (exit 2 if absent).
Tests: `tests/grant_authority.rs` (7) and
`cortex_via_fabric.rs::cortex_fabric_mode_refuses_to_start_without_explicit_authority`,
each refusal asserting no spawn, no launch record and (pre-journal refusals)
no journal file. Mutation-checked: ceiling constant, hard-coded admitted grant,
`require_approval` forced false, placeholder check, principal binding, digest
check, empty scanned row, cortex hard-coded principal/digest — each fails a
test. **Still OPEN:** the principal is bound, not authenticated;
`axon-loop/src/plan.rs` self-asserted approval (lane 2A's crate).

## D-017 — D-C3 / D-C5: duplicated canonicaliser; second budget algebra

**D-C3.** Two `acf1:` implementations either side of the cortex → fabric process
seam: `crates/axon-fabric/src/submit.rs:176-189` (`executable_digest`, `workspace_digest`, built on `axon_loop_contracts::canonical_bytes`) and
`crates/axon-cortex/src/runner.rs:2326-2345` (`fabric_executable_digest`,
`fabric_workspace_digest`: `serde_json::to_string` relying on sorted-map order,
correct only for the ASCII values the Runner produces; equality asserted in
tests, not by construction). The seam itself is accepted (a Cargo edge would
be a cycle); the duplicate is not.

**D-C5.** `crates/axon-fabric/src/journal.rs:796` `reserve` implements its own
per-scope committed ≤ ceiling arithmetic beside
`crates/axon-os/src/ledger.rs:36,72` `ResourceLedger::carve` (checked,
never-decreasing, still without a production caller). The journal adds
durability, `OutcomeUnknown` and liability that `ResourceLedger` lacks, so the
fix is to use `carve` for the arithmetic, not to delete the journal.

**Resolution.** One canonicaliser; ceiling arithmetic via `carve`.

**D-C3 resolved (Stage 2, lane 2B).** The single implementation is
`axon_cortex::runner::acf1_canonical_bytes` (flat string objects; keys sorted
explicitly so serde_json `preserve_order` cannot move a digest; `cl22`
escaping). It lives in `axon-cortex` because that is the lowest crate both
sides link — `axon-cortex` gains no dependency. `axon_fabric::submit::
{executable_digest, workspace_digest}` delegate to it. Tests:
`axon-cortex/tests/fabric_acf1.rs` pins the bytes and digests to Python
`json.dumps(sort_keys=True, separators=(',',':'), ensure_ascii=False)` output
for an adversarial path (quote, backslash, C0, DEL, non-ASCII);
`axon-fabric/tests/submit.rs::one_acf1_canonicaliser_serves_both_sides_of_the_seam`
checks equality with the `cl22` form and that the Fabric has no canonicaliser
of its own. Mutation-checked: re-introducing the Fabric's own implementation,
dropping the sort, and escaping DEL each fail a test. Measured honestly: the
pre-fix serde_json-based cortex implementation was byte-correct for these
inputs; the defect was the duplication and its reliance on the map type's
order, not a wrong digest today.

**D-C5 resolved (Stage 2, lane 2B).** `journal.rs` `Rec::Reserved` now decides
committed ≤ ceiling with `axon_os::ledger::ResourceLedger::carve` (its first
production caller), through its existing public API — `ledger.rs` unchanged.
Two semantic mismatches, handled rather than forced: (1) `ResourceLedger` has
three fixed axes (compute/budget/persist_bytes) and the journal four
dimensions, so each dimension is carved on its own single-axis ledger and the
refused dimension is reported by name (`BudgetExceeded.dimension`);
(2) `carve`'s `used` never decreases, but the journal must release a
never-launched cancel and settle liability to a known charge, so the ledger is
rebuilt per check from the journal-derived committed total rather than stored.
One gap in `carve` itself was found and guarded, not fixed (axon-os is outside
this lane's remit beyond reachability): it checks with `saturating_add` and
then adds UNCHECKED, so at a cap of `u64::MAX` an overflowing carve is admitted
and then overflows (panic in debug, wrap in release). The journal refuses an
overflowing sum before calling it. Tests:
`each_dimension_is_carved_through_the_axon_os_ledger`,
`an_overflowing_reservation_is_refused_not_wrapped`. Mutation-checked:
bypassing `carve` and removing the overflow guard each fail a behavioural
test. A parallel `used + want > cap` comparison is meant to AGREE with `carve`,
so no behavioural test can catch it; `the_reservation_check_is_resource_ledger_carve`
pins the structure instead (source check: `carve_within` calls
`ResourceLedger::carve`, the reserve path calls `carve_within`, `fits_within`
is gone) and fails on that mutation.

## D-018 — D-C6: "admission" is two concepts; an ACCEPT is never a grant

**Live implementation.** `crates/axon-loop/src/admission.rs:1-8` is CX-11
POLICY admission (a frozen experiment rule → ACCEPT / REJECT / INCONCLUSIVE,
which may move the fenced active-policy pointer). `crates/axon-os/src/gate.rs:51`
`admit` and `crates/axon-intent/src/admit.rs` are EFFECT admission (effects ⊆
grant).

**Invariant recorded.** An ACCEPT admission, or an active-policy pointer, confers
NO effect authority. No code path may read one as a grant. Verified at
`279da778` by reading: `axon-fabric` reads only the authority EPOCH from the
loop store, never the pointer or an admission, as authority. No test pins this
yet, so it holds by inspection only. **State.** Invariant recorded; OPEN until
a test enforces it.

## D-019 — the `axon-vm` library entry point bypasses `cmd_run`'s pre-launch gates

**Document passage.** The `axon-vm` CLI's hardening (null-grant refusal,
override-may-only-narrow, no-TOFU kernel attestation, extended-TCB compare,
quorum), listed in `CLAUDE.md`/`ARCHITECTURE.md` as properties of `axon-vm`.

**Live implementation.** B262 moved the launch path to
`crates/axon-vm/src/firecracker.rs` (`run_in_firecracker`, exported from
`src/lib.rs`). Those gates stay in `src/main.rs` `cmd_run`; the library applies
none of them, and `MmdsPayload.allowed_effects` stays an `Option`. The guest
still fails closed on a null policy, so the impact today is "may boot an
unattested kernel". It is **latent**: the only callers are `main.rs` and
`tests/lib_launch.rs`; `axon-fabric` references only `BACKEND_PROFILE`.
Pre-existing and moved verbatim: after the Firecracker child is spawned, any
`?` error path leaks the child and its sockets (ACF-G22); the vsock UDS name
is pid-keyed.

**Resolution.** Move the gates into the library, or make `LaunchSpec` require
a verified grant/attestation token; clean up on every post-spawn error.
**Stage 3. OPEN.** Source: `F_guest_vm.json` B262.

## D-020 — `linux-microvm-protected` is an enclosure, not a policed guest, and its eligibility check ignores BLOCKED

**Document passage.** The profile name, and `crates/axon-fabric/src/backend.rs`
`LinuxProfileConfig::qualification`: *"Eligible only if the manifest in use is
byte-identical to the one the evidence record qualified, and that record has
zero FAIL assertions."*

**Live implementation / evidence.**
* Qualification record `evidence/b263/20260924T080432Z.json` (operator-side,
  not in repo): **32 PASS / 0 FAIL / 4 BLOCKED**, `result: PASS_WITH_BLOCKED`,
  `host: WSL2-nested` (operator decision D2; L0 Hyper-V outside the boundary).
  BLOCKED: x1 guest policy channel (ACF-G25), x2 scope preservation (ACF-G26),
  x3 L0 boundary, x4 trusted evidence issuer — the record is **unsigned**,
  produced by an unauthenticated local root shell. Two earlier runs
  (`075735Z`, `075923Z`) had FAILs.
* `qualification()` checks schema, profile name, `FAIL == 0` and manifest sha.
  It ignores BLOCKED (including x4), host, freshness, source rev, engine
  digests and signatures, so an unsigned operator-writable JSON enables
  protected dispatch.
* The guest runs `profiles/linux-microvm/guest-init.sh`, NOT `axon-guest-init`:
  no MMDS, no effect ceiling, no in-guest policy. Only the VM boundary is
  enforced. Fabric honestly refuses requests needing an effect ceiling or
  path-scoped grant on this backend (`backend.rs` refusal branch), which is
  why it can only carry grant-free `interpreter_run`.
* `profiles/linux-microvm/manifest.json`: `axon_tree_dirty_at_build: true` —
  the guest `axon` is not reproducible from a commit.
* Firecracker and jailer are fixed paths in `scripts/fc_linux_profile.sh`,
  not digest-checked at launch.
* `acpi=off` deviation, recorded in the README and `kernel-overlay.config`,
  not in `manifest.json` fields.

**Stage 3 status (2026-09-26; each fix mutation-verified in its lane).**

| finding above | now |
|---|---|
| `qualification()` ignores BLOCKED / signature / freshness / digests / dirty trees | **fixed** — issuer-signed Ed25519 evidence, BLOCKED only under a signed unexpired waiver, fresh, engine digests pinned by the manifest (now REQUIRED), clean trees, host + caveat carried into receipts (`5fbf44d3`, `f10f055c`) |
| guest runs `guest-init.sh`, no in-guest policy | **fixed** — the workload runs under `axon-guest-init`, which reads `axon.policy=` from the kernel cmdline and fails closed; the no-policy bypass is compiled out (`b0b66c99`, `6b9d8c24`); the launcher requires and binds `--policy` (`46c9ec34`); Fabric sends the admitted grant ceiling as that policy and lifts its refusal only on signed evidence with x1 PASS (`ac763e21`) |
| manifest `axon_tree_dirty_at_build: true` | **fixed** — rebuilt from a clean tree (`d5a2c670`) |
| firecracker/jailer not digest-checked | **fixed** — pinned in `manifest.engine`, checked before acquisition, re-checked in the chroot and on the running VMM (`4cc62633`, `46c9ec34`; b263 `g4`) |
| `HardwareIsolated` accepted an unqualified VM | **fixed** (`0f21a9fd`) |

New qualification record `evidence/b263/20260926T002631Z.json` (operator-side, unsigned,
rev `d5a2c670`, clean): **39 PASS / 0 FAIL / 2 BLOCKED** — x1a–x1e PASS (ACF-G25, in-guest
policy enforced and bound), x2 PASS as "unsupported axis refuses" (ACF-G26, operator default
D8 — path/host projection NOT implemented), **x3 L0 boundary and x4 trusted issuer still
BLOCKED**. Fabric refuses it: unsigned, and x3/x4 unwaived.

**D-020 stays OPEN** until S3-6: the operator signs the record with their issuer key (x4) and
decides whether to sign a waiver for x3 on this WSL2-nested host (D7). Neither can be done by an
agent without making the record self-certifying.
* `axon-fabric` tests exercise Linux dispatch through a STAND-IN launcher
  (`crates/axon-fabric/tests/submit.rs:550-560`); they say nothing about the VM.
  Neither `fc_linux_profile.sh` nor `b263_qualify.sh` is invoked by `gate.sh`
  or CI.

**Resolution.** Profile README relabelled "enclosure-only, NOT qualified as
protected" on this branch. Code: require BLOCKED = 0 (or explicit waiver) plus
issuer, freshness and engine digests; pin firecracker/jailer; rebuild from a
clean tree; add the in-guest policy channel via `axon-guest-init` (operator
decision D5). **Stage 3. OPEN.**

## D-021 — the v0.22 package's `EXISTING_AXON_MAP.md` repeats two stale claims

**Package passages** (`docs/axon_cortex_v0_22/axon-cortex-build-v0_22/EXISTING_AXON_MAP.md`
rows 14-15, byte-identical to the v0.15 rows):

> Principal/kernel registry — *"Audit identity is not authorization; check each
> executor use …"* (as work to do)
>
> Sandbox APIs — *"Empty scoped list is not deny-all; …"*

**Live implementation.**
* `""` IS deny-all and `"*"` is unrestricted for `sandbox_create_scoped`
  (D-001, and the capability-policy section of `CLAUDE.md`). Acting on the
  package row would reintroduce empty = unscoped — a hole that was measured
  and closed.
* "Audit identity is not authorization" has LANDED for the ledger: `axon-ledger`
  treats `--as` / `AXON_PRINCIPAL` as a claim and takes admin authority from the
  real uid (`crates/axon-ledger/src/main.rs:559-583`, `authenticated_admins`;
  `AXON_LEDGER_DEV_IMPERSONATE=1` is the only escape). What is NOT done is
  applying that rule to the v0.22 stores (D-015, D-016).

**Resolution.** Do not copy either row forward. Package update proposed; the
package is third-party input and vendored byte-for-byte, so it is not edited.
**Owner.** Repository owner.


---

# Records ported from the superseded v0.20 donor line (D-022 … D-028)

These were filed on `upgrade/cortex-v0_20` (`/home/cklaus/projects/axon`) as
D-014 … D-020. On this line those IDs already name v0.22 findings, so the donor
records were given **new** IDs here rather than reused. The full donor text, with
its measurements, is at `D-014@upgrade/cortex-v0_20` … in that branch's copy of
this file (`git show upgrade/cortex-v0_20:governance/cortex-v015/DISCREPANCIES.md`),
and the complete ID map is in `UPGRADE_V0_22_DONOR_PORT.md`.

* **D-022** (donor D-014): fixed on this line by the ported commits
  `6de01247` (constructor) and `ea9ba38d` (binop propagation, both engines).
* **D-023** (donor D-015): fixed on this line by `624dec42`.
* **D-024 … D-028** (donor D-016 … D-020): carried as OPEN. Nothing about them
  changes by being ported; each still needs the decision or work its donor record
  names. D-026's proposed schema name is NOT `axon-approval/2` as a new schema:
  that name is allocated by `governance/specs/R24-defended-approval-boundary.md`.
* **D-001** carried: the v0.22 package still says "empty scope = unscoped". The
  repo behaviour (`""` = deny-all) stands, and is now pinned by the ported
  `sandbox_scope_net_empty.ax` case, which asserts the refusal *reason*, not just exit 8.


## D-029 — the v0.22 package's own fixture bundle carries the G10 price-schedule mismatch

The package fixture bundle's `acf_request` names `fixture:synthetic-not-pricing` as its
price schedule while its episode usage names a `cl22:` schedule Ref — exactly the
OpaqueRef/Ref mismatch G10 forbids. Stage 5 (B270, `07a19b13`) now REFUSES that
mismatch (`crates/axon-loop/src/price.rs`; `tests/tel_price.rs`), so the package fixture
would be refused by the implementation. Package-side; the vendored pack is not edited.
Proposed: file upstream.

## D-030 — Axon's WorkspaceVersion import is stricter than MiCode's recipe

Axon refuses case/NFC path collisions and a path that is both a file and a directory;
MiCode's recipe (`docs/axon-support/WORKSPACE_VERSION_RECIPE.md` §2 on the MiCode line)
does not. For every tree BOTH accept the digest is byte-identical (cross-language fixture,
`crates/axon-fabric/tests/fixtures/`). Proposed: MiCode adopts the collision class so a
tree cannot bind on one side and be refused on the other. Also recorded: `bind_acf` is now
stricter than the package's `tools/closed_loop_reference.py` on receipt roles and recheck.
