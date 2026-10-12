# Closed loop walkthrough (`axon-loop`, v0.22)

This walkthrough drives the v0.22 closed loop end to end on a scratch store,
using the `axon-loop` binary from `crates/axon-loop`. Each step below shows the
command and its real output from one run of
[`examples/closed-loop/run.sh`](../examples/closed-loop/run.sh). Output is
trimmed, and the long `--in` paths are shortened. Digests depend on the
contents, so the refs in your run will differ wherever a timestamp feeds into
a document (step 6 onwards).

What you need: a Rust toolchain, `bash`, `jq`, `openssl` 3.x (for Ed25519),
`sha256sum` and `od`. Nothing is read from or written to `/etc/axon`.

```text
$ cargo build -p axon-loop --bin axon-loop      # binary: ${CARGO_TARGET_DIR:-target}/debug/axon-loop
$ bash examples/closed-loop/run.sh "${CARGO_TARGET_DIR:-target}/debug/axon-loop" [work-dir]
...
== done: every step above ran and every assertion held
work dir: /var/tmp/r51-tmp/cl1
```

The script exits 0 only if every step returned the expected exit code and
every asserted field had the expected value. In the run quoted here that was
35 field assertions and 10 deliberate refusals. If you pass a `work-dir` it
must be empty or not yet exist; otherwise the script makes one with
`mktemp`. Under the work dir:

* `store/` is the loop store.
* `docs/` holds every document the script actually sent.
* `out/` holds the stdout and stderr of every call.

## Concepts in plain words

**Scope.** Everything is per `(tenant_id, task_family)`. Here that is
`demo-tenant/demo-coding`.

**Policy.** An `axon.closed-loop.policy/1` document. In v0.22 a policy is only
a *tool shortlist*: an ordered subset of a registered candidate list (`read`,
`search`, `edit`, ...). A policy is named by its `cl22:` digest, which is
sha256 over its canonical JSON (sorted keys, no whitespace). Every
cross-reference in the loop is such a digest.

**Policy pointer and epoch.** Each scope has one pointer saying which policy
is active, plus an integer *authority epoch*. A scope starts *paused* at epoch
0, and `pointer resolve` refuses (exit 6) while nothing usable is active. The
pointer moves only through `pointer transition`, which is a compare-and-swap:
the caller states the policy and epoch it believes are current
(`expected_policy_ref`, `expected_epoch`). If another writer moved the
pointer first, the transition is refused with exit 5 and nothing is written.
Every move adds 1 to the epoch.

**Baseline.** The operator's designation of the *incumbent of record*: the one
policy that may be activated from paused without an admission.

**Frozen plan.** The experiment register (`axon.closed-loop.pilot/1`). It
names the incumbent and the candidate, the task manifest, the corpora, and the
admission rule (margin, budget, missing-data rule and so on). A plan can be
registered with fields unset, but it can only be *frozen* once every operator
field is set and every rule parses (exit 7 otherwise). Once frozen it cannot
change. The trial population (*assignment*) is issued after the freeze and
before any trial.

**EVO.** The proposer. From the incumbent and at least one learning-eligible
*discovery* episode, `evo propose` makes exactly one candidate. The candidate
is one edit inside the incumbent shortlist: it removes one tool or swaps two.
It can never add a tool. Which edit is chosen depends on the request's `seed`
and on which shortlists were already tried in the scope. The episodes decide
whether a proposal is allowed at all, and they are recorded as
`discovery_evidence_refs`, but their content does not steer the edit.

**Trial evidence and intake.** Each trial of the experiment produces:

* an episode sidecar, which says what happened and at what cost;
* a context receipt, which says where it ran and who observed it;
* a policy ack, which says which policy the agent pinned;
* the agent run's Fabric request and receipt;
* an acceptance check's Fabric request and receipt;
* the check verifier's Ed25519 attestation over that check request and
  receipt.

