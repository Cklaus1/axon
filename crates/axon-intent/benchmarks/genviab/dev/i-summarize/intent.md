# Intent
Summarize ./data/report.txt into ./out/summary.txt without using the network. The summary is exactly `report length: N bytes`, where N is the size of the report in bytes in decimal, with no newline after it.

## Inputs
- ./data/report.txt

## Outputs
- ./out/summary.txt

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
