# Intent
Group the words in ./data/words.txt by their first letter and report how many words the largest group has. The file holds lowercase words separated by single spaces on one line that ends with a newline. Write the answer to ./out/answer.txt as a single line ending in a newline, nothing else. The answer is the count in decimal.

## Inputs
- ./data/words.txt

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