`intake episode` joins these documents, verifies the attestation against the
verifier key in the store config, and records the episode in the ledger.

**EVL.** The evaluator. `evl evaluate` takes the frozen experiment, the
assignment and the delivered trials. For each arm it reports
`verified_pass`, `fail`, `unknown` and `missing`. A trial whose evidence does
not bind is `unknown`, and unknown never counts as a pass.

**Admission.** `admit` applies the frozen plan's rule to a stored evaluation
and returns ACCEPT, REJECT or INCONCLUSIVE. Only an admitter listed in the
config may call it. An ACCEPT is what lets `pointer transition` activate the
candidate. It is *policy* admission only and grants no effect authority (see
Limits).

**TEL.** Whole-task economics over the episodes' `Usage` records. A cost that
was never reported stays `unknown`, so the total becomes `unresolved` instead
of a smaller number. An attempt counted twice is refused.

**Ledger.** Every accepted write is one entry in `store/ledger.jsonl`, hash
chained (`prev`), with `ledger.head` and `ledger.anchor`. With
`AXON_ATTEST_KEY` set, each entry and the head also carry an HMAC.

## Exit codes

| code | meaning |
|---|---|
| 0 | ok (one JSON document on stdout) |
| 2 | usage, I/O or corrupt store (including a keyed store opened without its key) |
| 3 | malformed input (strict parse) |
| 4 | refused by a rule; nothing written |
| 5 | CAS conflict (stale epoch or wrong expected policy); nothing written |
| 6 | paused (no usable active policy) |
| 7 | plan not ready (unset operator fields, not approved, not frozen) |

On failure stdout is empty and stderr carries
`{"schema":"axon.loop.error/1","error":...,"exit_code":N,"message":...}`.

## The inputs

Everything under `examples/closed-loop/inputs/` was written by hand, with
shapes taken from `crates/axon-loop/tests/` and the contract bundle
`crates/axon-loop-contracts/tests/fixtures/bundle.json`. A field whose value
starts with `SET-BY-RUN.SH` is a digest or a time that only exists once an
earlier step has run. `run.sh` fills those fields in with `jq` and writes the
finished document under `docs/`.

| file | role | who produces it in a real deployment |
|---|---|---|
| `config.json` | trusted admitters, verifiers, observers; the verifier's public key and pinned check executable; per-task acceptance check | the operator, by hand (no verb writes it) |
| `candidate-set.json` | the tools a policy may choose from | operator |
| `task-manifest.json` | the task ids the experiment may use | operator |
| `policy-incumbent.json` | the current shortlist `["read","search","edit"]` | operator, or Axon DEC (`axon-reflex` `to_policy_envelope`) |
| `baseline.json` | designates the incumbent of record | operator |
| `transition.json` | template for every pointer move (activate, rollback) | operator / admitter |
| `evo-request.json` | incumbent + eligible list + discovery episodes + seed | the proposer agent |
| `plan-unset.json` | `crates/axon-loop/tests/fixtures/pilot-template.json` with only `experiment_id` and scope set | - |
| `plan.json` | the configured pilot plan (rules below) | operator |
| `assignment.json` | the four trials: 2 tasks x {incumbent, challenger-1} | trusted admitter |
| `trial/episode.json` | `axon.closed-loop.episode/1` sidecar | **MiCode** (`<repo>/.micode/axon/closed-loop/episodes/`) |
| `trial/context.json` | `axon.closed-loop.context/1` receipt | **MiCode** |
| `policy-ack.json` | `micode.closed-loop.policy-ack/1` | **MiCode** |
| `trial/acf-request.json`, `trial/acf-receipt.json` | the agent run's ACF compute request / execution receipt | **Compute Fabric** |
| `trial/check-request.json`, `trial/check-receipt.json` | the acceptance check's request / receipt (`verification: passed`) | **Compute Fabric** |
| attestation (built by `run.sh`) | `acf-receipt-attestation/2` signed with a demo Ed25519 key | **Fabric's verifier signer** |
| `trial/projection.json` | the trial's policy projection (`sidecar_policy_ref`, ACF policy digest) | MiCode / Fabric |
| `evl-request.json` | policies + assignment + delivered trials | the evaluator |
| `admit-request.json` | evaluation ref + admitter | trusted admitter |
| `tel-request.json` | episodes to summarize | anyone (TEL reads no store) |

