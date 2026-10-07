# Autonomous build — programme state (recovery hint)

**This file is a recovery hint, not evidence.** Git history, the v0.22 programme
manifests (`docs/axon_cortex_v0_22/axon-cortex-build-v0_22/{task,gate}_manifest.json`),
the stage records under `.axon-v022/`, `AXON-COMPLETENESS.json`, managed-run
receipts (`.axon-runs/*/receipt`) and `scripts/release_check.sh` are authoritative.
Where this file disagrees with them, they win and this file is stale.

## Governing objective

**Finish the entire Axon v0.22 programme** — every required work package
(B255–B286 plus the prior tasks v0.22 retains), every declared stage, certified at
an exact final revision.

| | |
|---|---|
| Authoritative repository | `/home/cklaus/projects/aicoding/axon` (origin `github.com/Cklaus1/axon`) |
| Programme baseline | Axon Cortex v0.22 (`docs/axon_cortex_v0_22/`; plan `.axon-v022/UPGRADE_V0_22.md`) |
| Programme line | `v022/stage1` (accepted, `d14cd24`) → `v022/stage2` (`262c44a`, merged, NOT certified) → `v022/stage2-port` (current) |
| MiCode counterpart | `v022/micode` (`/home/cklaus/projects/aicoding/micode-v022-wt`) |
| Superseded | `upgrade/cortex-v0_20` in `/home/cklaus/projects/axon` — a **donor** line only. v0.22 contains the whole v0.20 package; v0.20 is not a merge parent |
| `AUTO_CONTINUE` | **true** |

## Valid terminal states — stop ONLY on one of these

- `COMPLETE` — the whole reconciled v0.22 programme certified at an exact revision
- `BLOCKED_USER_INPUT` — a decision only the operator can make, and nothing else is READY
- `BLOCKED_EXTERNAL` — an external dependency is unavailable and nothing else is READY
- `FAILED_TERMINAL` — an unrecoverable failure

Finishing a stage, a gate, a tag, a package, or an incident is **not** a terminal
state. Incidents and side investigations are child nodes of FINISH AXON v0.22; they
may interrupt the DAG only for host safety, evidence integrity, false greens /
false terminals, or required release correctness. When one closes, recompute the
DAG and continue.

## Current step (2026-09-25)

* **Stage 2 ACCEPTED** at `048367ea` (tag `v022/stage2-accepted`; `.axon-v022/stage2/STAGE2_RESULT.json`):
  strict gate 157/157 suites, 3076 tests, under run_managed `--mem-max 16G --swap-max 2G --deadline 18000`;
  release_check fails only on FG-041/FG-042, which are Stage 3 by design (Stage 1's criterion).
* **D3 satisfied** on MiCode `v022/micode-tui-merge` `e4943f88` (tui@65286581, incl. ed082601).
* **Stage 3 ACTIVE** on `v022/stage3` (base `048367ea`), lanes L1 (Fabric qualification, FG-042),
  L2 (HardwareIsolated), L3 (axon-vm admit + cleanup, D-019/ACF-G22), L5a (guest policy channel, D5);
  then L4 (kernel allow-path relabel, FG-041, KVM), L5b (engine pins + launcher policy, root/KVM),
  L5c (Fabric wiring), S3-6 (signed re-qualification — BLOCKED_USER_INPUT on the operator's
  evidence signature and x3 waiver; see operator_decisions D6–D8 defaults).
* Stage 7 needs operator pilot parameters (BLOCKED_USER_INPUT); everything before it proceeds.

## Standing operator decisions (from `.axon-v022/coordination/operator_decisions.json`)

D1 MiCode v0.22 on v014-reconciled · D2 WSL2 nested KVM accepted for B263 (caveat on
every record) · **D3 MiCode v0.22 not releasable until `tui` (credential isolation
`ed082601`) is merged — a declared release dependency, to be satisfied, not waived** ·
D4 reconcile on the candidates · D5 B263 gets an in-guest policy channel.
Stage 7 (pilot) requires operator-approved pilot parameters.
