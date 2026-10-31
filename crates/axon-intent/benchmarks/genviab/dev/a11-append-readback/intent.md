# Intent
Write the text `one` (no newline) to ./out/app.txt, then in a separate append operation append the text
`two` (no newline) to the same file. Then read ./out/app.txt back from disk and write its full contents,
followed by a single newline, to ./out/answer.txt.

## Inputs

## Outputs
- ./out/app.txt
- ./out/answer.txt

## Allowed
- fs_read: ./out/
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
