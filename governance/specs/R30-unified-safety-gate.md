# R30: Unified ASI Deployment Safety Gate

**Status:** Landed  
**Script:** `scripts/axon_safety_gate.sh`  
**Acceptance gate:** `scripts/r30_acceptance_gate.sh`  
**Schema:** `axon-safety-gate/1`

---

## §0 Acceptance Checks

| ID | Check | Verified by |
|----|-------|-------------|
| acc_a1 | `acc_a1_gate_passes_on_clean_repo` — with `AXON_CI_NO_KVM=1 AXON_AI_MOCK=1 SKIP_BUILD=1` all stages pass or skip | r30_acceptance_gate.sh |
| acc_a2 | `acc_a2_gate_fails_on_broken_r26` — if Stage 3 (R26 attestation) would fail, the gate exits non-zero | design: `set -euo pipefail` + per-stage early-exit |
| acc_a3 | `acc_a3_json_output_valid` — `JSON_OUT=path` produces a file with `schema == axon-safety-gate/1` and a `stages` array | r30_acceptance_gate.sh |
| acc_a4 | `acc_a4_gate_is_idempotent` — two successive runs on the same source produce identical pass/fail/skip vectors | property of pure stage-scripts + deterministic env |
| acc_a5 | `acc_a5_skip_unavailable_stages` — a stage whose script is absent is recorded as `skipped:true` and does not fail the gate | r30_acceptance_gate.sh |
| acc_a6 | `acc_a6_exit_code_matches_failure_stage` — exit 0 on all-pass, exit 1 on the first failing stage | r30_acceptance_gate.sh |

---

## §1 Motivation

R26 through R29 each shipped their own `scripts/rNN_acceptance_gate.sh`.  Running all four independently leaves four integration holes:

1. An operator might run only one script and miss a regression in another layer.
2. There is no machine-readable aggregate "is this revision safe to deploy?" signal.
3. The flagship end-to-end demo (`examples/flagship/demo.sh`) was never wired into the gate chain.
4. CI pipelines have no single entry-point to block on.

R30 closes all four gaps with one script: `scripts/axon_safety_gate.sh`.

The gate is **ordered by dependency**:

- Build must pass before tests can run.
- Crate-level unit tests (R26/R27 source) must pass before the CLI-level acceptance scripts are meaningful.
- R26 attestation must pass before R27 corrigibility (corrigibility depends on a measured kernel).
- R28/R29 are opt-in (not yet shipped); their stages skip gracefully when absent.
- The flagship demo is the final integration proof that all four layers compose correctly.

---

## §2 Gate Pipeline

```
Stage 1: BUILD          — cargo build (axon-core + safety crates that exist)
Stage 2: UNIT_TESTS     — cargo test -p axon-core (interpreter + checker suite)
Stage 3: R26_ATTESTATION— scripts/r26_acceptance_gate.sh
Stage 4: R27_CORRIGIBILITY — scripts/r27_acceptance_gate.sh
Stage 5: R28_AUDIT_LEDGER  — scripts/r28_acceptance_gate.sh [SKIP if absent]
Stage 6: R29_COMPLIANCE    — scripts/r29_acceptance_gate.sh [SKIP if absent]
Stage 7: FLAGSHIP_DEMO  — examples/flagship/demo.sh [SKIP if axon-vm absent]
Stage 8: REPORT         — emit machine-readable JSON summary
```

### Stage semantics

| Outcome | Meaning | JSON field |
|---------|---------|-----------|
| PASS | stage ran, exit 0 | `"ok":true` |
| SKIP | script absent or explicitly skipped | `"ok":true,"skipped":true,"reason":"..."` |
| FAIL | stage ran, non-zero exit | `"ok":false` → gate halts, exits 1 |

A stage failure **stops the pipeline** at that stage.  Stages after the failure are not recorded in the JSON output (the array is truncated at the failing stage).

### JSON report schema (`axon-safety-gate/1`)

```json
{
  "schema": "axon-safety-gate/1",
  "timestamp": "2026-01-01T00:00:00Z",
  "ok": true,
  "stages": [
    {"stage": 1, "name": "BUILD",             "ok": true},
    {"stage": 2, "name": "UNIT_TESTS",        "ok": true},
    {"stage": 3, "name": "R26_ATTESTATION",   "ok": true},
    {"stage": 4, "name": "R27_CORRIGIBILITY", "ok": true},
    {"stage": 5, "name": "R28_AUDIT_LEDGER",  "ok": true, "skipped": true, "reason": "r28 not yet shipped"},
    {"stage": 6, "name": "R29_COMPLIANCE",    "ok": true, "skipped": true, "reason": "r29 not yet shipped"},
    {"stage": 7, "name": "FLAGSHIP_DEMO",     "ok": true},
    {"stage": 8, "name": "REPORT",            "ok": true}
  ]
}
```

