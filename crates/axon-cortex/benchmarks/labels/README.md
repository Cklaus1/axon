# Labels from real `cortex repair` runs

Every benchmark here so far uses injected defects (`../ab/corpus.json`). A
`cortex repair` run that ends `verified_done` produces a label nobody had to
write: the hidden check accepted a new body of one function, and no check in
the file still fails. `--label-log` records that run as one JSON line.
`to_items.py` turns those lines into items for the two localization
benchmarks, so real use can grow the dev set.

```bash
cortex repair --file F --check HIDDEN ... --label-log labels.jsonl [--label-source]
CORTEX_LABEL_LOG=labels.jsonl cortex repair ...      # same; the flag wins over the env
python3 to_items.py convert labels.jsonl --out items  # -> items/{locrank_items,decision_loc}.jsonl, summary.json
python3 to_items.py selftest                          # $0, no binaries, no network
bash smoke.sh                                         # 3 repairs on pricing.ax copies, then convert
```

## Which runs write a row

Exactly one line per run that reached the candidate walk and finished it,
whatever its outcome: exit 0, 20–24, 25 when a named `--symbol` does not
exist, and 27. A run that stops before the walk, or aborts inside it, writes
nothing:

- exit 2 (usage, including a snapshot of the file that cannot be read);
- exit 22 when the checker cannot run before the walk, or the file cannot be
  restored between attempts;
- exit 25 when localization finds no target;
- exit 26 (the check already passes).

The row is written after the verdict, the exit code, the file and the audit
are settled. It is one `write(2)` on an `O_APPEND` descriptor, so concurrent
runs sharing a log never interleave inside a line. A new log file is created
with mode 0600. If the write fails (missing directory, full disk, permission),
the run prints one line, `label log PATH: cannot append (...); the verdict and
exit code are unaffected`, and exits exactly as it would have. A short write
is reported, not retried, because a second write could split the line around
another process's line.

## `cortex-label/1` fields

| field | type | meaning |
|---|---|---|
| `schema` | `"cortex-label/1"` | readers refuse any other value |
| `ts` | string | UTC, `YYYY-MM-DDTHH:MM:SSZ`, when the row was written |
| `cortex.version` | string | crate version of the binary |
| `cortex.sha256` | string \| null | sha256 of the running executable; null if it could not be read |
| `file` | string | the `--file` value, workspace-relative |
| `file_sha256` | string | sha256 of the file as the run FOUND it, before any patch |
| `check` | string | the adjudicating (hidden) check |
| `symbol` | string \| null | `--symbol` when the operator named the target; then `localization` and `ranker` are null |
| `generator` | string | the `--generator` spec; a `literal:` body is withheld and recorded as `literal` |
| `budget` | int | `--budget`, steps per candidate |
| `localization.failing` | [string] | visible failing checks (the hidden one excluded) |
| `localization.passing` | [string] | visible passing checks (the hidden one excluded) |
| `localization.ranked` | [{`symbol`, `score`}] | the full spectrum (Ochiai) ranking, most suspicious first |
| `localization.candidates` | int | `--candidates` |
| `localization.eligible` | [string] | the spectrum's top `candidates`: the set the ranker may only permute |
| `ranker` | object \| null | the ranker report, as `ranker` in `cortex repair --json`: `requested`, `used`, `reason`, `order`, `latency_ms`, `cost_usd` |
| `attempted` | [string] | every attempt in the order it ran; under the breadth-first walk a resumed candidate appears twice |
| `proposals` | [{`symbol`, `proposals`, `steps`}] | per attempted candidate, in walk order: generator calls and steps across all its resumptions |
| `outcome` | string | `verified_done`, `budget_exhausted`, `no_progress`, `blocked`, `refused`, `needs_input`, `no_such_symbol`, `adjudicated_not_clean` |
| `exit_code` | int | the process exit code |
| `steps` | int | steps of the final episode, as `steps` in `--json` |
| `truth` | string \| null | the repaired symbol when `outcome` is `verified_done`; null otherwise, including exit 27 |
| `audit` | object \| null | the post-fix audit report (`verdict` `clean`/`suspect`/`unavailable`/`off`); null unless verified |
| `source` | string \| null | the file's text as found; ONLY with `--label-source`, else null |

