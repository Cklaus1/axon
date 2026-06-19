# R18 Slice-0 Spike — Results (autonomous H1 + competitive scan)

**Spec:** `R18-slice0-spike.md` · **Source A:** `trainloop` (founder-supplied, 2026-06-19) · **Run:** ASI build-loop, main thread
**Verdict so far:** **H1 PASS** (measured) · **H2: author-non-triviality PASS; willingness-to-pay PENDING** (needs the 3-judge buyer jury)

> Autonomous parts of the reshaped spike, run against **trainloop** (a non-Axon, AI-coding-agent-built ML
> project — chosen for its real metric outcomes). The throwaway importer lives in `/tmp` (per spike scope);
> this doc is the recorded evidence.

---

## H1 — does the provenance schema generalize to a real external source? **PASS (measured)**

A one-shot importer mapped trainloop's git history + 13 Claude-Code transcripts into the target
`(principal, effect, causal-parent, hash, ts)` schema, **inferring agent→commit edges with the
`Co-Authored-By` trailer deliberately ignored** (per spike §4), then scored against that trailer as
ground truth.

| Quantity | Value |
|---|---|
| Commits analyzed | 200 (183 agent / 17 human by `Co-Authored-By` ground truth) |
| Transcript edit-events extracted | 485 |
| Edge-inference (timestamp + file-overlap, trailer ignored) | TP=96, FP=0, TN=17, FN=87 |
| **Precision** | **1.00** — every inferred edge is correct; `why` answers never fabricate a wrong agent link |
| **Recall** | **0.52** — catches ~half of agent commits; misses are commits whose session isn't in the 13 transcripts or fell outside the 6 h / file-overlap window (improvable: more transcripts, wider window, content matching) |

**Interpretation:** the schema generalizes. Precision 1.00 is the load-bearing property (trustworthy, no
fabrication). Recall 0.52 is honest partial coverage, not a soundness problem.

### Real decision → diff → consequential-outcome chain (the thing `git` alone can't give)

```
why ff5d42b6  →  "judge bake-off findings + per-check partial-credit exec re-grade"
   inferred agent session : c9fd4f13   (edge derived WITHOUT Co-Authored-By)
   consequential outcome  : metrics/judge-bakeoff/exec-grade-report.json  (a real metric, NOT CI status)
   as-of                  : 2026-06-17 01:49
```

**H2 author-non-triviality (the leg the author may grade): PASS.** Reconstructing session→commit→metric by
hand needs git (no session link, no rationale) + grepping 13 transcripts + correlating the metric file —
i.e. ≥3 sources + manual correlation. The ledger answers it in one query. It is "an answer you couldn't get
before," not a timeline view (clears the spike's hard-negative bar).

## H2 — willingness-to-pay: **PENDING** (needs real external judges)

Per the reshaped spike (§5/§8), WTP must be graded by **≥3 external target-population judges, not the
author** — else it is demoted, not self-graded. The autonomous run cannot supply real buyers. **Judge role
types (founder to fill with real contacts):**

1. **Coding-agent-platform PM** (Cursor / Cognition / Factory-class) — owns the agent-memory/history surface; the *build-vs-buy* decider.
2. **Platform / DevEx eng lead** at an AI-heavy eng org — runs many agents internally; feels the daily "why did the agent do X / what changed / rollback" pain (the *user*).
3. **VP Eng / CTO at an AI-native startup** — owns budget + audit/governance (the *economic buyer*).

## Competitive scan (the H2 non-triviality floor)

Agent-memory infra is a **funded but adjacent** category; the causal-decision-provenance gap the ledger
targets is **real and underserved** (2025–26: ~$2.9B agentic-AI funding, "agent memory systems" a named
sub-category, agent-OS/memory/context taking first financings — but *decision-provenance* is not yet a
named category):

| Category | Players | What they do | The gap |
|---|---|---|---|
| LLM-trace / observability | Langfuse, Helicone, LangSmith, Braintrust, W&B Weave | trace LLM/agent runs — have the *rationale* | no causal link to the *diff* + downstream *metric outcome* |
| Agent memory | mem0, Letta/MemGPT | per-agent *working* memory | not org-wide decision provenance |
| Coding agents | Cursor (~$50B), Cognition, Factory, Augment | own the agent | not the cross-system decision ledger |
| Eng analytics | DX, LinearB, Swarmia | diffs / PR flow | no agent rationale or outcome-causality |

**None do causal `decision → rationale → diff → metric-outcome`, queryable as-of, across systems** — the
ledger's exact claim. This raises the non-triviality floor (the answer must beat all of the above), and the
trainloop `why` chain above clears it.

## Decision-tree position (spike §7)

`H1 pass` + `H2 author-non-triviality pass` → **premise survived first contact**. Per §7, this does **NOT**
authorize a PRD/build — it authorizes running the §12-Q1 **wedge comparison** (defensibility vs a
fast-follower, compliance-deployability §R18 §5.6, opportunity cost vs the sandbox wedge) **once the external
WTP jury returns**. The single remaining gate is the 3 real buyer judges.
