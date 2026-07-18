# Axon Build Protocol — How Every Feature Gets Built

**Audience:** the autonomous builder (and any human who joins).
**Contract:** every change to Axon follows this lifecycle. No exceptions for "small" changes —
small changes that skip the gates are exactly where regressions enter an unsupervised system.

This protocol is the *process spine*. It references but does not duplicate:
`DEFINITION_OF_DONE.md` (the exit checklist), `TESTING_STANDARD.md` (what tests to write),
`CODE_REVIEW_RUBRIC.md` (what to self-check), `ARCHITECTURE_INVARIANTS.md` (what must never break).

---

## The INNER LOOP — the lifecycle (8 gates)

The 8 gates below are the **inner loop**: one feature, one tight cycle, ending in a green gated
commit. It is one of two loops this protocol defines. The **outer loop** (section below) never
builds features — it periodically re-verifies that every *claimed* status still matches a
*re-runnable* piece of evidence, because the 2026-07-18 audit proved the inner loop alone is not
enough: Gate 7's "update REQUIREMENTS.md" instruction silently lapsed for the entire R19–R34 wave,
and spec headers drifted from reality in both directions (R17 recorded at 1% with Slices 0–3
shipped; R31 headered "Draft" with a passing gate and a real feat commit).

Each feature passes through these gates **in order**. A gate that fails sends you back, not forward.

```
0. FRAME      → know the requirement + acceptance criteria
1. SPEC       → write/locate the tech spec (SPEC_TEMPLATE.md)
2. RED TEST   → write the failing test that encodes the acceptance criteria
3. IMPLEMENT  → smallest change that makes the test green
4. WIDEN      → adversarial/edge/property tests; codegen↔interp parity
5. REVIEW     → self-review against CODE_REVIEW_RUBRIC.md
6. GATE       → full suite + clippy green BEFORE commit; commit only on green
7. VERIFY     → run it for real (example/demo), update docs + REQUIREMENTS.md
```

---

### Gate 0 — FRAME

- Find the requirement in `governance/REQUIREMENTS.md`. If the work doesn't map to a requirement,
  either it's a bug-fix (allowed, but still gated below) or it's scope creep (stop, add a row first).
- State the **acceptance criterion** in one sentence: "this is done when `<observable>`."
- Decide the **risk class** (drives how heavy gates 1, 4, 5 are):
  - **Trivial** (typo, doc, single-line obvious fix) → gates 1 and 4 may be light; 2,3,6 still mandatory.
  - **Standard** (new builtin, stdlib fn, demo) → full lifecycle, normal depth.
  - **Structural** (AST node, pipeline phase, type rule, security boundary, codegen) → full lifecycle,
    *maximum* depth: spec required, parity required, adversarial tests required.

### Gate 1 — SPEC

- **Structural** changes: a tech spec (`SPEC_TEMPLATE.md`) is **required before code**. Architectural
  forks are cheap on paper and expensive in a merged compiler.
- **Standard** changes: a spec is optional but the acceptance criterion + test plan must be written in
  the commit body or the test's doc-comment.
- The spec names the **error codes** the feature introduces (E-codes / W-codes) — invented at spec time,
  not improvised in code.
- The spec carries the **`spec-meta` front-matter block and the §13 Dependency DAG**
  (`SPEC_TEMPLATE.md`, `governance/EXECUTION_MODEL.md`). Concretely, before the spec is buildable:
  - **Claim the number safely:** grep `governance/specs/` for `R<n>-` before using it. Two
    independent specs each claimed R21/R22/R23 and nobody noticed until a human grepped —
    `scripts/verify_all_specs.sh` now flags duplicates, but the check belongs here, at claim time.
  - **Declare edges:** `depends-on` / `blocks` / `supersedes` / `conflicts-with` as full spec IDs
    in `spec-meta`, and per-slice dependencies (including question-blocked nodes, like R36's Q1
    blocking its S3) as §13 DAG rows, each with its **gate named up front**.
  - **Reserve codes:** the `reserves:` key lists claimed E-blocks/exit codes so collisions
    (the E1300 class) surface at spec time.

### Gate 2 — RED TEST (test-first is mandatory)

- Write the test that encodes Gate 0's acceptance criterion. **Run it. Watch it fail for the right reason.**
  A test that was never red proves nothing.
