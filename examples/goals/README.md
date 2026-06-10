# Axon Goals — authoring guide (Phase 10)

A **goal file** (`*.md`) is the structured-prose surface of Axon: what an author
writes. `axon-surface` compiles it to a typed `.ax` AST, which `axon` then
type-checks and runs. One command does the whole two-track flow:

```bash
# Build the interpreter CLI once (no LLVM needed):
cargo build -p axon-core --no-default-features --bin axon

axon goal examples/goals/optimize-goal.md          # compile prose → .ax → check → run
axon goal --emit examples/goals/optimize-goal.md   # print the generated .ax instead of running
```

Goals that call the LLM (`ai_complete` / `ai_extract_*`) need one of:
- `AXON_AI_MOCK=1 axon goal <file>` — deterministic stub responses, **no key/network/feature** (great for demos, CI, tests), or
- a binary built `--features asi-runtime` with `ANTHROPIC_API_KEY` set — real inference.

## Anatomy: every section drives the program

A goal file has 10 required `##` sections. Each compiles to a concrete part of
the generated `.ax`:

| Section | Compiles to |
|---|---|
| `Intent` | file-header comment |
| `Inputs` / `Outputs` (`- \`name: type\``) | helper function signatures |
| `Score` | scoring intent (author fills the body — see below) |
| `Verify` (an ```axon `@[verify(...)]` block) | the runtime deploy gate |
| `Redteam` | adversarial gate (when an author `redteam_check` is supplied) |
| `Budget` (first integer, e.g. "Up to 20 …") | `goal_run`'s evaluation budget (`max_evals`) |
| `Constraints` | documented intent — but **enforced during the search** when you supply a `feasible` predicate (see below) |
| `Effect surface` | documented intent — but **compiler-enforced** when it carries a `@[contained(...)]` declaration: it's stamped on the generated loop, so `try_variant` and everything it calls must stay within the declared capabilities (E1001/E1004), transitively |
| `Provenance` | documented intent (advisory in v1) |

The compiler wraps the author's logic in a goal-loop harness: an `@[adaptive]`
`try_variant` driven by `goal_run` (hill-climb), the `@[verify]` deploy gate,
and `main`.

## The Implementation section — author-supplied bodies

Put real Axon in an ```axon code block under an `## Implementation` section.
The compiler **lifts these verbatim**; it stubs only the helpers you omit.
Three functions, if you define them, override the default harness:

- `fn try_variant(x: i64) -> i64` (`@[adaptive]`) — the goal-loop objective
  `goal_run` hill-climbs. Define it for full control (e.g. a pure objective, or
  your own LLM loop); otherwise the default loop calls `ai_complete` over prompt
  variants and scores with `score_output`.
- `fn assert_deployable(score: i64) -> …` — the deploy gate. Return an
  `Uncertain<i64>` with an `@[verify(confidence >= K)]` clause to make the gate
  **enforced** (it panics, blocking deploy, when confidence < K). The default is
  a documentary `@[verify(score >= N)]` pass-through.
- `fn redteam_check() -> bool` — an adversarial check. When present, the goal
  deploys only if the verify gate **and** `redteam_check()` pass.
- `fn feasible(x: i64) -> bool` — a per-candidate feasibility predicate over
  `try_variant`'s parameter (the prose **Constraints**, encoded). When present,
  the harness drives `goal_run_constrained` instead of `goal_run`, so the search
  only accepts feasible candidates — a **hard gate during the search**, not a soft
  penalty. The constraint is enforced, not merely documented.

### Prelude auto-bundling

A small set of scoring helpers (`normalize_score`, `score_to_confidence`,
`length_score`, `weighted2`, `budget_ok`) is auto-included **iff your code
references them** (and doesn't define its own). Just call them; the compiler
pulls in exactly the ones you use.

## Gates and exit codes

`axon goal` exits:
- `0` — ran, deploy gate (and redteam, if any) passed.
- `101` — an enforced `@[verify(confidence …)]` gate blocked deploy (panic).
- `1` — `redteam_check()` failed (deploy blocked), or a generated-`.ax` error.
- `2` — the goal file or generated `.ax` failed to parse / type-check.

## The demos

| Goal | Shows | Outcome (no key) |
|---|---|---|
| `optimize-goal.md` | pure goal-directed optimization to a global max | deploys (exit 0) |
| `constrained-goal.md` | constrained search — a `feasible` predicate holds the optimizer inside the valid range (`goal_run_constrained`) | deploys (exit 0) |
| `sandboxed-goal.md` | a prose `@[contained(...)]` effect surface stamped on the loop — the compiler enforces the declared capability boundary (the value wedge) | deploys (exit 0) |
| `compose-goal.md` | composing auto-bundled prelude helpers | deploys (exit 0) |
| `verified-goal.md` | enforced confidence gate blocking an under-target agent | **blocks** (exit 101) |
| `redteam-goal.md` | adversarial gate blocking a high-scoring but unsafe agent | **blocks** (exit 1) |
| `hello-goal.md` | the canonical summarizer (uses `ai_complete`) | deploys under `AXON_AI_MOCK=1` |
| `flagship-goal.md` | the **full stack**: LLM + prelude scoring + search + budget + confidence gate + redteam, all passing | deploys under `AXON_AI_MOCK=1` (exit 0) |

The `optimize` / `verified` / `redteam` / `compose` demos run with **no API key**
(pure objectives); `hello-goal` and `flagship-goal` need `AXON_AI_MOCK=1` (or a
real key) because they call `ai_complete`.

## The capability sandbox, from prose (the value wedge)

`sandboxed-goal.md` and its adversarial twin `sandboxed-goal-evil.md` are the
same agent, written in prose, with the same declared effect surface
(`@[contained(net: ["api.anthropic.com"], exec: none)]`). The honest one deploys;
the evil one's scorer secretly does `write_file(...)` to exfiltrate the input —
and the compiler **refuses it**, because the prose surface is stamped on the loop
and enforced transitively:

```bash
AXON_AI_MOCK=1 axon goal examples/goals/sandboxed-goal.md       # → deploys (exit 0)
AXON_AI_MOCK=1 axon goal examples/goals/sandboxed-goal-evil.md  # → E1001, refused (exit 2)
```

This is the wedge on the prose→code path: an AI-authored-from-prose agent the
compiler proves cannot widen its grant. (`*-evil.md` is excluded from the
allow-case sweep; `prose_sandboxed_evil_goal_is_refused` gates the refusal.)
