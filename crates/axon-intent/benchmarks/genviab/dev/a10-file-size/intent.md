# Intent
Report the size in bytes of the file ./data/five.txt. Write the answer to ./out/answer.txt as a single line ending in a newline, nothing else. The answer is the byte count in decimal.

## Inputs
- ./data/five.txt

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
