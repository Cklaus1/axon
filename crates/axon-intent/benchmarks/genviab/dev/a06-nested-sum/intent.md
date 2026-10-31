# Intent
Sum every integer anywhere inside the nested list written in ./data/nested.txt. The file holds one nested list in bracket notation (for instance `[7, [8, []]]`): square brackets, commas, spaces and non-negative decimal integers, followed by a newline. Lists nest to any depth. Write the answer to ./out/answer.txt as a single line ending in a newline, nothing else. The answer is the sum in decimal.

## Inputs
- ./data/nested.txt

## Outputs
- ./out/answer.txt

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