The plan's rule fields, as `crates/axon-loop/src/rules.rs` accepts them:

```json
"independent_units": 2, "repetitions": 1, "candidate_budget": 1,
"independent_unit": "task",
"quality_margin": "pass_rate_margin_ppm=0",
"economic_threshold": "report_only",
"budget_rule": "max_unresolved_liability_micro=100000000",
"uncertainty_rule": "exact_bounds",
"missing_data_rule": "unknown_bounds",
"multiplicity_rule": "single_candidate",
"order_rule": "paired_tasks",
"cache_rule": "not_enforced_here"
```

The other `*_ref` fields in `plan.json` (split, corpora manifests, analysis
method, data use, rollback, approval) are placeholder digests. The loop only
checks that they are set.

The trial documents are what **MiCode** (the episode sidecar, context receipt
and policy ack) and **Compute Fabric** (the request, receipt and attestation)
write in a real deployment. In this walkthrough no agent runs and no check
runs: `run.sh` writes the outcome (`passed`) and the cost (100 micro-USD per
incumbent trial, 50 per challenger trial) straight into these files. The loop
does not judge whether a check *should* have passed. It checks only that a
trusted verifier, with the operator-registered key, signed a receipt for the
pinned check executable, and that the receipt binds to this trial.

## Step by step

### 1. Operator config

```text
== 1. operator config (written by hand: no axon-loop verb writes config.json)
wrote $WORK/store/config.json: admitters=["op:admitter"] verifiers=["demo:verifier"] observers=["demo:observer"]
```

`run.sh` generates a demo Ed25519 key pair with `openssl`. It puts the public
key in `verifier_keys["demo:verifier"]` and the `acf1:` digest of the demo
check executable in `verifier_pins`. The admitter and observer names are
premises: in the development class nothing authenticates them.

### 2-3. Registries and the incumbent policy

```text
$ axon-loop --store $WORK/store candidates put --in examples/closed-loop/inputs/candidate-set.json
  ok: .candidate_set_ref = cl22:1bace852f123c7194935516e2d1eec44deac2ae5f1d8426e14a2205ca740debf
$ axon-loop --store $WORK/store tasks put --in examples/closed-loop/inputs/task-manifest.json
  task_manifest_ref = cl22:8d6af4085548c0de0bd5912b16396e8b76db58ce6c6331087f3d7f7c8b8c2288
$ axon-loop --store $WORK/store policy put --in $WORK/docs/policy-incumbent.json
  ok: .policy_ref = cl22:e899bed3f82ce17f5405b630e605578740fe684abde6adffd30385ea099168d9
```

`run.sh` asserts that `candidate_set_ref` equals its own `cl22` of the sorted
candidate list, and that `policy_ref` equals its own `cl22` of the policy
document. So the digests can be reproduced with `jq -S -c . | sha256sum`.

### 4-5. Paused, then baseline and activation (epoch 0 -> 1)

