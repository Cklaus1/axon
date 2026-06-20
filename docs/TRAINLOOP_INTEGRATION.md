# axon-signal × MegaBrain/Trainloop Integration

## The flywheel

```
┌─────────────────────────────────────────────────────────────────┐
│                                                                   │
│   Claude Code session                                             │
│        │                                                          │
│        ├──► Trainloop: records raw (prompt, response) pairs       │
│        │                                                          │
│        └──► axon-ledger: records (goal, files, commits)           │
│                  │                                                │
│                  ▼                                                │
│           axon-signal: scores session effectiveness               │
│                  │                                                │
│                  ▼                                                │
│   Filter: score > 75 AND linked_commits > 0                       │
│                  │                                                │
│                  ▼                                                │
│   Training corpus: prompts+responses from HIGH-SIGNAL sessions    │
│                  │                                                │
│                  ▼                                                │
│   LoRA / fine-tune: model trained on what actually worked        │
│                  │                                                │
│                  ▼                                                │
│   Better AI coding sessions → better signal scores               │
│        └───────────────────────────────────────────┘ (loop)      │
│                                                                   │
└─────────────────────────────────────────────────────────────────┘
```

**The key insight:** Trainloop gives you ALL the interaction data but doesn't know which interactions were *good*. axon-signal is the quality filter. Together they create a training corpus selected by real-world outcome — not by human raters, not by model self-evaluation, but by whether the session actually shipped code.

This is **outcome-supervised training data** — a new category.

---

## What Trainloop captures

Trainloop records Claude Code session data as (system_prompt, user_message, assistant_response) tuples. In a coding session this means:

