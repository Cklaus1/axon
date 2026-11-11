# R46 — In-Flight Operations: running, watching, and stopping a deployed agent

**Spec ID:** `R46-in-flight-operations`
**Status:** Draft
**Risk class:** Structural. It adds a new long-lived object (a deployment), a new path for
authority to reach a run (secrets), and a new way for a run to wait on a human.
**Author / date:** 2026-09-29, from `AI_Language_Plan.md` §Product Review, items PM1–PM4.

```spec-meta
id: R46-in-flight-operations
status-claim: Draft
depends-on: R21-axon-os-supervisor, R22-intent-approve-gateway, R15-resume-runtime, R28-capability-audit-ledger
blocks: none
blocked-by: R46 §12 Q1
supersedes: none
related: R27-corrigibility-resource-bounds, R29-continuous-compliance-monitor, R38-embedded-agent-runtime, R45-require-contained
conflicts-with: none
reserves: E25xx (confirmed free at spec time — no E25 code in crates/, governance/specs/, or AXON_REFERENCE.md), exit 16, exit 17, exit 18 (confirmed free — the codes in use are 3–15, 20–26 and 101; CX-35 (Draft) also proposes exit 16, first lander claims it, see §9)
evidence: none
```

## Step 1 review (2026-11-11)

Inputs: conformance audit **R46A-01..24** (3 blockers, 13 majors, 8 minors; probes in
`/var/tmp/r51-tmp/R46Audit/`) and dev-loop/cost review **R46D-1..8** (3 P1, 4 P2, 1 P3). Both
were read-only and taken at `f14bc25f`. Each finding was re-checked against its cited file:line
at `f14bc25f`. Probes `stdin.ax`, `e16.ax`, `x17.ax`, `p1.ax` and `p2.ax` were re-run with
`/var/tmp/r51-tmp/sn2-bin/axon` and reproduced. This review builds and authorizes nothing.

**Counts:** 32 findings: 30 accepted, 2 partial (R46A-05, R46A-24), 0 rejected.

**Not independently verified** (no `axon-os` binary; read from code only): the second bullet of
R46A-17 (silent `None` under `restricted` *through axon-os*), the runtime half of R46A-03(ii)
and R46A-11, and every R46D time or cost figure (estimates, `[INFERENCE]`).

| id | severity | disposition | where applied |
|---|---|---|---|
| R46A-01 | blocker | accepted | §6 rule 7; §3.1 layout; NH-3 |
| R46A-02 | blocker | accepted: egress broker | §6(c) intro; §6 rule 6 |
| R46A-03 | blocker | accepted | §5.2 decision channel and approver; §5.3; E2507 |
| R46A-04 | major | accepted | §5.1(a); §13 S3a/S3b |
| R46A-05 | major | partial. Claiming 16–18 and deriving them from supervisor state are accepted. Widening the protected row is kept only as a defence for values returned from `main`: a stated `exit(n)` passes as written even for protected codes (`EXIT_CODES.md:46-47`), so the protected row alone fixes nothing. The CX-35 collision is recorded. | §9; spec-meta `reserves` |
| R46A-06 | major | accepted | §11 I-18; §3.1 layout (`fired` row, version digest) |
| R46A-07 | major | accepted | §3.1 layout and workdir note; E2500 |
| R46A-08 | major | accepted | §4 promotion; §3.1 `history.jsonl`; E2501; NH-1 |
| R46A-09 | major | accepted | §7 status; §5.2 ledgering |
| R46A-10 | major | accepted | §5.1(b); §5.2 effect row; §12 Q1; §13 S6a |
| R46A-11 | major | accepted | §6 rule 4; §3.1 layout; §8 `replay`; E2508 |
| R46A-12 | major | accepted | §3(c); §5.2 decision channel; §8 `fire`; §13 S2a |
| R46A-13 | major | accepted | §6 rule 2 |
| R46A-14 | major | accepted | §3.1 locking note; §4.1 revoke |
| R46A-15 | major | accepted | §8 environment; E2509 |
| R46A-16 | major | accepted | §6 rule 8 |
| R46A-17 | minor | accepted | §5.2 effect row; §1 item 2 note |
| R46A-18 | minor | accepted | §5.2 approver |
| R46A-19 | minor | accepted | §6 rule 3 |
| R46A-20 | minor | accepted | §7 status (kills); §3.1 `history.jsonl` |
| R46A-21 | minor | accepted (same anchors as R46D-8) | §1 anchors |
| R46A-22 | minor | accepted | §7 alerts; §3.1 `alerted` row |
| R46A-23 | minor | accepted | §5.2 and §6(c) examples |
| R46A-24 | minor | partial. Applied: fail-first S0, g01 rows, per-slice costs, needs-human options with recommendations. Not applied: a rewrite into build-loop layout and a `DRAFT` status line. The review brief keeps this spec's structure and `status-claim: Draft`, and `verify_all_specs.sh` compares the prose **Status:** with the status-claim. | §13; §12.1 |
| R46D-1 | P1 | accepted | §13 gate column; acceptance-gate note |
| R46D-2 | P1 | accepted | §13 S1; §7 alerts; §5.2 timeout |
| R46D-3 | P1 | accepted | §8 `serve` contract; §13 S2b |
| R46D-4 | P2 | accepted | §13 S3a/S3b, S4a/S4b; §8 language surface; cost table |
| R46D-5 | P2 | accepted | §13 S0 and g01 note |
| R46D-6 | P2 | accepted | §12 Q1; §13 S6a |
| R46D-7 | P2 | accepted | §4 canary; §6 rule 5; §7 status fields; §6 "§8" corrected to "§13" |
| R46D-8 | P3 | accepted (same anchors as R46A-21) | §1 anchors |

---

## 1. Why

Axon is strong before a run and after it:

- **Pre-flight:** type-check, refinements, `@[contained]`, approval tokens bound to
  program + grant (`axon-os/src/approval.rs`), and the risk-gated deploy pipeline.
- **Forensics:** the host journal, `axon replay`, the hash-chained ledger, and
  `axon-os verify`/`replay`.

Axon has nothing for the stretch in between: a program that keeps running, day after day, on
real inputs. Each of the following was checked against `8b0d2dc9`, not assumed.
`[REVISED: R46A-21/R46D-8: anchors below re-pinned to f14bc25f.]`

**1. "Deploy" means "run once".** `axon deploy --help`: *"Runs type-check → assert_deployable
→ interpreter"*, then exits. There is no deployment object, no version, no trigger, and no
rollback. The PRD's §Governance lifecycle (sandbox → shadow → 10% → 50% → 100%, automatic
rollback) has no implementation.

**2. A job cannot ask a human, and fails silently when it tries.** Probe
(`host_await_opt("approve refund 240?")` inside an `axon-os` job):

| Profile | Result |
|---|---|
| `restricted` | exit **8** — `host_await_opt requires effect IO which is not in the active sandbox's allowed set {}` |
| `developer` | exit **0**, `✓ completed`. The job's stdin is `/dev/null` (`runtime.rs:582`, `Stdio::null()`; it was `:174` at 8b0d2dc9 `[REVISED: R46A-21/R46D-8]`), so the call returns `None` at once and the program proceeds down its "no human answered" branch |