```text
$ axon-loop --store $WORK/store pointer resolve --tenant demo-tenant --family demo-coding
  -> exit 6 (expected): {"error":"paused","exit_code":6,"message":"paused: scope demo-tenant/demo-coding is paused at epoch 0"}
$ axon-loop --store $WORK/store pointer baseline --in $WORK/docs/baseline.json
  baseline_ref = cl22:850ba48e06348ef8f099d815c09de3643e1905736d611d358829b87c29ad4b2f
$ axon-loop --store $WORK/store pointer transition --in $WORK/docs/transition-boot.json
  ok: .pointer.epoch = 1
  ok: .pointer.active_policy_ref = cl22:e899bed3f82ce17f5405b630e605578740fe684abde6adffd30385ea099168d9
$ axon-loop --store $WORK/store pointer resolve --tenant demo-tenant --family demo-coding
  ok: .pin.epoch = 1
  ok: .pin.version.digest = cl22:e899bed3f82ce17f5405b630e605578740fe684abde6adffd30385ea099168d9
```

`transition-boot.json` is an `activate` with
`expected_policy_ref = cl22:000…0` (paused), `expected_epoch = 0`, and the
baseline ref as its `admission_ref`. From paused, only the baseline's policy
can be activated this way.

### 6. EVO: one candidate

```text
$ axon-loop --store $WORK/store evo propose --in $WORK/docs/evo-request.json
  ok: .candidate_policy_ref = cl22:d5662300e454c142df64ca862dc0204577d882b241ce4e5df8c84ce3e22c62e8
  candidate shortlist: ["read","edit","search"] (incumbent: ["read","search","edit"])
```

The discovery episode `d-1` is a full trial bundle like the ones in step 10,
with `corpus_role: "discovery"`. The response's `hypothesis` records the edit:
`{"kind":"swap","a":"search","b":"edit"}`. EVO refuses a request whose
episodes include a confirmation or reporting episode. It also refuses one
with no learning-eligible episode at all, meaning no episode that is
completed, verified by a trusted verifier other than the proposer, and has
final usage.

### 7-8. Plan: not ready, then frozen and immutable

```text
$ axon-loop --store $WORK/store plan register --in examples/closed-loop/inputs/plan-unset.json
  ok: .unset_fields | length = 24
$ axon-loop --store $WORK/store plan freeze --experiment operator-must-configure
  -> exit 7 (expected): {"error":"not_ready","exit_code":7,"message":"not ready: cannot freeze operator-must-configure: operator fields unset: task_manifest_ref, repository_split_ref, discovery_manifest_ref, confirmation_manifest_ref, reporting_manifest_ref, controls_ref, incumbent_policy_ref, candidate_policy_ref, analysis_method_ref, data_use_ref, rollback_policy_ref, approval_ref, independent_units, repetitions, candidate_budget, independent_unit, quality_margin, economic_threshold, uncertainty_rule, missing_data_rule, multiplicity_rule, order_rule, cache_rule, budget_rule"}
$ axon-loop --store $WORK/store plan register --in $WORK/docs/plan.json
  ok: .unset_fields | length = 0
$ axon-loop --store $WORK/store plan freeze --experiment demo-exp
$ axon-loop --store $WORK/store plan show --experiment demo-exp
  ok: .frozen_ref = cl22:52043a40551d38ff87e3bc2f11bb7383d0548a0c6a776c404da5c4069840c502
  ok: .start_blockers | length = 0
$ axon-loop --store $WORK/store plan register --in $WORK/docs/plan-changed.json
  -> exit 4 (expected): {"error":"refused","exit_code":4,"message":"refused: plan demo-exp is frozen; a frozen plan cannot change"}
```

`plan-changed.json` is the frozen plan with `repetitions` changed from 1 to 9.

### 9. Assignment

```text
$ axon-loop --store $WORK/store plan assign --in $WORK/docs/assignment.json
  ok: .trials = 4
```

### 10. Trial evidence and intake (one forged attestation refused)

For each of the four trials (`i0`, `i1` on the incumbent; `c0`, `c1` on the
candidate), `run.sh` builds the documents listed above, computes every
cross-reference digest, and signs the check binding with the demo verifier
key. Before the real intake of `i0` it presents the same documents with an
attestation signed by a second, unregistered key:

