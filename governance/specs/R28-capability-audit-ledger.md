# Tech Spec — R28: Capability Audit Ledger (chained, tamper-evident, all-class)

**Spec ID:** `R28-capability-audit-ledger`
**Status:** ✅ Landed (re-verified 2026-07-18) — chained JSONL ledger for every capability-bearing
action (FS/Net/AI/Exec/Random/IO); `axon-os audit verify --ledger PATH` checks chain integrity.
Includes the 2026-07-18 fix (`0bfa74d`): `Ledger::open()` now eagerly creates the ledger file
(previously a zero-AI-call run produced no file at all), and `call_builtin` audits every
capability-bearing builtin, not just `ai_complete`. `scripts/r28_acceptance_gate.sh`: PASS. This
header said "Draft" long after the code shipped — same staleness class as
R17/R21/R22/R23/R31/R32, caught by the same outer-loop sweep (`EXECUTION_MODEL.md` §2).

```spec-meta
id: R28-capability-audit-ledger
status-claim: Landed
depends-on: R21-axon-os-supervisor, R27-corrigibility-resource-bounds
blocks: R29-continuous-compliance-monitor
blocked-by: none
supersedes: none
related: R31-extended-tcb-attestation, R36-full-asi-os
conflicts-with: none
reserves: none
evidence: scripts/r28_acceptance_gate.sh (re-verified 2026-07-18)
```
**Implements:** `VISION_OS.md` §4.4 ("auditability — what happened, provably, in order") and the
capability-audit pillar of the ASI safety stack. Provides a **single, chained, cross-class ledger**
that records every capability-bearing action: FS, Net, AI, Exec, Random, IO — in order, with a
hash chain that makes tampering or reordering detectable.
**Depends on:** `governance/specs/R21-axon-os-supervisor.md`, `governance/specs/R27-corrigibility-resource-bounds.md`.

---

## §0 — Acceptance-check index

| Req | What | Pinned acceptance check |
|---|---|---|
| **A1** | Real user journey: run with AXON_AUDIT_LEDGER, then audit verify, then audit show | `acc_a1_smoke_audit_journey` |
| **A2** | All six capability classes recorded in tests | `acc_a2_all_capability_classes_recorded` |
| **A3** | Quickstart commands execute verbatim | `acc_a3_quickstart_commands_execute` |
| **A4** | Hermetic isolated timeout: ledger is the complete record | `acc_a4_hermetic_isolated_timeout` |
| **A5** | Deterministic: same entries → identical hashes | `acc_a5_deterministic_byte_identical` |
| **A6** | Mandatory and chained: every entry holds prev_hash | `acc_a6_ledger_mandatory_and_chained` |
| **Core** | Tampered entry hash detected by verify | `ledger_tamper_fails_verification` |
| **Core** | Missing entry breaks the chain | `missing_entry_fails_verification` |
| **Core** | AI entry carries SHA-256 of prompt | `ai_call_entry_carries_prompt_hash` |
| **Core** | Export → re-import → re-verify succeeds | `ledger_export_and_reimport_verified` |
| **Gate** | Gate fails if any check is missing/stubbed | `scripts/r28_acceptance_gate.sh` |

---

## §1 — Motivation

R21 captures the final verdict; R27 captures resource debits. Neither captures the fine-grained,
in-order, tamper-evident sequence of capability-bearing events: which class, which principal, which
operation, at which sequence number. R28 provides that backbone.

---

## §2 — Ledger Format (JSONL, one entry per line)

```json
{"seq":0,"ts_ms":0,"principal":"root","effect":"AI","operation":"ai_complete:sha256:abc123...","prev_hash":"0000...0000","entry_hash":"deadbeef..."}
```

| Field | Type | Description |
|---|---|---|
| `seq` | u64 | Monotonically increasing from 0. Gap = missing entry. |
| `ts_ms` | u64 | Wall-clock ms since Unix epoch (0 in deterministic mode). |
| `principal` | string | Name of the executing principal (default "root"). |
| `effect` | string | One of "FS", "Net", "AI", "Exec", "Random", "IO". |
| `operation` | string | Human-readable description. AI: "ai_complete:sha256:<hex>". |
| `prev_hash` | string | 64-hex SHA-256 of previous entry (64 zeros for first). |
| `entry_hash` | string | 64-hex SHA-256 of this entry's content (see §4.1). |

Schema version: `axon-ledger/1`.

---

## §3 — Capability Classes

