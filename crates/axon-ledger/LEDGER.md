# axon-ledger — Provenance Ledger for AI-Coding Teams

Answer "why did this commit happen?" on any git repo with Claude Code sessions.

## What it does

`axon-ledger why <sha>` links a commit to the AI agent session that produced it, shows the original goal the engineer typed, the session's confidence score, and any metric outcomes recorded against that decision. It builds a causal, replayable timeline that none of your existing tools (git, Datadog, PostHog, Slack) can produce alone.

```
COMMIT   ed5775db2c16
  msg:   feat(week1): json_parse/json_stringify builtins + brief-gate.ax
  by:    chris@example.com
  files: crates/axon-core/src/builtins.rs  ...

AGENT SESSION  c8d17fba (inferred, confidence 0.88)
  goal:   analyze this project, whats the status
  start:  2026-06-20T15:58:18.700Z
  turns:  182
  files:  ...

OUTCOMES  (none recorded)
```

## Prerequisites

- Rust toolchain (`rustup show`)
- A git repository
- Claude Code sessions stored in `~/.claude/projects/<repo-path>/`

## Quickstart (your own repo, < 5 minutes)

```bash
# 1. Install (requires Rust — https://rustup.rs)
cargo install --git https://github.com/cklaus/axon axon-ledger
# or build from source:
#   git clone https://github.com/cklaus/axon && cd axon
#   cargo build -p axon-ledger
#   alias axon-ledger=./target/debug/axon-ledger

# 2. Ingest your repo's git history (one-time, ~10s for 1000 commits)
./target/debug/axon-ledger ingest git --repo /path/to/your-repo

# 3. Ingest your Claude Code sessions
./target/debug/axon-ledger ingest session-dir \
  ~/.claude/projects/$(ls ~/.claude/projects/ | grep your-repo-name)

# 4. Infer session→commit causal links
./target/debug/axon-ledger ingest edges

# 5. Ask why
git -C /path/to/your-repo log --oneline -5   # pick a recent SHA
./target/debug/axon-ledger why <sha-prefix>
```

## Continuous mode (set-and-forget)

```bash
# Watch for new sessions and auto-ingest them as they complete
./target/debug/axon-ledger watch \
  --dir ~/.claude/projects/$(ls ~/.claude/projects/ | grep your-repo-name) \
  --interval 60 &

echo "Ledger running in background. Ctrl-C or kill %1 to stop."
```

After each completed Claude Code session, new commits made during that session will be linkable via `why` within ~60 seconds.

## Search the ledger

```bash
# What decisions touched auth?
axon-ledger search auth

# Which sessions worked on the payment flow?
axon-ledger search payment checkout

# What commits changed the database schema?
axon-ledger search "schema migration"

# All terms must match (AND semantics):
axon-ledger search api rate limit --limit 20
```

Results show the commit message or session goal that matched, plus the linked agent goal for commit hits.

## Record metric outcomes

Link a metrics JSON file causally to a commit (precision/recall, A/B results, deploy success rate — anything):

```bash
echo '{"precision": 0.95, "recall": 0.71, "note": "edge inference v2"}' > metrics.json
./target/debug/axon-ledger ingest outcome --commit <sha> --file metrics.json

# Now `why <sha>` shows the outcome alongside the session that produced the change
```

## Ledger location

Default: `~/.axon/ledger/`  
Override: `--ledger-dir /path/to/ledger` (all subcommands accept this global flag)

## Commands

| Command | What it does |
|---|---|
| `ingest git --repo <path>` | Index all commits in a git repo |
| `ingest session <file.jsonl>` | Ingest one Claude Code session file |
| `ingest session-dir <dir>` | Ingest all sessions in a directory |
| `ingest edges` | Infer session→commit causal links (run after ingest git + session) |
| `ingest outcome --commit <sha> --file <json>` | Record a metric outcome against a commit |
| `why <sha>` | Explain why a commit happened (session + goal + outcomes) |
| `search <terms...>` | Full-text search across commit messages, session goals, and files |
| `as-of <ISO timestamp>` | Reconstruct what was known/shipped at a point in time |
| `diff --from <ISO> --to <ISO>` | List all ledger records in a time window |
| `stats` | Total records by type |
| `watch --dir <dir> [--interval 60]` | Continuous ingest daemon |
| `mcp` | Start MCP stdio server (ledger_why / ledger_search / ledger_as_of / ledger_stats tools) |
| Any command + `--json` | Machine-readable output |

## MCP server — let Claude Code query the ledger mid-session

Run the ledger as an MCP tool server so Claude Code (and other MCP clients) can call `ledger_why`, `ledger_search`, `ledger_as_of`, and `ledger_stats` automatically during a session.

```bash
# Start the MCP server (reads stdin, writes stdout — stdio transport)
axon-ledger mcp

# Or with a custom ledger directory:
axon-ledger --ledger-dir ~/.axon/ledger mcp
```

**Wire it into Claude Code** — add to `~/.claude/mcp_config.json`:

```json
{
  "mcpServers": {
    "axon-ledger": {
      "command": "/home/<your-username>/.cargo/bin/axon-ledger",
      "args": ["mcp"],
      "env": {}
    }
  }
}
```

> Tip: after `cargo install axon-ledger`, run `which axon-ledger` to get the exact path. On most systems it's `~/.cargo/bin/axon-ledger`. Substitute the full path (no `~` expansion in MCP configs).

After restarting Claude Code, the agent can call `ledger_why("ed5775d")` inline and get back the full provenance record — session goal, confidence score, metric outcomes — without leaving the conversation.

**Tools exposed:**

| Tool | Input | What it returns |
|---|---|---|
| `ledger_why` | `{ sha }` | Session that produced the commit + original goal + outcomes |
| `ledger_search` | `{ query, limit? }` | Commits and sessions matching all terms |
| `ledger_as_of` | `{ timestamp }` | State snapshot at a point in time |
| `ledger_stats` | `{}` | Record counts by type |

## Brief gate (optional)

Run a validation pass on session goals before ingesting them:

```bash
./target/debug/axon-ledger ingest session-dir <dir> --gate
```

Requires the `axon` binary on PATH and `examples/goals/brief-gate.ax` in the repo root. Sessions whose first user message is too short or missing structured context are rejected (exit, not silently skipped). Without `--gate`, ingestion always succeeds.

## Schema

Every record: `(id, principal, effect, causal_parent, ts_ms, payload)`

| Effect type | Set by | Payload keys |
|---|---|---|
| `git_commit` | `ingest git` | sha, message, author, files |
| `agent_session` | `ingest session` | session_id, goal, start_ts, turn_count, files_touched |
| `agent_edge` | `ingest edges` | session_id, commit_sha, confidence, overlap_count |
| `metric_outcome` | `ingest outcome` | commit_sha, file, metrics |

Ledger is append-only. Records are content-addressed (SHA-256). Safe to re-run any ingest command — duplicates are silently skipped.

## Self-hosted / VPC

The ledger stores everything locally under `--ledger-dir`. No data leaves your machine. Suitable for air-gapped or compliance-restricted environments. The `axon` binary and session files stay on-prem.
