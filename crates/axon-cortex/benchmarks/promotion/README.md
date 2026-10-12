# Promotion: Cortex's defaults change only through a frozen evaluation

`cortex repair`'s default config (ranker, audit, walk, generator model,
transport) is the policy in [`policy/cortex-policy.json`](policy/cortex-policy.json).
It changes only when `promote.py run` decides PROMOTE. That command drives
`axon-loop` (crates/axon-loop) over a frozen plan, using scorecard run
records as evidence. Every decision is appended to
[`decisions.jsonl`](decisions.jsonl).

`crates/axon-cortex/tests/promoted_policy.rs` connects the compiled
defaults to that file. It checks `rank.rs` `DEFAULT_CHAT_MODEL`/`select`,
`audit.rs` `select`, the breadth walk and the transport driver's default
`CORTEX_MODEL` against the policy. It also checks that the policy file's
sha256 is the `policy_after` of the last record in `decisions.jsonl`. So
editing a default, or editing the policy without a record, fails
`cargo test -p axon-cortex`.

## The policy in force: GRANDFATHERED, not promoted

The current defaults were set before this machinery existed:

- ranker `openrouter:anthropic/claude-haiku-5.5` (cb1afcf9, on offline LocRank 60/60)
- audit `openrouter:anthropic/claude-haiku-5.5` (c0584d26, on DecisionBench tamper 0.92)
- walk breadth (fd6a1c57), model `claude-haiku-5-5` over micode (70757c55)

None of them was admitted on held-out repair evidence. They are recorded as
they are: the first record in `decisions.jsonl` is `GRANDFATHERED`, and it
says so. The heldout runs in `../heldout/results/full/` (ranker vs spectrum,
PREREG-heldout PASS) ran with `CORTEX_AUDIT=off`. So they evaluate neither
this policy nor a one-edit neighbour of it. They were also committed before
any promotion plan, and `promote.py` refuses them (shown below).

The code defaults are unchanged. The pending challenge is
[`plans/heldout-ranker-challenge.json`](plans/heldout-ranker-challenge.json),
which compares the incumbent to the candidate
[`candidates/spectrum-ranker.json`](candidates/spectrum-ranker.json). A PASS
demotes the model ranker to spectrum. Anything else keeps it, and from then
on it is backed by evidence.

## How Cortex maps onto axon-loop

| axon-loop | Cortex |
|---|---|
| scope | tenant `axon`, family `cortex-repair` |
| candidate list (`candidates put`) | [`policy/options.json`](policy/options.json): every option id `slot.value` |
| policy (`policy put`) | a cortex-policy file, encoded as a shortlist: the chosen value of each slot first, in slot order, then the rest |
| incumbent-of-record (`pointer baseline` + `transition activate`) | `policy/cortex-policy.json`, reason = cl22 of the last decision record |
| `evo propose` | ONE seeded edit (a swap within a slot) of the incumbent. The discovery episodes are a real dev run from the scorecard history. The decoded candidate must equal the plan's candidate file, or the run refuses |
| frozen plan (`plan register` / `freeze`) | `plans/<id>.json`, committed BEFORE any of its evaluation rows; `analysis_method_ref` = cl22 of the plan, `approval_ref` = cl22 of its commit |
| trials, `intake episode` | one trial per (task, arm) from the two scorecard runs' rows. The verifier is the run_ab.py grader: its key is generated per store, its executable is pinned to sha256(run_ab.py), and the suite is pinned to the corpus sha256 |
| `evl evaluate` | per-arm `verified_pass` asserted equal to the scorecard's recomputed `verified` |
| `admit` | axon-loop's rule (`pass_rate_margin_ppm`, `economic_threshold report_only`: costs are list-price estimates) AND the Cortex rule, evaluated by `scorecard.judge` |
| `pointer transition` (promote) / `revoke` + rollback | PROMOTE moves the pointer. `rollback` revokes the candidate and restores the incumbent |
| `tel summarize` | whole-task usage over the trials |

