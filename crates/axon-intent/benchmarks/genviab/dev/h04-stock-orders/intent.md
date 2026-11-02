# Intent
Apply the orders in ./data/orders.txt to the stock in ./data/stock.txt and write the new stock to ./out/stock.txt. Every line of both files ends with a newline. stock.txt has lines `ITEM QTY` (one space, QTY a non-negative integer, each item once); orders.txt has lines `ITEM QTY`, applied in file order, each subtracting QTY from that item's current stock. Errors are checked line by line through orders.txt, so the first offending line decides: a line that is not exactly two fields separated by one space with QTY a positive integer prints `bad order: line N` and exits 2; an ITEM not in stock.txt prints `unknown item: ITEM (line N)` and exits 3; a QTY larger than the item's remaining stock prints `insufficient stock: ITEM (line N)` and exits 4, where N counts orders.txt lines from 1. If either file does not exist print `missing input` and exit 5. On any error write no file. On success write ./out/stock.txt with the stock.txt lines in their original order and format but updated quantities, print `applied K orders` where K is the number of lines in orders.txt, and exit 0. Every printed line ends with a newline.

## Inputs
- ./data/stock.txt
- ./data/orders.txt

## Outputs
- ./out/stock.txt

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
