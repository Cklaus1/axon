# R6 — record-only repair run ledger — DRAFT spec (designed against NH-R1; after Step 1 review)

**Status: DRAFT. Not authorized for build.**
- It depends on `tasks/unattended-auth-spec.md` (NH-R1) A0–A4.
- R6 writes records only. It grants nothing, gates nothing and changes no exit code.

**Source decisions:**
- O-S1-01: `[DECIDED by human 2026-10-18: deferred until the NH-R1 authorization is specified; R6 gets its own specification designed against it]`.
- The 2026-10-18 instruction: R6 is specified after NH-R1, with an identified consumer.

**Step 1 review:** `AdversarialSpecReviewer` R-F1…R-F6 and `FeasibilityReviewer` D17–D22, marked inline as
`[REVISED: …]`.

## 1. The S1 objections, answered

| S1 objection (`tasks/repair-spec.md:170-173`) | Answer |
|---|---|
| (a) self-signed: the harness's ephemeral ed25519 `Signer`, `crates/axon-cortex/benchmarks/promotion/promote.py:277-297` | Every record binds an operator-signed envelope (`envelope_digest`). The envelope is Ed25519 over a G5-derived seed, and the runner holds only its public key (NH-R1 §3, Q1), so a runner cannot mint the authority a record cites. The record seal attests only "a holder of the run-record subkey wrote this", and claims nothing beyond what that runner observed. If the human chooses HMAC in NH-R1 Q1, objection (a) stands again, and R6 stays deferred. `[REVISED: R-F1]` |
| (b) nothing read it | C1 is NH-R1's enabling prerequisite (§9): the human enabling decision reads C1's output. `[REVISED: R-F6, D17]` |
| (c) reusing `Evidence.trial` changes `ws_out` (`promote.py:399`, the scorecard ROW digest) | R6 never calls or alters `Evidence.trial`. Its digests have their own names (`output_file_sha256`) and their own task family. |

## 2. Consumers

| Id | Consumer | Reads | Status |
|---|---|---|---|
| **C1** | `axon-intent audit-runs --spent-store D --run-ledger D [--envelopes DIR]`: a read-only verb, separate from the mint verb `[REVISED: D22]` | per consumed envelope: a started and a terminal record, the outcome, the accepted digest, the write journal | built with R6. Consumed by the NH-R1 §9 enabling decision and by operator review of any unattended run. |
| C2 | a promotion adapter, `Evidence.trial_from_run` (a NEW function) | `output_file_sha256`, `episode_digest`, task family `cortex-repair-unattended` | **later**: only once an unattended-run learner or promotion candidate exists (scheduling, not a human question `[REVISED: D-C]`) |
| C3 | `benchmarks/labels/to_items.py` | `cortex-label/1` | exists today, unchanged |

## 3. Records

Schema `cortex-run-record/1`. Records are JSONL, two per run `[REVISED: R-F4 — a terminal-only record made every killed run look like truncation]`:

| `kind` | When it is written |
|---|---|
| `started` | in NH-R1 step 11, after consume and before any untrusted code runs. If the append fails, the run is refused with exit 29 (still before work begins), and the spent marker records the failure. |
| `terminal` | after the verdict, and also from the E3 watchdog path. If it fails, one stderr line is printed; the exit code and verdict are unchanged. |

| Field | Meaning |
|---|---|
| `schema`, `seq`, `prev`, `kind` | the chain: `prev` = cl22 digest of the previous record |
| `envelope_id`, `envelope_digest` | NH-R1 identity and authority. Attended runs write no record (a non-goal by structure: without an envelope a record would be self-signed `[REVISED: D-C, was Q3]`). |
| `task_family` | `"cortex-repair-unattended"` |
| `task` | `{workspace, file, symbol, pristine_sha256, imports_digest}` |
| `enforcement` | the NH-R1 §5.6 rows (in the started record) |
| `outcome`, `exit_code`, `stopped_by` | terminal only |
| `accepted` | `accepted_artifact`'s `{path, sha256}` (`bin/cortex.rs:1559-1575`): the **graded** digest. It is non-null only for exit 0 and 27, and null with the refusal reason when `settle_artifact` refused `[REVISED: R-F3, D18]`. |
| `writes` | the NH-R1 E5 helper's in-memory journal: `{site, path, before_sha256, after_sha256}` for all nine sites. Visible-copy create/remove pairs are tagged `site:"visible"`. Digests are hex sha256 without the `axc1:` prefix. `[REVISED: R-F2, D18]` |
| `pinned_tree_before`, `pinned_tree_after` | the digest of a manifest of every file under `write_prefixes` and `scopes.fs_write`. A check-child write appears as a difference that is not in `writes`. |
| `episode_digest`, `budget`, `generator`, `recorded_unix` | |
| `seal` | `"axrun1:" + HMAC(run-record subkey, canonical without seal)` |

