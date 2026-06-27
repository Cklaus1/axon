# Intent
Summarize ./data/report.txt into ./out/summary.txt.

The intent PERMITS network access (the operator was generous), but the
synthesized program never reaches the network — so the inferred
least-privilege grant has net=∅ regardless. This is the
`grant_is_least_privilege` demo: permission the program does not need is
never granted, even when the ceiling would allow it.

## Inputs
- ./data/report.txt

## Outputs
- ./out/summary.txt

## Allowed
- fs_read: ./data/
- fs_write: ./out/
- net: api.example.com
- exec: none
- max_label: internal

## Budget
- calls: 100
- tokens: 50000
- cost_micro: 1000000

## Seed
- 42