- Place it per `TESTING_STANDARD.md` (CLI e2e in `tests/cli_run.rs`, unit in the module, etc.).
- For interpreter features: the test asserts *observable behavior* (exit code, stdout, error code), not
  internal structure — so it survives refactors.

### Gate 3 — IMPLEMENT

- Smallest change that turns the red test green. Resist building Gate-4 generality now.
- Match surrounding code's idiom, naming, comment density (read the neighbors first).
- If implementing touches the AST: **every** exhaustive `match` across the pipeline must be updated in
  the same change (parser, resolver, infer, checker, borrow, capabilities, codegen, fmt, mono, comptime).
  Build with `--features codegen` to catch the codegen arms the default build skips.

### Gate 4 — WIDEN (this is where "deep testing" lives)

Per risk class, add until the feature is *trustworthy*, not just *passing*:

- **Edge cases:** empty, zero, negative, max, off-by-one, unicode, the boundary the spec named.
- **Adversarial:** malformed input, deep nesting, recursion bombs, hostile sizes. The interpreter must
  *fail gracefully* (catchable panic / clean error), never SIGABRT/overflow the host.
- **Property tests** (when applicable): invariants like round-trip (`from_str(to_str(x)) == x`),
  commutativity, idempotence, monotonicity.
- **Parity:** if the feature exists in both interpreter and codegen, a parity test asserts identical
  observable output. Interpreter is the reference semantics (`ARCHITECTURE_INVARIANTS.md` I-7).
- **Performance:** if the change is on a hot path or claims a perf property, a benchmark/assertion guards it.

### Gate 5 — REVIEW

- Self-review the full diff against `CODE_REVIEW_RUBRIC.md` **as an adversary trying to find the bug.**
- For Structural changes, run a second independent pass (or spawn a review sub-agent) — single-pass
  self-review misses its own blind spots.
- Fix findings before Gate 6. Do not commit "will fix in follow-up" on a correctness finding.

### Gate 6 — GATE (the commit barrier)

- **Run the full workspace suite + clippy. Commit ONLY on green.** `&&`-chain them so a red state
  cannot land (`gate-commits-on-tests` memory; this has bitten the project before).
  ```bash
  cargo test --workspace --no-default-features 2>&1 | grep -E "FAIL|error\[" && echo RED || \
  (cargo clippy -p axon-core --no-default-features --no-deps 2>&1 | grep -E "^(error|warning)" && echo LINT || \
   git add -A && git commit -F <msg-file>)
  ```
- Commit message: what + *why*, names the requirement (Rn), cites the acceptance test, ends with the
  required `Co-Authored-By` trailer. No backticks/`$()`/`<` in `-m` strings (shell mangles them; use `-F`).
- Branch first if on the default branch. Push only when the task authorizes it.

### Gate 7 — VERIFY

- **Run the feature for real** — an example, a demo, the actual CLI path a user hits. Tests can pass while
  the integrated experience is broken.
- Update: the relevant `examples/`, `spec/` or `stdlib.md`, `SESSION_STATUS.md`, and **`REQUIREMENTS.md`
  `%`/`Status`** in the same commit that completed the feature.
- **The status update is an evidence-table row, not prose.** Any status change (spec `**Status:**` +
  `status-claim`, §13 DAG row → `landed`, REQUIREMENTS.md `%`) must land together with a §14
  Evidence-ledger row: the exact verify command, the commit hash it was run at, the date, and the
  result (`EXECUTION_MODEL.md` §2). *A claim without a re-runnable evidence pointer is not a valid
  status.* This is structural, not honor-system: the outer loop re-runs the command column and
  diffs against the claim — a prose-only status is exactly what rotted for R17/R31/R21–R23.
- If the requirement reached DONE, the commit body cites the now-passing acceptance test.

---

## The OUTER LOOP — evidence re-verification sweep

The inner loop keeps a *feature* honest; nothing above keeps the *ledger of claims* honest over
time. The outer loop is the repeatable version of what a human did manually on 2026-07-18:
re-run every spec's cited gate, diff reality against the claimed status, and correct the drift.
That session found drift in **both directions** — stale-pessimistic (R31 "Draft" + ROADMAP
"Forward" while landed with a passing gate; R17 at 1% with Slices 0–3 shipped) and
stale-optimistic-adjacent (the R28 audit ledger claimed to audit capabilities but only ever logged
AI calls — a gate re-run plus a claim-vs-behavior read caught it). Neither direction is
catchable by the inner loop, because the inner loop only looks at the feature it is building.

