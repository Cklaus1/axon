# R27: Corrigibility & Resource Bounds

Quick-start whose commands the `acc_a3_quickstart_commands_execute` test executes verbatim.

## What R27 provides

Three mechanisms below the layer the model's code can influence:

1. **Kill-switch in the TCB** — supervisor owns the latch; contained code cannot clear/disable it
2. **Resource-acquisition bound** — every budget grant is carved from a Principal via R20-proven `mint`; no escalation
3. **Per-coalition collusion bound** — N instances sharing a lineage root can't exceed a coalition ceiling

See `governance/specs/R27-corrigibility-resource-bounds.md` for the full spec.

## Commands

```bash
# 1. Run a persistent agent under the kill-switch (no persistence grant):
axon-os run examples/agents/persistent.axjob --killable --out ./runs

# 2. Status: check latch state and ledger totals:
axon-os status --store ./runs

# 3. Kill from outside:
axon-os kill run --store ./runs --reason "operator shutdown"

# 4. Verify the run record is intact:
axon-os verify ./runs/run.json

# 5. Run an over-reaching agent (should fail):
axon-os run examples/r27/overreach_agent.axjob --out ./runs

# 6. Run a colluding agent (should hit coalition ceiling):
axon-os run examples/agents/collude.axjob --coalition demo-root --out ./runs
```

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | Completed normally |
| 4 | `HALTED_EXIT_CODE` — kill-switch tripped |
| 8 | Capability/sandbox denied |
| 9 | `RESOURCE_BOUND_EXIT_CODE` — resource acquisition exceeded carved cap |
| 10 | `COALITION_BOUND_EXIT_CODE` — coalition rollup exceeded per-coalition ceiling |
| 11 | Record tamper detected (verify mismatch) |

## Honest scope (§1.3: corrigibility ≠ alignment)

R27 is the off-switch and the resource meter, not a value-loader. It can prove:
- Contained code cannot disable the supervisor latch
- A program cannot acquire compute/budget beyond its carved grant
- A coalition cannot exceed its per-lineage ceiling

It **cannot** prove: that the model wants to be stopped; that stopping is timely; that harms within the granted bounds are acceptable; or that the operator set a wise grant.