```text
$ axon-loop --store $WORK/store intake episode --in $WORK/docs/trials/i0/episode.json --context $WORK/docs/trials/i0/context.json --ack $WORK/docs/acks --verification-request $WORK/docs/trials/i0/check-request.json --verification-receipt $WORK/docs/trials/i0/check-receipt.json --verification-attestation $WORK/docs/trials/i0/attestation-forged.json
  -> exit 4 (expected): {"error":"refused","exit_code":4,"message":"refused: verification attestation refused: shape: attestation is signed by ed25519:1bcdc4f58f1206a7, not by ed25519:acde2f3f05d65802, the key the operator registered for verifier demo:verifier"}
$ axon-loop --store $WORK/store intake episode --in $WORK/docs/trials/i0/episode.json ... --verification-attestation $WORK/docs/trials/i0/attestation.json
  ok: .recorded_now = true
  (i1, c0, c1: same, each ok: .recorded_now = true)
```

`--ack` is a directory of policy acks, one per policy. The intake picks the
ack whose pin matches the episode's `policy_ref`.

### 11. EVL

```text
$ axon-loop --store $WORK/store evl evaluate --in $WORK/docs/evl-request.json
  evaluation_ref = cl22:562748438c97926454c9512e3717595014526a548cc1e200e5628c1227a41fe5
{"arm_id":"challenger-1","verified_pass":2,"fail":0,"unknown":0,"missing":0}
{"arm_id":"incumbent","verified_pass":2,"fail":0,"unknown":0,"missing":0}
```

The per-arm economics in the evaluation are *unresolved*, not known:

```text
{"arm_id":"challenger-1","economics":{"known_sum_micro":100,"state":"unresolved","unknown_count":2,"unresolved_liability_micro":200000}}
{"arm_id":"incumbent","economics":{"known_sum_micro":200,"state":"unresolved","unknown_count":2,"unresolved_liability_micro":200000}}
```

EVL adds each trial's *Fabric execution* as a separate cost component.
v0.22 has no execution price schedule (D10, `crates/axon-loop/README.md`), so
that component is always unknown. It holds the request's reservation
(`limits.max_cost_micro = 100000`) as liability. Only the sidecars' model
costs (2 x 50 and 2 x 100) are known.

### 12. Admission (an untrusted admitter refused)

```text
$ axon-loop --store $WORK/store admit --in $WORK/docs/admit-request-stranger.json
  -> exit 4 (expected): {"error":"refused","exit_code":4,"message":"refused: admitter op:stranger is not in the trusted-admitter set"}
$ axon-loop --store $WORK/store admit --in $WORK/docs/admit-request.json
  ok: .admission.decision = ACCEPT
  admission_ref = cl22:14fa928d1a77f6789cb9d539b9d6b7995ff93a87ab6755faf6ab3d1655571261
["all frozen plan criteria established","economics report-only (ADR-001 D4): recorded in the arm facts, not assessed"]
```

With `pass_rate_margin_ppm=0` and 2/2 verified passes on both arms, the
candidate is non-inferior. `economic_threshold: report_only` means cost
decided nothing.

### 13. Promote, and a stale writer refused (epoch 1 -> 2)

```text
$ axon-loop --store $WORK/store pointer transition --in $WORK/docs/transition-promote.json
  ok: .pointer.epoch = 2
  ok: .pointer.active_policy_ref = cl22:d5662300e454c142df64ca862dc0204577d882b241ce4e5df8c84ce3e22c62e8
$ axon-loop --store $WORK/store pointer transition --in $WORK/docs/transition-stale.json
  -> exit 5 (expected): {"error":"conflict","exit_code":5,"message":"conflict: stale epoch: expected_epoch 1 but current is 2"}
$ axon-loop --store $WORK/store pointer resolve --tenant demo-tenant --family demo-coding
  ok: .pin.epoch = 2
  ok: .pin.version.digest = cl22:d5662300e454c142df64ca862dc0204577d882b241ce4e5df8c84ce3e22c62e8
  ok: .admission_ref = cl22:14fa928d1a77f6789cb9d539b9d6b7995ff93a87ab6755faf6ab3d1655571261
```

