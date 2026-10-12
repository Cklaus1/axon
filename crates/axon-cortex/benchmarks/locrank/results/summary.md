n = 60 tasks; eligible = spectrum top-6

| provider | n | truth in top-6 | top-1 | top-3 | MRR | abstain | p50 s | p90 s | $/task | $ total |
|---|---|---|---|---|---|---|---|---|---|---|
| `claude-cli:claude-haiku-4-5-20251001` | 60 | 60 | 59 | 60 | 0.9889 | 1 | 7.23 | 9.38 | 0.01621 | 0.9723 |
| `openrouter:anthropic/claude-haiku-5.5` | 60 | 60 | 60 | 60 | 1.0 | 1 | 2.02 | 3.47 | 0.00068 | 0.0408 |
| `openrouter:anthropic/claude-haiku-5.5#reasoning=off` | 60 | 60 | 48 | 59 | 0.8839 | 3 | 1.08 | 1.32 | 0.00055 | 0.0331 |
| `openrouter:cohere/north-mini-code:free` | 60 | 60 | 56 | 60 | 0.9639 | 11 | 12.16 | 29.71 | 0.00000 | 0.0000 |
| `openrouter:cohere/north-mini-code:free#reasoning=off` | 60 | 60 | 30 | 51 | 0.6906 | 0 | 0.48 | 0.85 | 0.00000 | 0.0000 |
| `openrouter:nvidia/nemotron-3-super-120b-a12b:free` | 60 | 60 | 52 | 56 | 0.9114 | 8 | 9.19 | 39.85 | 0.00000 | 0.0000 |
| `openrouter:nvidia/nemotron-3-super-120b-a12b:free#reasoning=off` | 60 | 60 | 41 | 55 | 0.7978 | 4 | 0.87 | 1.76 | 0.00000 | 0.0000 |
| `openrouter:nvidia/nemotron-3.5-lightning:free` | 60 | 60 | 41 | 56 | 0.8081 | 30 | 44.88 | 60.34 | 0.00000 | 0.0000 |
| `openrouter:nvidia/nemotron-3.5-lightning:free#reasoning=off` | 60 | 60 | 31 | 54 | 0.7075 | 2 | 0.76 | 1.45 | 0.00000 | 0.0000 |
| `openrouter:openai/gpt-6-luna` | 60 | 60 | 57 | 59 | 0.9672 | 0 | 3.79 | 6.05 | 0.00053 | 0.0321 |
| `openrouter:openai/gpt-6-luna#reasoning=off` | 60 | 60 | 50 | 59 | 0.8986 | 0 | 1.14 | 1.54 | 0.00044 | 0.0265 |
| `openrouter:qwen/qwen3-coder-next` | 60 | 60 | 42 | 57 | 0.8347 | 1 | 1.08 | 1.62 | 0.00044 | 0.0267 |
| `openrouter:typesafe/jev-router` | 60 | 60 | 60 | 60 | 1.0 | 0 | 2.54 | 4.32 | 0.01064 | 0.6385 |
| `spectrum` | 60 | 60 | 25 | 53 | 0.6494 | 0 | 0.0 | 0.0 | 0.00000 | 0.0000 |

OpenRouter call health (errors after bounded 429/5xx backoff are abstains):

| provider | provider errors | >10% | retries | queue p50/p90 s | upstream |
|---|---|---|---|---|---|
| `openrouter:anthropic/claude-haiku-5.5` | 0 | False | 0 | 0.0/0.0 | {'Azure': 60} |
| `openrouter:anthropic/claude-haiku-5.5#reasoning=off` | 0 | False | 0 | 0.0/0.0 | {'Azure': 60} |
| `openrouter:cohere/north-mini-code:free` | 0 | False | 0 | 0.0/0.0 | {'Cohere': 60} |
| `openrouter:cohere/north-mini-code:free#reasoning=off` | 0 | False | 0 | 0.0/0.0 | {'Cohere': 60} |
| `openrouter:nvidia/nemotron-3-super-120b-a12b:free` | 0 | False | 0 | 0.0/0.0 | {'Nvidia': 60} |
| `openrouter:nvidia/nemotron-3-super-120b-a12b:free#reasoning=off` | 0 | False | 0 | 0.0/0.0 | {'Nvidia': 60} |
| `openrouter:nvidia/nemotron-3.5-lightning:free` | 0 | False | 0 | 0.0/0.0 | {'Nvidia': 60} |
| `openrouter:nvidia/nemotron-3.5-lightning:free#reasoning=off` | 0 | False | 1 | 0.0/0.0 | {'Nvidia': 60} |
| `openrouter:openai/gpt-6-luna` | 0 | False | 0 | 0.0/0.0 | {'OpenAI': 60} |
| `openrouter:openai/gpt-6-luna#reasoning=off` | 0 | False | 0 | 0.0/0.0 | {'OpenAI': 60} |
| `openrouter:qwen/qwen3-coder-next` | 0 | False | 0 | 0.0/0.0 | {'Parasail': 60} |
| `openrouter:typesafe/jev-router` | 0 | False | 0 | 0.0/0.0 | {'OpenAI': 51, 'Amazon Bedrock': 8, 'Together': 1} |

