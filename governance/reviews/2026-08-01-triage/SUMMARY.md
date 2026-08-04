# Step 1 — adversarial triage of the 20 CRITICAL findings

4 agents × 5 findings, each instructed to **refute by default**. Per-finding
evidence in `verdict-1.md` … `verdict-4.md`. Source of record: `raw/findings.json` (rescued 2026-08-04 from the gitignored `.archive/`, where it was not under version control).

## Outcome

| verdict | n |
|---|---|
| CONFIRMED — stays CRITICAL | 3 |
| CONFIRMED — downgraded to HIGH | 11 |
| CONFIRMED — downgraded to MEDIUM | 4 |
| REFUTED — downgraded to MEDIUM/LOW | 2 |

18/20 describe a real, reproducible defect. 3/20 warrant CRITICAL.

## The 3 survivors — all one class: capability-sandbox escape

| id | file | defect |
|---|---|---|
| F153 | `@[contained]` walker | string-dispatch builtins bypass the walker entirely; both vectors reproduced |
| F041 | `sandbox_run` | **replaces** rather than **intersects** the effect ceiling; escape reproduced |
| F013 | axon-os | a zero-capability job re-widened its own sandbox; the control exits 8 |

Two HIGHs are the same system and were found independently by two agents from
two different findings: **F014** and **F040** — `effect_set()` reduces capabilities
to a boolean set, discarding path prefixes and host allowlists, and *no path or
host check exists anywhere downstream*.

So `@[contained](fs: [write("./out/")], net: ["api.example.com"])` enforces
"may write **somewhere**" and "may reach **some** host". The allowlists parse,
type-check, appear in the approval UI, and are then dropped before they can
constrain anything.

This is the project's headline value proposition (README, flagship demo,
"~28 of 40 CVE-Bench by construction"). It is the top of the DAG.

## Why so many downgrades — and what that says

The pattern is consistent: the *facts* in these findings held up (18/20), the
*grades* did not (3/20). Recurring reasons for downgrade:

1. **Defense-in-depth counted as sole defense** (F162 kernel `0xFF` fail-open) —
   a real fail-open, but behind an already-enforcing layer.
2. **Error direction unexamined** (F139) — the divergence can over-report but
   cannot hide a real violation. A correctness wart, not a security hole.
3. **Unbuilt feature reported as bug** (F160) — the attestation stand-in is
   documented as a stand-in; no `hw-attest` feature exists. Roadmap, not DAG.
4. **Mechanism real, impact nil** (F154) — see below.

## The correction that matters most — F154

I hand-verified F154 *myself* and graded it CRITICAL: `principals: Vec<Principal>`
makes handles dense array indices, so `child - 1` reaches the parent and defeats
attenuation. Those facts are correct and undisputed.

They are also irrelevant. **`principal_root` is ungated** — an attacker mints a
root principal directly. Forging a parent handle grants nothing they could not
already have for free.

Verifying that cited code says what a finding claims is *not* verifying that the
finding matters. This is the same failure the review named for the codebase —
*"the check that exists is not the check that was claimed"* — committed here in
the review of the review. Worth remembering before trusting any future
spot-check of my own.

Ungated `principal_root` is a larger issue than the finding that surfaced it and
is covered nowhere in the 185. Logged as O003.
