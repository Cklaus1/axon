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
