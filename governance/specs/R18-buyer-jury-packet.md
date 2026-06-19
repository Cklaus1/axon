# Axon Provenance Ledger — Buyer Jury Packet (R18 Spike)
*One page. For three external judges: coding-agent-platform PM / DevEx eng lead / AI-native CTO.*

---

## What it is and the gap it fills

The Axon provenance ledger is a causal, replayable decision memory that links agent reasoning sessions to the code diffs they produced, the metrics those diffs moved, and the knowledge-state that existed at decision time — queryable as a point-in-time snapshot across git, agent transcripts, and outcome signals. The gap: incumbents log events, not decisions. Langfuse and Helicone capture LLM rationale but have no link to the downstream diff or metric outcome. DX and LinearB track diffs and PR flow but carry no agent rationale or outcome-causality. mem0 and Letta store per-agent working memory, not org-wide causal decision provenance. None do `decision → rationale → diff → metric-outcome`, queryable as-of, in one place.

---

## The Demo — `why ff5d42b6`

This is a real answer reconstructed from the trainloop codebase.

| Layer | What you see |
|---|---|
| **Commit** | `ff5d42b6` — "judge bake-off findings + per-check partial-credit exec re-grade" |
| **Inferred agent session** | `c9fd4f13` — the Claude Code session that authored it, edge derived from timestamp + file-overlap **without the Co-Authored-By trailer** (precision 1.00 / recall 0.52 on 200-commit ground truth) |
| **Consequential outcome** | `metrics/judge-bakeoff/exec-grade-report.json` — a real metric move, not CI status |
| **As-of** | 2026-06-17 01:49 |

**What it took to answer this by hand:** git has no session link, no rationale, and no outcome-causality. Reconstructing the same chain manually required opening the git log, grepping 13 separate transcript files, and correlating the metric file — three sources, manual join, no tooling support.

---

## 4 Pre-Registered WTP Questions (ask blind, unprimed, after the demo)

Ask each question, then stop and listen. Do not suggest answers or price points first.

**Q1 — Materiality.** "Before I tell you how this was assembled — walk me through how you would find out today why a specific agent commit moved a metric the way it did. What would you open, and how long would it take?"
*(Listen for: how many sources they name, whether they say "I couldn't" or "I'd ask the person who wrote it.")*

**Q2 — Non-triviality vs tools you already have.** "You probably already use something for LLM tracing and something for PR analytics. What would those tools tell you about this commit that we just showed? What's the part they wouldn't tell you?"
*(Listen for: whether they identify the rationale-to-outcome causal link as the missing piece, or whether they say their existing stack covers it.)*

**Q3 — Willingness to pay and price band.** "If this were available today as a product, what would you pay for it — and what model would make sense for you: per seat, per ingested event, or an OEM into your platform?" Price-band hypotheses to probe if they stall: per-seat ($20–80/mo/dev), per-ingested-event ($0.001–0.01/event), platform-OEM (rev-share or flat license). Do not anchor first.
*(Listen for: a concrete number or a concrete rejection with a reason.)*

**Q4 — Compliance deployability.** "Would your security or legal team permit ingesting agent transcripts, source code diffs, and metric outcomes into a single append-only store that you don't control? What would they need to see first?"
*(Listen for: hard no / conditional / easy yes. A hard no here kills the enterprise beachhead regardless of the technical result.)*

---

## Hard-negative reminder for the founder grading the session

**If the judge's most enthusiastic reaction is to the timeline view — "this is a nice visual," "I love seeing everything in one place" — that is a FAIL.** The bar is an answer the judge could not get before, not a prettier interface over information they already had. A timeline-impressed judge is not a buyer; they are a product-feedback respondent. Score only the Q3 response.
