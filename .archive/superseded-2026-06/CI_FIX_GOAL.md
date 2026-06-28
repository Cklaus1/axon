# Goal: make the CI gate green

The roadmap work is functionally complete (1,126 Rust tests + ~300 Axon tests
pass, Acid Test #1 passes end-to-end). But the project's CI quality gate
(`./dev.sh ci` = cargo check + cargo test + fmt-check + clippy) is RED. Close
that gap — nothing else.

## The only task

Make `./dev.sh ci` pass cleanly. Known failures as of this writing:

1. **`cargo fmt --check`** reports formatting drift — run `cargo fmt` (or
   `./dev.sh fmt`) to normalize.
2. **`cargo clippy` with `-D warnings`** fails with 13 errors:
   - 12 × `float has excessive precision` — float literals written with more
     decimal digits than an `f64` can represent; trim each literal to a value
     `f64` round-trips (do not change the intended numeric meaning).
   - 1 × `unneeded return statement` — replace the trailing `return x;` with a
     bare tail expression `x`.

## Branch discipline (hard rules — same as the roadmap goal)

- Work ONLY on git branch `asiloop/roadmap` (check it out each iteration).
- NEVER commit to `merge-asi-layer3` or `main`.
- Pre-existing uncommitted changes (e.g. in `crates/axon-surface`,
  `crates/axon-wasm`) are NOT yours: never commit, revert, or extend them.

## Each iteration

1. Run `./dev.sh fmt-check` and `./dev.sh clippy`; pick the next concrete
   failure.
2. Fix it minimally (formatting or the specific lint) without altering behavior.
3. Re-run to confirm that failure is gone and nothing regressed.
4. Commit the fix on `asiloop/roadmap` with a clear message
   (`style: cargo fmt` / `fix(clippy): trim excessive float precision`, etc.).

## Definition of done (verify ALL before declaring)

- `./dev.sh ci` exits clean: `cargo check`, `cargo test` (no-codegen),
  `cargo fmt --check`, and `cargo clippy` all pass with zero errors.
- No behavioral change: the full test suite still passes (no test edited to
  pass).
- Only then output `__ASILOOP_DONE__`.
