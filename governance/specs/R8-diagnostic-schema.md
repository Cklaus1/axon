# Tech Spec — R8: Versioned Machine-Stable Diagnostic Schema

**Status:** ✅ Reviewed (2026-06-02)
**Requirement:** `../REQUIREMENTS.md` R8 — *Built-in testing + structured errors; AI-parseable diagnostics.*
**Decisive fork:** *What is the stable JSON shape an external tool (or an AI agent) parses, and how is it versioned so the shape can evolve without silently breaking consumers?* Today `axon check --json` emits an unversioned, unstructured `{"error": "<whole message string>"}` — a consumer must regex the message to recover the code, and any wording change breaks it. **→ Resolved below.**

---

## 1. Motivation

`emit_error(msg, as_json=true)` (`main.rs`) currently emits one NDJSON object per error: `{"error": "[E1234] message text\n  help: ..."}`. The error *code* (E1234), *severity*, and *fix hint* are all buried in the prose `msg` string. An AI agent or editor integration that wants to branch on "is this E1300 (offline AI)?" must regex the blob — fragile, and there is no version tag, so the day the wording changes, every consumer breaks silently.

R8's remaining gap (per REQUIREMENTS.md) is exactly: *"error JSON schema not yet versioned (machine-stable diagnostics)."* This spec settles the **schema** and makes the emitter produce it.

