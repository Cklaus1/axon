# AGENTS.md

Guidance for AI coding agents and LLMs working in this repository.

## Writing Axon (`.ax`) code

**Read [`spec/axon-for-llms.md`](spec/axon-for-llms.md) first.** It is the concise,
task-oriented guide to the language: syntax, the `Option`/`Result` idiom, the builtin
surface, the common gotchas (borrow with `&[T]`, no operator-led line breaks, exhaustive
`match`), and the AI/capability features (`@[goal]`, `@[adaptive]`, `@[verify]`,
`@[contained]`, refinement `where`, effect rows).

Then, as needed:
- [`spec/stdlib.md`](spec/stdlib.md) — every builtin with signature and semantics
- [`spec/language-tour.md`](spec/language-tour.md) — full feature walkthrough
- [`spec/grammar.ebnf`](spec/grammar.ebnf) — exact grammar
- [`examples/asi/`](examples/asi/) — end-to-end AI workflows; the language's intended public face

## Running code

Execution is **interpreter-first** — do not reach for `axon build` (native LLVM) during
normal development.

```bash
cargo build -p axon-core --no-default-features --bin axon   # build the interpreter CLI (sub-second)
axon run file.ax      # type-check + interpret
axon check file.ax    # type-check only
axon test file.ax     # run @[test] functions
```

The tree-walking interpreter (`crates/axon-core/src/interp.rs`) is the **reference
execution semantics**. Any new language feature must be implemented there; native codegen
follows the interpreter, never the reverse.

## Working on the compiler

See [`CLAUDE.md`](CLAUDE.md) for the compiler-project conventions (pipeline, crate layout,
how to add a builtin, phase status) and [`ROADMAP.md`](ROADMAP.md) for forward planning.

Key constraint: **do not enable the `codegen` + `serde-json` features together** — it
reintroduces a build stall (see `BUILD_RESOLVED.md`).