Cortex rule (in every plan): the candidate has 0 false successes; its
verified count is not significantly lower (paired exact McNemar, alpha 0.05);
and its proposals OR cost per verified fix is strictly lower. PROMOTE requires
both axon-loop ACCEPT and the Cortex rule.

`promote.py run` refuses before touching a store when any of these holds:

- the plan is uncommitted, edited after its commit, or not an ancestor of
  BOTH evaluation rows' commits;
- the incumbent file differs from the last decision's `policy_after`;
- a run's config (ranker, audit, walk, model, transport) contradicts its
  policy;
- a run is not in the scorecard history, or the history fails
  `scorecard.verify`;
- stand-in rows appear in a non-dry-run plan;
- the discovery run is not a dev run.

## Next candidate, step by step

1. Write `candidates/<name>.json`: a one-slot edit of `policy/cortex-policy.json`.
2. Write `plans/<id>.json`, modelled on `heldout-ranker-challenge.json` (rule,
   corpus sha256, incumbent and candidate sha256, `axon_loop` margins), and
   **commit it**. Preregister the paid run as `ab/PREREG*.md` did.
3. Run both arms on the heldout set (`ab/run_ab.py --arm B`, with the
   incumbent's settings and with the candidate's). Do not pass `--ranker`:
   it forces `CORTEX_AUDIT=off`. Instead, set `CORTEX_RANKER` / `CORTEX_AUDIT`
   explicitly. Commit the rows, then `python3 ../scorecard/scorecard.py add
   --set heldout ...` for each.
4. `python3 promote.py run --plan plans/<id>.json --incumbent-run RUN_A
   --candidate-run RUN_B --work /var/tmp/<dir> --record`. With `--record`, a
   PROMOTE copies the candidate over `policy/cortex-policy.json` and appends
   the decision. Then change the code defaults (`rank.rs`, `audit.rs`, driver
   `CORTEX_MODEL`) in the same commit, so `promoted_policy.rs` passes.
5. To undo: `python3 promote.py rollback --work <same dir> --reason TEXT
   --record`. It restores the incumbent and appends ROLLBACK. Then revert the
   code defaults.

`python3 promote.py check` confirms that the policy file is the last
decision's `policy_after`.

## Dry run (zero cost)

`plans/dryrun-*.json` (`dry_run: true`) were committed in 7e434d3d before
their rows. The rows in `dryrun/` (340c5cd5) are `run_ab.py --arm B
--generator oracle|none` over `../heldout/heldout.json`, converted by
`promote.py standin` (model `stand-in:<variant>`). They exercise the flow and
decide nothing about Cortex.

```text
H=/var/tmp/promo-dry; C=../heldout/heldout.json
cfg=$(python3 promote.py standin --rows RAW.jsonl --corpus $C --out dryrun/X.jsonl [--audit SPEC])
python3 ../scorecard/scorecard.py --history $H/h.jsonl add --set heldout --agent cortex \
    --rows dryrun/X.jsonl --corpus $C --config-json "$cfg" --run-id dry-X --notes "DRY RUN"
python3 promote.py run --plan plans/dryrun-spectrum-promote.json \
    --incumbent-run dry-inc-none-default --candidate-run dry-cand-oracle-spectrum \
    --history $H/h.jsonl --work $H/promote            # -> PROMOTE, exit 0
python3 promote.py rollback --work $H/promote --reason "demo"   # -> ROLLBACK, exit 0
python3 promote.py run --plan plans/dryrun-auditoff-reject.json \
    --incumbent-run dry-inc-oracle-default --candidate-run dry-cand-none-auditoff \
    --history $H/r.jsonl --work $H/reject             # -> REJECT, exit 0
```

Without `--record`, nothing in the repository changes. The decision is
written only to `<work>/decision.json`.