`transition-stale.json` is the same promotion under a new `transition_id`,
still claiming epoch 1. It stands for a second writer that read the pointer
before the first one moved it. Re-sending the *same* `transition_id` with the
same bytes is instead replayed idempotently. Trimmed `pointer resolve` output:

```json
{
  "admission_ref": "cl22:14fa…1261",
  "mechanism_test": false,
  "pin": {
    "ack": { "state": "acknowledged" },
    "epoch": 2,
    "version": { "digest": "cl22:d566…62e8", "policy_id": "candidate-1" }
  },
  "policy": { "policy_id": "candidate-1", "shortlist": ["read", "edit", "search"], "...": "..." },
  "schema": "axon.loop.resolve/1"
}
```

This is what an agent runtime (MiCode) consumes: the policy to pin, and the
epoch to stamp on its context receipt and episode.

### 14. TEL

```text
$ axon-loop tel summarize --in $WORK/docs/tel-request.json
  ok: .summary.records = 4
  ok: .summary.by_currency[0].total = {"cost_micro":300,"state":"known"}
$ axon-loop tel summarize --in $WORK/docs/tel-request-unknown.json
  ok: .summary.records = 5
  ok: .summary.by_currency[0].total.state = unresolved
  ok: .summary.by_currency[0].total.known_sum_micro = 300
  ok: .summary.by_currency[0].total.unknown_count = 1
$ axon-loop tel summarize --in $WORK/docs/tel-request-dup.json
  -> exit 4 (expected): {"error":"refused","exit_code":4,"message":"refused: attempt cl22:0376810927abd249b5fe491e80b0e676bcb457893cb0f02a592baf44d0315dd2 is accounted twice (duplicate usage would double-count)"}
```

The first request covers the four trial episodes (2 x 100 + 2 x 50). The
second adds a fifth episode whose `usage.cost_micro` is `null`: the total
becomes `unresolved`, not 300. The third lists the same episode twice. TEL
reads no store. Without `price_schedule`/`fabric_attempts` it sums only the
sidecar usages, which is why its total is `known` where EVL's is not.

### 15. Revoke, then roll back (epoch 2 -> 3)

```text
$ axon-loop --store $WORK/store pointer revoke --tenant demo-tenant --family demo-coding --policy cl22:d5662300e454c142df64ca862dc0204577d882b241ce4e5df8c84ce3e22c62e8 --reason cl22:aaaa…aaaa --issuer op:admitter
$ axon-loop --store $WORK/store pointer resolve --tenant demo-tenant --family demo-coding
  -> exit 6 (expected): {"error":"paused","exit_code":6,"message":"paused: active policy cl22:d5662300e454c142df64ca862dc0204577d882b241ce4e5df8c84ce3e22c62e8 is revoked; issue a fenced pause or rollback"}
$ axon-loop --store $WORK/store pointer transition --in $WORK/docs/transition-rollback.json
  ok: .pointer.epoch = 3
  ok: .pointer.active_policy_ref = cl22:e899bed3f82ce17f5405b630e605578740fe684abde6adffd30385ea099168d9
$ axon-loop --store $WORK/store pointer resolve --tenant demo-tenant --family demo-coding
  ok: .pin.epoch = 3
  ok: .pin.version.digest = cl22:e899bed3f82ce17f5405b630e605578740fe684abde6adffd30385ea099168d9
$ axon-loop --store $WORK/store pointer show --tenant demo-tenant --family demo-coding
  ok: .transitions = 3
  ok: .revocations.revoked | length = 1
```

A revocation does not move the pointer. It makes `resolve` refuse right
away. The rollback is a fenced transition (`kind: "rollback"`, expected =
candidate @ 2, target = incumbent). It is accepted only because the incumbent
was previously active in this scope and its baseline is still valid.

