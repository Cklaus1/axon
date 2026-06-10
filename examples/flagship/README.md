# Flagship demo — AI-written code, sandboxed by the compiler

**The pitch in one sentence:** Axon is the language where AI-written code is
sandboxed *by construction* — the compiler refuses any capability you didn't
grant, before the code can run even once.

This is the answer to the hardest problem in shipping AI agents: how do you let
a model write and run code without it doing something catastrophic? Every other
runtime answers with *trust* (a policy, a code review, a Docker flag you hope
holds). Axon answers with a *proof*.

## Run it

```bash
# Build the interpreter CLI once (sub-second, no LLVM):
cargo build -p axon-core --no-default-features --bin axon

# The guided demo (good → evil → subtle → credential-thief → python → the point):
examples/flagship/run.sh
#   (set DEMO_NOPAUSE=1 to run without the pauses)
```

## The files

| File | Role |
|---|---|
| `agent_task.ax` | The **good** agent. Same task, stays in its sandbox → compiles + runs. |
| `agent_task_evil.ax` | The **evil** agent. Same caps, tries to exfiltrate → compiler refuses (3× E1001). |
| `agent_task_subtle.ax` | The **subtle** agent. *Granted* `write("./out/")`, tries to write out-of-lane via a dynamic path → compiler refuses (E1001). A real grant can't be abused beyond its scope. |
| `agent_task_secrets.ax` | The **credential thief**. *Granted* `net: ["api.anthropic.com"]` for its real task, it reads `ANTHROPIC_API_KEY` from the host environment to smuggle out via that lane → compiler refuses the env read (E1001). A granted net lane lets you *talk* to the model, not loot the host. |
| `foil_python.py` | The **same escapes in Python** — the "sandbox" is a comment; every one runs (incl. reading the env credential). |
| `run.sh` | Scripts the narrative: good → evil-refused → subtle-refused → thief-refused → python-escapes → the point. |

## What you see

- **Good agent:** `axon check` exit 0, `axon run` prints scores. It declared
  `@[contained(fs: [], net: [], exec: none)]` and stayed inside it.
- **Evil agent:** the identical task plus `read_file("/etc/passwd")`,
  `ai_complete(...)`, and `exec("curl", ...)`. `axon check` emits **three
  `E1001` errors** (fs-read, net, exec) and exits **2**. The data is never read,
  no packet is sent, curl never runs — the escape is impossible by construction.
- **Credential thief:** granted `net: ["api.anthropic.com"]` for a real summary
  task, it reads `ANTHROPIC_API_KEY` from the environment and smuggles it through
  that allowed call. `axon check` emits **one `E1001`** — on the *env read*, not
  the network call. The net lane stays open for the legitimate work; the
  environment is an ungrantable ambient secret channel, so harvesting the host's
  credential is refused. A granted capability can't be turned into host looting.
- **Python foil:** the same logic with a `# @contained(...)` *comment*. It reads
  `/etc/passwd`, attempts the network POST, spawns a process, **and reads the host
  environment** — all permitted, because nothing enforces the comment.

## The boundary is the product

`@[contained(...)]` is enforced by `axon check` (error code `E1001`) as a
compile-time pass. It is:

- **Transitive** — a helper function can't launder a capability past the
  boundary (the import-edge + call-following walker follows the call graph).
- **Path-traversal-safe** — `../` escapes out of an allowed prefix are denied.
- **Fail-closed on dynamic targets** — a string-interpolated or computed argument
  (`ai_complete("leak {x}")`, `read_file("/etc/{p}")`) does **not** slip past an
  empty allowlist: zero capability means the call is denied regardless of how its
  argument is built. (A *non-empty* allowlist with a dynamic target is deferred to
  runtime — the function already holds the capability; only the specific target
  awaits the Phase-9 runtime `Sandbox<P>`.)

The same capability machinery underpins the rest of Axon's AI-safety surface:

- **`@[corrigible]`** — a one-way kill-switch; once tripped, a corrigible
  function never runs again (graceful wind-down, or fail-closed exit 4). See
  `../asi/corrigible.ax`.
- **AI-call provenance** — every `ai_complete`/`ai_extract_*` call is logged with
  its model/params/prompt hashes, so agent runs are auditable and replayable.
- **`@[sensitive]`** — PII-tagged types are taint-checked and cannot flow into an
  external AI call. See `../sensitive_data.ax`.
