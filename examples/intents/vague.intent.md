# Intent
Do something useful with the data, you figure out what.

This intent is deliberately under-specified: it names no concrete output,
so the synthesizer cannot produce a real program — only a stub. The
confidence gate REFUSES it (exit 5) rather than ship a plausible-but-empty
"best effort". This is the headline negative demo: refuse, don't downgrade.

## Inputs
- ./data/report.txt

## Allowed
- fs_read: ./data/
- fs_write: ./out/
- net: none
- exec: none
- max_label: internal

## Budget
- calls: 100
- tokens: 50000
- cost_micro: 1000000

## Seed
- 42
