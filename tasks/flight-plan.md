# Flight plan — R42 stdlib gaps

**Mission:** close the six stdlib gaps a 16-task benchmark measured, starting with a UTF-8 soundness
bug that makes `str_slice` silently return `""` on non-ASCII input.

## Step 2 decisions

- Q1 does Slice 1 break callers? → **No, land it first** — all 18 in-repo callers slice ASCII, no
  `examples/*.ax` uses `str_slice`, both non-ASCII fuzz rows probe character boundaries.
- Q2 codepoints or graphemes? → **codepoints** — graphemes need a segmentation table, no measured
  failure needs them.
- Q3 `file_remove` capability policy → **needs-human, excluded** (see below).
- Q4 who owns hashing? → **no task** — crypto already out of scope; carried to the report as an
  unresolved cross-spec ownership question.
- Q5 `classify_call` widening → **per-ARGUMENT `&[(IoKind, usize)]`**, own commit, before any new
  builtin uses it — a kind *list* checks the source path twice and the destination not at all.
- Q6 expected-value parity rows generally → **out of scope**, logged to opportunities; this spec adds
  them for its own slices.
- D7 ordering → **regex last**: largest effort, lowest measured cost (that task passed hand-rolled).
- D8 card update → **cross-repo**, into the atlas card, same window as T1 per §2.

## Excluded: needs-human

**`file_remove` (Q3).** Irreversible data deletion whose risk-typing integration (R11) is unresolved.
Pruned subtree: 1 of 6 filesystem ops; the other 5 build. Not solved with a default-off flag — that
would ship built-but-uncalled code under an undecided policy.

## Step 1 revisions folded in (at 4634ade)

Four factual corrections (`json_path_str` DOES index arrays · `ln` exists · `char_from_code`
duplicated the existing `chr` · no `BrowserHost` impl, and the Host trait already has default-deny
methods) · regex semantics replaced leftmost-longest → leftmost-first/Pike VM · `&[IoKind]` found
insufficient → per-argument · missed gap added (JSON construction) · two review findings rejected with
evidence (`chr(34) + "abc"` type-checks; JSON literals ARE writable via `{{`).

## Critical path

**T1 — `str_slice` boundary refusal.** Extra gate (T3): expected VALUES plus interp↔native
byte-identical output and identical exit codes over a boundary corpus — deliberately not
agreement-only, since agreement oracles are blind to bugs both engines share (that is how this bug
survived).

⚠ close call: **T1 lands before T4**, so between them the card's taught idiom becomes a *panic* on
non-ASCII where it was silently wrong. Loud beats wrong, but a benchmark re-run inside that window
will dip — predicted here so it is not read as a regression.

## Shape

13 tasks · 6 tiers · longest chain T0→T7→T11 (3) · parallel candidates {T8,T9,T10} and {T2} ·
`cargo test --workspace` · budget unbounded · baseline **1808 passed / 0 failed / failure set `{}`**.

## First 3 tasks

1. **T0** — `classify_call` per-argument refactor, no new builtin, existing capability tests green.
2. **T1** — `str_slice` E2200 refusal in interp + native (critical path).
3. **T2** — retire the card's slicing idiom for the byte idiom (atlas repo).
