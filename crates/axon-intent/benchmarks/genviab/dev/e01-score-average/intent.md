# Intent
Average the scores in ./data/scores.csv. Each line is `name,score` where score is an integer from 0 to 100, and every line ends with a newline. Write the average, rounded down, to ./out/avg.txt as one line ending in a newline. If the file is empty, the average is 0. If a line has no comma or its score is not an integer from 0 to 100, write no file, print `bad input: line N` (N counts from 1, the first bad line) and exit 3. If the file does not exist, write no file, print `missing input` and exit 4.

## Inputs
- ./data/scores.csv

## Outputs
- ./out/avg.txt

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
