# Intent
Keep a running word tally across runs. Each run reads ./data/words.txt, splits it into words on spaces and newlines (empty pieces are not words), lowercases every ASCII letter of each word, and adds each occurrence to the tally stored in ./out/tally.txt by the previous run; if ./out/tally.txt does not exist the tally starts empty, and an existing empty file is an empty tally. The tally file holds one line `word=count` per word, sorted by word in ascending byte order, every line ending in a newline (an empty tally is an empty file). Each run rewrites ./out/tally.txt with the updated tally, prints one line `total: N` followed by a newline, where N is the sum of all counts in the updated tally, and exits 0.

## Inputs
- ./data/words.txt
- ./out/tally.txt

## Outputs
- ./out/tally.txt

## Allowed
- fs_read: ./data/, ./out/
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
