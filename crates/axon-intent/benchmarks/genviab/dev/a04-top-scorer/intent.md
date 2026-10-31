# Intent
Find the name of the highest-scoring record in ./data/records.csv. The file is CSV: a header line `name,score`, then one record per line (a name, a comma, a non-negative integer score); every line ends with a newline. Scores are distinct. Write the answer to ./out/answer.txt as a single line ending in a newline, nothing else. The answer is the name.

## Inputs
- ./data/records.csv

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