Top-1 split by whether the spectrum's top score is unique (`sole_top`) or shared (`tied_top`):

| provider | sole_top n | top-1 | tied_top n | top-1 |
|---|---|---|---|---|
| `claude-cli:claude-haiku-4-5-20251001` | 23 | 23 | 37 | 36 |
| `openrouter:anthropic/claude-haiku-5.5` | 23 | 23 | 37 | 37 |
| `openrouter:anthropic/claude-haiku-5.5#reasoning=off` | 23 | 21 | 37 | 27 |
| `openrouter:cohere/north-mini-code:free` | 23 | 23 | 37 | 33 |
| `openrouter:cohere/north-mini-code:free#reasoning=off` | 23 | 16 | 37 | 14 |
| `openrouter:nvidia/nemotron-3-super-120b-a12b:free` | 23 | 21 | 37 | 31 |
| `openrouter:nvidia/nemotron-3-super-120b-a12b:free#reasoning=off` | 23 | 21 | 37 | 20 |
| `openrouter:nvidia/nemotron-3.5-lightning:free` | 23 | 21 | 37 | 20 |
| `openrouter:nvidia/nemotron-3.5-lightning:free#reasoning=off` | 23 | 16 | 37 | 15 |
| `openrouter:openai/gpt-6-luna` | 23 | 22 | 37 | 35 |
| `openrouter:openai/gpt-6-luna#reasoning=off` | 23 | 21 | 37 | 29 |
| `openrouter:qwen/qwen3-coder-next` | 23 | 20 | 37 | 22 |
| `openrouter:typesafe/jev-router` | 23 | 23 | 37 | 37 |
| `spectrum` | 23 | 14 | 37 | 11 |

PROJECTION over the arm-B snapshot (60 rows, taken 2026-10-12T17:48:21Z):

| provider | projected rows | proposals (observed -> projected) | wall s (observed / fitted -> projected) | model $ (observed -> projected incl. ranker) |
|---|---|---|---|---|
| `claude-cli:claude-haiku-4-5-20251001` | 54 | 398 -> 168.0 | 13938.4 / 14004.7 -> 5854.9 | 29.555 -> 12.272 |
| `openrouter:anthropic/claude-haiku-5.5` | 54 | 398 -> 160.0 | 13938.4 / 14004.7 -> 5275.0 | 29.555 -> 10.821 |
| `openrouter:anthropic/claude-haiku-5.5#reasoning=off` | 54 | 398 -> 222.4 | 13938.4 / 14004.7 -> 7458.7 | 29.555 -> 15.558 |
| `openrouter:cohere/north-mini-code:free` | 54 | 398 -> 180.0 | 13938.4 / 14004.7 -> 6823.9 | 29.555 -> 12.467 |
| `openrouter:cohere/north-mini-code:free#reasoning=off` | 54 | 398 -> 382.8 | 13938.4 / 14004.7 -> 13834.7 | 29.555 -> 27.253 |
| `openrouter:nvidia/nemotron-3-super-120b-a12b:free` | 54 | 398 -> 228.2 | 13938.4 / 14004.7 -> 8879.1 | 29.555 -> 15.621 |
| `openrouter:nvidia/nemotron-3-super-120b-a12b:free#reasoning=off` | 54 | 398 -> 312.8 | 13938.4 / 14004.7 -> 11102.1 | 29.555 -> 21.979 |
| `openrouter:nvidia/nemotron-3.5-lightning:free` | 54 | 398 -> 306.0 | 13938.4 / 14004.7 -> 13159.7 | 29.555 -> 22.774 |
| `openrouter:nvidia/nemotron-3.5-lightning:free#reasoning=off` | 54 | 398 -> 373.8 | 13938.4 / 14004.7 -> 13443.7 | 29.555 -> 26.625 |
| `openrouter:openai/gpt-6-luna` | 54 | 398 -> 180.1 | 13938.4 / 14004.7 -> 6127.0 | 29.555 -> 12.347 |
| `openrouter:openai/gpt-6-luna#reasoning=off` | 54 | 398 -> 219.4 | 13938.4 / 14004.7 -> 7446.0 | 29.555 -> 15.274 |
| `openrouter:qwen/qwen3-coder-next` | 54 | 398 -> 254.5 | 13938.4 / 14004.7 -> 8890.8 | 29.555 -> 17.859 |
| `openrouter:typesafe/jev-router` | 54 | 398 -> 160.0 | 13938.4 / 14004.7 -> 5315.3 | 29.555 -> 11.419 |
| `spectrum` | 54 | 398 -> 398.0 | 13938.4 / 14004.7 -> 14004.7 | 29.555 -> 29.555 |

