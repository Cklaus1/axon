# Intent
Maintain running statistics of integer readings across runs. Each run reads ./data/readings.txt, which holds zero or more lines, each an integer (possibly negative) followed by a newline. The state ./out/stats.txt left by the previous run is one line `C S L H` followed by a newline: the count, sum, minimum and maximum of all readings so far, separated by single spaces; if the file does not exist the state is `0 0 0 0` (whenever C is 0, L and H are 0 and do not count as readings). Each run adds its readings to the state, rewrites ./out/stats.txt in the same format, prints one line followed by a newline, which is `no readings` if the updated count is 0 and otherwise `count=C sum=S min=L max=H mean=M` where M is S divided by C with the fractional part discarded (rounded toward zero, so -7 over 2 is -3), and exits 0.

## Inputs
- ./data/readings.txt
- ./out/stats.txt

## Outputs
- ./out/stats.txt

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
