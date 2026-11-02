# Intent
Merge the address lists ./data/a.txt and ./data/b.txt. Each holds one email address per line, every line ending in a newline; normalize each line by removing surrounding spaces and lowercasing ASCII letters, and skip lines that are empty after that. Write ./out/merged.txt with every distinct normalized address from either file, one per line, each ending in a newline (an empty file when there are none), sorted by domain (the text after the last `@`) in ascending byte order and, for equal domains, by the whole address in ascending byte order. Print one line `a-only: X, both: Y, b-only: Z` followed by a newline, counting the distinct normalized addresses found only in a.txt, in both files, and only in b.txt, and exit 0.

## Inputs
- ./data/a.txt
- ./data/b.txt

## Outputs
- ./out/merged.txt

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
