# Intent
For the matrix in ./data/matrix.txt: transpose it, sum each row of the transposed matrix, and take the product of those sums. The file holds one matrix row per line, the integers of a row separated by single spaces; every row has the same number of columns and every line ends with a newline. Write the answer to ./out/answer.txt as a single line ending in a newline, nothing else. The answer is the product in decimal.

## Inputs
- ./data/matrix.txt

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
