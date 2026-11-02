# Intent
Sum the columns of ./data/table.csv. Every line ends with a newline. The first line is a header of comma-separated column names; each later line is a row of comma-separated integers, possibly negative. If the file does not exist print `missing input` and exit 5; if it is empty print `no header` and exit 2. Otherwise rows are checked in order, so the first offending line decides, and within a line the field count is checked first: a row whose number of fields differs from the header's prints `line N: expected A fields, got B` (A the header's field count, B the row's) and exits 3; a field that is not an integer prints `line N: bad number "FIELD"` (the first such field in the row, between double quotes) and exits 4, where N counts lines from 1 with the header as line 1. On any error write no file. On success write ./out/sums.txt with one line `NAME=SUM` per column in header order, each ending in a newline, print `rows: R` where R is the number of rows after the header, and exit 0. Every printed line ends with a newline.

## Inputs
- ./data/table.csv

## Outputs
- ./out/sums.txt

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
