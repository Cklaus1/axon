//! Generate the shipped example artifacts (R23 §5.6) into `examples/proofs/`:
//! `mint_o1.obl`+`.cert`, `mint_o2.obl`+`.cert`, `carve.obl`+`.cert`. Pure (uses
//! the deterministic synthesizer — NO solver needed), so the certs are
//! byte-stable (A5).
//!
//! The obligations themselves live in `axon_certcheck::obligations` (the single
//! in-code lowering of the mint law); this binary only synthesizes their certs
//! and writes both. `obligations::shipped_obligations_match_the_code_lowering`
//! locks the on-disk `.obl` equal to that lowering, so a hand-edited obligation
//! cannot drift past the gate (R23 ASI-review must-fix).
//!
//! Run from the workspace root:  `cargo run -p axon-certcheck --example gen_artifacts`
//! This is a build-time helper, not part of the trusted path.

use axon_certcheck::obligation::Obligation;
use axon_certcheck::{carve, check, mint_o1, mint_o2, synth, CheckResult};
use std::path::{Path, PathBuf};

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
    emit(&dir, "mint_o1", &mint_o1());
    emit(&dir, "mint_o2", &mint_o2());
}
