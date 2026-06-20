# axon-signal — Product Spec

**One-liner:** Weekly AI coding effectiveness report for engineering teams — who's using AI well, what patterns to copy, what to stop doing.

**Problem:** Every startup is spending $20–100k/year on AI coding tools. Nobody can measure whether it's working. "We ship faster" is not a metric. There's no tool that tells you *which engineers are using AI effectively*, *which prompt patterns work*, or *whether your team's AI coding is improving week over week*.

**Relationship to axon-ledger:** Signal is the intelligence layer on top of the ledger. Ledger = data collection. Signal = insight and recommendation engine. They share the same data store.

---

## The core insight

As every team uses AI coding, the differentiator shifts from *"are you using AI?"* to *"how well is your team using AI?"*

Effective AI coding is measurable. It leaves a signal in the ledger:
- Specific goals → fewer turns → more commits → cleaner history
- Vague goals → many turns → rework → technical debt accumulation
- Loop usage → autonomous completion → time freed for harder problems
- Session scope control → focused changes → reviewable PRs

Signal surfaces these patterns at the team level, week over week.

---

## Effectiveness score (per session)

Computed entirely from ledger data — no LLM calls required.

```
score = (
  goal_clarity    × 0.30  +   # word count + specificity heuristic
  turns_per_commit × 0.25 +   # inverse: fewer = better
  scope_fit        × 0.20 +   # files_touched vs. goal breadth
  rework_absence   × 0.15 +   # same files re-touched within 7d?
  commit_lag       × 0.10     # time from session end to first commit
) × 100
```

### Goal clarity heuristic (no LLM)
- Base: 40 points for any goal text
- +20: contains a filename or module name
- +20: contains a measurable outcome ("< 50ms", "test passes", "error rate drops")
- +10: verb is specific ("fix", "add", "refactor", "migrate") not vague ("improve", "clean", "work on")
- −20: under 5 words
- −30: under 3 words or empty

### Score interpretation
| Score | Label | Meaning |
|---|---|---|
| 85–100 | Excellent | Clear goal, efficient execution, shipped |
| 65–84  | Good | Minor inefficiencies, good outcome |
| 45–64  | Fair | Vague goal or scope drift |
| 25–44  | Poor | Many turns, no commits, or rework triggered |
| 0–24   | Low signal | Session needs review |

---

## Weekly report format

```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
axon-signal  ·  Week of June 16–20, 2026
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

TEAM VELOCITY
  Sessions: 12  ·  Commits: 34  ·  Coverage: 91%
  Avg effectiveness: 71  (↑ 8 from last week)
  Turns/commit: 18  (team best ever: 8)

ENGINEER BREAKDOWN
  chris   ████████░░  82  4 sessions  12 commits  ★ most efficient this week
  alice   ██████░░░░  67  3 sessions   8 commits
  bob     ████░░░░░░  43  5 sessions   6 commits  ← see recommendations

TOP SESSIONS THIS WEEK
  ★ "fix JWT clock skew for iOS — reproduces when device time > 5min off"
    score 94 · 9 turns · 3 commits · chris

  ★ "add rate limiting to /api/auth — 429 after 5 fails/minute"
    score 88 · 12 turns · 2 commits · alice

WHAT WORKED
  Goal pattern: "fix [specific error] in [file] — reproduces when [condition]"
  → avg score 89, avg 10 turns/commit (your team's #1 pattern)

  Sessions under 90 minutes: 2.4× more commits than longer sessions
  
  Loop sessions (3 this week): 0 human interventions needed

PATTERNS TO BREAK
  ⚠ bob — 3 sessions with vague goals ("clean up things", "fix stuff", "improvements")
    89 total turns · 0 commits produced · 1 rework trigger

  ⚠ auth/ touched by 4 sessions in 2 weeks (rework signal)
    Each session partially fixed the same area without a clear exit criterion

  ⚠ 2 sessions ran 100+ turns — likely scope too broad

RECOMMENDATIONS THIS WEEK
  1. Bob: Scope goals to one file or one function.
     Instead of "clean up auth" → "extract token validation from auth.go into validator.go"
     Your 2 specific-goal sessions this week: avg score 71 (vs. 18 for vague ones)

  2. Team: auth/ is a rework hotspot. One dedicated session:
     "audit all auth edge cases, write a test for each one that failed in the last month"
     Then lock it for 2 weeks.

  3. The "run test, fix error, repeat" pattern appeared 8× manually.
     Suggested loop: /loop 3m "cargo test, fix first failure, commit if green"
     Estimated savings: ~35 min/week

LOOP OPPORTUNITIES DETECTED
  • alice/payments — same cargo check pattern 6× across 2 sessions → /loop candidate
  • bob/auth — manual retry loop on test failure → automate

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
```

---

## Commands

```bash
# Score a session or date range
axon-ledger signal score [--session <id>] [--week] [--engineer <name>]

# Detect rework hotspots
axon-ledger signal rework [--days 14] [--module <path>]

# Find loop automation opportunities
axon-ledger signal loops [--min-repetitions 3]

# Per-engineer goal clarity analysis
axon-ledger signal goals [--engineer <name>] [--week]

# Full weekly report (stdout or Slack)
axon-ledger signal weekly [--from <ISO>] [--to <ISO>] [--slack-webhook <url>]

# Team prompt pattern library (what worked historically)
axon-ledger signal patterns [--top 10] [--min-score 75]

# Export training data for Trainloop (see TRAINLOOP.md)
axon-ledger signal export-training --min-score 75 --format trainloop
```

---

## Per-engineer insights (with privacy controls)

Signal tracks per-engineer metrics but follows a privacy-by-default model:

