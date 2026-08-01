# Lessons — build-loop over `governance/reviews/2026-07-31-deep-review.md`

Mistake → rule. Appended only on a correction or failure, so it compounds.

---

## L001 — Never put a long-running job's input files in the session scratchpad

**Mistake:** Step 1's 9 triage agents read their finding batches from
`/tmp/claude-0/<project>/<session-id>/scratchpad/batches/*.md`. The session ended
mid-run; the scratchpad is keyed by session id and was cleared with it. On resume
the workflow relaunched with zero agents cached AND zero input files — the agents
would have read nothing and returned confident verdicts about an empty batch.

**Rule:** Any file a background/subagent job reads as input goes somewhere durable
(repo `.archive/`, or a committed path), never the session scratchpad. The
scratchpad is for outputs this turn consumes, nothing that has to survive a
process boundary. Corollary: after relaunching a resumed workflow, verify its
inputs still exist before assuming the resume is sound.

---

## L002 — A resumed workflow with 0 cached agents is a fresh run, not a resume

**Mistake:** Treated `resumeFromRunId` as if it guaranteed progress. Both stopped
jobs had `journal.jsonl` with zero `{"type":"result"}` lines — every agent was
mid-flight at teardown, so nothing was cached and the "resume" re-ran everything
from scratch at full cost.

**Rule:** Before relaunching, `grep -c '"type":"result"' journal.jsonl`. Zero means
budget for a full re-run and re-verify preconditions (inputs, working tree, branch)
as if starting cold. Non-zero means only the uncached tail re-runs.
---

## L003 — Background workflows do not survive `/compact`; a partial-build baseline is not a baseline

**Mistake (two parts, same root):**

1. Relaunched the 9-agent triage Workflow after it died with the session, then
   `/compact` ended the process and killed it *again*. Two full re-runs paid for,
   zero results. Workflows are process-bound; a long fan-out cannot straddle a
   compaction.
2. Recorded "2 baseline failures" from a run against a stale `target/`. The clean
   full-workspace run has **1**. Treating the partial number as the baseline would
   have permanently widened the gate — every later task would have been allowed to
   leave `wasm_browser_examples_run_identically_via_js_host` broken.

**Rule:** For work that must survive a process boundary, prefer many small
foreground/async `Agent` calls that persist each result to a durable path as it
finishes, over one long `Workflow` whose value only materialises at the end. Cost
of a kill then scales with one agent, not the whole fan-out.

And: a baseline is only valid from a **clean, complete** run. Never adopt gate
numbers from a partial or interrupted run — a too-loose gate is worse than no
gate, because it silently licenses regressions for the rest of the loop.
---

## L004 — `git stash pop` is not the inverse of `git stash push <pathspec>`

**Mistake:** Used `git stash push <one-file>` … `git stash pop` repeatedly to
verify fails-before. On one iteration the pop applied a DIFFERENT, pre-existing
stash (another session's 374-line WIP), leaving 34 conflict markers in
`crates/axon-vm/src/main.rs` — a file this run never touched — and reporting a
conflict on a file I had not stashed.

Nothing was lost: `pop` retains the entry on conflict, so the other session's
work stayed in `stash@{0}`, and restoring the file to HEAD undid only what my
own command had done.

**Rule:** `pop` always takes `stash@{0}`, which is NOT necessarily the entry you
just pushed — any repo with pre-existing stashes can hand you someone else's
work. For the fails-before check, don't use the stash at all:

```
git diff <file> > /tmp/fix.patch     # save
git apply -R /tmp/fix.patch          # revert, run the test, expect FAIL
git apply  /tmp/fix.patch            # restore
```

That is idempotent, names the exact change, and cannot collide with an unrelated
stash. Corollary: `git stash list` before any pop in a shared/long-lived repo,
and treat a conflict in a file you never edited as a signal to STOP and inspect
rather than resolve.
