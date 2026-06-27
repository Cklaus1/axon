//! Generate the adversarial forgery corpus (R23 §5.6 / S4) into
//! `crates/axon-certcheck/tests/corpus/`: REAL `.cert` files the checker MUST
//! reject (`acc_a6_checker_rejects_forged_and_mutated`). Each is a distinct
//! attack class. Run: `cargo run -p axon-certcheck --example gen_corpus`.
//!
//! Each forgery is paired with the obligation it is presented against (always
//! `carve.obl`), so the acceptance test loads `carve.obl` + each corpus cert and
//! asserts the checker returns NOT-Valid.

use axon_certcheck::certificate::{FactOp, LinFact, ProofCertificate, Refutation};
use axon_certcheck::obligation::build::*;
use axon_certcheck::obligation::{Obligation, Op, Sort, Var};
use axon_certcheck::{check, synth, CheckResult};
use std::path::{Path, PathBuf};

fn carve() -> Obligation {
    Obligation {
        id: "carve/return".into(),
        vars: vec![
            Var {
                name: "g".into(),
                sort: Sort::Int,
            },
            Var {
                name: "avail".into(),
                sort: Sort::Int,
            },
        ],
        claim: cmp(Op::Le, min(ivar("g"), ivar("avail")), ivar("avail")),
    }
}

fn rehash(mut c: ProofCertificate) -> ProofCertificate {
    // Re-hash so a forgery passes the cert_digest binding (forcing the checker to
    // catch the LOGICAL flaw, not just a hash mismatch). Some forgeries below
    // deliberately leave the digest stale instead — that is its own attack.
    c.cert_digest = ProofCertificate::compute_cert_digest(&c.refutation);
    c
}

fn write(dir: &Path, name: &str, c: &ProofCertificate) {
    // Sanity: the corpus entry must be REJECTED by the checker.
    let o = carve();
    assert_ne!(
        check(&o, c),
        CheckResult::Valid,
        "corpus entry {name} must NOT be valid"
    );
    std::fs::write(dir.join(name), c.to_json()).unwrap();
    println!("wrote forgery {name} (checker rejects)");
}

fn main() {
    let dir = PathBuf::from("crates/axon-certcheck/tests/corpus");
    std::fs::create_dir_all(&dir).unwrap();
    let o = carve();
    let valid = synth::synthesize(&o).expect("carve synthesizes");

    // 1. Wrong obligation_digest (a cert for a DIFFERENT obligation transplanted).
    {
        let mut c = valid.clone();
        c.obligation_digest =
            "axsha256:0000000000000000000000000000000000000000000000000000000000000000".into();
        // leave cert_digest valid (the binding check should still reject)
        write(&dir, "wrong_obligation_digest.cert", &c);
    }

    // 2. Non-exhaustive split: drop the on_false branch by making it a trivially
    //    non-contradicting leaf (a single satisfiable fact).
    {
        let mut c = valid.clone();
        if let Refutation::IteCase { on_false, .. } = &mut c.refutation {
            **on_false = Refutation::LinearContradiction {
                facts: vec![LinFact {
                    terms: vec![(1, "g".into())],
                    op: FactOp::Le,
                    constant: 100, // g ≤ 100: satisfiable, NOT a contradiction
                }],
                coeffs: vec![1],
            };
        }
        write(&dir, "non_exhaustive_split.cert", &rehash(c));
    }

    // 3. Negative Farkas coefficient (an unsound multiplier flips an inequality).
    {
        let mut c = valid.clone();
        if let Refutation::IteCase { on_true, .. } = &mut c.refutation {
            if let Refutation::LinearContradiction { coeffs, .. } = on_true.as_mut() {
                coeffs[0] = -1;
            }
        }
        write(&dir, "negative_farkas_coeff.cert", &rehash(c));
    }

    // 4. Sum is not a contradiction (a Farkas combination that reduces to 0 ≤ 3).
    {
        let mut c = valid.clone();
        if let Refutation::IteCase { on_true, .. } = &mut c.refutation {
            if let Refutation::LinearContradiction { facts, coeffs } = on_true.as_mut() {
                // Replace with two facts that cancel vars but sum to 0 ≤ 3.
                *facts = vec![
                    LinFact {
                        terms: vec![(1, "g".into())],
                        op: FactOp::Le,
                        constant: 5,
                    },
                    LinFact {
                        terms: vec![(-1, "g".into())],
                        op: FactOp::Le,
                        constant: -2,
                    },
                ];
                *coeffs = vec![1, 1]; // 0 ≤ 3 — NOT a contradiction
            }
        }
        write(&dir, "sum_not_contradiction.cert", &rehash(c));
    }

    // 5. Forged leaf fact not entailed by the negated claim (the checker
    //    re-derives the hypotheses and must reject a fabricated one).
    {
        let mut c = valid.clone();
        if let Refutation::IteCase { on_true, .. } = &mut c.refutation {
            if let Refutation::LinearContradiction { facts, coeffs } = on_true.as_mut() {
                // A fabricated complementary pair that DOES sum to 0 ≤ -1 but
                // whose facts are NOT among the branch hypotheses.
                *facts = vec![
                    LinFact {
                        terms: vec![(1, "avail".into())],
                        op: FactOp::Le,
                        constant: -1,
                    },
                    LinFact {
                        terms: vec![(-1, "avail".into())],
                        op: FactOp::Le,
                        constant: -1,
                    },
                ];
                *coeffs = vec![1, 1]; // 0 ≤ -2: a real contradiction, but BOGUS facts
            }
        }
        write(&dir, "fact_not_entailed.cert", &rehash(c));
    }

    // 6. Stale cert_digest (refutation mutated but digest left unchanged).
    {
        let mut c = valid.clone();
        if let Refutation::IteCase { on_true, .. } = &mut c.refutation {
            if let Refutation::LinearContradiction { coeffs, .. } = on_true.as_mut() {
                coeffs[0] = 2; // change the witness without rehashing
            }
        }
        // NOTE: deliberately NOT rehashed.
        write(&dir, "stale_cert_digest.cert", &c);
    }

    println!("corpus written to {}", dir.display());
}
