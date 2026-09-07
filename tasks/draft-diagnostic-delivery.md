# DRAFT — Diagnostic delivery completeness

**Status:** DRAFT — not reviewed, not scheduled. Must pass a build-loop Step 1
adversarial review before anyone runs Step 0 against it.
**Source:** `tasks/opportunities.md` O-RLM-01, O-RLM-02, from the 2026-08-06
build-loop over `AXON_FOR_RLM.md`.
**Risk class:** Standard (touches ten CLI verbs; no new public surface)

## Why

The RLM build fixed `axon run`: it now emits the same structured, located,
help-carrying diagnostics `axon check` does. That fix was scoped to one verb
because that is the verb the spec measured.

`run_check_pipeline` — whose entire body is `format!("[{code}] {message}")` over
typed diagnostics, because it passes `""` as the source so no span can resolve —
has **eleven** callers. Ten remain. Each drops `help`, `file`, `line`, `col`,
`expected` and `found`, so `axon test` and `axon deploy` report diagnostics with
no location, and the containment refusal (E1001) that `deploy` exists to surface
arrives without its help — the exact defect §2b fixed for `check`/`run`.

> **[REVISED: the draft's line numbers were already stale when written — `main.rs`
> had been edited by the same session that wrote them. A spec that sends the next
> reader to the wrong lines is worse than one that omits them, so here is the
> list re-derived at `f16626b`, by enclosing function, which does not rot when
> lines shift.]**

| # | enclosing fn | verb | how it consumes the strings |
|---|---|---|---|
| 1 | `cmd_target_mobile` | `target mobile` | prints `error: {e}` |
| 2 | `mobile_emit_object` | (internal) | **discards them** — binds `_e`, wants only the infer ctx |
| 3 | `build_wasm_object_cli` | `target wasm` | prints `error: {e}` |
| 4 | `cmd_build_bpf` | `build --bpf` | prints, then returns `Err(count)` |
| 5 | `cmd_goal` | `goal` | `emit_error` + tty/JSON switch — **the same shape `cmd_run` had** |
| 6 | `cmd_test` | `test` | prints |
| 7 | `run_build_pipeline` | `build` | prints |
| 8 | `cmd_ast_review` | `ast review` | JSON consumer |
| 9 | `cmd_deploy` | `deploy` | JSON consumer; embeds text into `axon-deploy/1`'s `message` |
| 10 | `cmd_redteam` | `redteam` | JSON consumer; embeds text into `axon-redteam/1`'s `message` |

Four classes, not one, and #2 is the awkward one: it emits nothing, so converting
it changes no observable behaviour and **cannot be covered by a test** — yet B2
cannot delete the wrapper until it is converted. That is stated here so it is not
mistaken for an oversight later.

Decision D2 of the original build assumed this wrapper had one caller and could
simply be deleted. That was wrong, and it is why this is a separate spec rather
than scope creep in the last one.

## Items

### B1 — convert the remaining ten callers

Each becomes `run_check_pipeline_located(program, &src, &path)` plus
`emit_pipeline_diag`, the two helpers `cmd_run`/`cmd_check` already share.

> **[REVISED: the external-contract risk is real but lands somewhere other than
> the draft claimed.]** The draft worried that changing #8/#9/#10 would break
> `axon-web`, which shells out to the CLI. Probed: `run_json_merged`
> (`crates/axon-web/src/api.rs:140`) reads **stdout** and takes the last valid
> JSON line, while every diagnostic is written to **stderr**. The two are
> disjoint, so **no `axon-*/1` schema is affected** and the web UI cannot break
> on a diagnostic change.
>
> What *does* change: #9 and #10 fold diagnostic text into their report's
> `message` field, which the UI displays. So the words a user sees change — they
> get a location and a hint they did not have. That is the intended improvement,
> not a break, but it is a user-visible change and belongs in the commit message
> rather than being discovered by whoever is looking at the pane.

### B2 — delete `run_check_pipeline` once callerless

This is what D2 originally intended and could not have. Two functions differing
only in how much they discard is how the defect arose; once nothing calls the
lossy one, it goes. If a caller genuinely wants strings, it converts at its own
call site.

### B3 — generalise the equivalence gate to a verb × corpus matrix