What `truth` means: a function whose new body made the hidden check pass with
no check in the file left failing. On a file with one defect, that is the
defective function. It is not proof that no other fix site existed. A fix in a
caller that compensates for a callee's bug also verifies. The audit verdict is
carried into each item's provenance so such items can be filtered.

## Privacy

A row without `--label-source` holds names (file, functions, checks), scores,
digests and counts, but no code. With `--label-source` it holds the whole file
as found, and the file may contain anything: credentials in fixtures,
proprietary code, personal data in test strings. So:

- source capture is opt-in per run; `CORTEX_LABEL_LOG` alone never records it;
- a new log is created 0600;
- `to_items.py` copies the source into its items (the benchmarks need it).
  Treat the items directory like the log;
- a row records the generator kind but never a `literal:` body.

Before sharing a log or items, check that every file in them may leave the
machine. A model benchmark that sends an item to a provider sends its source
too.

## `to_items.py`

`convert` reads a `cortex-label/1` JSONL and writes three files:

- **`locrank_items.jsonl`**: `{"id", "task", "loc", "provenance"}`. `task`
  has the corpus-task fields `../locrank/locrank.py` reads (`id`, `class`,
  `file`, `symbol`, `visible`, `hidden`, `broken`, `given_a`). `given_a` is the
  source with the hidden check's test block cut out (`ab/corpus.py`
  `cut_test`). `loc` has the `results/locate.jsonl` fields (`truth`, `ranked`,
  `eligible`, `spectrum_rank`, `in_eligible`, `tied_at_top`, `tied_with_truth`,
  `failing_visible_to_cortex`). Pass both to `locrank.rank_task(task, loc,
  call, provider)` unchanged; the selftest does that with the scripted oracle,
  reverser and adversary providers.
- **`decision_loc.jsonl`**: DecisionBench items with `cat: "loc"`, the shape of
  `crates/axon-reflex/benchmarks/decisions/decision_set.jsonl`. The state is
  the source without the hidden check (an excerpt above 12,000 characters, the
  same rule as `build_set.py`), plus `failing_test`. `question.criteria` maps
  each eligible candidate to its signature, `label` is `truth`, and
  `incumbent.order` is the spectrum order.
- **`summary.json`**: row and item counts, and every skip with its reason.

The id of both item kinds is `label-<file_sha256[:12]>-<check>` (DecisionBench
prefixes it with `loc/`). `class` is `field`.

`visible` (and the DecisionBench `failing_test`) is the first entry of
`localization.failing` that is a test block in the source. The corpus tasks
have one visible check; a field run can have several. The full list is kept in
`loc.failing_visible_to_cortex`. The production ranker (`src/rank.rs`) shows
the model every visible failing check, so a replay through `locrank.py` shows
it less than `cortex repair` did.

Rows that become no item are counted per output under one of these reasons:

| reason | cause |
|---|---|
| `schema is ..., not cortex-label/1` | not a label row |
| `not JSON: line N: ...` | a line that does not parse |
| `no truth: outcome X` | the run did not verify |
| `no localization: the run was given --symbol` | nothing was ranked, so there is no candidate set |
| `no source: the run did not pass --label-source` | the item needs the text |
| `source does not match file_sha256` | the recorded text is not what the run saw |
| `truth outside the eligible set` | cannot happen for a walked run; checked anyway |
| `the adjudicating check is not a test block in the source` | the hidden check cannot be cut out |
| `no visible failing check is a test block in the source` | no failing test to show |
| `duplicate of an earlier row (same file_sha256 and check)` | the same question again; the first row wins |
| `single candidate: not a decision` | DecisionBench only, as in `build_set.py`; the LocRank item is still written |
