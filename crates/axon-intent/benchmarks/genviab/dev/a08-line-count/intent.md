# Intent
Count the lines of ./data/lines.txt. Every line of the file, the last one included, ends with a newline character; the file is not empty. Write the answer to ./out/answer.txt as a single line ending in a newline, nothing else. The answer is the number of lines in decimal.

## Inputs
- ./data/lines.txt

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