T-R3's `run_and_check_emit_identical_diagnostics_across_a_corpus` asserts `run`
and `check` agree byte-for-byte over every diagnostic-producing program in
`examples/` plus hand-written cases. Extend the same test to every converted
verb: **every verb that type-checks a program must emit the same diagnostics for
it.** That is one assertion covering all ten conversions, and no partial
conversion satisfies it.

Keep the existing minimum-comparison-count assertion, so a corpus that silently
matches nothing cannot pass vacuously.

> **[REVISED: the matrix cannot naively include every verb — some EXECUTE.]**
> `axon deploy` runs the program through its gate chain and then runs it;
> `axon test` runs its `@[test]` functions; `axon goal` runs the goal loop. A
> matrix that invokes them over a corpus would execute arbitrary example
> programs as a side effect of a diagnostics test.
>
> The guard already exists and is the one T-R3 used: **ask `check` first and
> compare only where `check` already fails.** A program that fails type-checking
> stops at the diagnostic stage under every verb, so nothing executes. Any verb
> that does not type-check at all (`fmt`, `doc`) is out of the matrix by
> definition rather than by exception.

### B4 — help at the resolve tier (`const`, `var`)

`AXON_FOR_RLM.md` §1 names both. Probing showed they lex as ordinary identifiers
and fail at name **resolution** (`cannot find name \`const\` in this scope`), so
`parse_help` is never called for them and cannot be. Same fix one tier down: a
help row on the unresolved-name diagnostic when the name is a known foreign
keyword. Currently pinned as a negative test
(`parse_help_probe.rs::const_and_var_do_not_reach_the_parse_tier`) so the tier
fact is not re-discovered — that test must be updated, not deleted, when this
lands.

## Open questions (resolve at Step 2)

1. **Do any of the ten verbs deliberately want terse output?** `axon fmt` and
   `axon doc` are not compiler-diagnostic surfaces in the same sense. Check each
   before assuming uniformity is desirable — the goal is that no verb *silently
   discards* information, not that every verb prints identically.
2. **Is the `[CODE] message` string format load-bearing anywhere?** Any consumer
   parsing it (a script, a test asserting on it) breaks. B3's matrix will catch
   test consumers; external scripts it will not. Grep `scripts/` before B2.
3. **Does B4 belong here or with the RLM measurement spec?** It is a diagnostics
   change, but its *evidence* is the fluency number. Proposal: build it here,
   measure it there.

## Acceptance

- Ten callers converted; `run_check_pipeline` deleted.
- The verb × corpus matrix passes, with a minimum-count assertion.
- `cargo test --workspace` shows no new failures against the then-current
  baseline (`tasks/baseline-rlm.md` records the method; re-capture, do not reuse
  the numbers).
- One worked example in the commit: `axon deploy` on a containment violation now
  showing file, line and help.

---

## Decisions — build-loop Step 2, 2026-08-06 (second loop)

### E1 — B1's open Q1: verbs that "want terse output" (engineering)

Resolved: **uniformity of information, not of formatting.** Every verb that
type-checks gets the typed diagnostic; each keeps its own presentation. `fmt` and
`doc` do not type-check and are not in scope at all — they were named in the
draft's question by assumption, not by inspection, and inspection removes them.

### E2 — B1's open Q2: is `[CODE] message` load-bearing anywhere? (engineering)

Resolved by probe, not by assumption. `axon-web` reads stdout JSON, not stderr
diagnostics, so it is unaffected. Before B2 deletes the wrapper, `grep -rn` over
`scripts/` and `crates/*/tests/` for consumers matching on the flattened form;
any hit becomes a conversion at that consumer, not a reason to keep the wrapper.

### E3 — B4's placement (engineering)

Build it here (it is a diagnostics change), measure it in the A-spec's harness.
But **subject to A2b**: it must not land before A1 is measured, or the ceiling
measurement confounds a repair-channel change with new help content.

### E4 — caller #2 (`mobile_emit_object`) has no observable behaviour (engineering)

It discards diagnostics. Converting it is a no-op for output, so the fail-first
rule does not apply and no honest regression test exists. Its gate is the
behaviour-PRESERVING one from the verification bar: the wrapper deletes cleanly
and the full suite stays at baseline. Do not contrive a test for it.

### E5 — `needs-human`: none in this spec

Every question here is an implementation call. The one `needs-human` across both
specs is the A-spec's task-set expansion (Open Q2), resolved as: do not grow
`r9::TASKS`; run more trials instead.
