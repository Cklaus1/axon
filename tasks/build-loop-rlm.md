# Build harness — `AXON_FOR_RLM.md`

The DAG, loops and gates for the autonomous build of the RLM-engine spec.
Distinct from `tasks/build-loop.md`, which is the completed harness for the
governance-audit spec; this run does not touch that one's files.

**Resolved placeholders**

| slot | value | how resolved |
|---|---|---|
| `<SPEC>` | `AXON_FOR_RLM.md` (repo root) | from args |
| `<TEST_CMD>` | `cargo test --workspace` | `Cargo.toml` at root, 19-crate workspace; matches the prior run's baseline command |
| `<RUN_CMD>` | `./target/debug/axon` | the interpreter CLI built by `cargo build -p axon-core` |
| `<SMOKE_SCENARIO>` | see **Smoke** below | the measured dominant failure, `let mut count` |
| `<FULL_SUITE_BUDGET>` | one measured `<TEST_CMD>` run | recorded in `tasks/baseline-rlm.md` |
| `<RUN_BUDGET>` | unbounded | none given at invocation |
| `<REVIEW_MODEL>` | self, adversarial | no `fable` in this environment |

**Two repos.** Changes land in `/home/cklaus/projects/axon`. The measurement
harness is `/home/cklaus/projects/aicoding/atlas/spikes/rlm-engine` and is a
separate git tree with its own history. Commits are made to each tree only for
work that belongs to it; neither is committed on the other's behalf, and the
atlas tree's cleanliness is checked before it is touched.

---

## Task DAG

Nodes are spec sections. `§3` does not appear: Step 1 merged it into `§1` as
that task's first table row (it is the same edit to the same function).

```
  ┌─────────────────────────────── TIER 1 — diagnostics ──────────────────────────────┐
  │                                                                                   │
  │   T-R1  parse-tier help table            T-R4  E1001 help → the `help` field       │
  │   §1 + §3                                §2b                                       │
  │   NEW  parse_help.rs                     checker/contained emission site           │
  │     │                                          │                                   │
  │     ▼                                          │                                   │
  │   T-R2  run: located parse diagnostics          │                                   │
  │   §2 half A — the parse tier                    │                                   │
  │   main.rs cmd_run                               │                                   │
  │     │                                           │                                   │
  │     ▼                                           │                                   │
  │   T-R3  run: stop flattening ★CRITICAL PATH★    │                                   │
  │   §2 half B — delete run_check_pipeline         │                                   │
  │   main.rs cmd_run + wrapper deletion            │                                   │
  │     │                                           │                                   │
  └─────┼───────────────────────────────────────────┼───────────────────────────────────┘
        │                                           │
        └─────────────────┬─────────────────────────┘
                          ▼
  ┌──────────────────── TIER 2 — the gate measurement ────────────────────┐
  │   T-R5  language card + R9 ×3, in atlas (D5)                          │
  │         reports spread; proposes nothing where the spread straddles   │
  └───────────────────────────────────────────────────────────────────────┘
                          │
                          ▼
                 ┏━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
                 ┃  D6 needs-human: the bar → §4 and §5        ┃
                 ┃  PRUNED. Not built, not flagged, not        ┃
                 ┃  built-behind-a-flag. Reported instead.     ┃
                 ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
```

**Edges.** `T-R1 → T-R2` because T-R2 wires T-R1's help onto the run path — a
help table nothing calls is the uncalled-mechanism defect. `T-R2 → T-R3` is
**file contention, not logic**: both edit `cmd_run`. `T-R4` shares no file with
the T-R1/2/3 chain and is the one node eligible for parallel dispatch; it is run
sequentially anyway because the chain is only three deep and the dispatch
overhead exceeds the saving.

**Acyclicity + coverage.** Topological order: T-R1, T-R2, T-R3, T-R4, T-R5.
Five nodes, four edges, no cycle. Spec-section coverage: §1 → T-R1, §2 → T-R2 +
T-R3, §2b → T-R4, §3 → T-R1 (merged), §4 → pruned (D6), §5 → pruned (D6),
sequencing step 2 → T-R5. Every section is addressed exactly once or explicitly
pruned.

