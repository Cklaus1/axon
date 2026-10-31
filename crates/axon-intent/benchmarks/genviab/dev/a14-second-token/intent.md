# Intent
For the comma-separated tokens in ./data/tokens.txt: count how often each token occurs, then give the token with the second-highest count. The file holds one line of tokens (lowercase letters) separated by commas with no spaces, ending with a newline. The counts are distinct. Write the answer to ./out/answer.txt as a single line ending in a newline, nothing else. The answer is the token.

## Inputs
- ./data/tokens.txt

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
