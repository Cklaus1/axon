# Tech Spec — R29: Continuous Compliance Monitor

**Spec ID:** `R29-continuous-compliance-monitor`
**Status:** ✅ Landed (re-verified 2026-07-18) — `axon-os run --monitor` polls the R28 ledger every
100ms; any denied-effect entry trips the R27 kill-switch within 2s (exit 12).
`scripts/r29_acceptance_gate.sh`: 24 passed, 0 failed. This header said "Draft" long after the
code shipped — same staleness class as R17/R21/R22/R23/R31/R32, caught by the same outer-loop
sweep (`EXECUTION_MODEL.md` §2).

```spec-meta
id: R29-continuous-compliance-monitor
status-claim: Landed
depends-on: R26-confidential-microvm-substrate, R27-corrigibility-resource-bounds, R28-capability-audit-ledger
blocks: R30-unified-safety-gate, R31-extended-tcb-attestation
blocked-by: none
supersedes: none
related: R36-full-asi-os
conflicts-with: none
reserves: none (exit 12)
evidence: scripts/r29_acceptance_gate.sh (re-verified 2026-07-18)
```
**Implements:** The feedback loop that closes R26 + R27 + R28:
- R26 attests *what is running*
- R27 can *stop it*
- R28 *records what it does* (capability audit ledger JSONL)
- R29 **watches R28 in real time**, compares each ledger entry against the declared effect constraints, and **trips the R27 kill-switch if a violation is detected**

**Depends on:**
- `governance/specs/R26-confidential-microvm-substrate.md` — attestation & hardware isolation
- `governance/specs/R27-corrigibility-resource-bounds.md` — kill-switch mechanism (FileKillChannel, write_kill_state, HALTED_EXIT_CODE=4)
- `crates/axon-os/src/runtime.rs` — run_bounded loop that polls the kill file every 100 ms
- `crates/axon-os/src/killchan.rs` — file-backed kill channel abstraction
- `crates/axon-os/src/corrigible.rs` — exit codes 4/9/10
- The supervisor `run` → `run_bounded` pipeline

**Audience:** an implementer who builds *strictly* against this document and reads only it.

> **Read this framing first.** R26–R28 together create an *audit trail* of what a contained job
> actually did at runtime. R27 provides the *kill-switch* (file-backed latch, polled every 100 ms
> in `run_bounded`). R29 closes the loop: it is the **feedback signal** that watches the audit trail
> as it grows and trips the kill-switch the moment it sees an effect that was not in the declared
> policy. Without R29 the kill-switch is only operator-triggered; WITH R29 it is
> **automatically triggered by any runtime effect constraint violation**, turning the capability
> bound from a static declaration into a *continuously enforced invariant*.

---

## §0 — Acceptance-check index (the build gate verifies none are skipped)

| Check | What |
|---|---|
| `acc_a1_smoke_compliance_journey` | Start a run with a net-denied policy; agent attempts Net call; monitor trips kill within 2s |
| `acc_a2_allowed_effects_pass_through` | Run with full-effects policy; monitor does NOT trip kill for allowed effects |
| `acc_a3_quickstart_commands_execute` | Quickstart commands work |
| `acc_a4_hermetic_isolated_timeout` | Monitor exits cleanly when the job exits normally |
| `acc_a5_deterministic_detection` | Same policy + same violation → same detection latency (within 500ms tolerance) |
| `acc_a6_monitor_mandatory_fail_closed` | If monitor crashes, the job is killed (fail-closed, not fail-open) |
| `violation_detected_within_2s` | Violation entry in ledger → kill tripped within 2 seconds |
| `false_positive_rate_zero` | 100 allowed operations → 0 kills |
| `monitor_exit_matches_job_exit` | Normal job exit → monitor exit CleanExit; killed job → monitor reports ViolationDetected |
| `monitor_survives_ledger_rotation` | Ledger file rotation (rename + new file) → monitor follows without losing entries |

The build is **not done** until every row's check exists, was seen to fail first, and now passes.

