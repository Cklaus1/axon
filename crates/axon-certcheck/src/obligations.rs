//! R23: the kernel mint/carve obligations, LOWERED IN CODE — not hand-pinned.
//!
//! The shipped `examples/proofs/*.obl` are generated FROM these constructors
//! (`gen_artifacts`), and `shipped_obligations_match_the_code_lowering` locks the
//! on-disk files equal to the in-code lowering. So a hand-edited / drifted `.obl`
//! (the "is the certified obligation actually the mint law?" gap from the R23 ASI
//! review) is caught by a failing test, not silently certified.

use crate::obligation::build::*;
use crate::obligation::{Obligation, Op, Sort, Var};

fn ivar_int(name: &str) -> Var {
    Var {
        name: name.into(),
        sort: Sort::Int,
    }
}
fn bvar_bool(name: &str) -> Var {
    Var {
        name: name.into(),
        sort: Sort::Bool,
    }
}

/// O1 — capability attenuation: `(want_X ∧ parent_X) ⇒ parent_X` for each axis
/// (the R20 mint law `child = want ∧ parent`, so a child cap implies the parent's).
pub fn mint_o1() -> Obligation {
    let attenuates =
        |want: &str, parent: &str| implies(and(vec![bvar(want), bvar(parent)]), bvar(parent));
    Obligation {
        id: "principal_mint/O1".into(),
        vars: vec![
            bvar_bool("want_net"),
            bvar_bool("parent_net"),
            bvar_bool("want_fs"),
            bvar_bool("parent_fs"),
            bvar_bool("want_exec"),
            bvar_bool("parent_exec"),
        ],
        claim: and(vec![
            attenuates("want_net", "parent_net"),
            attenuates("want_fs", "parent_fs"),
            attenuates("want_exec", "parent_exec"),
        ]),
    }
}

/// O2 — budget carve: `(rem ≥ 0) ⇒ max(0, min(g, rem)) ≤ rem` (a minted grant
/// never exceeds the parent's remaining budget).
pub fn mint_o2() -> Obligation {
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

/// The R22/R20 relational carve: `min(g, avail) ≤ avail`.
pub fn carve() -> Obligation {
    Obligation {
        id: "carve/return".into(),
        vars: vec![ivar_int("g"), ivar_int("avail")],
        claim: cmp(Op::Le, min(ivar("g"), ivar("avail")), ivar("avail")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_obligations_match_the_code_lowering() {
        // The shipped .obl MUST equal the in-code lowering — a hand-edited or
        // drifted obligation (certifying a formula that is NOT the mint law) fails
        // here. This is what makes "the certified obligation IS the mint law"
        // a checked property, not a transcription anyone could quietly change.
        assert_eq!(
            mint_o1().to_json(),
            include_str!("../../../examples/proofs/mint_o1.obl"),
            "mint_o1.obl drifted from the code lowering"
        );
        assert_eq!(
            mint_o2().to_json(),
            include_str!("../../../examples/proofs/mint_o2.obl"),
            "mint_o2.obl drifted from the code lowering"
        );
        assert_eq!(
            carve().to_json(),
            include_str!("../../../examples/proofs/carve.obl"),
            "carve.obl drifted from the code lowering"
        );
    }
}
