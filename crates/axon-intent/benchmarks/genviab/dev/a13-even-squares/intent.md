# Intent
Starting from the integers listed in ./data/numbers.txt: keep those divisible by 3, square each of them, and take the sum of the squares that are even. The file holds one positive decimal integer per line; every line ends with a newline. Write the answer to ./out/answer.txt as a single line ending in a newline, nothing else. The answer is the sum in decimal.

## Inputs
- ./data/numbers.txt

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
