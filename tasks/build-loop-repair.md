# Build loop: S1 governed repair episode (`tasks/repair-spec.md`)

Base `main` f83c6f86 → integration branch `repair/int` (`/home/cklaus/projects/axon-lanes/rep-int`). Integrator: the
top-level session. Env for every command: `export PATH=$HOME/.cargo/bin:$PATH TMPDIR=/var/tmp/r51-tmp` and a per-lane
`CARGO_TARGET_DIR=/home/cklaus/projects/axon-lanes/.doc-build/<lane>`.

## DAG

```
R0a caps verb ─┐
R0b fs/net scope┴─► R1 (grant, admission, fence, pre-flight, cortex grant, sweep) ─► R5 ─┐
R2 bounded exec ─► R4 structured feedback ─► R3 walk budget ───────────────────────────┴─► R8 docs ─► freeze ─► R7a paid regress ─► full gate ─► review loop ─► push
```

Critical path: R0 → R1 → R5 → freeze → R7a → gate.

## Tracks (lanes)

| Lane | Worktree / branch | Items | Owns |
|---|---|---|---|
| LaneCaps | `axon-lanes/rep-caps` / `repair/caps` | R0a, R0b | `crates/axon-core/**`, `crates/axon-rt/**` refusal row, `scripts/build_outcome_regression.sh`, `AXON_REFERENCE.md`, `spec/stdlib.md` caps/env text |
| LaneRepair | `axon-lanes/rep-cortex` / `repair/cortex` | R2 → R4 → R3, then (after LaneCaps merges into `repair/int`) R1 → R5, R8 cortex docs | `crates/axon-cortex/**` (src, tests, README, benchmarks/run_ab.py, benchmarks/fence_sweep.py) |
| Integrator | `rep-int` | merges, `Cargo.lock`, `CHANGELOG.md`, `AXON-COMPLETENESS.*`, `scripts/gate.sh` sweep hook, `scripts/v022_*` rows when lanes collide, `tasks/*`, R7a, gates, push | |

One cortex lane, not three: R1-R5 all edit the `runner.rs` episode loop and `bin/cortex.rs`; parallel lanes there would
spend more on merges than they save.

## Inner loop per lane (each item)

1. Fail-first test committed red (or shown red in the lane log), then the fix.
2. `cargo test -p <crate>` (filtered for axon-core `cli_run`), `cargo fmt --all --check`,
   `python3 -B scripts/v022_refusal_coverage.py`, `python3 -B scripts/v022_g01_mutations.py --changed-since` rows for
   moved anchors, pci theme + `python3 -B tasks/r51-pci-regen.py` + `python3 -B scripts/pci_delta.py --check` per
   `crates/*/src` commit, `reference_gate.sh` when a verb/env var changes.
3. `git commit -- <own paths>`; report hash.

## Outer loop (integrator)

1. Merge LaneCaps → `repair/int`; LaneRepair merges `repair/int` before R1.
2. Merge LaneRepair; R1f sweep green; `./scripts/gate.sh --fast`.
3. Freeze source (no further edit to the repair digest) → R7a `scorecard.py regress` → commit history row.
4. Full `./scripts/gate.sh`; smoke with real binaries in a temp dir (PWNED denied; real micode episode verified with
   `accepted.sha256 == sha256(file)`; `--max-proposals 2` → exactly 2 calls); `git status --short --ignored` clean of
   run debris.
5. Implementation review (reviewer agent) → fix → (re-run R7a only if the repair digest changed) → final gate.
6. Fast-forward `main`, push.

## Stop condition (compound)

Every non-deferred item R0-R5, R7a, R8 meets its acceptance; full gate rc 0 with no new failure vs baseline; R7a PASS
under the quoted rule with zero sandbox-violation tasks; review findings resolved or logged; `main` pushed.

## Known failures (baseline, do not chase)

From `tasks/repair-baseline.md`. Pre-existing: 4 serde-json-only clippy errors in `lsp.rs`; "REDTEAM FAILED: blocking
deploy" in gate logs is expected demo output; relative `--workspace` broken (review #27, opportunity).