Fields `skipped` and `reason` are only present when the stage was skipped.

---

## §3 Integration

### Local usage

```bash
# Full gate — all stages (takes ~2–3 min with a warm build cache):
./scripts/axon_safety_gate.sh

# CI-fast: skip the compile step (pre-built artifacts already on PATH):
SKIP_BUILD=1 AXON_CI_NO_KVM=1 AXON_AI_MOCK=1 ./scripts/axon_safety_gate.sh

# With machine-readable output:
JSON_OUT=/tmp/gate.json ./scripts/axon_safety_gate.sh

# Self-test (validates the gate script's own JSON output):
./scripts/axon_safety_gate.sh --self-test

# R30 acceptance gate only:
./scripts/r30_acceptance_gate.sh
```

### Wiring into gate.sh

The existing `scripts/gate.sh` runs the compiler test suite.  R30 extends the
outer gate; wire it in by calling `axon_safety_gate.sh` after `gate.sh`:

```bash
scripts/gate.sh && SKIP_BUILD=1 scripts/axon_safety_gate.sh
```

Or add to CI workflow:

```yaml
- name: Axon Safety Gate
  env:
    AXON_CI_NO_KVM: "1"
    AXON_AI_MOCK: "1"
  run: SKIP_BUILD=1 bash scripts/axon_safety_gate.sh
```

### Environment variables

| Variable | Effect |
|----------|--------|
| `SKIP_BUILD` | Non-empty → skip Stage 1 (build).  Use when artifacts are already built. |
| `AXON_CI_NO_KVM` | Forwarded to r26 and flagship demo; uses software-TPM stand-in instead of KVM. |
| `AXON_AI_MOCK` | Forwarded to flagship demo; uses deterministic stub AI responses. |
| `JSON_OUT` | Path to write the machine-readable JSON report.  Empty → no file written. |
| `AXON_SEED` | RNG seed forwarded to all sub-scripts. Default: 42. |

---

## §4 Threat Model

The gate guards against **regression-then-deploy**: a change that breaks a lower
safety layer is merged and deployed before the breakage is noticed.

### In-scope threats

| Threat | Gate stage that catches it |
|--------|--------------------------|
| `axon-attest` build breaks (e.g. API change in a dependency) | Stage 1 (BUILD) |
| R26 acceptance test regresses (tamper detection weakened) | Stage 3 (R26_ATTESTATION) |
| R27 corrigibility test regresses (kill-latch bypassable) | Stage 4 (R27_CORRIGIBILITY) |
| Flagship demo breaks end-to-end (@[contained] check regresses) | Stage 7 (FLAGSHIP_DEMO) |
| Gate script itself is broken (self-test fails) | `--self-test` mode |
| CI operator deploys without running the gate | JSON schema field `ok:false` blocks deploy scripts |

### Out-of-scope

- Hardware TEE attestation (R26 software-TPM stand-in is honest about this; full SEV-SNP/TDX is `hw-attest` feature-gated).
- R28 / R29 (not yet shipped; stages skip gracefully until those scripts land).
- The gate does not substitute for human review of the underlying safety properties it gates.

### Trust assumption

The gate script itself is part of the TCB: a compromised `axon_safety_gate.sh`
could skip stages silently.  Mitigations:

1. The script is checked into the repo and reviewed like code.
2. The `set -euo pipefail` top-level ensures unhandled errors propagate.
3. `skip_stage` records `"ok":true,"skipped":true` — an auditor can see which stages were skipped in the JSON report.
4. `--self-test` mode validates the JSON output structure.

---

## §5 Quickstart

```bash
# 1. Build (first time only):
cargo build -p axon-core --no-default-features --bin axon
cargo build -p axon-attest -p axon-os -p axon-vm

# 2. Run the unified gate in CI-safe mode:
AXON_CI_NO_KVM=1 AXON_AI_MOCK=1 SKIP_BUILD=1 ./scripts/axon_safety_gate.sh

# Expected output (clean repo, R28/R29 not yet shipped):
#   Stage 1: BUILD        SKIP (--skip-build)
#   Stage 2: UNIT_TESTS   PASS
#   Stage 3: R26          PASS
#   Stage 4: R27          PASS
#   Stage 5: R28          SKIP (r28 not yet shipped)
#   Stage 6: R29          SKIP (r29 not yet shipped)
#   Stage 7: FLAGSHIP     PASS
#   Stage 8: REPORT       <JSON>
#   ALL STAGES PASSED — safe to deploy

# 3. Write a JSON report:
JSON_OUT=/tmp/gate_report.json AXON_CI_NO_KVM=1 AXON_AI_MOCK=1 SKIP_BUILD=1 \
    ./scripts/axon_safety_gate.sh

# 4. Run the R30 acceptance gate (meta-gate that tests the gate itself):
./scripts/r30_acceptance_gate.sh
```
