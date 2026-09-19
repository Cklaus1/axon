# Build harness — Axon Cortex M0/M1 vertical slice

Spec: `tasks/spec-axon-cortex.md`. Source package:
`docs/axon_cortex_v0_7/axon-cortex-build-v0_7/`.

## Task DAG

```
C1  verify existing-Axon claims        (no deps)
 └─ C2  typed contracts  [CRITICAL PATH]
     ├─ C3  resettable broken fixture
     ├─ C4  partial observation
     ├─ C5  denial-first action catalog
     │   ├─ C6  patch transaction (needs C3)
     │   └─ C7  registered checks     (needs C3)
     └─ C8  mock decision adapter     (needs C4, C5)
         └─ C11 deterministic end-to-end conformance  [SMOKE TEST]
C6,C7 ─┬─ C9  independent verification ──┐
       └─ C10 evidence + replay ─────────┴─> C11 ─> C12 negative gates
```

Longest chain: C1→C2→C5→C6→C9→C11→C12 (7).
Parallel tracks after C2: {C3}, {C4}, {C5} are disjoint; {C6,C7} disjoint
after C3+C5; {C9,C10} disjoint after C6+C7.

**Self-check:** acyclic (topological order C1,C2,C3,C4,C5,C6,C7,C8,C9,C10,
C11,C12 — each node's deps precede it). Covers every in-scope spec item
exactly once. needs-human subtree (B00, B15, B16, M2–M9) excluded and not
represented as nodes.

## Loops

**Inner loop (per task):** implement → verify (behaviour change ⇒ regression
test fails first, passes after; behaviour-preserving ⇒ baseline stays clean)
→ mutation-verify each new test (break the thing it claims to catch; if the
mutation survives, suspect the test's PREMISE before its assertions) → caller
check (grep for a production caller outside the test tree; a mechanism with
tests and no caller is not built) → self-review checklist (scope-confined
diff; no dead code; no secret VALUES; reuse before writing; obsolete code
deleted not shimmed; codebase simpler after) → `cargo fmt` + `cargo clippy
-D warnings` → artifact scan → commit.

**Artifact scan (mechanical, `grep -P`, check exit code):**
- secret-like: `(?i)(\bbearer\s+[a-z0-9._~+/=-]{16,}|(?<![a-z0-9_])(ak|sk)[-_=:][a-z0-9_=-]{10,}|\btoken\s*[=:]\s*[^\s\`'"<>]{12,})`
- local path: `(?<![\w./-])/(Users|Volumes|var/folders|private/tmp|tmp)/[^\s\`'"<>]+`

Run over files THIS run generates. A hit blocks the commit.

**Outer loop:** next ready node by DAG order; before writing code, open the
CX section the task cites (the spec is a synthesis and is lossy exactly where
an author-call lives).

**Meta loop:** on any correction/failure, append mistake→rule to
`tasks/lessons.md`.

**Full-suite regression loop:** after each tier, `cargo test --workspace`,
diff the failure set BOTH ways against `tasks/cortex-baseline.md`. One run is
~14 min, so tier = a DAG level.

**Smoke loop:** once C11 exists, run it each tier. Until then the smoke slot
is UNFILLED and reported as such — no passing proxy.

## Gates

- **Critical path:** C2 (typed contracts). Extra proof beyond a regression
  test: round-trip byte-identity (serialize→deserialize→re-serialize) plus a
  rejection proof per malformed shape. If C2 is BLOCKED the whole run stops.
- **Poison-task ceiling:** attempts 1–2 diagnose/re-plan/retry; attempt 3
  fails ⇒ park `blocked`, block dependents, continue. Append to
  `tasks/attempts.log` BEFORE each attempt. A retry whose plan matches the
  previous one (normalised) is not a retry — state what is materially
  different or park it.
- **Repo conventions:** no new `R*`/`E****`/exit codes; any new `AXON_*` var
  needs an `env_registry.rs` row; every new fixture must be referenced by a
  test; `AXON_REFERENCE.md` regenerated if the surface changes.

## Stop condition

```
DONE = every non-needs-human node DONE or blocked-and-logged
   AND cargo test --workspace shows no NEW failures vs the baseline set
   AND every in-scope spec item addressed or tagged needs-human
   AND C11 (the conformance run) succeeds on its concrete signal
```