- The system prompt (Claude Code's persona + tool descriptions)
- Every user turn (engineer's messages, tool results)
- Every assistant turn (Claude's responses, tool calls)
- Timestamps, model, token counts

Without signal, you can't distinguish:
- A 200-turn session that produced nothing (vague goal, scope drift)
- A 15-turn session that shipped a critical security fix

Both look like "Claude Code interaction data." Only one should be in your training corpus.

---

## What axon-signal adds

Signal scores each session and tags the Trainloop records:

```json
{
  "session_id": "c8d17fba",
  "signal_score": 91,
  "goal": "fix JWT clock skew for iOS — reproduces when device time > 5min off",
  "turns": 12,
  "commits": 3,
  "files_touched": ["auth/jwt.go", "auth/jwt_test.go"],
  "rework_triggered": false,
  "outcome": "deployed, error rate dropped from 2.3% to 0.1%",
  "training_tier": "positive"
}
```

### Training tier classification

| Tier | Criteria | Use in training |
|---|---|---|
| **Positive (gold)** | score ≥ 85, commits ≥ 1, no rework | Primary training examples |
| **Positive (silver)** | score 65–84, commits ≥ 1 | Training with lower weight |
| **Negative** | score < 40, 0 commits, rework triggered | Contrastive training (DPO/RLHF) |
| **Filtered** | score 40–64, ambiguous outcome | Hold out, don't train on |

The negative examples are as valuable as the positives — contrastive training (DPO) needs "what not to do" pairs.

---

## Export command

```bash
# Export training data in Trainloop format
axon-ledger signal export-training \
  --min-score 75 \
  --since "90 days ago" \
  --format trainloop \
  --out training_data.jsonl

# Include contrastive negatives (for DPO)
axon-ledger signal export-training \
  --min-score 75 \
  --include-negatives \
  --format dpo \
  --out training_dpo.jsonl

# Export only a specific goal type
axon-ledger signal export-training \
  --min-score 75 \
  --goal-pattern "fix * in *" \
  --format trainloop
```

### Output format (Trainloop-compatible)
```jsonl
{
  "messages": [
    {"role": "system", "content": "..."},
    {"role": "user", "content": "fix JWT clock skew for iOS..."},
    {"role": "assistant", "content": "..."},
    ...
  ],
  "metadata": {
    "session_id": "c8d17fba",
    "signal_score": 91,
    "goal": "fix JWT clock skew for iOS",
    "commits_produced": 3,
    "training_tier": "positive_gold"
  }
}
```

### DPO output format (chosen/rejected pairs)
```jsonl
{
  "prompt": "...",
  "chosen": [/* turns from high-score session */],
  "rejected": [/* turns from low-score session on similar task */],
  "metadata": {
    "chosen_score": 91,
    "rejected_score": 23,
    "task_type": "bug_fix"
  }
}
```

---

## What you can fine-tune

### 1. Codebase-specific LoRA
**What:** Fine-tune on YOUR team's high-signal sessions. The model learns your conventions, architecture patterns, variable naming, test structure.

**Effect:** "Add rate limiting to the auth endpoint" generates code that looks like *your* codebase wrote it, not generic examples.

**Data needed:** 50+ high-signal sessions (score ≥ 75) on your codebase.

**How:**
```bash
axon-ledger signal export-training --min-score 75 --format trainloop > my_team.jsonl
# Upload my_team.jsonl to MegaBrain/Trainloop fine-tuning pipeline
# Deploy LoRA weights alongside base model
```

### 2. Goal → implementation LoRA
**What:** Fine-tune specifically on (goal_text → good_session_transcript) pairs. Teaches the model to behave well when given goals in your team's style.

**Effect:** Sessions starting with "fix [error] in [file] — reproduces when [X]" immediately enter the efficient pattern rather than exploring.

**Data needed:** 100+ sessions with goal text, sorted by score.

### 3. Prompt structure optimizer
**What:** Fine-tune a small model to score goal quality (is this goal specific enough to likely succeed?) before the session starts.

**Effect:** Engineers get a pre-session warning: "This goal is likely to produce a low-efficiency session. Try: [specific suggestion]."

**Data needed:** The signal scores themselves as labels, goals as input. 200+ examples.

### 4. Contrastive coding style (DPO)
**What:** Use (high-score session, low-score session on same file) pairs for Direct Preference Optimization.

**Effect:** Model learns to prefer concise, goal-directed responses over exploratory wandering.

**Data needed:** Matched pairs where similar files were edited by sessions with very different scores.

---

## The compound learning effect

Week 1: Team uses AI coding, ledger records everything.
Week 4: Signal has enough data to score sessions meaningfully.
Week 8: First training export — 50 high-signal sessions.
Week 10: LoRA weights deployed. Sessions on your codebase are noticeably better.
Week 14: Better sessions → higher scores → more gold training data.
Week 20: Second LoRA iteration. The model knows your codebase deeply.

Each fine-tuning cycle makes future sessions more effective, which generates better training data, which improves the next cycle. **This is the moat.** After 6 months, your team's AI is materially better than a team that started at the same time but didn't close the loop.

---

## Integration architecture

```
axon-ledger (local NDJSON)
    │
    ├── axon-signal score (local compute, no LLM)
    │
    └── axon-ledger signal export-training
              │
              └──► Trainloop API (or local pipeline)
                        │
                        ├──► Fine-tuning job (LoRA or full)
                        │
                        └──► Model weights
                                  │
                                  └──► ANTHROPIC_BASE_URL override
                                       (route Claude Code to fine-tuned endpoint)
```

The key: `ANTHROPIC_BASE_URL` in Claude Code (or Axon's `AXON_AI_BASE_URL`) can point to your fine-tuned model endpoint. The fine-tuned model sits behind the same API interface — engineers don't change their workflow.

---

## Axon language improvements enabled by this pipeline

Once the trainloop pipeline is running, the ledger data reveals which **language primitives** are missing. When engineers consistently work around a gap, that gap becomes a language feature:

### Currently missing, evidenced by session patterns

**`@[prompt_template("name", learn: true)]`**

Engineers copy-paste prompt boilerplate across sessions. The ledger shows which prompt structures produce high-signal sessions. Axon can encode them as named, learnable templates:

```axon
@[prompt_template("security_review", learn: true)]
fn review_for_security(code: str) -> [Finding] {
    ai_extract(
        "Review for OWASP top 10. Output JSON. Max 5 issues.",
        code
    )
}
```

`learn: true` means: record (prompt_hash, session_score) in the ledger. Over time, Axon auto-selects the prompt variant with the best average score for this template type.

**`@[loop(checkpoint: true, exit_when: "fn")]`**

Sessions that run `/loop` manually and restart when the session times out. Durable checkpointed loops survive session restarts and are automatically logged to the ledger as a single logical unit.

**`@[session(scope, budget, exit_criterion)]`**

Declared session intent. The runtime warns when a session drifts outside declared scope (too many files, too many turns). Acts as a pre-commit gate for scope discipline.

**`ai_complete(prompt, learn: true, template: "name")`**

Extended `ai_complete` that records (prompt_structure, response, session_score) to the ledger. The runtime routes to the best-performing variant for this template type.

---

## Privacy and data handling

- **All ledger data is local by default.** Nothing leaves the machine until `export-training` is run explicitly.
- **Engineers control their own export.** Each engineer's session data is tagged with their principal. Export filters can be scoped per-engineer.
- **Anonymization option:** `--anonymize` strips author email and replaces with pseudonymous IDs before export.
- **No raw code in training by default:** The export format includes prompts and assistant responses but can be filtered to exclude file contents if the codebase is sensitive. `--exclude-code-content` strips everything after "Here is the file:" in prompts.
- **Compliance:** The training corpus stays within your infrastructure. MegaBrain/Trainloop fine-tuning can run on-prem or in your VPC.

---

## Near-term integration milestones

| Milestone | What | When |
|---|---|---|
| M1 | `signal export-training --format trainloop` command | Sprint 4 |
| M2 | Session score metadata injected into Trainloop records | Sprint 4 |
| M3 | DPO pair generation (chosen/rejected) | Sprint 5 |
| M4 | First LoRA trained on axon-ledger sessions | External (MegaBrain) |
| M5 | `AXON_AI_BASE_URL` routes to fine-tuned endpoint | Sprint 6 |
| M6 | Second LoRA iteration using improved session data | External |
| M7 | `@[prompt_template(learn: true)]` in Axon language | Sprint 7+ |
| M8 | Automatic prompt routing based on outcome history | Sprint 8+ |
