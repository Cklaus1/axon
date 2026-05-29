# Modular Axon — libraries and imports

Axon programs are modular: a file can be a **library** (no `main`) that other
programs import. This works through the normal `axon` pipeline — module
resolution runs during type-checking, and the interpreter executes the merged
program.

- `scorelib.ax` — an importable library (no `main`, just functions + `@[test]`s).
- `agent.ax` — declares `mod scorelib` and `use scorelib.{weighted, approve}`,
  then uses them.

Modules are resolved as `path/segments.ax` under each `AXON_PATH` directory
(so `mod scorelib` → `scorelib.ax`):

```bash
# run the library's own tests:
axon test examples/modular/scorelib.ax

# run a program that imports it (AXON_PATH tells the resolver where to look):
AXON_PATH=examples/modular axon run   examples/modular/agent.ax
AXON_PATH=examples/modular axon check examples/modular/agent.ax
```

This is the basis for a real Axon standard library: the Tier-1 primitives in
`examples/stdlib/` can be refactored into `main`-free importable modules that
goals and agents `use` instead of inlining.
