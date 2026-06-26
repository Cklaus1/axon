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

## Exit codes

`0` ok · `2` usage/malformed · `6` refinement · `7` budget · `8`
capability/denied · `9` record tamper / replay divergence.