---

## Critical path — T-R3, and the extra gate it must pass

T-R3 is the node the rest builds on: T-R1's help text and T-R4's `help` field
are both **unobservable to an RLM host** until the run path stops flattening
diagnostics, and T-R5 measures the combination. It is also the only node that
*deletes* a shared function rather than adding to one.

Normal regression tests are not sufficient for it. The extra gate is a
**round-trip equivalence proof**, the closest available analogue to a migration's
reversal proof:

> For every diagnostic-producing program, `axon run` and `axon check` must emit
> **byte-identical diagnostic JSON**.

That is the invariant the flattening broke, it is checkable mechanically over a
corpus rather than on hand-picked cases, and it cannot be satisfied by a partial
fix: any field T-R3 fails to carry across shows up as a diff. The corpus is the
existing `examples/` tree plus the fixtures T-R1 adds.

---

## Loops

**Inner (per task).** Write the failing regression test → implement → mutation-
verify each new test (break the implementation in the specific way the test
claims to catch; assert the mutation edit landed before trusting a survival) →
self-review against the checklist → **caller check: grep for a non-test caller of
every mechanism the task claims to wire** → `cargo fmt --check` + `cargo clippy
-D warnings` on every crate touched → artifact scan → commit.

The clippy step names the crates explicitly. `scripts/gate.sh` lints an explicit
crate **allowlist**, not the workspace, and `axon-core` is linted only under
`--no-default-features`; a crate absent from that list is unlinted by the project
gate. Do not infer coverage from a green gate.

**Outer.** Next ready node in topological order, respecting the tier boundary.

**Meta.** On any correction or failure, append to `tasks/lessons.md` (this run's
section, appended after the governance-audit run's — that file compounds across
runs and is not reset).

**Full-suite regression.** After each tier. Compares against
`tasks/baseline-rlm.md`, **diffed both ways**: new failures block; baseline
failures that now pass without a task claiming them mean the baseline drifted,
and on that signal the baseline is re-captured and every gate result since the
last capture is marked unverified.

**Smoke.** Once per tier, and it is the spec's own headline case:

```
printf 'fn main() -> i64 {\n    let mut count = 0\n    0\n}\n' > /tmp/smoke.ax
./target/debug/axon run /tmp/smoke.ax 2>&1 | grep -q '"code":"E0000"' &&
./target/debug/axon run /tmp/smoke.ax 2>&1 | grep -q '"help":.*mut'  &&
./target/debug/axon run /tmp/smoke.ax 2>&1 | grep -q '"line":2'
```

Concrete signal: all three greps hit, and the process exits 2. Before T-R1/T-R2
land, all three miss — the smoke test is red at the start of the run by
construction, which is what makes it a test rather than a formality.

**Measurement (T-R5 only).** Three full R9 runs, per D5. Reported as three
numbers plus their spread. A single run is not a result.

---

## Gates

- **Verification bar.** Behaviour change → fail-first regression test. T-R3's
  wrapper deletion is behaviour-preserving *for its other callers* and
  behaviour-changing for `run`; both apply — the run-path change gets a
  fail-first test, the deletion is gated on the baseline staying clean.
- **Poison-task ceiling.** 3 attempts, logged to `tasks/attempts.log` **before**
  each attempt. Attempt N's plan must differ materially from attempt N-1's or
  the task is parked instead of spending the attempt.
- **Transitive blocking.** T-R3 blocked ⇒ T-R5 blocked. T-R3 is the critical-path
  item, so a T-R3 block **stops the whole run** and reports.
- **Dependency hygiene.** No new third-party dependency is expected; the parse
  help table is `std` only. Adding one requires a stated reason in the commit.
- **needs-human.** D6's subtree (§4, §5) is not built, not stubbed, and not
  built behind a default-off flag.

## Stop condition

```
DONE = T-R1…T-R5 each DONE or blocked-and-logged
   AND cargo test --workspace shows no NEW failures vs tasks/baseline-rlm.md
   AND every spec section is addressed or pruned under D6
   AND the smoke scenario's three greps hit and the exit code is 2
```