The second row is the dangerous one. The run record gives a reviewer no sign that a human
decision was requested and never made. R15 built the suspend/resume runtime, but the supervisor
never answers the question.

`[REVISED: R46A-17]` At f14bc25f the `restricted` row is stale. axon-os always grants `IO`
(`runtime.rs:884`, `grant_sandbox_tags`), so the gate admits the call under `restricted` too,
and it returns the same silent `None` [INFERENCE: read from the gate, not run through axon-os].
The silent path therefore applies under every profile.

**3. A contained job cannot hold a credential.** `axon-os` launches the job with an explicit
minimal environment (`runtime.rs:512`, *"Hermetic: a minimal explicit env"*; was `:128` `[REVISED: R46A-21/R46D-8]`), and `env_var` is
refused inside `@[contained]` (E1001). Both are correct, and together they mean a bounded agent
that calls a paid API has **no sanctioned way to get its key**. In practice, operators widen the
profile or paste the key into source. Both are worse than a designed path.

**4. The monitor stops things and tells no one.** R29 polls the ledger and trips the R27
kill-switch within 2 s. `monitor.rs`, `supervisor.rs` and `cli.rs` contain no notification
path: no webhook, no email, no Slack. The kill is ledgered. The on-call person hears nothing.

## 2. Requirement link

`AI_Language_Plan.md` §Product Review: **PM1** (deployments, triggers, rollback), **PM2**
(monitoring and alerts), **PM3** (mid-run approval inbox) and **PM4** (secrets broker).
ROADMAP §10 Acid Test 3 (*"sees deploy + post-deploy metrics"*) is not satisfiable without
PM1 and PM2.

Out of scope, each with its own spec to follow: PM5 MCP (`R47-mcp-bridge`, planned), G1/G2 taint
and sensitive-to-stdout (`R48-untrusted-data-flow`, planned), and G3/PM6 sealed evaluation
(`R49-sealed-evaluation`, planned). They are named here, not in `spec-meta`, because the
files do not exist yet and the DAG validator rejects dangling references.

---

## 3. Fork 1 — what is a deployment? (the decisive fork)

### (a) A long-lived interpreter process — REJECTED

One process starts and loops forever, handling events. This is how most agent frameworks
deploy, and it throws away every property Axon has:

- **No per-event bound.** Budget, grant and kill-latch are scoped to a *run*
  (`axon-os run_bounded`). A process that never ends has one run, with a budget that either
  starves or never binds.
- **No per-event replay.** A journal that never closes cannot be replayed until the process
  dies, and `AXON_REPLAY` treats a partly consumed journal as a divergence.
- **State accumulates invisibly** across events. A bad event poisons the next thousand, and
  there is no record boundary where that could be seen.

### (b) A new daemon with its own execution model — REJECTED

This is a second supervisor. Everything R21/R27/R28/R29 proved about `axon-os run` would have to
be proved again for it, and the two would drift. That is the I-2 failure mode applied to
operations.

### (c) A deployment is a named, versioned pointer; each event is one ordinary bounded run — **CHOSEN**

```
deployment "refunds"
  versions:  v1 → v2 → v3          each = {program bytes, .axjob, approval token}, content-addressed
  current:   v3                     the version a trigger fires
  triggers:  cron "*/5 * * * *", webhook /hooks/refunds
  alerts:    budget_burn, failure_rate, kill, approval_timeout
  runs/      <run-id>.json …        each one an unmodified axon-os run record
```

**A deployment adds no authority and no execution semantics.** When a trigger fires, it calls
the same supervisor path an operator calls today (`axon-os run <current version's axjob>`).
The event payload is delivered as the job's input. Every guarantee (approval verified at the run
boundary, grant enforced, budget per run, kill-latch, ledger, record, replay) holds per event
because each event *is* a run.

`[REVISED: R46A-12]` That path has no payload channel today: `Runtime::run_sandboxed` takes no
input (`runtime.rs:148-156`), and the `axon-os run` usage has no payload flag. So `fire` is
"the operator path plus one input", not "exactly what an operator calls today". The supervisor
writes the payload to `runs/<run-id>.payload` and gives the child a read-only copy.
`run_sandboxed` gains `input: Option<&Path>`. The program reads the payload through a new
host-seam builtin, `event_payload() -> Option<str>` (row `{IO}`, which axon-os always grants;
journaled like every host call). `env_var` stays refused under `@[contained]`. **The payload
never goes to stdin**, and stdin stays `/dev/null` (see the §5.2 decision channel).
[DECIDED (engineering) 2026-11-11: the payload is delivered by `event_payload()`, backed by a supervisor-written file; core slice S2a.]

Consequences, stated so they are chosen, not discovered:

- **State between events must be explicit.** Use `Store<T,C>` (Phase 7) or a granted
  `fs_write` path. The deployment offers no hidden memory. That is a feature: the state is in
  the grant, so it is reviewable.
- **Per-event cost** is one process spawn plus one type-check. That is acceptable for
  event-driven agents (seconds-scale work, LLM-bound) and wrong for request/response serving
  at high QPS, which is explicitly a non-goal (§10).

### 3.1 Where it lives

In **`axon-os`**, as new verbs over new files. It is not a new crate: `JobManifest`, `Grant`,
`verify_approval`, `RunRecord` and the ledger are all already there, and the deployment layer
is bookkeeping over them.

On-disk layout. It is plain files so it survives restarts and can be read without a server,
the same choice R28 made for the ledger:

```
$AXON_OS_HOME/deployments/<name>/       must pass check_private (owner = supervisor euid, no group/other) at every fire  [REVISED: R46A-01]
  deployment.toml        name, triggers, alerts, secrets bindings (see §6), workdir (absolute), forwarded env (§8), current = "v3"
  lock                   one flock per deployment  [REVISED: R46A-14]
  versions/v3/           program.ax + its full import closure (`axon imports --json`), job.axjob, job.approval  (copied, never referenced by path)  [REVISED: R46A-07]
  versions/v3/digest     axsha256 over (program digest, imports_digest, canonical_grant, canonical_manifest with `program=` replaced by the program digest)  — the version id  [REVISED: R46A-06/07]
  runs/<run-id>.json     axon-os-record/1, unmodified
  runs/<run-id>.journal  AXON_RECORD host journal of the fired run, supervisor-private  [REVISED: R46A-11]
  runs/<run-id>.payload  the event payload as delivered  [REVISED: R46A-11/12]
  history.jsonl          hash-chained AND MAC'd under a new derived domain `axon-os deployment v1` (beside TOKEN_DOMAIN/RECORD_DOMAIN, approval.rs:58-62):
                         created, promoted, rolled_back, paused, revoked, fired (run-id, version digest), killed (principal), alerted (delivery status)  [REVISED: R46A-06/08/20/22]
```

Versions are **copied** in at creation. A deployment that pointed at a path in the working
tree would run whatever is there now, not what was approved. Approval tokens already bind
program + grant, but a copy makes the invariant local rather than dependent on the token check
being reached.