- **Individual view:** engineers see their own full score breakdown
- **Manager view:** team averages + anonymized distribution (who is above/below median, not who is "worst")
- **Opt-in named breakdown:** team lead can enable named rankings if the team agrees
- **No keylogging:** signal scores sessions by outcome, not by reading content

The goal is improvement, not surveillance. Recommendations are framed as patterns, not evaluations.

---

## Goal quality linting (Axon integration)

When Axon's `@[goal(...)]` annotation has structured fields, Signal can lint at write-time:

```axon
// Compile-time warning W2001: goal missing measurable criterion
@[goal("improve auth")]
fn fix_auth() { ... }

// Clean — has intent, metric, scope
@[goal(
    intent: "reduce auth latency",
    metric: "p99_ms < 50",
    scope: ["auth/"]
)]
fn optimize_auth() -> Result<(), str> { ... }
```

This makes goal quality a first-class compiler concern, not a retrospective report.

---

## Prompt pattern library

Over time, Signal builds a team-specific library of goal structures that reliably produced commits:

```
Your team's top prompt patterns (last 90 days):

Rank  Pattern                                          Avg Score  Uses
  1   "fix [error] in [file] — reproduces when [X]"      91       23
  2   "add [feature] — success = [test passes]"           84       18
  3   "refactor [fn] in [file] — before/after [shape]"   81       14
  4   "migrate [A] to [B] — [N] call sites in [module]"  79        9

Avoid:
  ✗   "improve/clean/fix [area]" (no specifics)           31       12
  ✗   Goals under 5 words                                 22        8
```

This becomes the team's **living playbook** for AI coding — not generic advice, derived from what actually shipped on your codebase.

---

## Axon language improvements driven by Signal data

Signal's aggregate data reveals which language primitives are missing or underused. These feed back into Axon's roadmap:

### `@[loop(checkpoint: true)]` — durable loops
Signal detects engineers running the same loop pattern manually across multiple sessions (session ends, restart, repeat). A durable checkpointed loop survives session restarts.

```axon
@[loop(
    interval: 5m,
    exit_when: fn() -> bool { run_tests() == Pass },
    checkpoint: true,
    max_iterations: 20,
    budget: Budget { time: 2h }
)]
fn fix_failing_tests() -> LoopResult { ... }
```

### `@[goal(intent, metric, scope)]` — structured goals
Signal shows that goal clarity is the #1 predictor of session effectiveness. Make it a typed, compiler-checkable annotation.

### `@[prompt_template("name")]` — learnable prompts
Signal identifies which `ai_complete` call structures work best. Axon can register them as named templates with ledger-backed outcome tracking.

```axon
@[prompt_template("security_review", learned: true)]
fn review_security(code: str) -> Result<[Finding], str> {
    ai_extract(
        "Review for OWASP top 10. Output JSON array of {severity, line, description}.",
        code
    )
}
```

### `@[session(scope, budget, goal)]` — declared session boundaries
Enforces focus at the language level. The compiler warns if a session annotated with `scope: ["auth/"]` issues writes to `payments/`.

---

## Roadmap

### v0.1 (first sprint)
- [x] `signal score` — per-session effectiveness from ledger data
- [x] `signal weekly` — formatted team report (stdout)
- [x] `signal rework` — rework hotspot detection
- [x] `signal loops` — loop opportunity detector

### v0.2
- [x] `signal patterns` — team prompt pattern library
- [x] `signal goals` — goal quality per engineer
- [x] Slack webhook integration for weekly report
- [x] GitHub Action: post session score on PR
- [x] MCP server — all 7 analytics tools exposed (signal_score/weekly/rework/patterns/goals/loops/trends)
- [x] `score --ingest` — write scores back into ledger as MetricOutcome records

### v0.3 (Axon integration)
- [ ] `@[goal(intent, metric, scope)]` structured annotation (parser extension, deferred)
- [x] W2001 lint for vague goals — fires when `@[goal("...")]` has no file ref, no metric, or < 5 words
- [x] `signal export-training` — Trainloop/LoRA export (see TRAINLOOP.md)

### v1.0
- [x] Web dashboard — `axon-signal dashboard [--port 7373]`; single-page HTML/JS; 5 panes: Overview (avg score, top sessions, coverage), Trends (per-engineer bar charts), Rework hotspots, Loop opportunities, All sessions table; REST API (/api/score|weekly|rework|trends|loops)
- [x] Per-engineer trend charts (score week-over-week) — `signal trends [--weeks N] [--engineer prefix]`; MCP `signal_trends`; direction + delta + ASCII bar chart + tailored recommendation
- [x] Team benchmark — `signal benchmark [--days N] [--json]`; MCP `signal_benchmark`; REST `/api/benchmark`; Developing/Bronze/Silver/Gold/Platinum tiers with dimension-level comparison (goal clarity, turns/commit, rework rate, commit rate) vs estimated industry medians; per-engineer intra-team percentile; ranked recommendations
- [x] Recommendation engine with A/B tracking — `signal weekly --track` auto-records the top recommendation shown each week; `signal ab-track --text "..." [--engineer eng] [--week YYYY-WNN]` for manual entries; `signal ab-status [--json]` shows per-type outcome table (avg score delta + improved rate); MCP `signal_ab_status`; REST `/api/ab`; disclaimer labels this observational (not a randomised trial)

---

## Pricing

Signal is the monetization layer on top of the free ledger.

| Tier | Price | What you get |
|---|---|---|
| **Free** | $0 | `signal score` for your own sessions |
| **Team** | $99/mo | Full weekly report, all engineers, Slack |
| **Growth** | $299/mo | Prompt pattern library, Trainloop export, trend charts |
| **Enterprise** | Custom | Custom recommendations, benchmark data, model fine-tuning pipeline |
