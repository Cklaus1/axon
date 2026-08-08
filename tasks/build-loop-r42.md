# Build harness — R42 stdlib gaps

Spec: `governance/specs/R42-stdlib-gaps.md` · Branch: `rlm-engine-diagnostics`

Named `build-loop-r42.md` rather than `build-loop.md` because that filename holds a previous run's
harness; clobbering it would destroy the record of what that run decided.

## Resolved placeholders

| slot | value | how |
|---|---|---|
| `<TEST_CMD>` | `cargo test --workspace` | Cargo workspace at root |
| `<FULL_SUITE_BUDGET>` | ~9 min | one measured run (`cli_run` alone is 488s) |
| `<RUN_CMD>` | `./target/debug/axon run <file>` | the interpreter CLI |
| `<SMOKE_SCENARIO>` | `examples/stdlib/r42_smoke.ax` — sums a JSON array, iterates a non-ASCII string by character, round-trips a file. Signal: exact stdout `6 / 4 / onetwo` and exit 0 | built in T11 |
| `<REVIEW_MODEL>` | fable | ran Step 1; folded in at 4634ade |
| `<RUN_BUDGET>` | unbounded | none given; compound stop condition + poison ceiling govern |

## Step 0 baseline (verbatim)

```
cargo test --workspace  →  1808 passed, 0 failed, 1 ignored
failure set = {}   (empty — every later gate diffs against this set, both directions)
```

Valid for the current tree: the only commits between that run and now (`32368ef`, `4634ade`) touch
`governance/specs/R42-stdlib-gaps.md` and nothing else. An empty baseline set means ANY failure is a
new failure, and also that an unexpected *pass* cannot signal drift — so drift detection here relies
on the count moving down without a task claiming it.

## Step 2 decisions (canon)

| # | question | decision | class |
|---|---|---|---|
| D1 | Q1 — does Slice 1 break callers? | No. All 18 in-repo `str_slice` callers slice ASCII; no `examples/*.ax` uses it; both non-ASCII fuzz rows probe character boundaries. Land it first. | engineering |
| D2 | Q2 — codepoints or graphemes? | Codepoints. Graphemes need a segmentation table and no measured failure needs them. | engineering |
| D3 | Q3 — `file_remove` capability policy | **`needs-human`.** Irreversible deletion whose risk-typing integration (R11) is unresolved. `file_remove` is EXCLUDED from this run; the other five fs ops build. | **needs-human** |
| D4 | Q4 — who owns hashing? | No task: crypto is already out of scope. Carried to the report as an unresolved cross-spec ownership question. | engineering |
| D5 | Q5 — `classify_call` widening | Per-ARGUMENT `&[(IoKind, usize)]`, not `&[IoKind]`. Lands in its OWN commit before any new builtin uses it, with a new test proving a write-only contained fn cannot `file_copy` out of a read-denied path. | engineering |
| D6 | Q6 — expected-value parity rows generally | Out of scope; logged to `opportunities.md`. This spec adds them for its own slices only. | engineering |
| D7 | slice ordering | Regex LAST: largest effort, lowest measured cost (its task passed by hand-rolling). Soundness first, then the measured failure (JSON), then fs, then userland. | engineering |
| D8 | the card update Slice 1 mandates | The language card lives in the ATLAS repo (`atlas-axon-card/spikes/rlm-engine/src/axon_card.rs`), so this is a cross-repo task, done in the same window as T1 per §2. | engineering |

**Why D3 is not solved with a flag.** Building `file_remove` behind a default-off switch would be
built-but-uncalled code shipping in the binary with an undecided policy — the exact compromise this
harness forbids. It stays unbuilt.

## Task DAG