`[REVISED: R46A-07]` Copying only `program.ax`, `.axjob` and `.approval` would change what runs.
The token binds `imports_digest` (`approval.rs:117,140-147`), so the import closure is copied
too. The job's cwd is today the program's directory (`runtime.rs:1043-1046`), which would put
relative state under `versions/vN/`, split it per version and lose it on rollback. A deployment
therefore declares an absolute `workdir`, which is the job's cwd for every version, and grants
with relative paths are refused (E2500). The `.env` that `axon-ai` finds from cwd
(`axon-ai/src/lib.rs:59-90`) is not a key source (§6 rule 8).

`[REVISED: R46A-14]` **Pointer updates are locked and ordered.** There is one `flock` on
`deployments/<name>/lock` (the NH-R1 E5 pattern). `promote`, `rollback`, `revoke` and `pause`
take the lock, append the `history.jsonl` row first, then replace `deployment.toml` by
write-temp plus atomic rename. `history.jsonl` is the source of truth: startup and every `fire`
reconcile, and a `deployment.toml` that disagrees with the last history row is rewritten from
history. `fire` resolves `current`/`candidate` to a version digest under the lock, then runs that
digest. `[DECIDED (engineering) 2026-11-11.]`

---

## 4. Fork 2 — versions, rollout, rollback

**Version identity** is the digest over `(program bytes, canonical_grant, canonical_manifest)`,
reusing `approval::canonical_grant` and `record::canonical_manifest` so no new encoding exists
to drift.

**Promotion requires a verifying approval** when the manifest sets `require_approval = true`.
That is the existing policy, unchanged. `axon-os deploy promote refunds v4` runs
`verify_approval` against the copied bytes and refuses (exit **18**, E2501) on failure.

`[REVISED: R46A-08]` **Deployments require `AXON_APPROVAL_KEY` and refuse unkeyed tokens**
(E2501) at create, version, promote, rollback and fire. An unkeyed token is "integrity only,
anyone can mint one" (`approval.rs:258-260`), and before R46A-01 was fixed a deployed job could
mint one. `[DECIDED (engineering) 2026-11-11: keyed symmetric tokens are mandatory now; per-approver or attested identity is NH-1.]`

**Rollback is promotion of an older version**, and gets no special path:
`axon-os deploy rollback refunds` → `promote refunds <previous>`. The previous version's
approval still verifies because nothing in it changed. That is the point of binding approval
to content. There are two refusals:

- the old token was **revoked** (§4.1), or
- the old version's grant is **wider than the current version's**, and the operator did not
  pass `--allow-widen`. A rollback that silently restores authority someone removed on purpose
  is how a revoked capability returns. This is the monotonicity principle from the ASI Review
  (§E.3), applied to operations.

**Staged rollout, v1: canary by fraction.** `deployment.toml` may set
`candidate = "v4"` and `canary = 0.1`. Each trigger firing picks candidate or current with a
**seeded draw** whose seed is recorded in `history.jsonl`, so the routing of any past event
is reproducible. `promote` and `abort` end the canary. The criterion for promotion is written
as a comparison over the two versions' run records (§7 `[REVISED: R46D-7: was "§5"]`), and in
v1 it is advisory only: the tool reports it and a human promotes. `[REVISED: R46D-7]` The
signal is `canary.compare` in `deploy status --json`: per version digest
`{runs, completed, failure_rate, spend_usd, approvals_timed_out}` over the canary window. S7
checks it against a golden fixture.

**Shadow stage (actions logged but not performed) is deferred.** A shadow run that skips an
irreversible write observes a world that never existed (ASI Review §B10). Doing it soundly
needs a reversibility row (ASI Review §G8), so that only irreversible effects are suppressed,
and simulated responses served from a journal. That is `R48`/G8 work. Shipping a shadow mode
before it exists would produce a confident, wrong score.

### 4.1 Revocation

`axon-os deploy revoke refunds v2 --reason "…"` appends to `history.jsonl`. A revoked
version cannot be promoted, rolled back to, or fired. If it is `current` at revocation time, the
deployment is **paused**: triggers are acknowledged and dropped, and an alert is raised. It does
**not** fall back to the previous version automatically, because an automatic fallback is a
rollback nobody approved.

`[REVISED: R46A-14]` **Runs in flight.** Revoke runs under the deployment lock. It R27-kills
every live run of the revoked version (latch per run-id) and auto-denies its pending approval
requests (`Denied(by = "revocation", reason)`). Without this, a mode (a) wait lasting hours
would execute after its version was revoked.

---

## 5. Fork 3 — how a run asks a human (PM3)

### 5.1 The two candidate mechanisms

**(a) Hold the process: in-process suspend via `host_await` (R15), supervisor answers.**
The supervisor becomes the `host_await` peer, replacing `/dev/null`. The request goes to an
inbox, and the reply resumes the parked interpreter.
*Pro:* ~~it is built (R15 Slice 2 carries full `Value` payloads)~~. *Con:* a process (and its
grant, its budget clock and its kill-latch) stays alive for as long as the human takes. That
could be hours, and a supervisor restart loses the run.
`[REVISED: R46A-04]` Only the in-process primitive exists: R15 provides `host_await_val` with
full `Value` payloads. The cross-process peer is S3 work. The CLI substrate stringifies non-str
requests (`interp.rs:4405-4408`, `send_value_display`), every substrate replies `SendValue::Str`
(`interp.rs:4196-4214`), and axon-os only buffers stdout while the job runs
(`runtime.rs:578-601`). S3a/S3b add a wire protocol (`axon-hostmsg/1` JSON lines on a dedicated
inherited fd, `AXON_HOST_FD`) and the supervisor answer loop.

**(b) End the run: checkpoint by journal, resume by replay.** The run records its journal up to
the request and exits with a new status, **pending**. The decision starts a new run that
**replays the journal prefix** (same seed, same host answers, so identical state by
construction), receives the decision as the next host reply, and continues **live**.
*Pro:* it survives restarts, holds no authority while waiting, and the prefix/suffix
boundary is visible in the record. *Con:* it needs a mode that does not exist: replay, then
go live at the end of the journal. Today, an exhausted journal is a divergence (exit 11) by
design.
`[REVISED: R46A-10]` "Identical state by construction" holds only if every nondeterministic
input is in the journal. Today `host_await` is not an `AXON_RECORD` method (the method list in
CLAUDE.md:276; `host_await` does not appear in `axon-core/src/host.rs` or `replay.rs`), and AI
replies go through `AXON_AI_REPLAY`, a separate cache. Journaling `host_await` and `ai_*` calls
is a named prerequisite slice (S6a, §13) and part of Q1.

### 5.2 Resolution — **(a) now, (b) as the durable path, one surface for both**

The program-visible surface is identical, so the choice is operational and can change
without touching `.ax`:

```axon
// [REVISED: R46A-23] Axon has no dict literal (probe p5: E0000); build `detail` with dict_from_pairs
let detail = dict_from_pairs([("amount", "240.00"), ("order", oid)])
match approval_request("refund", detail, 3600) {
    Approved(by) => issue_refund(oid)        // by: the authenticated approver
    Denied(by, reason) => log_denial(oid, reason)
    TimedOut => hold_for_review(oid)         // never treated as approval
}
```

- `approval_request(kind: str, detail: Dict, timeout_s: i64) -> Decision`. The kernel sends a
  reserved envelope over `host_await_val`. The supervisor recognises the envelope and does not
  forward it as ordinary host I/O.
