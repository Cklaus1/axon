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
reserves: E25xx (confirmed free at spec time — no E25 code in crates/, governance/specs/, or AXON_REFERENCE.md), exit 16, exit 17, exit 18 (confirmed free — the codes in use are 3–15, 20–26 and 101)
evidence: none
```

---

## 1. Why

Axon is strong before a run and after it:

- **Pre-flight:** type-check, refinements, `@[contained]`, approval tokens bound to
  program + grant (`axon-os/src/approval.rs`), and the risk-gated deploy pipeline.
- **Forensics:** the host journal, `axon replay`, the hash-chained ledger, and
  `axon-os verify`/`replay`.

Axon has nothing for the stretch in between: a program that keeps running, day after day, on
real inputs. Each of the following was checked against `8b0d2dc9`, not assumed.

**1. "Deploy" means "run once".** `axon deploy --help`: *"Runs type-check → assert_deployable
→ interpreter"*, then exits. There is no deployment object, no version, no trigger, and no
rollback. The PRD's §Governance lifecycle (sandbox → shadow → 10% → 50% → 100%, automatic
rollback) has no implementation.

**2. A job cannot ask a human, and fails silently when it tries.** Probe
(`host_await_opt("approve refund 240?")` inside an `axon-os` job):

| Profile | Result |
|---|---|
| `restricted` | exit **8** — `host_await_opt requires effect IO which is not in the active sandbox's allowed set {}` |
| `developer` | exit **0**, `✓ completed`. The job's stdin is `/dev/null` (`runtime.rs:174`), so the call returns `None` at once and the program proceeds down its "no human answered" branch |

The second row is the dangerous one. The run record gives a reviewer no sign that a human
decision was requested and never made. R15 built the suspend/resume runtime, but the supervisor
never answers the question.

**3. A contained job cannot hold a credential.** `axon-os` launches the job with an explicit
minimal environment (`runtime.rs:128`, *"Hermetic: a minimal explicit env"*), and `env_var` is
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
exactly what an operator calls today: `axon-os run <current version's axjob>`, with the
event payload delivered as the job's input. Every guarantee (approval verified at the run
boundary, grant enforced, budget per run, kill-latch, ledger, record, replay) holds per event
because each event *is* a run.

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
$AXON_OS_HOME/deployments/<name>/
  deployment.toml        name, triggers, alerts, secrets bindings (see §6), current = "v3"
  versions/v3/           program.ax, job.axjob, job.approval  (copied, never referenced by path)
  versions/v3/digest     axsha256 over (program, canonical_grant, manifest)  — the version id
  runs/<run-id>.json     axon-os-record/1, unmodified
  history.jsonl          hash-chained: created, promoted, rolled_back, paused, fired, alerted