---

## §1 — Motivation

R27 ships a kill-switch that an operator can trip (via `axon-os kill`) or that the interpreter
trips when it detects a `@[corrigible]` exit. But neither catches a *runtime effect that exceeds
the declared policy* — a Net call made by a job whose manifest says `net: []`. The static gate
(`admit`) blocks this before execution; the runtime sandbox (`sandbox_run`, exit 8) catches it at
the builtin level. R29 adds a **third, independent enforcement point** at the *audit ledger*:

```
Static gate (pre-run)  →  Runtime sandbox (builtin dispatch)  →  Compliance monitor (post-fact)
      [R21]                      [R6 / sandbox_run]                       [R29]
```

The monitor is *defence in depth*, not the primary control. Its unique property is that it is
**independent of the interpreter**: it watches a JSONL file that any producer (interpreter,
codegen, sidecar) can write, and it operates at the supervisor level (outside the contained
program). This means a compromised interpreter that bypasses sandbox_run cannot bypass the
monitor without also suppressing its own audit writes — detectable.

---

## §2 — Architecture

```
  ┌──────────────────────────┐          ┌─────────────────────────────────────┐
  │   Contained Axon job     │          │   ComplianceMonitor (thread)        │
  │                          │          │                                     │
  │   capability-bearing     │          │   loop {                            │
  │   action (net, fs, ...)  │ ──────►  │     read new JSONL lines            │
  │         │                │  audit   │     parse `effect` field            │
  │         ▼                │  entry   │     if !allowed → write kill file   │
  │   AXON_AUDIT_LEDGER.jsonl│          │     sleep 100ms                     │
  │   (append-only JSONL)    │          │   }                                 │
  └──────────────────────────┘          └─────────┬───────────────────────────┘
                                                  │ writes
                                                  ▼
                                         <run-id>.kill  (FileKillChannel)
                                                  │ polls every 100ms
                                                  ▼
                                         run_bounded → kills child → exit 4
                                                  │
                                         cmd_run detects violation flag
                                                  │
                                         return exit 12 (CONTAINMENT_VIOLATION)
```

**JSONL audit entry format** (one JSON object per line):
```json
{"seq": 1, "effect": "net", "operation": "http_get", "target": "api.evil.com", "ts_ms": 1234567890}
```

Required fields: `seq` (u64, monotone), `effect` (string key), `operation` (string).
Optional: `target`, `ts_ms`, `principal`, `label`.

**Allowed effects vocabulary:** `fs_read`, `fs_write`, `net`, `exec`, `ai`, `io` (alias for
fs_read+fs_write+exec). The monitor normalizes to lowercase before comparison.

---

## §3 — Monitor Protocol

### §3.1 Startup

1. Receive: `ledger_path` (JSONL file to watch), `kill_file` (R27 file-backed latch path),
   `allowed_effects: Vec<String>`, `stop: Arc<AtomicBool>`.
2. The monitor does NOT pre-create the ledger file — it waits for the job to create it.
3. Track `current_offset: u64 = 0` (byte position).

### §3.2 Poll loop (every 100 ms)

```
loop:
  if stop → return CleanExit
  try open ledger_path:
    not found → continue (job hasn't written yet)
    error → continue (transient)
  stat the file:
    size < current_offset → rotation detected; reset current_offset = 0
  seek to current_offset
  read available bytes
  advance current_offset by bytes_read
  for each complete newline-terminated line:
    parse as JSON
    extract effect: lowercase(entry["effect"])
    if effect not in allowed_effects:
      write kill_file: {"latch":"tripped","reason":"effect <X> not in policy","effect":"<X>","seq":<N>}
      return ViolationDetected { effect, operation, seq }
  sleep 100ms
```

### §3.3 Fail-closed guarantee

If the monitor thread panics or the `JoinHandle` returns an error, `cmd_run` MUST:
1. Write the kill file (trip the latch)
2. Increment a violation counter
3. Return exit code 12

