# R6 — record-only repair run ledger — DRAFT spec (designed against NH-R1)

**Status: DRAFT. Not authorized for build.** It depends on `tasks/unattended-auth-spec.md` (NH-R1) being built. R6
writes records only: it grants nothing, gates nothing and changes no exit code.

**Source decisions:**
- `tasks/opportunities.md` O-S1-01: `[DECIDED by human 2026-10-18: deferred until the NH-R1 authorization is specified; R6 gets its own specification designed against it]`.
- The 2026-10-18 instruction: R6 is specified after NH-R1, with an identified consumer of its data.

## 1. Why S1 deferred it, and what changes

`tasks/repair-spec.md:170-173` gave three reasons:

| S1 objection | Answer in this spec |
|---|---|
| (a) the ledger would be self-signed (the harness's ephemeral ed25519 `Signer`, `crates/axon-cortex/benchmarks/promotion/promote.py:277-297`) | Every record binds the operator-minted envelope's digest (`axon-approval/4`, NH-R1 §4). The authority a record claims is independently signed. The record itself is sealed by the runner under its own subkey (§4), and claims nothing beyond what that runner observed. |
| (b) nothing read it | §2 names the consumers. Building R6 is conditional on a human confirming at least C1 (§8 Q1). |
| (c) reusing `Evidence.trial` changes `ws_out` (`promote.py:399`: there it is the scorecard ROW digest) | R6 never calls or alters `Evidence.trial`. Its digests have their own names and their own task family (§3), so the two meanings cannot mix. |

## 2. Consumers (identified)

| Id | Consumer | Reads | Exists today? |
|---|---|---|---|
| **C1 (primary)** | Unattended-run accountability check: `axon-intent authorize-repair --audit <spent-store> <ledger>` | for every consumed envelope (NH-R1 spent store), exactly one terminal record with the outcome, the accepted file's bytes digest, and the files written | NEW, built with R6. Required by NH-R1 §9 / Q3: the enabling decision needs evidence of what unattended runs did, and an operator reviewing a run needs it afterwards. |
| C2 (secondary) | Promotion pipeline, via a NEW function `Evidence.trial_from_run` in `promote.py` | real accepted-bytes digests (`output_file_sha256`) and `episode_digest` in place of the row digest, under task family `cortex-repair-unattended` | `promote.py` exists and its `Evidence.trial` stays unchanged. C2 is built only when a learner or promotion candidate for unattended runs exists (§8 Q2). |
| C3 (existing, unchanged) | `benchmarks/labels/to_items.py` | `cortex-label/1` | yes. R6 does not replace `--label-log`. |

## 3. Record

**Schema** `cortex-run-record/1`. JSONL, one record per run, written after the verdict.

Like `--label-log` (`crates/axon-cortex/src/label.rs:1-25`):
- a write failure is one stderr line and never changes the exit code or the verdict;
- unlike it, the record is chained and sealed.

| Field | Meaning |
|---|---|
| `schema`, `seq`, `prev` | chain: `prev` = `cl22:` digest of the previous record's canonical bytes; `seq` is monotone |
| `envelope_id`, `envelope_digest` | NH-R1 run identity and authority. **Absent ⇒ the record is not written**: attended runs write no R6 record in v1 (§8 Q3). |
| `task_family` | `"cortex-repair-unattended"` |
| `task` | `{workspace, file, symbol, pristine_sha256}` (copied from the verified envelope) |
| `outcome`, `exit_code`, `stopped_by` | from the `cortex-repair/1` JSON |
| `accepted` | `{output_file_sha256}` = sha256 of the bytes on disk after the run (re-read), or null |
| `writes` | list of `{path, before_sha256, after_sha256}` for every write cortex performed (patch apply, restore), from the episode and FU-4's we-wrote set |
| `episode_digest` | the `axc1:` digest (`crates/axon-cortex/src/episode.rs:145-155`) |
| `budget` | `{max_proposals, proposals_used, deadline_ms, elapsed_ms}` |
| `enforcement` | NH-R1 §5.5 rows |
| `generator`, `ranker` | identities |
| `recorded_unix` | |
| `seal` | `"axrun1:" + HMAC(record subkey, canonical without seal)` |

**Naming.** The field is called `output_file_sha256`, never `ws_out` or `output_workspace_ref`. C2 maps it explicitly
and documents the mapping.

## 4. Key and sink

- **Subkey.** A fourth `ApprovalKey` subkey, domain `"axon-approval run-record v1"` (`crates/axon-os/src/approval.rs:59-70`).
  It gets a KAT and an entry in `crates/axon-fabric/tests/signing_domains.rs`. It is distinct from the envelope subkey,
  so a record can never be presented as an envelope. The same cross-protocol tests apply as NH-R1 S7.
- **Sink.** `<spent-store>/../run-ledger.jsonl` (the same protected directory rules as the spent store, NH-R1 §5.4).
  - Append under an exclusive `flock`; a `ledger.head` file holds `{seq, record_digest, seal}`.
  - **Rejected alternative:** axon-loop `intake episode` (`crates/axon-loop/src/ledger.rs`). Its events are trial or
    evaluation events of an experiment plan (`IssuedTrial`, `plan.rs:465-475`). An unattended operational run is not a
    trial of any plan, and putting it there would give the axon-loop ledger a second meaning. C2 may later intake
    selected records as episodes through its own adapter.
- **Threat model.** Rollback (restoring an older ledger plus head) is not detected, the same OPEN item as axon-loop R1
  (`ledger.rs:1-91`). C1 cross-checks the spent store: a `.used` marker without a record is a FINDING. That catches a
  truncated ledger as long as the spent store survives.

## 5. Verify (C1)

`authorize-repair --audit` does the following:
1. verifies the chain and every seal;
2. verifies every `envelope_digest` against the envelope files it is given (optional);
3. joins against the spent store.

Each of these is a FINDING:
- a consumed envelope with no record;
- a record with no consumed envelope;
- two records for one envelope;
- a seal or chain break;
- an `accepted.output_file_sha256` that differs from the last `writes[].after_sha256`.

Exit codes: 0 clean, 1 findings, 2 usage.

## 6. Tests (when built)

- KAT for the subkey; a seal tamper per field; chain splice, reorder and truncation; cross-protocol (record as envelope
  and the reverse).
- A failed append (read-only dir) → verdict and exit code unchanged, plus one stderr line.
- An attended run writes no record.
- C1 findings, each seeded once.
- Concurrency: 4 parallel unattended runs (distinct envelopes) → 4 records, chain valid.
- g01 rows on the seal compare and the "absent envelope ⇒ no write" rule.

## 7. Build plan (when authorized; after NH-R1 A1–A4)

| Unit | Content |
|---|---|
| L1 | Record type, subkey, sink, chain/seal in axon-os (next to `repair_envelope`) |
| L2 | cortex writes after the verdict (unattended only) |
| L3 | C1 audit in axon-intent |
| L4 | C2 adapter, only when §8 Q2 says so |

L2 touches `axon-cortex/src/*`, so it needs a receipt regress. Attended output must be byte-identical.

## 8. needs-human

| Q | Question | Default |
|---|---|---|
| Q1 | Is C1 (accountability audit of unattended runs) the consumer that justifies building R6? | yes, required; without a confirmed consumer R6 stays deferred |
| Q2 | Build C2 (the promotion adapter) now, or only when an unattended-run learner or promotion candidate exists? | later |
| Q3 | Should attended runs also write records? This would need a signer when there is no envelope, which is the S1 self-signing problem again. | no |
