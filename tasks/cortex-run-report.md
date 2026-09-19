# Build-loop run report — Axon Cortex v0.7

Branch `cortex-m0-m1`. Spec: `docs/axon_cortex_v0_7/` (67 files, 20 specs
CX-00…CX-19, 69 work packages, 122 proposed gates).

## What the stop condition asked for

> DONE = every non-needs-human node DONE or blocked-and-logged
>    AND `cargo test --workspace` shows no NEW failures vs the baseline set
>    AND every in-scope spec item addressed or tagged needs-human
>    AND C11 (the conformance run) succeeds on its concrete signal

## Built (12 of 12 in-scope DAG nodes)

| Node | What | Evidence |
|---|---|---|
| C1 | 10 Cortex claims about existing Axon rerun against the live binary | `docs/axon_cortex_v0_7/C1-VERIFIED-SEAMS.md` |
| C2 | Typed contracts: `Observed<T>`, strict parse, canonical digest | 9 tests, 5/5 mutations killed |
| C3 | A genuinely broken fixture (two `@[test]`s fail before, pass after) | `fixtures/broken.ax` |
| C4 | Partial observation that never fabricates | `cxg_g02_partial` |
| C5 | Denial-first action catalog | `cxg_c12_catalog_is_denial_first` |
| C6 | Patch transaction over a staged COPY, before/after digests | `cxg_c11` |
| C7 | Registered checks driving the real `axon test` | `cxg_c11` |
| C8 | Deterministic mock decision adapter | `cxg_c11` |
| C9 | Independent verification (claim cannot drive the verdict) | `cxg_g03_done` |
| C10 | Episode evidence + replay identity | `cxg_c11`, `cxg_c2_event_order…` |
| C11 | **The smoke test** — the whole episode end to end | `cxg_c11_repair_episode_runs_end_to_end` |
| C12 | Negative gates G00/G02/G03×3 + two hardening gates | 6 tests, 11/11 mutations killed |

**C11's concrete signal**, as required by Step 0: the fixture's
`visible_repro` check must exit non-zero BEFORE the patch and zero after,
`hidden_completion` must pass only after, and the episode's event sequence
must be exactly `snapshot, observed, check, patch, check, verified` with a
digest stable across a serialise/parse round trip. All asserted, not
narrated. The test FAILS rather than skips when the interpreter is absent —
a smoke test that did not run is not one that passed.

## Defects found, all in code written during this run

1. **`snapshot_id` was a constant.** Authority never expired: a grant issued
   over one workspace state was accepted against any later state, because
   every snapshot compared equal. Found by mutation, not by the green suite —
   the negative gate passed while testing nothing, because it forged a stale
   id the production path could not produce.
2. **`verify` collapsed "did not run" into "failed."** Verdict fail-closed
   and correct; the recorded evidence made a broken verifier
   indistinguishable from a wrong repair.
3. **A write prefix was a string prefix.** `broken.ax` authorized
   `broken.ax.evil`.
4. **An empty prefix silently meant "everything."** The same shape as the
   `sandbox_create_scoped` hole fixed earlier in this branch, and contrary to
   the product policy: permissiveness is stated (`*`), never implied by a
   blank.

## Deferred, with reasons — a skipped gate is not a passed gate

- **No production caller.** Every `Runner` method is exercised, but only from
  `tests/`. There is no `axon cortex` verb; the CLI does not call Cortex.
  For C11 this is the correct shape — the conformance run *is* the consumer —
  but Cortex is not on any user's path, and nothing here should be read as
  saying it is. Wiring a new user-facing verb is a product decision.
- **needs-human (44 packages):** B00 (owner-reviewed naming/ID mapping),
  B15/B16 (live-model credentials and spend), M2–M9, Research, Compiler,
  OS-K. Excluded from the autonomous run per Step 2 and not built behind a
  default-off flag.
- **110 of 122 proposed gates unimplemented.** The 12 built cover the
  vertical slice; the rest belong to the needs-human subtrees above.
- **Deterministic only.** The decision is a fixed mock. This run measures
  plumbing. Per the package's own BUILD_PLAN, model-driven solver behaviour
  is a separate number and was not measured.
