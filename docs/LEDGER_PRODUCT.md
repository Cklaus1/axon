# axon-ledger — Product Spec

**One-liner:** Answer "why did this commit happen?" on any repo with Claude Code sessions.

**Problem:** AI-assisted codebases accumulate decisions at 5–10x the rate of human-only teams. Git commits explain *what* changed. Nothing explains *why* — what the engineer was trying to accomplish, which AI session drove it, whether it worked. Six months in, nobody remembers. Debugging is archaeology.

**Solution:** An append-only provenance ledger that links git commits to the AI agent sessions that produced them, the original goal the engineer typed, and the outcomes measured against that decision.

---

## Who buys it

**Primary:** Engineering manager at a 5–50 person startup using Claude Code / Cursor / Copilot heavily. Their team ships fast but institutional memory is degrading. They need "explainable commits" before their next audit, onboarding hire, or incident post-mortem.

**Secondary:** CTO who wants to know whether the team's AI investment is compounding or creating debt.

**Tertiary:** Compliance/security lead at a growth-stage company needing SOC2-level audit trail for AI-assisted changes.

---

## Core user flows

### Flow 1 — "Why does this code look like this?"
```bash
axon-ledger history auth/middleware.rs
```
Shows the AI sessions that shaped each section of the file, in chronological order, with original goal text. Replaces 45-minute "walk me through this" conversations.

### Flow 2 — "Why did this break?"
```bash
git bisect # find the bad commit
axon-ledger why <sha>
```
Shows the session goal ("fix JWT clock skew for mobile"), the engineer, confidence, and any outcomes recorded. Debugging becomes understanding intent, not reading code.

### Flow 3 — "What shipped this sprint?"
```bash
axon-ledger weekly --from 2026-06-13 --to 2026-06-20
```
Goals, not commit messages. The sprint report a PM actually wants.

### Flow 4 — "Is this deploy safe?"
```bash
axon-ledger pre-deploy HEAD~8..HEAD
```
Flags commits with no linked session (unexplained changes = higher risk). Each session goal is shown alongside the commit for human review.

### Flow 5 — "Show me all AI changes to payments in Q3" (compliance)
```bash
axon-ledger audit --module payments/ --since 2026-07-01 --json
```
SOC2-ready structured output of every AI-assisted change to a sensitive module.

---

## Data model

Every record: `(id, principal, effect, causal_parent, ts_ms, payload)`

Append-only. Content-addressed (SHA-256). Safe to re-run any ingest command.

| Effect | Produced by | Key payload fields |
|---|---|---|
| `git_commit` | `ingest git` | sha, message, author, files |
| `agent_session` | `ingest session` | session_id, goal, start_ts, turn_count, files_touched |
| `agent_edge` | `ingest edges` | session_id, commit_sha, confidence, overlap_count |
| `metric_outcome` | `ingest outcome` | commit_sha, metrics JSON |

### Edge confidence scoring

`confidence = file_coverage_ratio × time_decay`

- `file_coverage_ratio` = commit files covered by session / total commit files
- `time_decay` = 1 − 0.9 × (gap_hours / 7h), clamped 0–1
- Sessions *after* the commit are penalized 10× (unusual but valid for pre-staging workflows)

---

## CLI surface

```bash
# Ingest
axon-ledger ingest git --repo . --since "90 days ago"
axon-ledger ingest session-dir ~/.claude/projects/<repo-hash>/
axon-ledger ingest edges
axon-ledger ingest outcome --commit <sha> --file metrics.json

# Query
axon-ledger why <sha>               # why did this commit happen?
axon-ledger history <file>          # how did this file evolve?
axon-ledger search <terms...>       # find decisions by keyword
axon-ledger as-of <ISO timestamp>   # what was known/in-flight at this moment?
axon-ledger pre-deploy <range>      # flag unexplained commits before deploy

# Reports
axon-ledger weekly [--from X --to Y] [--slack-webhook URL]
axon-ledger audit --module <path> --since <date>
axon-ledger stats                   # coverage %, record counts

# Continuous
axon-ledger watch --dir <sessions-dir> --interval 60

# MCP server (Claude Code integration)
axon-ledger mcp
```

---

## MCP integration

`axon-ledger mcp` runs as a JSON-RPC 2.0 stdio server. Add to `~/.claude/mcp_config.json`:

```json
{
  "mcpServers": {
    "axon-ledger": {
      "command": "~/.cargo/bin/axon-ledger",
      "args": ["mcp"]
    }
  }
}
```

Claude Code can then call `ledger_why`, `ledger_search`, `ledger_as_of`, `ledger_stats` mid-session — closing the loop where the AI knows the history of the code it's editing.

---

## Roadmap

### v0.1 (shipped)
- [x] ingest git / session / session-dir / edges / outcome
- [x] why, search, as-of, diff, stats (with coverage %)
- [x] watch daemon
- [x] MCP server (4 tools, JSON-RPC 2.0 / stdio)
- [x] standalone crate (no workspace deps)
- [x] `--since` for git ingest (fast onboarding for large repos)
- [x] session-dir progress display

### v0.2 (next)
- [x] `history <file>` — AI session lineage per file
- [x] `pre-deploy <range>` — unexplained commit flagging
- [x] `weekly` report — goals, not commit counts
- [ ] GitHub Action — PR enrichment with session goal comment
- [x] `audit --module <path>` — compliance query

### v0.3 (signal integration)
- [ ] Per-session effectiveness score (feeds axon-signal)
- [ ] Rework detection (same file, multiple sessions, short window)
- [ ] Loop opportunity detection
- [ ] Outcome integrations (Datadog, Sentry, PostHog via JSON schema)

### v1.0 (enterprise)
- [ ] Multi-repo ledger (microservices)
- [ ] RBAC — per-engineer vs. per-team visibility
- [ ] Webhook egress (Slack, PagerDuty on unexplained deploy)
- [ ] Retention policy (GDPR / data minimization)

---

## Pricing hypothesis

| Tier | Price | Limit | Target |
|---|---|---|---|
| **Open source** | Free | Single engineer, local only | Individual adoption, word-of-mouth |
| **Team** | $49/mo | 10 engineers, shared ledger | Seed-stage startup |
| **Growth** | $199/mo | Unlimited engineers, GitHub Action, Slack | Series A+ |
| **Enterprise** | $2k/mo | Multi-repo, audit export, RBAC, SSO | Compliance-driven |

The open source version is the acquisition channel. The team tier is the conversion event (when the second engineer joins and they want shared provenance).

---

## Distribution

1. `cargo install axon-ledger` — Rust ecosystem, developer word-of-mouth
2. GitHub Action — every PR gets a session goal comment, reviewers ask "how do I get this?"
3. Claude Code MCP marketplace — installed directly from Claude Code
4. axon-signal weekly email — retention mechanic, makes ledger sticky