- **Effect row: `{Approval}`, a new row, not `IO`.** A request for permission must never need
  broad authority to be made. `[REVISED: R46A-17]` `Approval` obeys ceilings like any row. It is
  added to `GRANTABLE_EFFECTS` (`builtins.rs:2724`); without that, a ceiling naming it is
  rejected as a misspelling, and an existing `["IO"]` ceiling denies the call (exit 8). Every
  profile grants it by default, including `hermetic`, and a ceiling may remove it. The
  "measured above" rationale (IO unusable under `restricted`) is stale: see the §1 item 2 note.
  `[REVISED: R46A-10]` Under `hermetic`, the decision is served from the journal on replay only
  after S6a journals `host_await`.
- **`TimedOut` is a distinct variant, not `Denied`.** Collapsing them loses the one fact a
  reviewer needs: *nobody looked*. The supervisor enforces `timeout_s ≤ remaining run
  timeout` under (a) and refuses a longer wait (E2502) instead of letting the run die mid-wait.
  `[REVISED: R46D-2]` Remaining time, `timeout_s` expiry and every other window read the
  supervisor's `Clock` trait (§13 S1), never `Instant::now()` directly, so tests use the fake.
- **Authenticated approver.** `by` is not a claimed name. ~~In v1 it is the uid of the process
  that wrote the decision, mapped through the same `authenticated_admins` table the ledger
  uses (`axon-ledger/src/rbac.rs`)~~, with the same lesson (FG-012): a claim is not authority.
  `[REVISED: R46A-18]` `authenticated_admins` belongs to `axon-ledger`, the git-provenance
  ledger (`rbac.rs:42`; crate description), not R28's `axon-audit`. axon-os owns
  `$AXON_OS_HOME/approvers.json` (subject to `check_private`) and reuses
  `axon_ledger::rbac::authenticated_principal` (`rbac.rs:293`).
  `[REVISED: R46A-03(ii)]` Jobs run as the supervisor's uid (§6 rule 7), so
  `inbox approve|deny` refuses (E2507, exit 2) when the caller's uid equals the uid jobs run
  as. Otherwise a job could approve itself.
- **Everything is ledgered:** request, delivery, decision, approver, latency, and timeout.
  `[REVISED: R46A-09]` These are recorded by the supervisor (inbox plus `history.jsonl`), not
  only in the ledger the child writes.
- **Without a supervisor** (`axon run` directly), `approval_request` answers `TimedOut`
  immediately and prints a one-line warning to stderr. It never answers `Approved`. Fail closed
  is the default here, as everywhere.
- `[REVISED: R46A-03(i)/R46A-12]` **Decisions and payloads never share a channel.** The stdio
  host reads `host_await` replies from the same stdin the program's `read_line` uses
  (`interp.rs:4432-4443`). Re-run at f14bc25f:
  `printf 'payload-line\n{"decision":"approved","by":"root"}\n' | axon run stdin.ax` printed
  `reply={"decision":"approved","by":"root"}`, rc 0, so a webhook sender who controls the
  payload could answer the approval. In a deployed run, decisions travel only on the
  supervisor's `AXON_HOST_FD` (`axon-hostmsg/1`), the payload only through `event_payload()`
  (§3(c)), and stdin is `/dev/null`. The host never accepts an `approval_request` reply from
  any other channel.

