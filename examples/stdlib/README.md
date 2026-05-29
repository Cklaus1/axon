# examples/stdlib

Small, pure, reusable Axon helpers — building blocks for the ASI demos in
`examples/asi/`, composed only from existing builtins (no Rust changes).

## `asi_prelude.ax`

Numeric helpers for turning raw model outputs into bounded scores,
confidences, and budget checks:

- `bound_i64` / `bound_f64` — clamp into `[lo, hi]` (inline; the `clamp_*`
  builtins are codegen-only and not implemented in the interpreter).
- `normalize_score(raw, max)` — map `0..max` onto `0..100` (rounded, clamped).
- `score_to_confidence` / `confidence_to_score` — bridge `0..100` ⇄ `0.0..1.0`.
- `length_score(n_chars, ideal, cap)` — generalized summarizer length heuristic.
- `budget_ok` / `budget_remaining` / `budget_used_pct` — budget accounting.
- `weighted2(a, wa, b, wb)` / `mean2(a, b)` — integer weighted average.

Validate:

```bash
./target/debug/axon check examples/stdlib/asi_prelude.ax
./target/debug/axon test  examples/stdlib/asi_prelude.ax   # 8 tests
./target/debug/axon run   examples/stdlib/asi_prelude.ax
```

Note: `clamp_i64`, `clamp_f64`, `min_f64`, `max_f64`, `sign_i64`, `pow_i64`
exist in the type table but are not implemented in the codegen-free
interpreter, so this prelude avoids them at runtime and clamps inline instead.
