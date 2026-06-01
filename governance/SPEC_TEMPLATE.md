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
in a mandatory area (§4, §5, §6, §11) blocks implementation.)
