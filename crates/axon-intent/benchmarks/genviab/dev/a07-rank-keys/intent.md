# Intent
Rank the keys in ./data/counts.csv by value, highest first; where two values tie, order those keys alphabetically (ascending). The file has no header: each line is a key (lowercase letters), a comma and a non-negative integer value, and ends with a newline. Write the answer to ./out/answer.txt as a single line ending in a newline, nothing else. The answer is the ranked keys joined by commas with no spaces.

## Inputs
- ./data/counts.csv

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