Per task, the five slowest arm-B tasks (projected wall s incl. ranker; spectrum rank -> provider rank):

| task | observed s | `claude-cli:claude-haiku-4-5-20251001` | `openrouter:anthropic/claude-haiku-5.5` | `openrouter:anthropic/claude-haiku-5.5#reasoning=off` | `openrouter:cohere/north-mini-code:free` | `openrouter:cohere/north-mini-code:free#reasoning=off` | `openrouter:nvidia/nemotron-3-super-120b-a12b:free` | `openrouter:nvidia/nemotron-3-super-120b-a12b:free#reasoning=off` | `openrouter:nvidia/nemotron-3.5-lightning:free` | `openrouter:nvidia/nemotron-3.5-lightning:free#reasoning=off` | `openrouter:openai/gpt-6-luna` | `openrouter:openai/gpt-6-luna#reasoning=off` | `openrouter:qwen/qwen3-coder-next` | `openrouter:typesafe/jev-router` | `spectrum` |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| operator-02 | 445.8 | 26.2 (5->1) | 19.6 (5->1) | 18.6 (5->1) | 30.2 (5->1) | 442.3 (5->5) | 476.8 (5->5) | 444.5 (5->5) | 494.9 (5->5) | 18.3 (5->1) | 22.3 (5->1) | 19.5 (5->1) | 18.7 (5->1) | 19.5 (5->1) | 441.6 (5->5) |
| operator-04 | 457.5 | 24.4 (3->1) | 19.2 (3->1) | 18.7 (3->1) | 32.0 (3->1) | 170.4 (3->2) | 21.3 (3->1) | 18.2 (3->1) | 400.7 (3->3) | 18.3 (3->1) | 21.1 (3->1) | 19.3 (3->1) | 18.5 (3->1) | 19.6 (3->1) | 357.2 (3->3) |
| operator-05 | 338.2 | 26.2 (3->1) | 20.8 (3->1) | 20.7 (3->1) | 34.5 (3->1) | 19.9 (3->1) | 23.6 (3->1) | 20.4 (3->1) | 359.2 (3->3) | 20.4 (3->1) | 23.9 (3->1) | 20.7 (3->1) | 20.5 (3->1) | 22.4 (3->1) | 311.4 (3->3) |
| operator-07 | 451.8 | 21.1 (3->1) | 18.0 (3->1) | 17.2 (3->1) | 24.3 (3->1) | 16.5 (3->1) | 21.7 (3->1) | 16.7 (3->1) | 70.0 (3->1) | 16.6 (3->1) | 19.4 (3->1) | 17.2 (3->1) | 17.1 (3->1) | 18.2 (3->1) | 469.2 (3->3) |
| operator-11 | 286.0 | 297.5 (3->3) | 18.2 (3->1) | 17.2 (3->1) | 21.5 (3->1) | 462.9 (3->4) | 26.9 (3->1) | 291.2 (3->3) | 349.1 (3->3) | 169.4 (3->2) | 21.9 (3->1) | 17.0 (3->1) | 18.2 (3->1) | 18.3 (3->1) | 289.5 (3->3) |
