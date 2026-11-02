# axon-os — the Axon OS containment supervisor (R21)

Run an untrusted Axon program under a **declared, proven capability + budget
bound**: the supervisor proves the program can't exceed its grant *before it
runs*, enforces it, writes a **tamper-evident, replayable** audit record, and
**fails closed** on any over-reach.

This is the back half of the Vision-OS v1 loop (`VISION_OS.md`): *run → prove
the bound → enforce → audit → replay.* Spec: `governance/specs/R21-axon-os-supervisor.md`.

## Quickstart

```bash
# Build the supervisor and the interpreter it drives.
cargo build -p axon-os --bin axon-os
cargo build -p axon-core --no-default-features --bin axon
export AXON_BIN="$PWD/target/debug/axon"

# 1. See, in plain English, exactly what a job is allowed to do (no execution):
target/debug/axon-os explain examples/jobs/summarize.axjob

# 2. Run it under that proven bound; get an audited, replayable record:
target/debug/axon-os run examples/jobs/summarize.axjob --run-id demo --out ./runs

# 3. Confirm the record hasn't been tampered with:
target/debug/axon-os verify ./runs/demo.json

# 4. Reproduce the run deterministically and prove it matches:
target/debug/axon-os replay demo --store ./runs

# 5. Watch the supervisor REFUSE an over-reaching job (exit 8, audited):
target/debug/axon-os run examples/jobs/overreach.axjob --out ./runs ; echo "exit=$?"
```

## What a job looks like

A `.axjob` manifest declares the program plus the exact capability grant:

```toml
program = "summarize.ax"
intent  = "Summarize ./data/report.txt into ./out/summary.txt; no network."
seed    = 42
[grant]
fs_read   = ["./data/"]
fs_write  = ["./out/"]
net       = []
exec      = "none"
max_label = "internal"
[grant.budget]
calls = 100
tokens = 50000
cost_micro = 1000000
```

`overreach.axjob` runs the *same kind of* job but its program also tries to reach
the network while the grant withholds `net` — so the static gate **denies it
before it runs** (exit 8). That is the containment thesis in one command.

## Approval tokens

A job's `.approval` sibling (written by `axon-intent approve`) is verified
before anything runs: the program, grant and decision must match, and the
token digest must recompute (R22 spec §3.5). With `AXON_APPROVAL_KEY` set only
a keyed `axmac1:` token is accepted; without it only an unkeyed `axtok3:` one,
reported as "integrity only". An `axon-approval/3` token also binds the
program's import closure: `axon-os run` hands its `imports_digest` to the
interpreter child as `AXON_REQUIRE_IMPORTS_DIGEST` (hermetic and non-hermetic
jobs alike), and the interpreter refuses (exit 8, nothing executed) a module
rewritten since approval. A legacy `axon-approval/1` token is refused under a
key and, keyless, accepted only for a program that imports nothing; otherwise
re-approve. An unknown token schema is refused.

## How a run's end is decided

The program owns its stdout and stderr, and every code the interpreter uses for
a fault (2-9, 101) is also a legal `exit(n)`, so neither can say how a run
ended. Every interpreter run `axon-os` starts (`axon-os run` jobs and the
`run_sandboxed_capture` the genviab harness grades through) gets a pipe on fd 3
and a per-run nonce from the OS random source (`AXON_OUTCOME_FD`,
`AXON_OUTCOME_NONCE`). `axon run` removes both from its environment, makes the
fd close-on-exec, and before it exits writes exactly one `axon-outcome/1` JSON
line there: `kind` `exit` (the program returned or called `exit(n)`), `fault`
(the interpreter stopped it: sandbox, verify, halted, ai-policy, refine,
goal-budget, panic, steps, wall, replay-divergence) or `compile` (nothing ran),
with the code, a class and the reason. The supervisor decides from its own
kills first (the deadline is `TimedOut`; a termination signal it received,
the kill-file latch), then from that record. No record, two, a malformed one,
another run's nonce, a code that is not the exit status, or death by a signal
nobody here sent is a sandbox refusal (fail closed). A program printing
`axon: sandbox violation: ...` and exiting 8 is graded as its own exit 8.

Limit: when a grant allows `exec`, a process the program spawns runs as the
same uid and could open `/proc/<interpreter>/fd/3` and read its environment,
so it could forge a record. genviab therefore refuses a once task whose
ceiling grants `exec`.

## Exit codes

`0` ok · `2` usage/malformed · `6` refinement · `7` budget · `8`
capability/denied · `9` record tamper / replay divergence.
