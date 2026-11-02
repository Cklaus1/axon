# Intent
Process ./data/events.txt incrementally across runs, remembering processed ids in ./out/seen.txt. Each line of events.txt is `id,payload` and ends with a newline; the id is the text before the first comma. ./out/seen.txt lists the ids processed by earlier runs, one per line, each line ending in a newline; if it does not exist no id has been processed. An event is new when its id is not in seen.txt and did not already appear on an earlier line of the same events.txt. Each run writes ./out/new.txt containing the new events' lines, unchanged and in input order, each ending in a newline (an empty file when there are none), appends each new event's id as a line to ./out/seen.txt in the same order (creating the file if needed, and leaving it unchanged when there are none), prints one line `new: X, skipped: Y` followed by a newline, where X is the number of new events and Y the number of other lines of events.txt, and exits 0.

## Inputs
- ./data/events.txt
- ./out/seen.txt

## Outputs
- ./out/new.txt
- ./out/seen.txt

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
