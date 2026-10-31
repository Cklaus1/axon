# Intent
Starting from the sentence in ./data/sentence.txt: build a mapping from each distinct word to its length, discard the entries whose length is 3 or less, and take the sum of the lengths that remain. The file holds one line of lowercase words separated by single spaces, ending with a newline. Write the answer to ./out/answer.txt as a single line ending in a newline, nothing else. The answer is the sum in decimal.

## Inputs
- ./data/sentence.txt

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
