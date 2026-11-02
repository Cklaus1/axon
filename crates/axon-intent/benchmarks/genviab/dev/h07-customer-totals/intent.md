# Intent
Join ./data/customers.csv and ./data/orders.csv into a spending report. Every line of both files ends with a newline and neither has a header. customers.csv lines are `id,name`; if an id appears on more than one line, the first such line wins. orders.csv lines are `order_id,customer_id,amount` with amount a non-negative integer; an order_id that appears on more than one line counts only at its first line. Write ./out/report.txt with one line `NAME: TOTAL` for each customer that has at least one counted order, where TOTAL is the sum of the amounts of that customer's counted orders, sorted by TOTAL descending (as numbers) and, for equal totals, by NAME in ascending byte order; then a last line `unmatched: U`, where U is the number of counted orders whose customer_id is not an id in customers.csv. Every line ends with a newline. Print nothing and exit 0.

## Inputs
- ./data/customers.csv
- ./data/orders.csv

## Outputs
- ./out/report.txt

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
