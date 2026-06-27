# axon-certcheck (R23) — proof certificates + a minimal, solver-free checker

**Get Z3 out of the trusted base.** Today the R20 capability proofs and the R21
admission gate trust Z3's verdict directly — a ~500K-line unaudited solver in the
TCB. R23 makes the prover emit a **proof certificate** that a **tiny, auditable,
independent checker** re-validates *without calling any SMT solver*. You trust a
few hundred readable lines, not the solver.

The trust shift: before R23, "proven" = "Z3 said UNSAT" (trust Z3). After R23,
"proven" = "a tiny checker re-derived the verdict from a certificate" (trust the
checker; Z3 is now an untrusted *oracle* that merely *suggests* certificates).

The checker is **sound by construction**: it returns `Valid` only after itself
re-deriving the contradiction from the negated claim (re-deriving the leaf facts,
checking the Farkas coefficients are nonnegative and sum to a literal `0 < 0`,
checking case-splits are exhaustive, and verifying the obligation binding). It
never returns a false `Valid`; anything it cannot constructively confirm is
`Invalid` or `Unsupported` (fail-closed).

## Quickstart

```bash
# Build the SOLVER-FREE checker (no z3 needed to CHECK):
cargo build -p axon-certcheck --no-default-features --bin certcheck

# 1. Independently verify that a capability obligation holds for ALL inputs —
#    using only the small checker, with NO SMT solver in the trust path:
certcheck check examples/proofs/mint_o2.obl examples/proofs/mint_o2.cert

# 2. Read the proof in plain English (the refutation the checker confirmed):
certcheck explain examples/proofs/mint_o2.obl --cert examples/proofs/mint_o2.cert

# 3. Watch a tampered certificate get REJECTED (exit 8):
sed 's/"coeffs":\[1,1\]/"coeffs":[1,2]/' examples/proofs/carve.cert > /tmp/bad.cert
certcheck check examples/proofs/carve.obl /tmp/bad.cert ; echo "exit=$?"

# (Optional, needs the solver to GENERATE a fresh certificate:)
# cargo build -p axon-certcheck --features smt --bin certcheck
# certcheck prove examples/proofs/mint_o2.obl --out /tmp/fresh.cert
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | **Valid** — the certificate proves the obligation for all inputs |
| `2` | Malformed / usage (bad `.obl` / `.cert` syntax) |
| `4` | **Unsupported** — the obligation is outside the certified fragment (fail-closed) |
| `8` | **Invalid** — the certificate does NOT prove the claim (fail-closed) |

## The certified fragment

R23 certifies exactly the obligation shapes R20/R21 already discharge — the
**quantifier-free linear-integer + boolean fragment**: bound obligations
`∀ params. R(params) OP K` where `R` is built from `+ - *`(by constant)`,
`min/max/abs` over i64 params, and boolean/relational obligations over capability
atoms and linear comparisons. Nonlinear arithmetic, quantifier alternation,
arrays, bitvectors, and the f64 path are **out** in v1: the checker returns
`Unsupported` (fail-closed for admission), never a false `Valid`.

## What is and isn't authenticated

A certificate is **content-addressed and self-checking**: it carries the
`obligation_digest` it was issued against (binding — a transplanted cert fails)
and a `cert_digest` over its own refutation bytes (tamper-evidence). It is **not**
signed: *who* produced it is not authenticated here — combine with R20 attestation
for provenance. The checker is *small and auditable* (you can read all of it), not
*formally verified* (a machine-checked checker is a future stretch).

## Files

- `crates/axon-certcheck/src/obligation.rs` — the Obligation IR (LIA+bool).
- `crates/axon-certcheck/src/certificate.rs` — the ProofCertificate format.
- `crates/axon-certcheck/src/eval.rs` — the checker's own LIA+bool evaluator (checked arithmetic, Farkas).
- `crates/axon-certcheck/src/checker.rs` — **THE TRUSTED CHECKER** (pure, solver-free).
- `crates/axon-certcheck/src/synth.rs` — deterministic refutation synthesizer (untrusted).
- `crates/axon-certcheck/src/emit.rs` — Z3 oracle + emission (`smt` feature, untrusted).
- `crates/axon-certcheck/src/policy.rs` — the `--require-certificates` fail-closed gate.
- `examples/proofs/` — real obligations + valid certificates.
- `crates/axon-certcheck/tests/corpus/` — forged certs the checker must reject.
- `scripts/r23_acceptance_gate.sh` — the pinned acceptance gate (§10).