Slice order: **S3 ships (a)**. **S6 ships (b)** behind `approval.durable = true` in
`deployment.toml`, and needs the replay-then-live mode (§12 Q1, the one decision that blocks this
spec's later slices).

### 5.3 The inbox

v1 is files and a CLI: `$AXON_OS_HOME/inbox/<request-id>.json`, `axon-os inbox list`,
`axon-os inbox approve|deny <id> [--reason …]`. Notification (§7) tells a human there is
something to decide. A web pane in `axon-web` is a later slice over the same files. The CLI is
the contract; the UI is a view.

`[REVISED: R46A-03]` `inbox/` must pass `check_private` on every read and write. Request files
are written by the supervisor only, from the `AXON_HOST_FD` envelope, and never by the job.
§6 rule 7 keeps the job's grant away from `inbox/`.

---

## 6. Fork 4 — secrets (PM4)

### (a) Inject as environment variables — REJECTED

This is how most platforms do it. In Axon, the value becomes an ordinary `str`: printable,
interpolatable, sendable to a model, and **journaled verbatim**. `CLAUDE.md` already warns that
a journal holds every env value the run read. It also reopens the ambient channel that E1001
closes on purpose.

### (b) An opaque `Secret` value type in the language — DEFERRED

Sound, but it needs type-system work (no `to_str`, no interpolation, and flow checking at every
sink). That is exactly G1/G2's walker (`R48`). Doing it first would build that walker twice.

### (c) Host-side substitution at the HTTP seam — **CHOSEN for v1**

Every HTTP builtin already takes `headers` as a JSON string (`http_get(url, headers)`,
`http_post(url, headers, body)`, and both `http_sse*`). The program writes a **reference**, and
~~the real `AxonHost` substitutes the value~~ `[REVISED: R46A-02]` a supervisor-owned egress
broker substitutes the value (rule 6):

```axon
let h = "{{\"Authorization\": \"Bearer ${{secret:stripe}}\"}}"   // [REVISED: R46A-23] literal braces doubled (probe p1 E0000; p2 prints the JSON)
http_post("https://api.stripe.com/v1/refunds", h, body)
```

```toml
# deployment.toml — the operator binds names; the program never sees a value
[secrets.stripe]
source = "file:/etc/axon/secrets/stripe"    # v1 sources: file:, env: (of the SUPERVISOR)
hosts  = ["api.stripe.com"]                 # REQUIRED — a secret is bound to its destinations
```

Rules (each is a test in §13 `[REVISED: R46D-7: was "§8"]`):

1. **Substitution happens only in header values**, never in the URL, body, `exec` argv, or any
   other builtin. A `${secret:…}` anywhere else is sent literally, and the run gets a W2503
   warning in its record.
2. **Only when the request's host matches the secret's `hosts` list.** This is the control that
   matters. `net = ["*"]` plus a key means the key can go anywhere, so a secret that may be sent
   to any host the program chooses is exfiltrated by one line. On a mismatch the call is refused
   (**exit 17**, E2504) rather than sent without the header: a silent strip would turn an attack
   into a mysterious 401.
   `[REVISED: R46A-13]` Matching alone is not enough. reqwest's default client follows
   redirects, and on a cross-host hop it strips only Authorization, Cookie, Proxy-Authorization
   and WWW-Authenticate (`reqwest-0.12.28/src/redirect.rs:239-250`), so a custom header such as
   `x-api-key` survives. The broker therefore sends secret-bearing requests with
   `redirect::Policy::none()`, over `https` only, and only on an exact `host_str` match against
   `hosts`: URL userinfo is refused, and a non-default port must be listed as `host:port`. A
   3xx response is returned to the program as is; the broker never follows it with the secret.
3. **The job's grant must name the secret** (`[grant] secrets = ["stripe"]`). The name joins
   the grant, and therefore `canonical_grant` and the approval digest. Adding a secret to a job
   is a change of authority and invalidates its approval. `canonical_grant` destructures
   exhaustively, so adding the field is a compile error until it is encoded, which is the
   guarantee that file was built to give.
   `[REVISED: R46A-19]` The consequences, stated: every `grant_digest` changes, so the token
   schema becomes `axon-approval/6` (`/2` is reserved by R24, `/3` is the current
   `TOKEN_SCHEMA` at `approval.rs:123`, `/4` is retired and `/5` is the repair envelope at
   `repair_envelope.rs:48-52`). `Grant::intersect` (`grant.rs:141`) gets set intersection on
   `secrets`, and `broad_supervisor_grant` (`cli.rs:46`) gets an explicit secrets rule (none).
4. **The journal and ledger record the reference (`${secret:stripe}`), never the value.** Replay
   needs no secret: host replies come from the journal.
   `[REVISED: R46A-11]` That holds for `axon replay` against an `AXON_RECORD` journal, not for
   `axon-os replay`, which re-executes live (`axon-os/src/replay.rs:4-7,41` →
   `supervisor::run`) and needs `<store>/<run-id>.axjob` (`cli.rs:747-748`), which `runs/` does
   not keep. So `fire` records an `AXON_RECORD` journal and the payload (§3.1,
   supervisor-private), and replay of a fired run is served from the journal. `axon-os replay`
   refuses (E2508, exit 2) any run whose grant named a secret or that used `Approval`, so a
   replay never re-sends a secret-bearing request or re-raises an approval.
5. **Response scrubbing.** Before a response body reaches the program, the host replaces exact
   occurrences of the secret's bytes (raw, base64 and URL-encoded) with `${secret:stripe}`. This
   is defence in depth against echo endpoints, **not** the boundary: other encodings get
   through. Rule 2 is the boundary, and the spec says so rather than overstating what scrubbing
   does.
   `[REVISED: R46D-7]` Observable: the broker counts replacements, and the run record carries
   `secrets_scrubbed: {"stripe": n}`. S4b's golden fixture uses a loopback echo transport that
   returns the header raw, base64- and URL-encoded. It asserts n = 3 and that the body the
   program sees contains no secret bytes. Tests inject the broker's transport, so the
   https/redirect policy is tested as a pure function and the send goes over loopback.
6. ~~**The interpreter process never holds the value.** Substitution happens in the host
   implementation behind the seam, so native code can't enforce it.~~ `[REVISED: R46A-02]` The
   old seam ran inside the interpreter: `DefaultHost::http_post` (`axon-core/src/host.rs:324-345`,
   reqwest), wrapped by `RecordingHost`, inside the job's child process. "Never holds the value"
   could not hold there. **Secret-bearing requests leave the job through a supervisor-owned
   egress broker**: a Unix socket the supervisor passes to the child as an inherited fd
   (`AXON_EGRESS_FD`). When the grant names any secret, the host sends every `http_*` call whose
   headers contain `${secret:…}` to the broker as (method, url, headers with references, body).
   The broker checks rules 1–3, 7 and 8, substitutes, sends, scrubs (rule 5) and returns the
   response. The interpreter never receives the value.
   `[DECIDED (engineering) 2026-11-11: broker in the axon-os supervisor, fd-passed socket. The alternative, accepting that the value sits in process memory and dropping the claim, is rejected.]`
   Consistent with `AXON_RECORD`/`AXON_REPLAY`, a **native binary refuses to run a job that
   grants secrets** (exit 2), because it has no broker client. The alternative is a native build
   that silently sends `${secret:stripe}` as a literal bearer token.
7. `[REVISED: R46A-01]` **Deployed jobs are confined away from deployment and secret files.**
   axon-os never drops privileges (no `setuid`/`setgid` in `crates/axon-os/src` or
   `crates/axon-jobctl/src`), so a job runs as the supervisor's uid. Under the developer default
   it gets `fs_read`/`fs_write = ["*"]` and `exec = any` (`profile.rs:113-133`), and balanced
   gets `"./"` and `any`. Such a job could read `file:/etc/axon/secrets/stripe` directly
   (bypassing rule 2), edit `deployment.toml`, plant `versions/vN` and write inbox decisions.
   So `deploy create|version` refuses (E2500) any version whose grant has `exec = any`, or
   whose `fs_read`/`fs_write` prefix covers `$AXON_OS_HOME`, `inbox/` or any `file:` secret
   source. `$AXON_OS_HOME/deployments/<name>`, `inbox/`
   and every `file:` secret must pass `private_path::check_private` (owner = supervisor euid,
   no group/other access) at every `fire`; a failure refuses the fire (exit 18). A job that
   holds a secret or calls `approval_request` must use `restricted` or `hermetic`; NH-3 has the
   product question.
8. `[REVISED: R46A-16]` **The model key is a secret like any other.** `ai_*` reads
   `AXON_AI_API_KEY`/`ANTHROPIC_API_KEY` or a `.env` found from cwd
   (`axon-ai/src/lib.rs:47,59-90,368-375`), which these rules did not cover, although the model
   is the most common paid API. Under a deployment, `axon-ai` takes its key only from a broker
   binding (`[secrets.anthropic] hosts = ["api.anthropic.com"]`), and model calls go through the
   broker as in rule 6. The supervisor never forwards those variables, and dotenv is disabled
   (`axon_ai::forbid_dotenv`, as NH-R1 does). `R47-mcp-bridge` (planned) must route tool egress
   through the same broker.

---

## 7. Monitoring and alerts (PM2)

**Status is derived, not collected.** `axon-os deploy status refunds [--json]` reads
~~`runs/*.json` and the ledger~~ `[REVISED: R46A-09]` supervisor-observed inputs only: run
records, `history.jsonl` (fire, approval, kill and alert rows) and broker counters. No metrics
daemon exists to drift from the records. The child's ledger is advisory. `AXON_AUDIT_LEDGER`
reaches a job only if it is set in the supervisor's env and never for hermetic jobs
(`runtime.rs:1020-1031`), and post-hoc authenticity of a child-written ledger is the open S1b
decision. A source the agent writes cannot feed alerts about that agent. It reports:

| Field | Source |
|---|---|
| runs, by verdict (`Completed`, `Denied`, `Halted`, `Budget`, …) and exit code | run records |
| spend: total, last hour, and burn rate against `grant.budget` × expected firings (`spend.total_usd`, `spend.last_hour_usd`, `spend.burn_rate_usd_per_h` `[REVISED: R46D-7]`) | ~~ledger `ai_call` cost~~ broker-observed model-call usage (§6 rule 8) `[REVISED: R46A-09]` |
| gate refusals and sandbox violations (exit 8) | run records |
| kills, and by whom (`kills[].principal`) | ~~R27 latch ledger~~ `[REVISED: R46A-20]` the latch holds only `{"latch":"tripped","reason":…}` (`killchan.rs:160-168`), so the R27 kill path records the caller's `authenticated_principal` in a `killed` history row |
| approvals: requested, approved, denied, timed out, median latency (`approvals.latency_ms_p50` `[REVISED: R46D-7]`) | ~~ledger (§5)~~ supervisor inbox + `history.jsonl` `[REVISED: R46A-09]` |
| candidate vs. current, when a canary is live (`canary.compare`, §4) | run records split by version digest |

`[REVISED: R46D-7]` Each named `--json` field (`axon-deploy-status/1`) is checked against a
golden fixture built from synthetic records, in S5 (spend, kills, approvals) and S7 (canary).

**Alerts** are declared in `deployment.toml` and evaluated after every run and on every R29
monitor trip. `[REVISED: R46D-2]` Evaluation is a pure function
`evaluate(records, history, now) -> Vec<Alert>`, with `now` taken from the supervisor `Clock`,
and is tested at fixed times with no sleeps:

```toml
[alerts]
failure_rate   = { over = "1h", above = 0.2 }
budget_burn    = { projected_exhaustion_within = "24h" }
kill           = true            # any R27 trip
approval_timeout = true          # any TimedOut
[alerts.notify]
webhook = "https://hooks.example.com/axon"    # v1: webhook only
```

- **v1 notifies by webhook only**, sending a JSON `axon-alert/1` body. Slack, email and paging
  are all a webhook away, and a built-in integration per vendor is not this spec's job.
  `[REVISED: R46A-22]` Delivery reuses `axon-ledger/src/webhook.rs` (best-effort `curl`, slack
  and pagerduty shapes already exist) rather than a second sender. Delivery status
  (`delivered|failed`, HTTP code) is recorded in the `alerted` history row, so a failed alert is
  visible in `deploy status`. Webhook-only v1 is product question NH-5.
- **The notifier is outside the job.** It runs in the supervisor, under the supervisor's
  authority, never in the agent's grant. An agent that could suppress its own alerts would make
  the alert worthless, for the same reason the kill switch lives outside what it kills.
- **Alert payloads carry no run output**: IDs, counts, verdicts and links only. Run output can
  contain `@[sensitive]` data, and stdout is not yet sensitivity-checked (ASI Review §G2).
- **R29 gets one hook.** When the monitor trips, it also emits an alert. That is the whole
  change to R29.

---

## 8. Surface summary

```
axon-os deploy create  <name> --job path.axjob          # copies program+job+approval → versions/v1
axon-os deploy version <name> --job path.axjob          # new version; not current until promoted
axon-os deploy promote <name> <vN> [--allow-widen]
axon-os deploy rollback <name> [--allow-widen]
axon-os deploy canary  <name> <vN> --fraction 0.1 | --abort
axon-os deploy revoke  <name> <vN> --reason "…"
axon-os deploy pause|resume <name>
axon-os deploy status  <name> [--json]                  # axon-deploy-status/1
axon-os fire   <name> [--payload FILE]                  # the ONE entry every trigger calls
axon-os serve  [--webhook-bind ADDR]                    # built-in cron + webhook → `fire`
axon-os inbox  list | approve <id> | deny <id> [--reason …]
axon-os replay <run-id>                                  # [REVISED: R46A-11] journal-served for fired runs; E2508 if secrets/Approval
```

`[REVISED: R46A-12]` `fire --payload FILE` copies `FILE` to `runs/<run-id>.payload`; the program
reads it with `event_payload()` (§3(c)), never from stdin.

`[REVISED: R46D-3]` **`serve` contract.** `serve --webhook-bind 127.0.0.1:0` prints
`listening <addr>` on stdout once it is ready. `serve --once-at <RFC3339>` fires every cron
trigger due at that instant and exits. SIGTERM stops accepting, drains in-flight `fire`s and
exits 0. axon-os has no HTTP server dependency today (`crates/axon-os/Cargo.toml`), so S2b
adds a minimal one or a hand-rolled loopback listener; the test is a loopback POST checked
against the resulting record.

`[REVISED: R46A-15]` **Environment.** For non-hermetic jobs the supervisor today forwards
`AXON_AI_MOCK`, `AXON_AI_REPLAY` and `AXON_PATH` from its own environment
(`runtime.rs:1020-1027`), so the supervisor's env steers every fired run. `serve` and `fire`
refuse to start (E2509, exit 2) when any NH-R1 `REFUSED_ENV` variable
(`axon-cortex/src/unattended.rs:46`, `tasks/unattended-auth-spec.md:181`) or `AXON_PATH` is
set. A deployment lists the controls it forwards in `deployment.toml` `[env] forward = [...]`,
and nothing else is forwarded.

`fire` is the primitive. `serve` is a convenience. On day one, a systemd timer, a Kubernetes
CronJob or a CI step can call `axon-os fire refunds`, and `serve` adds nothing those cannot do.
That keeps the scheduler out of the trusted computing base.

Language surface: ~~one builtin (`approval_request`)~~ two builtins (`approval_request`, and
`event_payload` `[REVISED: R46A-12]`), one enum (`Decision`), one effect row (`Approval`).
~~There is no other `.ax` change.~~ `[REVISED: R46D-4]` "No other `.ax` change" hid a cost.
These builtins, the `Approval` row, the `AXON_HOST_FD` protocol and the broker client in the
HTTP host (`host.rs:302/325`) are axon-core changes. Each one pulls in S16 core tests, the S17
codegen rebuild, S22 VM parity and strict all-examples parity, so they are split into core
a-slices (§13).

---

## 9. Error codes and exit codes

| Code | Meaning |
|---|---|
| E2500 | malformed `deployment.toml` (unknown key, bad trigger spec, secret with no `hosts`); `[REVISED: R46A-01/07]` version grant with `exec = any`, an fs prefix covering `$AXON_OS_HOME`/`inbox/`/a `file:` secret, or a relative grant path |
| E2501 | promote/rollback/fire refused: no verifying approval for that version, or version revoked; `[REVISED: R46A-08]` or the token is unkeyed / `AXON_APPROVAL_KEY` is unset |
| E2502 | `approval_request` timeout exceeds the run's remaining time (mode (a)) |
| W2503 | `${secret:…}` reference outside a header value; sent literally |
| E2504 | secret reference to a host not in that secret's `hosts` |
| E2505 | rollback would widen the grant without `--allow-widen` |
| E2506 | job references a secret its grant does not name |
| E2507 | `[REVISED: R46A-03]` `inbox approve` or `inbox deny` by the uid jobs run as (exit 2) |
| E2508 | `[REVISED: R46A-11]` `axon-os replay` of a run that granted a secret or used `Approval` (exit 2) |
| E2509 | `[REVISED: R46A-15]` `serve`/`fire` started with a refused environment control set (exit 2) |
| **exit 16** | run ended **pending approval** (mode (b) only; the run is not failed and not complete) |
| **exit 17** | secret refused (E2504/E2506) |
| **exit 18** | deployment not runnable (E2501, paused, or revoked current version) |

`[REVISED: R46A-05]` **Exits 16–18 are reported by `axon-os` from supervisor state, never derived
from the child's exit status.** A program can state any of them: `fn main() -> i64 { 16 }`
exits 16 and `exit(17)` exits 17 (probes `e16.ax`, `x17.ax`, re-run at f14bc25f), because
`16..=125` pass as written (`EXIT_CODES.md:46`). Exit 16 requires a pending envelope received on
`AXON_HOST_FD`, exit 17 a refusal raised by the broker, and exit 18 a refusal before spawn. A
child that exits 16–18 on its own is a failed run: `fire` exits 1, and the child's code is kept
in the record. The same commit that first uses a code adds its `EXIT_CODES.md` row and widens
the protected row from `2..=15, 101` to `2..=18, 101`. That widening stops only a value falling
out of `main`, because a stated `exit(n)` passes as written even for protected codes
(`EXIT_CODES.md:47`). CX-35 (Draft, `governance/cortex-v015/CX-35-reflex-serving.md:318`) also
proposes claiming 16. `[DECIDED (engineering) 2026-11-11: whichever spec lands first takes 16 with the same-commit EXIT_CODES.md row; the other renumbers to the next free code.]`

## 10. Non-goals

- **Request/response serving at high QPS.** One process per event is deliberate (§3c). A
  serving mode is a different spec with a different proof burden.
- **Autoscaling and multi-host placement.** One supervisor host per deployment in v1.
  Multi-host is where R33's quorum and R26's attestation come in, later.
- **Shadow stage** (§4). Needs reversibility (ASI Review §G8).
- **Automatic promotion on a metric.** v1 reports; a human promotes. Automating it needs the
  sealed-holdout discipline (`R49`), or the canary optimizes for noise.
- **Web UI.** Every surface is CLI + files first. `axon-web` panes are later views over them.

## 11. Invariants touched

- **I-11 (capabilities).** Strengthened for secrets: a credential is authority, so it joins
  the grant and the approval digest (§6 rule 3).
- **I-12 (self-modification cannot widen authority).** Extended to operations: rollback may
  not silently widen (E2505), and revocation pauses rather than falling back.
- **I-13 (provenance cannot be opted out of).** Every deployment action is appended to
  hash-chained `history.jsonl`. Every run stays an unmodified `axon-os-record/1`.
- **New, proposed as I-18: *a deployment adds no authority and no execution semantics beyond
  the run it fires.*** Gate: the same job run via `axon-os run` and via `axon-os fire` produces
  ~~records that differ only in `run_id` and the `deployment` back-reference~~
  `[REVISED: R46A-06]` equivalent runs. Equivalence is over (program digest, imports digest,
  canonical_grant, seed, events, verdict, approval), not over raw record bytes.
  `canonical_manifest` seals `program.display()`, an absolute path (`record.rs:135-138`), so a
  copied version changes `manifest_digest` and `record_digest`. `RunRecord` has no deployment
  field (`record.rs:54-71`), and the record stays unmodified `axon-os-record/1`: the
  back-reference lives in the `fired` history row (run-id, version digest).

## 12. Open questions

- **Q1 (blocks S6): replay-then-live.** Mode (b) needs `AXON_REPLAY` to serve a journal prefix
  and then continue live instead of declaring divergence at exhaustion. The divergence latch is
  deliberate and gated (`replay_host_gate.sh`). The new mode must be a separate variable
  (`AXON_RESUME`), never a relaxation of `AXON_REPLAY`, and must still diverge on any
  *mismatch* inside the prefix. The open part: does the resumed run get a new run-id with a
  `resumed_from` link, or extend the original record? Leaning towards a new run-id, since records
  are immutable.
  `[REVISED: R46D-6]` Nothing runs that gate today: no file under `scripts/` calls
  `replay_host_gate.sh` except the script itself. It is wired into the core gate before S6b
  relies on it as its regression guard (§13 S6a). `[REVISED: R46A-10]` Q1 also covers what a
  resumed prefix needs: `host_await` and `ai_*` journaled as host methods (S6a), so the prefix
  is complete without `AXON_AI_REPLAY`.
- **Q2: canary state across events.** Routing is per event. If an agent's state lives in a
  `Store` shared by both versions, a candidate can corrupt what current reads. v1 answer: a
  canary requires the two versions to declare **disjoint** `fs_write` paths, or the canary is
  refused. Is that too strict for real agents? `[REVISED: Step 1 review]` Product question NH-4.
- **Q3: secret sources beyond `file:`/`env:`.** Vault, cloud KMS and 1Password are each one
  adapter. Defer until a design partner names one.
- **Q4: who may approve which `kind`.** v1 has one admin set. Per-kind routing ("refunds over
  $100 → finance") is RBAC work that overlaps PM10 (teams). `[REVISED: Step 1 review]` Product question NH-6.

### 12.1 Needs human `[REVISED: Step 1 review 2026-11-11, R46A-24]`

**Decided (engineering), canon from this review.** These change no product promise:

- `[DECIDED (engineering) 2026-11-11: secret egress via a supervisor-owned broker on an fd-passed Unix socket (R46A-02, §6 rule 6).]`
- `[DECIDED (engineering) 2026-11-11: host protocol axon-hostmsg/1 on AXON_HOST_FD; decisions only there; payload via event_payload(); stdin /dev/null (R46A-03/04/12).]`
- `[DECIDED (engineering) 2026-11-11: one flock per deployment, history row first, atomic rename, history wins on reconcile; revoke kills live runs and auto-denies (R46A-14).]`
- `[DECIDED (engineering) 2026-11-11: exits 16–18 supervisor-derived; first lander between R46 and CX-35 takes 16 (R46A-05).]`
- `[DECIDED (engineering) 2026-11-11: host_await and ai_* journaled as S6a, before S6b (R46A-10).]`
- `[DECIDED (engineering) 2026-11-11: fired runs keep journal + payload; axon-os replay refuses secret/Approval runs (R46A-11).]`
- `[DECIDED (engineering) 2026-11-11: token schema axon-approval/6 when secrets join the grant (R46A-19).]`
- `[DECIDED (engineering) 2026-11-11: AXON_APPROVAL_KEY required and unkeyed tokens refused for deployments now (R46A-08; NH-1 option (a)).]`
- `[DECIDED (engineering) 2026-11-11: supervisor Clock trait with a fake; alert evaluation and cron matching are pure functions; slice tests in crates/axon-os/tests/r46_*.rs (R46D-1/2).]`

**Needs human.** These are product decisions or hard to reverse; each has a recommendation:

- **NH-1: signing identity beyond keyed symmetric** (needs-human). Options: (b) per-approver
  asymmetric signatures, or (c) the R26/R31 attestation identity. *Recommendation:* decide
  together with S1b/H4. Until then `AXON_APPROVAL_KEY` is mandatory (decided above), which closes
  R46A-08 without waiting.
- **NH-2: build R46 at all, and in what order** (needs-human). *Recommendation:* yes; S0, then
  S1, then S4a/S4b with the broker first, because secrets are the case where people paste keys
  into source today. Then S2, S3, S5, S7, and S6 last (cost table, §13).
- **NH-3: may developer/balanced jobs be deployed?** (needs-human) *Recommendation:* no, for
  any deployment that holds a secret or calls `approval_request`; those require `restricted` or
  `hermetic`. §6 rule 7 already refuses `exec = any` for every version, so the default
  developer/balanced grants are refused regardless.
- **NH-4: canary disjoint-state rule (Q2)** (needs-human). *Recommendation:* keep it strict:
  disjoint `fs_write` paths, or the canary is refused.
- **NH-5: webhook-only alerts in v1** (needs-human). *Recommendation:* yes; vendors are a
  webhook away (§7).
- **NH-6: per-kind approver routing (Q4)** (needs-human). *Recommendation:* defer; v1 has one
  approver set in `approvers.json`.

## 13. Dependency DAG

`[REVISED: R46D-1/D-4/D-5/D-6, R46A-04/10/12]` The original gates were `scripts/r46_s0_probe_gate.sh`,
`r46_trigger_gate.sh`, `r46_approval_gate.sh`, `r46_secrets_gate.sh`, `r46_alerts_gate.sh`,
`r46_durable_approval_gate.sh` and `r46_canary_gate.sh`. Acceptance scripts run only in the
strict block (`run_quiet`, gate.sh:1348, 1500-1503), which starts after a lane exits
(gate.sh:1238-1250), and lanes refuse strict (`tasks/gate-lane-spec.md:40`), so no lane run could
check a slice. They are replaced as follows. Each slice's checks live in
`crates/axon-os/tests/r46_<slice>.rs`, which runs in lane S20
(`release-verification.json` → `cargo test --locked -p axon-os`), or in axon-core tests for
a-slices. One thin `scripts/r46_acceptance_gate.sh` parses the `... ok` lines of the named
tests, the way R31's does, and is wired beside R29 (`gate.sh:1503`). No new crate means no new
gate-lanes row. Every behaviour flip gets one row in `scripts/v022_g01_mutations.py`.

| Node | Depends-on / blocked-by | Gate (named test or script) | Status |
|---|---|---|---|
| R46.S0 | — | `tests/r46_s0_probes.rs`: the §1 probes as **expected-current** assertions, not RED rows (a failing row fails gate.sh): `developer` `host_await_opt` → exit 0 + `None`; `restricted` → (current behaviour, per §1 note); `env_var` in contained → E1001; stdin forged decision accepted (R46A-03); `exit(17)` → 17. Each later slice flips its row in the same commit and adds a g01 row `[REVISED: R46D-5]` | todo |
| R46.S1 | R46.S0, R21-axon-os-supervisor, R22-intent-approve-gateway | `tests/r46_deploy.rs` (`deploy_*`): create/version/promote/rollback/revoke; E2500 (rule 7, relative paths), E2501 (incl. unkeyed), E2505; locking and reconcile; I-18 equivalence over the R46A-06 tuple; the supervisor `Clock` trait with a manual fake, and `cron::matches(spec, t)` as a pure function `[REVISED: R46A-01/06/07/08/14, R46D-2]` | todo |
| R46.S2a | R46.S1 | core: `event_payload()` builtin plus journal method; S16/S17/S22 `[REVISED: R46A-12]` | todo |
| R46.S2b | R46.S2a | `tests/r46_fire.rs` (`fire_*`): `fire` from a cron line (`serve --once-at`) and a loopback webhook POST produce ordinary records; paused deployment → exit 18; refused env → E2509; `serve` readiness line and SIGTERM drain `[REVISED: R46D-3, R46A-15]` | todo |
| R46.S3a | R46.S1, R15-resume-runtime | core: `approval_request`, `Decision`, the `Approval` row in `GRANTABLE_EFFECTS`, the `axon-hostmsg/1` client on `AXON_HOST_FD`; bare `axon run` answers `TimedOut`, never `Approved` `[REVISED: R46D-4, R46A-04/17]` | todo |
| R46.S3b | R46.S3a, R28-capability-audit-ledger | `tests/r46_approval.rs`: approve/deny/timeout on the fake clock; `Approval` admitted under `restricted`; a stdin-forged decision is ignored; E2507; inbox `check_private`; revoke auto-denies `[REVISED: R46A-03/14]` | todo |
| R46.S4a | R46.S1 | core: broker client in the HTTP host (`host.rs:302/325`) on `AXON_EGRESS_FD`; `axon-ai` key via broker; native-build refusal `[REVISED: R46D-4, R46A-02/16]` | todo |
| R46.S4b | R46.S4a | `tests/r46_secrets.rs`: one case per §6 rule 1–8, including wrong host → exit 17 (broker-derived), redirect not followed, `http://` refused, `secrets_scrubbed` golden, no secret bytes in the journal, the record or any response the program sees, `axon-approval/6` digest change, E2508 `[REVISED: R46A-11/13/19]` | todo |
| R46.S5 | R46.S2b, R29-continuous-compliance-monitor | `tests/r46_alerts.rs`: `evaluate(records, history, now)` at fixed times; each alert kind sends one `axon-alert/1` to a loopback listener; payload contains no run output; delivery status in `alerted`; status `--json` golden `[REVISED: R46A-09/20/22, R46D-2/7]` | todo |
| R46.S6a | R46.S3b, blocked-by R46 §12 Q1 | core: `host_await` and `ai_*` journaled as `AXON_RECORD` methods; `scripts/replay_host_gate.sh` wired into the core gate `[REVISED: R46A-10, R46D-6]` | todo |
| R46.S6b | R46.S6a | `tests/r46_durable.rs`: pending → exit 16 only from a supervisor envelope; supervisor killed and restarted; decision resumes; replayed prefix byte-identical | todo |
| R46.S7 | R46.S2b, R46.S5, blocked-by R46 §12 Q2 | `tests/r46_canary.rs`: seeded routing reproducible from `history.jsonl`; `canary.compare` golden `[REVISED: R46D-7]` | todo |

No R46 test sleeps longer than 2 s; every window runs on the fake `Clock` `[REVISED: R46D-2]`.

**Per-slice cost and order** `[REVISED: R46D-4; order per NH-2 recommendation]`. No slice needs
root, GPU or network. The fakes are a loopback `TcpListener` (as in `ceiling.rs:707`), the fake
`Clock` and a tempdir `$AXON_OS_HOME`. Times are `[INFERENCE]` estimates. Every slice then needs
one strict run (≤ 50 min) to land, so S2a, S3a and S4a may share one core commit to save strict
runs.

| Order | Slice | One run | Where it runs | Re-run cost |
|---|---|---|---|---|
| 1 | S0 probes + S1 (Clock, locking, I-18) | ~3 min | lane S20 | axon-os only |
| 2 | S4a core broker client + AI key | ~25 min | S16, S17, S22 | strict for parity |
| 2 | S4b broker, grant, journal, exit 17 | ~3 min | lane S20 | axon-os only |
| 3 | S2a core `event_payload` | ~25 min | S16, S17, S22 | strict for parity |
| 3 | S2b `fire` + `serve` loopback | ~4 min | lane S20 | axon-os only |
| 4 | S3a core builtin, row, hostmsg | ~25 min | S16, S17, S22 | strict for parity |
| 4 | S3b inbox, approver uid | ~3 min | lane S20 | axon-os only |
| 5 | S5 alerts | ~3 min | lane S20 | axon-os only |
| 6 | S7 canary (after NH-4/Q2) | ~3 min | lane S20 | axon-os only |
| 7 | S6a journal `host_await`/`ai_*` + replay_host_gate wiring | ~25 min | core + strict | strict |
| 7 | S6b durable (after Q1) | ~3 min | lane S20 | axon-os only |

## 14. Evidence ledger

None. The status is Draft. The §1 measurements were taken at `8b0d2dc9` on 2026-09-29, using
`target/debug/axon-os run` against a scratch job calling `host_await_opt`, under `restricted`
(exit 8) and `developer` (exit 0, `None` branch taken). S0 turns them into a re-runnable
script.
`[REVISED: Step 1 review]` The §1 anchors were re-checked at `f14bc25f` on 2026-11-11 (see the
§1 notes). Status remains Draft, and no evidence is claimed.