A monitor crash is treated as a detected violation (I-6: fail-closed). The rationale: a compromised
monitor that crashes has failed to provide its guarantee → we must conservatively assume violation.

### §3.4 Ledger rotation

If `current_offset > file_size`: the file was truncated or replaced (rotation). Reset offset to 0.
The monitor re-reads from the beginning of the new file. A malicious rotation attack that truncates
the ledger is itself auditable: the monitor sees entries from 0 again and quickly re-checks them.
A rotation that drops entries undetectably would require the adversary to also control the kill
file, which is supervisor-owned (not accessible from the contained job).

---

## §4 — Threat Model

**In scope (what R29 defends against):**
- A job that attempts an effect outside its declared policy (net call without net grant)
- A slow accumulation of violations that the static gate missed (over-approximate scan)
- A compromised interpreter that bypasses `sandbox_run` but still writes audit events

**Out of scope (R29 does NOT defend against):**
- A job that suppresses its own audit writes (then R25/R26 catches the anomaly)
- A monitor that is itself compromised (then R26 attestation catches the TCB tamper)
- Sub-100ms violations that complete before the first poll (acceptable: the monitor is defence in
  depth, not the primary control; the runtime sandbox already caught it at exit 8)

**Fail-closed invariants (I-*):**
- **I-1:** Monitor crash → job killed (§3.3)
- **I-2:** Kill file write failure → monitor panics (propagates to I-1)
- **I-3:** Unrecognized effect field → treated as violation (deny-by-default)
- **I-4:** Malformed JSON line → logged, skipped (not treated as allowed)
- **I-5:** Ledger absent at start → wait (not treated as allowed)
- **I-6:** Any monitor error → fail-closed, kill job

---

## §5 — Gate Script

`scripts/r29_acceptance_gate.sh` runs:

1. **Presence check** — every named check in §0 exists in `crates/axon-os/tests/r29_compliance.rs`
2. **Anti-stub check** — no `#[ignore]` / `todo!()` / `unimplemented!()` / `assert!(true)` stubs
3. **Cargo test** — `cargo test -p axon-os r29` must be green
4. **Exit-code check** — `CONTAINMENT_VIOLATION_EXIT_CODE = 12` is defined in `corrigible.rs`
5. **Spec presence** — this file exists at `governance/specs/R29-continuous-compliance-monitor.md`

---

## §6 — CLI surface

```
axon-os run <job.axjob> --monitor <allowed-effects> --ledger <path> [--run-id ID] [--out DIR]
```

Where `<allowed-effects>` is a comma-separated list of effect names: `fs_read,fs_write,net,exec,ai`.

`--killable` is implied when `--monitor` is used (the monitor uses the kill file mechanism).

Exit codes when `--monitor` is active:
- `0` — job completed cleanly (no violations)
- `4` — job killed by operator (kill command)
- `12` — job killed by compliance monitor (R29: `CONTAINMENT_VIOLATION_EXIT_CODE`)

---

## §7 — Data flow example

```
1. axon-os run job.axjob --monitor "fs_read,fs_write" --ledger /tmp/audit.jsonl
2. Job starts; monitor thread starts
3. Job appends: {"seq":1,"effect":"fs_read","operation":"read_file","target":"./data.txt"}
   → monitor: effect "fs_read" in allowed → OK
4. Job appends: {"seq":2,"effect":"net","operation":"http_get","target":"api.evil.com"}
   → monitor: effect "net" not in allowed → write kill file
5. run_bounded polls kill file (within 100 ms) → kills job → exit 4
6. cmd_run: violation_flag set → exit 12 (CONTAINMENT_VIOLATION_EXIT_CODE)
```

---

## §8 — Addendum to TCB

R29 adds one module to the TCB: `crates/axon-os/src/monitor.rs`.
The `R29_TCB_ADDENDUM` constant (in `corrigible.rs`) captures the enforcement invariants so a
tampered monitor binary is detectable via the `axtcb1:` measurement chain.

```
R29-monitor:poll-100ms-fail-closed-deny-by-default
```