## 4. Key and sink

**Subkey.** The domain is `"axon-approval repair-run-record v1"`. It is distinct from the axon-os record subkey
(`"axon-os record v1"`, `crates/axon-os/src/approval.rs:68, 77-80`) and from the envelope seed domain.
`[REVISED: D21]`
- The operator derives it and exports it into the runner's key file next to the verify key.
- The runner holds this HMAC subkey, which lets it seal records only: it cannot mint envelopes or `/3` tokens.
- It is registered as a `pub const` domain, with cross-protocol tests against the token, the axon-os record and the
  envelope.

**Sink** `[REVISED: R-F5, D19]`. A dedicated `--run-ledger DIR`.
- The directory is checked like the spent store: owner = euid, `mode & 0o077 == 0`, not a symlink, disjoint from the
  canonical workspace and prefixes.
- Its `ledger-id` is bound in the envelope as `run_ledger_id` (an NH-R1 schema field added when R6 is built).

**Rejected alternative:** axon-loop `intake episode`. Its events belong to an experiment plan (`IssuedTrial`,
`crates/axon-loop/src/plan.rs:465-475`); an operational run is not a trial. C2 may intake selected records through its
own adapter.

**Crash consistency** `[REVISED: D20]`. Under an exclusive `flock`:
1. append the record and `fsync` it;
2. replace `ledger.head` `{seq, record_digest, seal}` via temp + rename;
3. `fsync` the directory.

**Threat model.** Rolling back the ledger together with its head is not detected by the ledger alone. This is the same
OPEN item as axon-loop R1. C1 cross-checks the spent store, which catches truncation while the store survives. Both
directories are same-uid; NH-R1 §5.5's honest limit applies.

## 5. C1 audit

C1 verifies the chain and every seal, verifies `envelope_digest` against the supplied envelopes, and joins the records
against the spent store. Each finding has a distinct name:

| Finding | Condition |
|---|---|
| `consumed-no-record` | truncation |
| `started-no-terminal` | the run was killed or crashed |
| `record-not-consumed` | a record with no matching consumed envelope |
| `duplicate-run` | |
| `seal-break`, `chain-break` | |
| `unacknowledged-tail` | records past the head |
| `head-beyond-ledger` | |
| `accepted-mismatch` | `accepted.sha256` ≠ the last `after_sha256` of the target path (visible copies excluded) |
| `unattributed-change` | the pinned-tree diff is not explained by `writes` |

Exit codes: 0 clean, 1 findings, 2 usage. Exit 1 is added to axon-intent's USAGE (`crates/axon-intent/src/cli.rs:24-25`).

## 6. Tests (when built)

- **Crypto:** subkey KAT; seal tamper per field; chain splice, reorder and truncation; cross-protocol pairs.
- **Write failures:**
  - a failed started-append → exit 29, nothing run;
  - a failed terminal-append → verdict unchanged, one stderr line.
- **Coverage:**
  - the watchdog path writes a terminal record;
  - attended runs write nothing;
  - each C1 finding seeded once.
- **Concurrency:** 4 parallel unattended runs on distinct files → 8 records. Assert the set of `envelope_id`s, a valid
  chain, and head = last record; never assert order.
- **g01 rows:** seal compare, "no envelope ⇒ no write", started-before-work.

## 7. Build plan (after NH-R1 A0–A4)

| Unit | Content |
|---|---|
| L1 | record type, subkey, sink, chain/seal (axon-os, next to `repair_envelope`) |
| L2 | cortex writes the started and terminal records (needs the E5 journal) |
| L3 | the C1 `audit-runs` verb |
| L4 | C2, later |

L2 touches `axon-cortex/src/*`, so it shares NH-R1's receipt regress when landed in the same batch, else one more
≈ $0.14 run. Attended output stays byte-identical.

## 8. needs-human

| Q | Question | Default |
|---|---|---|
| Q1 | Confirm C1 (the accountability audit feeding the NH-R1 enabling decision) as R6's consumer | yes. If declined, R6 stays deferred and NH-R1 §9 drops its R6 clause. |

`[DECIDED by human 2026-10-19: Q1 yes — C1 is R6's consumer. NH-R1 §9 keeps its R6 clause, and NH-R1 Q1 was decided
Ed25519-from-G5, so objection (a) in §2 stays answered.]` R6 is still not authorized for build.
