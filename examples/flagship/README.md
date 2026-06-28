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

**One command, from a fresh clone** — `./flagship` builds any missing binaries
(no LLVM needed), then runs the four-layer demo and the Docker+seccomp foil:

```bash
./flagship          # interactive (pauses between sections)
./flagship --ci     # non-interactive (no pauses, no real KVM) — CI / screencast
./flagship docker   # just the Docker+seccomp foil
./flagship llm      # just the real-LLM prompt-injection segment
./flagship cve      # just the CVE-Bench exemplars (real CVEs refused)
./flagship threat   # print the threat model
```

**Real CVEs, not just synthetic demos.** [`cve/`](cve/) triages all 40 critical CVEs in
[CVE-Bench](../../../security/pentest/cve-bench) (ICML 2025) against Axon's capability
model — **≈ half are a bug class whose critical impact Axon refuses by construction**
(see [`cve/TRIAGE.md`](cve/TRIAGE.md)). The AI-stack CVEs cluster in the STRONG column.
First built exemplar: **CVE-2024-34359** (llama-cpp-python Jinja2 SSTI → RCE) — the
exploit's RCE / file-access / outbound payload is refused with 3× E1001 at compile time.

The `llm` segment is the visceral version: a **real model** (claude-sonnet),
prompt-injected, genuinely wrote three exfiltration channels — `read_file`,
`ai_complete`, `exec` — and the compiler refused all three (E1001). The model's
verbatim output is captured in `agent_task_llm_generated.ax` (no API key needed to
replay); `real_llm.sh --live` regenerates against a real endpoint if you set
`ANTHROPIC_API_KEY`.

**Evaluating Axon skeptically?** Start with **[EVALUATE.md](EVALUATE.md)** — the
one claim, the one command, where to be suspicious, and how to try to break it (≈5 min).
Then **[THREAT_MODEL.md](THREAT_MODEL.md)** for what this stops, what it does **not**,
and the TCB you have to trust.

<details><summary>Or run the pieces directly</summary>

```bash
# Build the interpreter CLI once (sub-second, no LLVM):
cargo build -p axon-core --no-default-features --bin axon
# Optional: axon-os (R27 kill-switch) + axon-vm (R26 attestation):
cargo build -p axon-os && cargo build -p axon-vm

examples/flagship/demo.sh            # four-layer guided demo
examples/flagship/compare_docker.sh  # serious foil: Docker + hand-written seccomp
examples/flagship/compare_python.sh  # naked-Python foil (Layer 1 highlight)
examples/flagship/run.sh             # classic walkthrough (good→evil→subtle→thief→python)
#   DEMO_NOPAUSE=1 — skip pauses;  AXON_CI_NO_KVM=1 — software-TPM mock in Layer 4
```
</details>

## The files

| File | Role |
|---|---|
| `agent_task.ax` | The **good** agent. Declared `@[contained(fs:[], net:[], exec:none)]`; also carries `@[goal]` + `@[verify]` annotations. Compiles and runs cleanly. |
| `agent_task_evil.ax` | The **evil** agent. Same caps, tries to exfiltrate (`read_file`, `ai_complete`, `exec`) — compiler refuses with 3× E1001. |
| `agent_task_subtle.ax` | The **subtle** agent. *Granted* `write("./out/")`, tries to write out-of-lane via a dynamic path — E1001. |
| `agent_task_secrets.ax` | The **credential thief**. *Granted* net for a real LLM task, reads `ANTHROPIC_API_KEY` from env to smuggle it — E1001 on the env read. |
| `foil_python.py` | The same escapes in Python — all run, because the "sandbox" is a comment. |
| `../../flagship` | **One-command runner** (repo root) — builds missing binaries, runs the demo + foil. |
| `demo.sh` | Four-layer demonstration (compile proof + runtime + kill-switch + attestation). |
| `compare_python.sh` | Side-by-side Python vs Axon: all 3 escapes run in Python, refused in Axon. |
| `compare_docker.sh` | **The serious foil** — Docker + a hand-written seccomp profile blocks 1 of 3 escapes; Axon blocks all 3 at compile time. Shows provenance/timing/granularity. |
| `seccomp-agent.json` | The hand-written seccomp profile used by `compare_docker.sh` (with comments on what seccomp structurally *cannot* express). |
| `docker_probe.py` | Reports the OS-level allow/block verdict of each escape inside a container. |
| `real_llm.sh` | **Real LLM, prompt-injected** — a real model's exfil code, refused by the compiler. `--live` to regenerate against an endpoint. |
| `agent_task_llm_generated.ax` | Verbatim output of claude-sonnet under the injection scenario — 3 exfil channels, all refused (E1001). |
| `real_llm.prompt.txt` | The exact prompt + injection that produced it (auditable / reproducible). |
| `EVALUATE.md` | **5-minute skeptic's guide** — the claim, the one command, where to be suspicious, how to break it. Hand this to a security/AI-safety reviewer. |
| `THREAT_MODEL.md` | Attacker model, what's stopped, **what is not**, and the TCB. Read this before believing the demo. |
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
