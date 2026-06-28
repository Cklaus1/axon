# Flagship demo — AI-written code, sandboxed by the compiler

**The pitch in one sentence:** Axon is the language where AI-written code is
sandboxed *by construction* — the compiler refuses any capability you didn't
grant, before the code can run even once.

This is the answer to the hardest problem in shipping AI agents: how do you let
a model write and run code without it doing something catastrophic? Every other
runtime answers with *trust* (a policy, a code review, a Docker flag you hope
holds). Axon answers with a *proof* — backed by four independent safety layers.

## Four safety layers

```
Layer 1  @[contained]     Compile-time capability proof (axon check)
Layer 2  Runtime sandbox  @[contained] re-enforced at every builtin call site
Layer 3  R27 Kill-switch  Operator halts any agent in < 1s (axon-os kill)
Layer 4  R26 Attestation  Kernel ELF measured + signed before the agent boots
```

Each layer is independent. A bug in any one layer leaves three more standing.

## Run the demo

```bash
# Build the interpreter CLI once (sub-second, no LLVM):
cargo build -p axon-core --no-default-features --bin axon

# Optional: build axon-os (R27 kill-switch) and axon-vm (R26 attestation):
cargo build -p axon-os
cargo build -p axon-vm

# Four-layer guided demo:
examples/flagship/demo.sh
#   DEMO_NOPAUSE=1   — run without pauses (CI/automation)
#   AXON_CI_NO_KVM=1 — skip real KVM in Layer 4 (use software-TPM mock)

# Python vs Axon contrast (Layer 1 highlight):
examples/flagship/compare_python.sh

# Classic guided walkthrough (good -> evil -> subtle -> credential-thief -> python):
examples/flagship/run.sh
#   DEMO_NOPAUSE=1 to skip pauses
```

## The files

| File | Role |
|---|---|
| `agent_task.ax` | The **good** agent. Declared `@[contained(fs:[], net:[], exec:none)]`; also carries `@[goal]` + `@[verify]` annotations. Compiles and runs cleanly. |
| `agent_task_evil.ax` | The **evil** agent. Same caps, tries to exfiltrate (`read_file`, `ai_complete`, `exec`) — compiler refuses with 3× E1001. |
| `agent_task_subtle.ax` | The **subtle** agent. *Granted* `write("./out/")`, tries to write out-of-lane via a dynamic path — E1001. |
| `agent_task_secrets.ax` | The **credential thief**. *Granted* net for a real LLM task, reads `ANTHROPIC_API_KEY` from env to smuggle it — E1001 on the env read. |
| `foil_python.py` | The same escapes in Python — all run, because the "sandbox" is a comment. |
| `demo.sh` | **NEW** — four-layer demonstration (compile proof + runtime + kill-switch + attestation). |
| `compare_python.sh` | **NEW** — side-by-side Python vs Axon: shows all 3 escape attempts run in Python and are refused in Axon. |
| `run.sh` | Classic guided walkthrough: good → evil-refused → subtle-refused → thief-refused → python-escapes → the point. |

## Layer 1: @[contained] — compile-time proof

`@[contained(...)]` is enforced by `axon check` (error code `E1001`) before the
program can execute even once. It is:

- **Transitive** — a helper function can't launder a capability past the boundary.
- **Path-traversal-safe** — `../` escapes out of an allowed prefix are denied.
- **Fail-closed on dynamic targets** — a computed argument doesn't slip past an empty allowlist.

Also demonstrated in `agent_task.ax`:
- **`@[verify(value >= 0)]`** — postcondition on `score()` enforced at every call site (Phase 5 refinement types, exit 6 on violation).
- **`@[goal("maximize quality")]`** — marks the agent as goal-directed (Phase 7 / R12).

## Layer 2: Runtime sandbox

The `@[contained]` boundary is re-enforced at every builtin call site by the
interpreter. Even if the static pass were bypassed, the runtime refuses:

```
axon run examples/flagship/agent_task.ax    # exit 0 (good agent)
axon run examples/flagship/agent_task_evil.ax   # exit 2 (refused at check)
```

## Layer 3: R27 Kill-switch (axon-os)

R27 ships a one-way kill-latch. Once tripped by the operator, the agent stops
within one polling cycle (~100ms) and exits 4 (`HALTED`). No code inside the
agent can clear the latch.

```bash
# Run an agent with kill-switch enabled:
axon-os run examples/r27/killable_agent.axjob --killable --run-id my-run --out ./runs

# Trip the latch from another terminal:
axon-os kill my-run --store ./runs --reason "operator shutdown"

# Inspect the tamper-evident audit record (R21 section 3.4):
axon-os verify ./runs/my-run.json
```

The run record (`runs/my-run.json`) is a SHA-256 hash-chained audit log: every
capability-bearing action is an `AuditEvent` with a `prev_hash` chain. Any
mutation, drop, or reorder breaks every subsequent hash and the chain head.

> **TODO R28:** `crates/axon-audit` (in progress) will add `axon-os audit verify
> --ledger PATH` for a standalone immutable capability ledger with append-only
> semantics. Until it ships, `axon-os verify <record.json>` is the audit gate.

Spec: `governance/specs/R27-kill-switch.md` (if present) / `crates/axon-os/src/`

## Layer 4: R26 Attestation (axon-vm)

R26 measures the kernel ELF image (SHA-256 + `axtcb1` chain root) and produces
a software-TPM attestation report before any agent boots. A tampered kernel
produces a different digest — the attestation is refused (exit 10).

```bash
# Measure the kernel and produce an attestation report:
AXON_CI_NO_KVM=1 axon-vm attest --kernel dist/guest/vmlinuz

# With a real kernel path and an expected digest to verify against:
axon-vm attest --kernel dist/guest/vmlinuz \
    --verify-digest <expected-hex> \
    --run examples/flagship/agent_task.ax
```

`AXON_CI_NO_KVM=1` uses a software-TPM mock (no hardware required). The caveat
`substrate: qemu-swtpm (stand-in — no memory encryption)` is printed to stderr —
an honesty check mandated by R26 §8. Use `--features hw-attest` for SEV-SNP/TDX.

Spec: `crates/axon-attest/src/lib.rs` / `crates/axon-vm/src/main.rs`

## The boundary is the product

`@[contained(...)]` is the visible surface of a much deeper stack:

- **`@[corrigible]`** — one-way kill-switch at the language level (see `../asi/corrigible.ax`).
- **AI-call provenance** — every `ai_complete` call logged with model/prompt hashes (replayable with `AXON_AI_REPLAY`).
- **`@[sensitive]`** — PII-tagged types taint-checked, cannot flow into external AI calls.
- **Effect rows** — `fn f() -> T | {IO, Net}` tracks effects transitively; subsumption checker (E1310) prevents laundering.
- **Refinement types** — `@[verify(value >= 0)]`, `T where _>=0` with static SMT discharge when provable.