```
T0  classify_call → per-argument mapping (refactor, no new builtin)     [D5, gate: existing cap tests green + new exfil test]
T1  str_slice UTF-8 boundary refusal (E2200), interp + native   ★CRITICAL PATH
T2  language card: retire the slicing idiom for the byte idiom  (atlas repo)   ← T1
T3  utf8_boundary_parity.sh — EXPECTED-VALUE gate, not agreement-only          ← T1
T4  char access: str_chars/str_len_chars/str_char_at/str_char_slice,
    char_code, char_is_digit/alpha/space; + chr native lowering                ← T1
T5  JSON read: json_get_json/json_path_json/json_path_i64/json_path_f64/
    json_len/json_at/json_keys/json_arr_i64/_f64/_str
T6  JSON construct: json_from_pairs/dict_to_json/json_arr_from_i64/_f64/_str   ← T5
T7  filesystem (5 ops; file_remove EXCLUDED per D3): file_exists, dir_create,
    dir_list, file_copy, file_rename — default-deny Host methods               ← T0
T8  encoding: base64_encode/decode, hex_encode/decode (E2204 non-UTF-8)
T9  userland examples/stdlib/date.ax   (+ pre-1970 test per B7)
T10 userland examples/stdlib/set.ax, examples/stdlib/path.ax (path traversal test)
T11 smoke scenario examples/stdlib/r42_smoke.ax                     ← T4, T5, T7
T12 regex: Pike VM, leftmost-first, {n,m} cap, pattern cache, 6 fns  [D7, last]
```

Acyclic; topological order `T0 T1 T2 T3 T4 T5 T6 T7 T8 T9 T10 T11 T12` covers every non-needs-human
spec item exactly once. Disjoint-scope parallel candidates: {T8, T9, T10} (userland `.ax` + a pure
builtin pair, no shared file), {T2} (different repo entirely).

**Critical path: T1.** It changes shipped behaviour that T4's justification and the card both depend
on. Extra gate beyond a regression test: T3 asserts EXPECTED VALUES plus interp↔native byte-identical
output *and* identical exit codes over a boundary corpus — the closest invariant-check equivalent to a
migration's round-trip proof, and specifically not an agreement-only check, since §2 showed agreement
oracles are blind to bugs both engines share.

## Loops

- **Inner** (per task): fail-first test → implement → mutation-verify (break it the specific way the
  test claims to catch; assert the scripted edit landed, match count == 1) → self-review checklist →
  caller check (grep for a non-test caller of every new builtin; a builtin with no `examples/` or
  test-corpus caller outside its own test is not built) → `cargo clippy` per `scripts/gate.sh`'s
  allowlist → artifact scan (`grep -P`, check exit code) → commit.
- **Outer**: next ready DAG node, respecting edges and D7.
- **Registration check** (R42-specific, replaces nothing): every new builtin must appear at ALL FOUR
  sites — `BUILTINS`, interp dispatch, `capabilities.rs` where applicable, `wasm_parity.sh`
  `HOST_BUILTINS` for host-touching ones. A builtin registered at three of four is the failure mode
  `divergent_builtins_are_excluded_from_the_wasm_pure_corpus` caught during the previous slice.
- **Full-suite regression**: after each tier, `cargo test --workspace`, diff the failure set BOTH ways
  against `{}`; a count below 1808 without a task claiming it means baseline drift → re-capture, and
  treat prior gate results as unverified.
- **Smoke**: once per tier from T11 onward, `<RUN_CMD>` on `r42_smoke.ax`, bounded timeout, a hang is
  a failure.
- **Meta**: `lessons.md` on corrections/failures only; `opportunities.md` for anything discovered
  without a task.

## Poison-task ceiling

Append to `tasks/attempts.log` BEFORE each attempt: `<iso8601>  <task-id>  attempt <N>  <plan>`.
Attempt 3 fails → park `blocked`, block dependents, continue. A retry whose normalized plan matches
the previous attempt's is not a retry — state what is materially different or park it.

## Stop condition

```
DONE = every non-needs-human node DONE or blocked-and-logged
   AND cargo test --workspace shows no NEW failures vs {} (i.e. still 0 failed)
   AND every spec item addressed or tagged needs-human   (file_remove = needs-human)
   AND axon run examples/stdlib/r42_smoke.ax prints 6 / 4 / onetwo and exits 0
```
