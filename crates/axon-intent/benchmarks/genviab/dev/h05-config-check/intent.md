# Intent
Validate ./data/app.conf and write a normalized copy. Every line of it ends with a newline. Blank lines and lines whose first character is `#` are ignored. Every other line must be `key=value`: split it at the first `=`; the key is the text before it and the value the text after it, each with surrounding spaces removed. Lines are checked in order, so the first offending line decides: a line with no `=` or with an empty key prints `syntax error: line N` and exits 2; a key that already appeared on an earlier line prints `duplicate key: KEY (line N)` and exits 3; a key `port` whose value is not an integer from 1 to 65535 prints `bad port: line N` and exits 4, where N counts every line of the file from 1, blank and comment lines included. After all lines pass, if key `name` is absent print `missing key: name` and exit 5, else if key `port` is absent print `missing key: port` and exit 5. Keys are case-sensitive. On any error write no file. On success write ./out/app.conf with one `key=value` line per key, sorted by key in ascending byte order, each line ending in a newline, print `ok: K keys` where K is the number of keys, and exit 0. Every printed line ends with a newline.

## Inputs
- ./data/app.conf

## Outputs
- ./out/app.conf

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
