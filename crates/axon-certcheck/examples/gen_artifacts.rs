//! Generate the shipped example artifacts (R23 §5.6) into `examples/proofs/`:
//! `mint_o2.obl`+`.cert`, `carve.obl`+`.cert`. Pure (uses the deterministic
//! synthesizer — NO solver needed), so the certs are byte-stable (A5).
//!
//! Run from the workspace root:  `cargo run -p axon-certcheck --example gen_artifacts`
//! This is a build-time helper, not part of the trusted path.

use axon_certcheck::obligation::build::*;
use axon_certcheck::obligation::{Obligation, Op, Sort, Var};
use axon_certcheck::{check, synth, CheckResult};
use std::path::{Path, PathBuf};

fn ivar_int(name: &str) -> Var {
    Var {
        name: name.into(),
        sort: Sort::Int,
    }
}

fn carve() -> Obligation {
    // ∀ g, avail. min(g, avail) ≤ avail   (R22/R20 relational `_ ≤ avail`).
    Obligation {
        id: "carve/return".into(),
        vars: vec![ivar_int("g"), ivar_int("avail")],
        claim: cmp(Op::Le, min(ivar("g"), ivar("avail")), ivar("avail")),
    }
}

fn mint_o2() -> Obligation {
    // ∀ g, rem. (rem ≥ 0) ⇒ max(0, min(g, rem)) ≤ rem
    // The R20 budget-carve clamp: the minted grant never exceeds `rem`.
    Obligation {
        id: "principal_mint/O2".into(),
        vars: vec![ivar_int("g"), ivar_int("rem")],
        claim: implies(
            cmp(Op::Ge, ivar("rem"), int(0)),
            cmp(
                Op::Le,
                max(int(0), min(ivar("g"), ivar("rem"))),
                ivar("rem"),
            ),
        ),
    }
}

fn emit(dir: &Path, stem: &str, obl: &Obligation) {
    let cert = synth::synthesize(obl).expect("obligation synthesizes");
    // Self-check before shipping: the artifact MUST validate with the checker.
    assert_eq!(
        check(obl, &cert),
        CheckResult::Valid,
        "generated cert for {stem} must be checker-valid"
    );
    std::fs::write(dir.join(format!("{stem}.obl")), obl.to_json()).unwrap();
    std::fs::write(dir.join(format!("{stem}.cert")), cert.to_json()).unwrap();
    println!("wrote {stem}.obl + {stem}.cert (checker-VALID)");
}

fn main() {
    let dir = PathBuf::from("examples/proofs");
    std::fs::create_dir_all(&dir).unwrap();
    emit(&dir, "carve", &carve());
    emit(&dir, "mint_o2", &mint_o2());
}
