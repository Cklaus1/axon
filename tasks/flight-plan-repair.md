# Flight plan: S1 governed repair episode

Spec `tasks/repair-spec.md` (R0-R8; R6 and R7b deferred by Step 1 review). Base f83c6f86.

⚠ Production level: this run changes a security boundary (what model-proposed code may do on the operator's host). The
default plan still runs every fail-first test, the full gate, the offline fence sweep and one implementation review
before push.

| Plan | What it does | Wall time [INFERENCE] | Confidence | USD |
|---|---|---|---|---|
| **Fastest (default)** | 2 lanes (caps ∥ cortex serial), one review loop, one paid R7a | 6-9 h | 90 % | ~$0.15-0.45 |
| Most reliable | + second independent review, full g01 scope run, R7a replicate | 10-14 h | 95 % | ~$0.45-0.90 |
| Cheapest | as Fastest, no paid run (R7a skipped; regression claim unverified) | 5-8 h | 75 % | $0 |

Critical path: R0 → R1 → R5 → freeze → R7a → full gate. Excluded: NH-R1 (episode approval token), NH-R2 (hard-once).