```

Versions are **copied** in at creation. A deployment that pointed at a path in the working
tree would run whatever is there now, not what was approved. Approval tokens already bind
program + grant, but a copy makes the invariant local rather than dependent on the token check
being reached.

---

## 4. Fork 2 — versions, rollout, rollback

**Version identity** is the digest over `(program bytes, canonical_grant, canonical_manifest)`,
reusing `approval::canonical_grant` and `record::canonical_manifest` so no new encoding exists
to drift.

**Promotion requires a verifying approval** when the manifest sets `require_approval = true`.
That is the existing policy, unchanged. `axon-os deploy promote refunds v4` runs
`verify_approval` against the copied bytes and refuses (exit **18**, E2501) on failure.

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
as a comparison over the two versions' run records (§5), and in v1 it is advisory only: the
tool reports it and a human promotes.

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

---

## 5. Fork 3 — how a run asks a human (PM3)

### 5.1 The two candidate mechanisms

**(a) Hold the process: in-process suspend via `host_await` (R15), supervisor answers.**
The supervisor becomes the `host_await` peer, replacing `/dev/null`. The request goes to an
inbox, and the reply resumes the parked interpreter.
*Pro:* it is built (R15 Slice 2 carries full `Value` payloads). *Con:* a process (and its
grant, its budget clock and its kill-latch) stays alive for as long as the human takes. That
could be hours, and a supervisor restart loses the run.

**(b) End the run: checkpoint by journal, resume by replay.** The run records its journal up to
the request and exits with a new status, **pending**. The decision starts a new run that
**replays the journal prefix** (same seed, same host answers, so identical state by
construction), receives the decision as the next host reply, and continues **live**.
*Pro:* it survives restarts, holds no authority while waiting, and the prefix/suffix
boundary is visible in the record. *Con:* it needs a mode that does not exist: replay, then
go live at the end of the journal. Today, an exhausted journal is a divergence (exit 11) by
design.

### 5.2 Resolution — **(a) now, (b) as the durable path, one surface for both**

The program-visible surface is identical, so the choice is operational and can change
without touching `.ax`:

```axon
match approval_request("refund", {"amount": "240.00", "order": oid}, 3600) {
    Approved(by) => issue_refund(oid)        // by: the authenticated approver
    Denied(by, reason) => log_denial(oid, reason)
    TimedOut => hold_for_review(oid)         // never treated as approval
}
```

- `approval_request(kind: str, detail: Dict, timeout_s: i64) -> Decision`. The kernel sends a
  reserved envelope over `host_await_val`. The supervisor recognises the envelope and does not
  forward it as ordinary host I/O.
- **Effect row: `{Approval}`, a new row, not `IO`.** Measured above: gating it on `IO` makes it
  unusable under `restricted`, where it matters most. A request for permission must never need
  broad authority to be made. `Approval` is granted by every profile, including `hermetic`
  (where the decision is served from the journal on replay).
- **`TimedOut` is a distinct variant, not `Denied`.** Collapsing them loses the one fact a
  reviewer needs: *nobody looked*. The supervisor enforces `timeout_s ≤ remaining run
  timeout` under (a) and refuses a longer wait (E2502) instead of letting the run die mid-wait.
- **Authenticated approver.** `by` is not a claimed name. In v1 it is the uid of the process
  that wrote the decision, mapped through the same `authenticated_admins` table the ledger
  uses (`axon-ledger/src/rbac.rs`), with the same lesson (FG-012): a claim is not authority.
- **Everything is ledgered:** request, delivery, decision, approver, latency, and timeout.
- **Without a supervisor** (`axon run` directly), `approval_request` answers `TimedOut`
  immediately and prints a one-line warning to stderr. It never answers `Approved`. Fail closed
  is the default here, as everywhere.

Slice order: **S3 ships (a)**. **S6 ships (b)** behind `approval.durable = true` in
`deployment.toml`, and needs the replay-then-live mode (§12 Q1, the one decision that blocks this
spec's later slices).

### 5.3 The inbox

v1 is files and a CLI: `$AXON_OS_HOME/inbox/<request-id>.json`, `axon-os inbox list`,
`axon-os inbox approve|deny <id> [--reason …]`. Notification (§7) tells a human there is
something to decide. A web pane in `axon-web` is a later slice over the same files. The CLI is
the contract; the UI is a view.

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
the real `AxonHost` substitutes the value:

```axon
let h = "{\"Authorization\": \"Bearer ${secret:stripe}\"}"
http_post("https://api.stripe.com/v1/refunds", h, body)
```

```toml
# deployment.toml — the operator binds names; the program never sees a value
[secrets.stripe]
source = "file:/etc/axon/secrets/stripe"    # v1 sources: file:, env: (of the SUPERVISOR)
hosts  = ["api.stripe.com"]                 # REQUIRED — a secret is bound to its destinations
```

Rules (each is a test in §8):

1. **Substitution happens only in header values**, never in the URL, body, `exec` argv, or any
   other builtin. A `${secret:…}` anywhere else is sent literally, and the run gets a W2503
   warning in its record.
2. **Only when the request's host matches the secret's `hosts` list.** This is the control that
   matters. `net = ["*"]` plus a key means the key can go anywhere, so a secret that may be sent
   to any host the program chooses is exfiltrated by one line. On a mismatch the call is refused
   (**exit 17**, E2504) rather than sent without the header: a silent strip would turn an attack
   into a mysterious 401.
3. **The job's grant must name the secret** (`[grant] secrets = ["stripe"]`). The name joins
   the grant, and therefore `canonical_grant` and the approval digest. Adding a secret to a job
   is a change of authority and invalidates its approval. `canonical_grant` destructures
   exhaustively, so adding the field is a compile error until it is encoded, which is the
   guarantee that file was built to give.
4. **The journal and ledger record the reference (`${secret:stripe}`), never the value.** Replay
   needs no secret: host replies come from the journal.
5. **Response scrubbing.** Before a response body reaches the program, the host replaces exact
   occurrences of the secret's bytes (raw, base64 and URL-encoded) with `${secret:stripe}`. This
   is defence in depth against echo endpoints, **not** the boundary: other encodings get
   through. Rule 2 is the boundary, and the spec says so rather than overstating what scrubbing
   does.
6. **The interpreter process never holds the value.** Substitution happens in the host
   implementation behind the seam, so native code can't enforce it. Consistent with
   `AXON_RECORD`/`AXON_REPLAY`, a **native binary refuses to run a job that grants secrets**
   (exit 2), because it has no seam to substitute at. The alternative is a native build that
   silently sends `${secret:stripe}` as a literal bearer token.

---

## 7. Monitoring and alerts (PM2)

**Status is derived, not collected.** `axon-os deploy status refunds [--json]` reads
`runs/*.json` and the ledger. No metrics daemon exists to drift from the records. It reports:

| Field | Source |
|---|---|
| runs, by verdict (`Completed`, `Denied`, `Halted`, `Budget`, …) and exit code | run records |
| spend: total, last hour, and burn rate against `grant.budget` × expected firings | ledger `ai_call` cost |
| gate refusals and sandbox violations (exit 8) | run records |
| kills, and by whom | R27 latch ledger |
| approvals: requested, approved, denied, timed out, median latency | ledger (§5) |
| candidate vs. current, when a canary is live | run records split by version digest |

**Alerts** are declared in `deployment.toml` and evaluated after every run and on every R29
monitor trip:

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
```

`fire` is the primitive. `serve` is a convenience. On day one, a systemd timer, a Kubernetes
CronJob or a CI step can call `axon-os fire refunds`, and `serve` adds nothing those cannot do.
That keeps the scheduler out of the trusted computing base.

Language surface: one builtin (`approval_request`), one enum (`Decision`), one effect row
(`Approval`). There is no other `.ax` change.

---

## 9. Error codes and exit codes

| Code | Meaning |
|---|---|
| E2500 | malformed `deployment.toml` (unknown key, bad trigger spec, secret with no `hosts`) |
| E2501 | promote/rollback/fire refused: no verifying approval for that version, or version revoked |
| E2502 | `approval_request` timeout exceeds the run's remaining time (mode (a)) |
| W2503 | `${secret:…}` reference outside a header value; sent literally |
| E2504 | secret reference to a host not in that secret's `hosts` |
| E2505 | rollback would widen the grant without `--allow-widen` |
| E2506 | job references a secret its grant does not name |
| **exit 16** | run ended **pending approval** (mode (b) only; the run is not failed and not complete) |
| **exit 17** | secret refused (E2504/E2506) |
| **exit 18** | deployment not runnable (E2501, paused, or revoked current version) |

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
  records that differ only in `run_id` and the `deployment` back-reference.

## 12. Open questions

- **Q1 (blocks S6): replay-then-live.** Mode (b) needs `AXON_REPLAY` to serve a journal prefix
  and then continue live instead of declaring divergence at exhaustion. The divergence latch is
  deliberate and gated (`replay_host_gate.sh`). The new mode must be a separate variable
  (`AXON_RESUME`), never a relaxation of `AXON_REPLAY`, and must still diverge on any
  *mismatch* inside the prefix. The open part: does the resumed run get a new run-id with a
  `resumed_from` link, or extend the original record? Leaning towards a new run-id, since records
  are immutable.
- **Q2: canary state across events.** Routing is per event. If an agent's state lives in a
  `Store` shared by both versions, a candidate can corrupt what current reads. v1 answer: a
  canary requires the two versions to declare **disjoint** `fs_write` paths, or the canary is
  refused. Is that too strict for real agents?
- **Q3: secret sources beyond `file:`/`env:`.** Vault, cloud KMS and 1Password are each one
  adapter. Defer until a design partner names one.
- **Q4: who may approve which `kind`.** v1 has one admin set. Per-kind routing ("refunds over
  $100 → finance") is RBAC work that overlaps PM10 (teams).

## 13. Dependency DAG

| Node | Depends-on / blocked-by | Gate (named test or script) | Status |
|---|---|---|---|
| R46.S0 | — | `scripts/r46_s0_probe_gate.sh`: encodes the §1 probes as RED tests (host_await under `developer` silently `None`; under `restricted` exit 8; `env_var` in contained job refused) so each later slice flips a named row | todo |
| R46.S1 | R46.S0, R21-axon-os-supervisor, R22-intent-approve-gateway | `cargo test -p axon-os deploy_` — create/version/promote/rollback/revoke; E2501, E2505; I-18 record-equivalence test | todo |
| R46.S2 | R46.S1 | `cargo test -p axon-os fire_` + `scripts/r46_trigger_gate.sh` — `fire` from a cron line and a webhook POST produce ordinary records; paused deployment → exit 18 | todo |
| R46.S3 | R46.S1, R15-resume-runtime, R28-capability-audit-ledger | `scripts/r46_approval_gate.sh` — approve/deny/timeout paths; `Approval` row granted under `restricted`; bare `axon run` answers `TimedOut`, never `Approved`; approver is uid-derived | todo |
| R46.S4 | R46.S1 | `scripts/r46_secrets_gate.sh` — the six §6 rules, one case each, incl. wrong host → exit 17, journal contains no secret bytes, native binary refuses | todo |
| R46.S5 | R46.S2, R29-continuous-compliance-monitor | `scripts/r46_alerts_gate.sh` — each alert kind fires one `axon-alert/1` webhook to a local listener; payload contains no run output | todo |
| R46.S6 | R46.S3, blocked-by R46 §12 Q1 | `scripts/r46_durable_approval_gate.sh` — pending exits 16; supervisor killed and restarted; decision resumes; replayed prefix byte-identical | todo |
| R46.S7 | R46.S2, R46.S5, blocked-by R46 §12 Q2 | `scripts/r46_canary_gate.sh` — seeded routing reproducible from `history.jsonl`; status splits metrics by version | todo |

## 14. Evidence ledger

None. The status is Draft. The §1 measurements were taken at `8b0d2dc9` on 2026-09-29, using
`target/debug/axon-os run` against a scratch job calling `host_await_opt`, under `restricted`
(exit 8) and `developer` (exit 0, `None` branch taken). S0 turns them into a re-runnable
script.
