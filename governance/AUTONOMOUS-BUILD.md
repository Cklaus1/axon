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

## Current recovery step (2026-09-25)

1. Port the proven host-safety fixes from the donor line onto `v022/stage2-port`,
   semantically, one commit each, with provenance
   (`UPGRADE_V0_22_DONOR_PORT.md` maps every donor ID to its destination ID):
   Asyncify `-O2`, borrowed-source ABI + `axon_free`, bounded runner (cgroup memory +
   swap + deadline + descendant containment), exit-status laundering fixes, the
   browser artifact stamp guard, build/wasm-opt failure ≠ SKIP, and the
   `source_tag` / reflex fixes where still applicable.
2. Re-certify Stage 2 under hard containment: the previous Stage-2 strict run
   (`axon-s2/.axon-runs/stage2-strict-20260924T200938Z-2849197`) is **lost** — it died
   with WSL during the 2026-09-24 Asyncify incident while running the (then
   unoptimized) asyncify test. Never re-run that gate uncontained.
3. Recompute the v0.22 DAG from the manifests and continue with Stage 3.

## Standing operator decisions (from `.axon-v022/coordination/operator_decisions.json`)

D1 MiCode v0.22 on v014-reconciled · D2 WSL2 nested KVM accepted for B263 (caveat on
every record) · **D3 MiCode v0.22 not releasable until `tui` (credential isolation
`ed082601`) is merged — a declared release dependency, to be satisfied, not waived** ·
D4 reconcile on the candidates · D5 B263 gets an in-guest policy channel.
Stage 7 (pilot) requires operator-approved pilot parameters.
