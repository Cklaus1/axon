# axon-signal — AI Coding Effectiveness Analytics

Score every AI coding session. Surface what works, fix what doesn't.

## What it does

`axon-signal score` reads from the axon-ledger and produces a 0–100 effectiveness
score for every Claude Code session — based on goal clarity, turns-per-commit,
scope fit, and rework signal. No LLM calls: all analytics are derived from
ledger data.

## Quickstart

```bash
cargo install axon-signal

# Score all sessions in the ledger
axon-signal score

# Weekly team report
axon-signal weekly

# Find sessions that needed a /loop
axon-signal loops

# Export training data for Trainloop / fine-tuning
axon-signal export-training --format dpo --out training.jsonl
```

## Commands

| Command | What it does |
|---|---|
| `score [--engineer X] [--days N] [--ingest]` | Effectiveness score per session (0–100) |
| `weekly [--from X] [--to Y] [--slack-webhook URL]` | Team report: scores, goals, rework hotspots |
| `rework [--days N]` | Files touched by multiple sessions in a short window |
| `patterns [--min-score N] [--min-sessions N]` | Which goal patterns produce the best outcomes |
| `goals [--days N]` | Per-engineer goal clarity breakdown |
| `loops [--turns-threshold N] [--days N]` | Sessions that would benefit from `/loop` |
| `export-training [--format trainloop\|dpo] [--min-score N]` | Trainloop/MegaBrain fine-tuning export |
| `mcp` | Start MCP stdio server — exposes all analytics as tools |

## MCP server — query analytics inside Claude Code

Run axon-signal as an MCP tool server so Claude Code can call `signal_score`,
`signal_weekly`, `signal_rework`, `signal_patterns`, `signal_goals`, and
`signal_loops` mid-session.

```bash
axon-signal mcp
```

**Wire it into Claude Code** (`~/.claude/mcp_config.json`):

```json
{
  "mcpServers": {
    "axon-signal": {
      "command": "/home/<your-username>/.cargo/bin/axon-signal",
      "args": ["mcp"],
      "env": {}
    }
  }
}
```

To point at a non-default ledger:

```json
{
  "mcpServers": {
    "axon-signal": {
      "command": "/home/<your-username>/.cargo/bin/axon-signal",
      "args": ["--ledger-dir", "/shared/team/.axon/ledger", "mcp"]
    }
  }
}
```

**MCP tools exposed:**

| Tool | Input | Returns |
|---|---|---|
| `signal_score` | `{ engineer?, days? }` | Session scores with tier, goal clarity, turns/commit |
| `signal_weekly` | `{ days? }` | Weekly report: avg score, top goals, rework hotspots |
| `signal_rework` | `{ days? }` | Files with multiple sessions in a short window |
| `signal_patterns` | `{ min_score?, min_sessions? }` | Top + anti goal patterns |
| `signal_goals` | `{ days? }` | Per-engineer goal clarity with team recommendation |
| `signal_loops` | `{ turns_threshold?, days? }` | Sessions that should have used /loop |

## Scoring formula

```
score = goal_clarity × 0.30
      + turns_efficiency × 0.25
      + scope_fit × 0.20
      + no_rework × 0.15
      + commit_lag × 0.10
```

| Dimension | What it measures |
|---|---|
| Goal clarity | Specificity of the session goal — file refs, measurable outcomes, specific verbs |
| Turns efficiency | Turns per commit (sweet spot: 8–20) |
| Scope fit | Files touched vs. goal breadth |
| No rework | Same file touched by multiple sessions in a short window |
| Commit lag | Sessions with linked commits score higher |

## Training tiers

| Tier | Criteria | Use |
|---|---|---|
| `PositiveGold` | score ≥ 85, commits ≥ 1, no rework | Primary fine-tuning examples |
| `PositiveSilver` | score 65–84, commits ≥ 1 | Secondary examples |
| `Negative` | score < 40, or rework | DPO contrastive training |
| `Filtered` | score 40–64 | Hold out |

## Signal → Ledger feedback loop

```bash
# Write signal scores back into the ledger as MetricOutcome records
axon-signal score --ingest

# Now `axon-ledger why <sha>` shows the effectiveness score alongside the
# causal chain — provenance + quality in one place
```

## GitHub Actions

`github-action/pr-session-score.yml` — posts a session score table on every PR
(score, tier, turns, commits, goal). Copy to `.github/workflows/` in your repo.
