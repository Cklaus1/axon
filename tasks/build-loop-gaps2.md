# build-loop — gaps2 (four reviewed DRAFT specs)

Specs (Step 1 reviewed, human decisions recorded `[DECIDED by human]`): tasks/killgroup-spec.md,
tasks/approval-residue-spec.md, tasks/lsp-residue-spec.md, tasks/harness-hygiene-spec.md.
Run mode: **Balanced** (human). Base: main 43fa1857. Baseline: strict full gate at 3060b866 (rc 0, 387 binaries,
6862 passed, 0 failed, 12 ignored, parity_all 55/2/0, 43m24s); 3060b866..43fa1857 changes only tasks/*.md, so it stands.

## Excluded (needs-human, not built)
Entry-dir module search (AR H1); `approve --program` removal (AR6/H3); AR5 "continue session" (H4); reflex sidecar
own process group (KG4/NH2).

## Lanes (worktree /home/cklaus/projects/axon-lanes/gaps2-<lane>, branch gaps2/<lane>, target .doc-build/gaps2-<lane>)
| Lane | Items, in order | g01 ids |
|---|---|---|
| jobs | KG1 → KG2 → KG3 → KG5 | M3600-M3619 |
| approval | AR1 → AR2 → AR3 | M3620-M3639 |
| core | AR4 → AR5 → AR7 | M3640-M3659 |
| lsp | L3 → L2 → L1 | M3660-M3679 |
| harness | H5, H2 → H1 → H4, H3 → H6, H7 | M3680-M3699 |

Contention (from the specs' Lane plans): jobs/approval share axon-os runtime.rs (:288-806 vs :1199-1302) and cli.rs
(KG1 :522,:590 vs AR cmd_run/verify/explain) — disjoint hunks, integrator merges jobs first. core/harness share
axon-core main.rs and tests/cli_run.rs — disjoint hunks. gate.sh: jobs :840, lsp :16-17/:356-362/:786-798, harness
:384+1/:1002/:1321/:1336. Registries (v022_g01_mutations.py, v022_refusal_coverage.py, v022_attack_markers.py): each
lane appends in its own block at the end, using its id range. Integrator-only, last: AXON_REFERENCE.md,
AXON-COMPLETENESS.*, CHANGELOG.md, governance/notes/v022-pci-delta.md regeneration. Cargo.toml/Cargo.lock: only jobs
(KG1); integrator re-runs `cargo metadata --locked` after merge.

## Inner loop (per item)
Fail-first test red on the base → implement → test green → mutation rows killed (g01 --only) → refusal coverage,
pci_delta (theme file per commit under crates/axon-core/src), fmt → `git commit -- <paths>`.
Lane done = `gate.sh --fast` + the touched crates' full tests (+ serde-json LSP tests / cli_run where the spec says).

## Outer loop
Wave 1: all five lanes in parallel. Integrator merges in order jobs, approval, core, harness, lsp; runs full gate;
smoke per spec; adversarial implementation review; fixes verified findings; strict gate (parity_all) at the end;
tracker/changelog/reference; ff main, push.
Poison ceiling: 3 failed attempts on an item → park blocked, log, continue.
