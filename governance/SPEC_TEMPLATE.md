# Tech Spec Template

Copy this file to `governance/specs/<id>-<slug>.md` (or `spec/` for compiler-phase specs) and fill every
section. A spec with a `TODO` in a mandatory section is not ready to build against. The spec is written
**before** code for Structural changes (`BUILD_PROTOCOL.md` Gate 1).

Delete the parenthetical guidance as you fill each section.

---

## <Feature Name>

**Spec ID:** `R<n>-<slug>` (ties to `REQUIREMENTS.md`)
**Status:** Draft | Reviewed | Implementing | Shipped
**Risk class:** Trivial | Standard | Structural
**Author / date:**

```spec-meta
id: R<n>-<slug>
status-claim: Draft
depends-on: none
blocks: none
blocked-by: none
supersedes: none
related: none
conflicts-with: none
reserves: none
evidence: none
```

(The `spec-meta` block is **required** (see `EXECUTION_MODEL.md` §2) and is the machine-readable
mirror of this header — `scripts/verify_all_specs.sh` parses it. Rules:
- `id` must be the full `R<n>-<slug>` **and the `R<n>` number must be unclaimed** — grep
  `governance/specs/` before picking it. The R21/R22/R23 dual-numbering collision happened because
  nothing checked this; the verifier now flags duplicate numbers.
- `depends-on` / `blocks` / `supersedes` / `related` / `conflicts-with` take comma-separated full
  spec IDs (`R17-freestanding-substrate`, never bare `R17` — bare numbers are ambiguous under
  dual-numbering). `blocked-by` may also name an open question (`R36 §12 Q1`) when a decision, not
  a spec, is the blocker.
- `reserves` lists error-code blocks / exit codes this spec claims (`E37xx`, `exit 7`) so the
  verifier can flag code collisions (the E1300 AI-policy collision class).
- `evidence` names the acceptance-gate command (usually `scripts/r<n>_acceptance_gate.sh`) once it
  exists; `none` is only valid while `status-claim: Draft`.
- `status-claim` must always equal the prose `**Status:**` line's first word. When you update one,
  update both — the verifier diffs `status-claim` against §14's evidence.)

---

### 1. Motivation
(Why this exists. The user problem and the requirement it satisfies. One paragraph. If you can't state the
user-visible win in two sentences, the feature isn't framed yet.)

### 2. Requirement link
(Which `REQUIREMENTS.md` row(s) this advances, and the acceptance criterion it must satisfy. Quote it.)

### 3. Surface (what the user writes)
(The syntax, builtin signatures, attributes, or CLI flags. Show real code. Show the *common* case and the
*error* case. This is the contract.)
```axon
// example usage
```

### 4. Semantics (what it does)
(Precise behavior. A behavior table is ideal — one row per input class → output/effect. Cover the happy
path, every boundary, and every failure. This table *is* the test plan in §8.)

| Input class | Behavior |
|---|---|
| (normal) | |
| (empty/zero) | |
| (boundary) | |
| (malformed) | |

### 5. Type rules (if it touches the type system)
(New inference rules, constraints, unification behavior. How it composes with generics/traits/Option/Result.
What `parse_type_str` / the checker must learn. If none, write "N/A".)

### 6. Error codes
(Every diagnostic this feature can emit, invented here, not improvised in code. `E####` / `W####`, the
condition that triggers it, and the message shape. Codes are contract per `ARCHITECTURE_INVARIANTS.md` I-14.)

| Code | Trigger | Message shape |
|---|---|---|
| E#### | | |

### 7. Invariants touched
(List every `ARCHITECTURE_INVARIANTS.md` ID this feature must *preserve*, and any it *changes* — if it
changes one, this spec doubles as the invariant-change proposal per that file's process. Pay special
attention to I-2 parity, I-8/I-9 success-signal, I-11 capability boundary.)

### 8. Test plan (maps 1:1 to §4's behavior table)
(For each behavior row: the test that proves it, the layer it lives at (`TESTING_STANDARD.md` 1–6), and
whether it needs a parity test. List the adversarial inputs explicitly. Name the red test that must fail
first.)

- [ ] Unit:
- [ ] Integration:
- [ ] CLI e2e (observable: exit code / stdout / error code):
- [ ] Adversarial:
- [ ] Property (invariant): 
- [ ] Parity (interp↔codegen):
- [ ] Journey/red-team (if user-facing):

### 9. Acceptance criteria (the done gate)
(The binary conditions under which `REQUIREMENTS.md` may mark this DONE. Each must be a *passing named
test*. "It works" is not an acceptance criterion; "test `foo_bar_baz` passes" is.)

- [ ] 
- [ ] 

### 10. Performance budget
(If on a hot path or claiming a perf property: the budget and the benchmark guarding it. Else "N/A".)

### 11. Rollout & rollback
(How it ships — feature flag? gated behind `--features`? Is it a small revertible commit? If `git revert`
of this would leave a broken tree, decompose it. What's the blast radius if it's wrong in production?)

### 12. Open questions
(Anything unresolved that a human or a deeper analysis must answer before/while building. An open question
in a mandatory area (§4, §5, §6, §11) blocks implementation. If a question blocks a §13 node, name it in
that node's `blocked-by` cell — R36's Q1 blocking its S3 is the canonical example.)

### 13. Dependency DAG (required — see `EXECUTION_MODEL.md` §1)
(One row per node. A node is a slice or a gate of THIS spec (`R<n>.S0`, `R<n>.S1`, …). `Depends-on` lists
the nodes or external full spec IDs (`R17-freestanding-substrate`, or a finer node like
`R17-freestanding-substrate.S1`) that must be green first; `blocked-by` names an open question when a
*decision* is the blocker. `Gate` is the named test/script that turns the node green — invented here, like
error codes, not improvised later. This is the R36 S0–S5 slice table made a required convention: the
outer-loop verifier walks these rows, so a slice with no gate named is a slice whose "done" can silently rot.)

| Node | Depends-on / blocked-by | Gate (named test or script) | Status |
|---|---|---|---|
| R<n>.S0 | — | | todo |
| R<n>.S1 | R<n>.S0 | | todo |

### 14. Evidence ledger (required for any non-Draft status — see `EXECUTION_MODEL.md` §2)
(**A claim without a re-runnable evidence pointer is not a valid status.** Every status claim this spec
makes — the header's `Landed`/`Shipped`/`Slices 0–3 done`/`70%`, and each §13 row marked `landed` — must
have a row here: the *exact command* that verifies it, the commit at which that command was last actually
run and seen passing, and the date. Prose like "verified, tests pass" with no command is the R17/R31
failure mode: the header rots and nobody notices until a human re-greps commit history. The outer loop
(`BUILD_PROTOCOL.md`) re-runs these commands and diffs the result against `status-claim` — in both
directions (a Draft header over a passing gate is drift too).)

| Claim | Verify command | Expected | Last verified (commit @ date) | Result |
|---|---|---|---|---|
| (e.g. "Slice 0 landed") | `scripts/r<n>_acceptance_gate.sh` | exit 0, "ALL PASS" | `abc1234` @ 2026-07-18 | PASS |