### 16. The ledger

```text
  ledger.jsonl: 18 entries
  1 candidate_set      7 plan_registered   13 episode_intake
  2 task_manifest      8 freeze            14 evaluation
  3 baseline           9 assignment        15 admission
  4 transition        10 episode_intake    16 transition
  5 hypothesis        11 episode_intake    17 revocation
  6 plan_registered   12 episode_intake    18 transition
```

(The script prints one entry per line; this table is reflowed.) The refusals
added nothing. Store layout after the run:

```text
admissions/<hex>.json  assignments/<hex>.json  baselines/<hex>.json
candidate-sets/demo-tenant/demo-coding/<hex>.json  config.json
contexts/<hex>.json  episodes/<hex>.json  evaluations/<hex>.json
fabric-attestations/<hex>.json  fabric-receipts/<hex>.json  fabric-requests/<hex>.json
ledger.anchor  ledger.head  ledger.jsonl  locks/root.lock  plans/<hex>.json
policies/<hex>.json  scopes/demo-tenant/demo-coding/pointer.json
task-manifests/demo-tenant/demo-coding/<hex>.json
```

### 17. Keyed ledger (`AXON_ATTEST_KEY`)

The script always runs a separate demo on its own store:

```text
$ AXON_ATTEST_KEY=00010203... axon-loop --store $WORK/keyed-store candidates put --in examples/closed-loop/inputs/candidate-set.json
  ledger entry carries a MAC: true head: ["entry_ref","mac","schema","seq"]
$ env -u AXON_ATTEST_KEY axon-loop --store $WORK/keyed-store pointer show --tenant demo-tenant --family demo-coding
  -> exit 2 (expected): {"error":"io","exit_code":2,"message":"io: store corrupt: ledger is keyed but no key is configured: set AXON_ATTEST_KEY"}
$ AXON_ATTEST_KEY=ffeeddcc... axon-loop --store $WORK/keyed-store pointer show --tenant demo-tenant --family demo-coding
  -> exit 2 (expected): {"error":"io","exit_code":2,"message":"io: store corrupt: ledger entry seq 1 is not authenticated under the operator key (forged, or written without AXON_ATTEST_KEY)"}
$ AXON_ATTEST_KEY=00010203... axon-loop --store $WORK/keyed-store pointer show --tenant demo-tenant --family demo-coding
  ok: .transitions = 0
```

You can also key the whole flow. Running
`AXON_ATTEST_KEY=8899aabb…6677 bash examples/closed-loop/run.sh …` exited 0,
and all 18 ledger entries of the main store carried a `mac`. Opening that
store afterwards without the key:

```text
$ env -u AXON_ATTEST_KEY axon-loop --store /var/tmp/r51-tmp/cl2/store pointer resolve --tenant demo-tenant --family demo-coding
{"error":"io","exit_code":2,"message":"io: store corrupt: ledger is keyed but no key is configured: set AXON_ATTEST_KEY","schema":"axon.loop.error/1"}
```

## What the walkthrough shows the loop preventing

| attempt | refusal |
|---|---|
| using a scope before any policy is active | `pointer resolve` exit 6 |
| freezing a plan with unset operator fields | `plan freeze` exit 7, lists all 24 fields |
| changing a frozen plan | `plan register` exit 4 |
| an attestation signed by a key not registered for the verifier | `intake episode` exit 4 |
| admission by a party not in `trusted_admitters` | `admit` exit 4 |
| a pointer move based on a stale epoch | `pointer transition` exit 5 |
| counting one attempt's cost twice | `tel summarize` exit 4 |
| serving a revoked policy | `pointer resolve` exit 6 until a fenced rollback |
| opening a keyed store without, or with the wrong, key | exit 2 |

## In a real deployment: MiCode and Fabric

In the walkthrough, files stand in for two systems:

