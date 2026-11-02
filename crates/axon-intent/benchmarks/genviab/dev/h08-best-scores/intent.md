# Intent
Combine ./data/roster.txt and ./data/scores.csv into ./out/ranking.txt. Every line of both files ends with a newline. roster.txt lines are `ID NAME`, split at the first space (NAME may itself contain spaces), each ID once; scores.csv lines are `ID,SCORE` with SCORE an integer from 0 to 100, and a line repeated identically anywhere in scores.csv counts once. A roster student's best score is the largest of their scores. Write one line `RANK. NAME BEST` for each roster student with at least one score, sorted by BEST descending and, for equal BEST, by ID in ascending byte order, where RANK is the line's position counting from 1; then one line `NAME -` for each roster student without a score, in roster order. Every line ends with a newline, and the file is empty when no line is due. Print one line `ignored: K` followed by a newline, where K is the number of distinct lines of scores.csv whose ID is not in the roster, and exit 0.

## Inputs
- ./data/roster.txt
- ./data/scores.csv

## Outputs
- ./out/ranking.txt

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