**What a sweep does** (mechanized as far as `scripts/verify_all_specs.sh` reaches; the rest manual):

1. **Static lint** (cheap, always): parse every `governance/specs/*.md` `spec-meta` block —
   duplicate `R<n>` numbers (the dual-numbering class), dangling `depends-on`/`blocks` targets,
   evidence commands that name nonexistent scripts, `status-claim` ≠ prose `**Status:**`,
   non-Draft specs with `evidence: none`, reserved-code collisions.
2. **Evidence re-run** (expensive, scoped): re-execute the §14 Evidence-ledger command column
   (and the REQUIREMENTS.md wave-table Evidence column) for the specs in scope; compare exit
   status against the claimed Result.
3. **Diff and flag**: every divergence, in either direction, is a finding. A Draft header over a
   passing gate is drift. A "Landed" over a failing gate is drift. A gate that passes but whose
   claim the code doesn't actually satisfy (the R28 pattern — the gate itself was too narrow) is
   a *gate bug*, the worst class: fix the gate first, then re-judge the claim.

**Cadence / triggers** (any one of these obliges a sweep):
- **Before any session that will edit status** in `governance/specs/`, `REQUIREMENTS.md`, or
  `ROADMAP.md` status tables — never edit a status you haven't just re-verified.
- **Every 10 autonomous inner-loop ticks** during `/loop` runs (one sweep ≈ one tick of budget;
  the R19–R34 wave was ~16 specs of unmaintained drift, so 10 is the ceiling, not the target).
- **After any merge from a parallel track** (the R21 Vision-OS collision showed parallel dev is
  where dual-numbering and status races enter).
- Static lint (step 1) is cheap enough to run at every Gate 6 alongside the test suite.

**Closing the loop on a divergence** (detection without correction is just better-organized rot):
- A confirmed divergence requires a **correction commit** — same discipline as Gates 6/7, landed
  in the *same sweep session*, message prefixed `truth:` and citing the re-run evidence (command +
  commit + date). Stale-pessimistic drift is corrected upward with the new Evidence row;
  stale-optimistic drift is corrected downward **and** spawns a REQUIREMENTS.md gap entry (a
  feature that regressed is a defect, not a status typo).
- A gate-bug finding (R28 class) opens an inner-loop tick to fix the gate, red-test-first, before
  the claim may be re-asserted.
- A sweep that finds nothing records that too: bump the `Last verified` column dates it re-ran.
  Undated evidence decays back into prose.

---

## Anti-patterns (auto-reject in self-review)

- **Green without red.** A test added already-passing tests nothing. Always see it fail first.
- **Skipping parity** because "the interpreter is what runs." Codegen drift is silent until someone builds native.
- **Widening before red-green.** Build the general thing only after the specific thing works and is tested.
- **`unwrap()`/`expect()` on user-reachable paths.** User input → `Result`/graceful panic, never host abort.
- **Silent capability widening.** Any change that lets code reach net/fs/exec must update `ARCHITECTURE_INVARIANTS.md`
  TCB section and have an explicit allow/deny test.
- **Doc drift.** Shipping behavior without updating the spec/status is how the next builder inherits a lie.
- **Status without evidence.** Writing `Landed`/`70%`/`Slices 0–3 done` anywhere (spec header, DAG row,
  REQUIREMENTS.md) without a same-commit Evidence row (command + hash + date). The prose-only status is
  the single failure mode behind the R17/R31/R21–R23 drift.
- **Claiming a spec number without grepping for it.** Two files named R21/R22/R23 each; run the static
  lint (or `grep 'R<n>-' governance/specs/`) before claiming.

---

## Cadence for autonomous loops

When running unsupervised (e.g. `/loop`), one "tick" = one feature through all 8 gates, ending in a
green gated commit + push. Do not batch multiple unverified features into one tick — small, verified,
reversible steps are how an unsupervised system stays correct. If a tick can't reach Gate 6 green,
revert to the last green state and record why; never leave the tree red.

Every 10th tick (or per the outer-loop triggers above) is spent on an **outer-loop sweep**, not a
feature: `scripts/verify_all_specs.sh` + evidence re-runs + `truth:` correction commits. An
autonomous loop that only ever runs the inner loop reproduces the R19–R34 lapse by construction.