* **MiCode (the agent runtime).** It calls `pointer resolve`, pins the
  returned policy, and runs the task with that tool shortlist. It writes the
  episode sidecar, the context receipt and the policy ack under
  `<repo>/.micode/axon/closed-loop/`. `axon-loop intake episode` reads those
  files. Neither workspace has a Cargo dependency on the other.
* **Compute Fabric.** It runs the agent attempt and the registered acceptance
  check, and emits the ACF requests and receipts. Its verifier signer emits
  the `acf-receipt-attestation/2`. Here those are hand-written JSON files,
  plus an `openssl`-generated key.

The real-bytes version of steps 2-5 and 10 is the paired interop gate
`scripts/loop_interop_gate.sh`. It produces a policy with Axon DEC
(`axon-reflex`), runs the real `micode exec` against a local fixture LLM
provider, and intakes the files MiCode wrote. It needs a MiCode checkout of
branch `v022/micode`:

```text
MICODE_DIR=/path/to/micode-v022 bash scripts/loop_interop_gate.sh
```

Without `MICODE_DIR`, the gate looks for `../micode-v022-wt` and, if that is
absent, prints `SKIP` (a non-result). Under `AXON_HARNESS_STRICT=1` that is
exit 3. **It was not run for this walkthrough.** The local MiCode checkout at
`/home/cklaus/projects/aicoding/micode` has no `v022/micode` ref, local or
remote: it is on `opp-specs-build` at `b64a7261`. Its `crates/` contain no
writer for `axon.closed-loop.episode/1` or `micode.closed-loop.policy-ack/1`.
So that checkout cannot serve as the MiCode peer.

As `crates/axon-loop/README.md` says, the MiCode side on `v022/micode` is not
releasable until MiCode `tui` (credential-to-host isolation, `ed082601`) is
merged into it.

## Limits: what this does NOT cover

From `crates/axon-loop/README.md` ("What it does NOT do"), and from what this
walkthrough leaves out:

* **No authentication of names.** `trusted_admitters` and `trusted_observers`
  are operator premises read from `config.json`. An `issuer_ref` names a
  party and proves nothing. In the development class used here, only the
  verifier's attestation is signature-checked, and the key it is checked
  against also lives in `config.json`, which whoever can write the store can
  write.
* **The PROTECTED evaluation class is not exercised.** A scope listed in
  `protected_scopes` needs the verifier and observer keys under the
  root-owned `/etc/axon/trust/<authority>/`, a signed context, and genuine
  PSV evidence. This machine has no `/etc/axon`, and the walkthrough does not
  write there.
* **Policy admission is not effect admission.** An ACCEPT, and the pointer
  move it allows, grants no effect authority. That is what
  `axon-os::gate::admit` decides.
* **Plan approval is self-asserted.** `operator_approved` is a bool the
  submitter sets, and `approval_ref` is only checked for being non-null.
* **No cost metering of its own.** The loop consumes `Usage` as reported.
  There is no execution price schedule (D10), so an evaluation's per-arm
  economics are never a known total.
* **The ledger key is optional.** Without `AXON_ATTEST_KEY`, a forged append
  with a rewritten head is undetectable. With a key, restoring a genuine
  older keyed state (R1) and deleting the anchor (R3) remain open.
* **EVO is not a learner.** It makes one seeded, never-repeated, one-edit
  change to the shortlist. The episodes gate the proposal; they do not steer
  it.
* **Not exercised here:** a REJECT or INCONCLUSIVE admission; `safety
  report`; `tel summarize` with `price_schedule` and `fabric_attempts`;
  `intake` of a D12 not-run outcome; the two-process race (covered by
  `crates/axon-loop/tests/cli.rs`). The full verb list is in the header of
  `crates/axon-loop/src/bin/axon-loop.rs`.
* **No `axon` CLI verb reaches this crate.** The only interface is the
  `axon-loop` binary.