Design-only constraint already in force: **no `serde_json`** in this binary (it collides with `inkwell`'s trait universe — see CLAUDE.md). The schema is emitted by hand-rolled JSON (the same discipline as `lockfile.rs` / `manifest.rs` / the existing `emit_error`). This is a small, self-contained slice: a new module + a one-call-site change in `emit_error`.

---

## 2. Requirement link

`../REQUIREMENTS.md` **R8** (82%). Quoted gap: *"error JSON schema not versioned (machine-stable diagnostics)."* This slice closes it for the **emit** side (`axon check --json` and any `as_json` emit path). Acceptance is a stable, versioned, parse-without-regex shape.

---

## 3. Surface (what a consumer sees)

`axon check --json prog.ax` emits one JSON object per line (NDJSON). Each object:

```json
{"schema":"axon-diag/1","severity":"error","code":"E1300","message":"`ai_complete` cannot run: no model reachable ...","help":"declare a fallback to run offline"}
```

- `schema` — **constant version tag** `"axon-diag/1"`. A consumer asserts this and knows the field set. A future breaking change becomes `"axon-diag/2"`; consumers detect the bump instead of silently mis-parsing.
- `severity` — `"error"` | `"warning"` | `"note"`.
- `code` — the diagnostic code (`E1300`, `W0003`, `I0001`, …) extracted as a first-class field, or `""` if the message carried none.
- `message` — the human message **with the `[CODE]` prefix and any `help:` line stripped out** (those are now their own fields), single-line (newlines escaped).
- `help` — the fix hint if the message had a `help:`/`fix:` line, else the field is **omitted** (not `null`).

Non-JSON (human) output is unchanged: `error: [E1300] message…`.

---

## 4. Semantics

### 4.1 Where the structure comes from

Errors arrive at the emitter as already-formatted strings of the shape the codebase uses everywhere:

```
[E1300] <message text>
  help: <fix hint>            (optional, indented continuation lines)
```

(also `[W0003]`, `[I0001]`; some have no code prefix at all.) The schema is recovered by **parsing that string**, not by threading structured `AxonError` through every call site (that would be a large refactor; out of scope). The parse is total and lossless: anything it can't classify lands in `message` verbatim.

### 4.2 The parse (deterministic, total)

Given a raw diagnostic string:
1. **Code:** if it starts with `[` `<CODE>` `]` where `<CODE>` matches `^[EWI]\d{4}$`, extract `<CODE>` and strip the `[CODE] ` prefix. Else `code=""`, message unchanged.
2. **Severity:** `E####`→`error`, `W####`→`warning`, `I####`→`note`. No code → default `error` (the caller's `as_json` path is the error path).
3. **Help:** split off any line whose trimmed form starts with `help:` or `fix:`; join the remainder of those lines as `help` (trimmed). The first line(s) before it are the `message`.
4. **message:** the remaining text, newlines replaced with `\n` literal escapes so each diagnostic is exactly one NDJSON line.

### 4.3 Emission

A new `diag_schema.rs` module owns:
- `pub const DIAG_SCHEMA: &str = "axon-diag/1";`
- `pub fn diagnostic_json(raw: &str) -> String` — parse `raw`, return the one-line JSON object (hand-rolled escaping; `help` omitted when absent).
- The JSON string escaping helper (reuse the `\"`/`\\`/`\n` shape from `manifest.rs`).

`main.rs::emit_error(msg, as_json=true)` calls `diagnostic_json(msg)` instead of the current inline `{"error": ...}`.

### 4.4 Behavior table

| Input string | Emitted JSON |
|---|---|
| `[E1300] no model reachable` | `{"schema":"axon-diag/1","severity":"error","code":"E1300","message":"no model reachable"}` |
| `[W0003] fn shadows builtin\n  help: rename it` | `{…,"severity":"warning","code":"W0003","message":"fn shadows builtin","help":"rename it"}` |
| `plain message, no code` | `{…,"severity":"error","code":"","message":"plain message, no code"}` |
| message containing `"` or `\` | escaped correctly; still one line |

### 4.5 Determinism

The parse is a pure function of the input string — same input → byte-identical JSON. No clock, no env. Field order is fixed (`schema`, `severity`, `code`, `message`, `help`).

---

## 5. Invariants touched

- **I-8/I-9 (success signal):** a machine consumer can now reliably distinguish error/warning/note and branch on the exact code — strengthens the success-signal story. **Preserved+.**
- **I-14 (stable codes):** the code is now a first-class field, not buried in prose; the `schema` tag versions the *shape* the way I-14 versions the *codes*. **Aligned.**
- No semantic invariant changes — this is an output-format addition. Human output is byte-identical to today.

---

## 6. Error codes

None new — this spec *exposes* existing codes in a structured field; it does not introduce diagnostics of its own.

---

## 7. Test plan

Red test that must fail first: **`diagnostic_json_is_versioned_and_structured`** — assert `diagnostic_json("[E1300] msg")` contains `"schema":"axon-diag/1"`, `"code":"E1300"`, `"severity":"error"`, `"message":"msg"`. Fails today (no such function; the emitter produces `{"error": "[E1300] msg"}`).

- [ ] **Unit (diag_schema.rs):** code extraction for E/W/I prefixes; severity mapping; help-line split; no-code fallback (`code=""`, severity `error`); quote/backslash/newline escaping → still one line; `help` omitted when absent (key not present, not `null`).
- [ ] **CLI e2e (cli_run.rs):** `axon check --json` on a program with a known error (e.g. a type mismatch → E0102, or an offline `ai_complete` → E1300) emits a line containing `"schema":"axon-diag/1"` and `"code":"E<...>"`; the human (non-`--json`) path is unchanged (`error: [E...]`).
- [ ] **Determinism:** `diagnostic_json(x) == diagnostic_json(x)` for representative inputs.

## 8. Acceptance criteria

R8 rises toward DONE when **all** pass:
- [ ] `diagnostic_json_is_versioned_and_structured` (the schema exists, is tagged `axon-diag/1`, exposes code+severity+message).
- [ ] `diagnostic_json_handles_help_and_no_code` (help split + no-code fallback).
- [ ] `diagnostic_json_escapes_and_stays_single_line`.
- [ ] `check_json_emits_versioned_schema` (CLI e2e — real error through `axon check --json`).
- [ ] human output unchanged (a non-`--json` check still prints `error: …`).

R8 may rise 82% → ~90% on this slice (the emit-side machine-stable schema). A fully *typed* end-to-end diagnostic (structured `AxonError` threaded to emit, location fields populated) is a larger follow-on; this slice delivers the versioned, parse-without-regex shape the gap names.

## 9. Scope / non-goals

- **In:** the `axon-diag/1` schema, `diag_schema.rs`, wiring `emit_error`'s JSON path, tests.
- **Out:** populating `line`/`col` location fields (errors arrive as strings without them here — a typed-diagnostic refactor); changing any human output; new diagnostic codes.