| Variant | Discriminant |
|---|---|
| FS | 0 |
| Net | 1 |
| AI | 2 |
| Exec | 3 |
| Random | 4 |
| IO | 5 |

---

## §4 — Core Logic

### 4.1 Chain computation

```
entry_hash(E) = SHA-256(
    E.seq.to_le_bytes()       [8 bytes]
 ++ E.ts_ms.to_le_bytes()    [8 bytes]
 ++ E.principal.as_bytes()
 ++ [effect_discriminant]    [1 byte]
 ++ E.operation.as_bytes()
 ++ E.prev_hash              [32 raw bytes]
)
```

### 4.2 Verification

Read entries in order; check: seq is 0,1,2,...; entry_hash matches recomputed; prev_hash matches
predecessor's entry_hash. Any failure → Err("tamper detected at seq N: reason").

### 4.3 Hermetic isolation

Global `OnceLock<Mutex<Option<Ledger>>>`. `set_ledger_path` initializes once. `append_global` is
called at every capability site. `flush_ledger` is called at process exit.

### 4.4 Determinism

`AXON_AUDIT_DETERMINISTIC=1` (or `#[cfg(test)]`): ts_ms = AtomicU64 counter (0, 1, 2, ...).
Production: SystemTime::now() milliseconds since epoch.

### 4.5 AI-call entries

`append_ai_call(principal, prompt_bytes)`: sha256(prompt_bytes) → hex → operation =
"ai_complete:sha256:<hex>" → append with EffectKind::AI.

### 4.6 Export / import

`export_json` → JSON object `{schema:"axon-ledger/1", entries:[...]}`.
`import_json(json, path)` → deserialize + verify chain → Ok(Ledger) or Err.

---

## §5 — Integration Points

### 5.1 axon-core interpreter

After each ai_complete dispatch, when AXON_AUDIT_LEDGER is set:
```rust
if std::env::var_os("AXON_AUDIT_LEDGER").is_some() {
    let _ = axon_audit::append_ai_call(&principal, prompt.as_bytes());
}
```

### 5.2 axon-core main.rs

Before run: set_ledger_path if AXON_AUDIT_LEDGER is set.
After run: flush_ledger().

### 5.3 axon-os CLI

New subcommands:
- `axon-os audit verify --ledger PATH`
- `axon-os audit show --ledger PATH [--json]`

---

## §6 — Threat Model

Closes: reordering (prev_hash breaks), deletion (seq gap + prev_hash break), field mutation
(entry_hash breaks successors), prompt substitution (auditor can check hash against known prompt).

Does NOT close: a patched interpreter that skips append_global; ledger-file confidentiality (JSONL
is plaintext); clock manipulation (ts_ms is informational only; ordering = seq + hash chain).

---

## §7 — Test Plan

Unit (lib.rs): acc_a5_deterministic_byte_identical, ledger_tamper_fails_verification,
missing_entry_fails_verification, ai_call_entry_carries_prompt_hash,
ledger_export_and_reimport_verified.

Integration (tests/r28_gate.rs): acc_a1_smoke_audit_journey,
acc_a2_all_capability_classes_recorded, acc_a3_quickstart_commands_execute,
acc_a4_hermetic_isolated_timeout, acc_a6_ledger_mandatory_and_chained.

---

## §8 — Invariants

- I-1: Every entry_hash commits to all fields including prev_hash.
- I-2: seq is strictly increasing.
- I-3: Prompt is never stored; only its SHA-256.
- I-4: Failed append logs to stderr but does not abort the run (fail-open for safety).
- I-5: Deterministic mode uses counter not SystemTime.

---

## §9 — Quickstart

```bash
cargo build -p axon-audit
AXON_AUDIT_LEDGER=/tmp/demo.jsonl AXON_AI_MOCK=1 cargo run -p axon-core -- run examples/hello.ax || true
cargo run -p axon-os -- audit verify --ledger /tmp/demo.jsonl
cargo run -p axon-os -- audit show --ledger /tmp/demo.jsonl --json
```

---

## §10 — Acceptance Gate

scripts/r28_acceptance_gate.sh: presence check all 10 checks; anti-stub check; cargo test -p
axon-audit green; CLI smoke (run + verify + tamper + export/import); exit 0 iff all pass.

---

## §11 — Definition of Done

cargo build -p axon-audit succeeds; all 10 acceptance checks pass; AXON_AUDIT_LEDGER=x axon run
produces a verifiable ledger; axon-os audit verify exits 0 on clean ledger, non-zero on tampered;
scripts/r28_acceptance_gate.sh exits 0.
