//! R22 §4.3 — admissibility proof. Pure. Reuses R21's `gate::admit` (declared
//! effects ⊆ grant) — does NOT reimplement admission logic.
//!
//! By construction of the least-privilege inference (§4.2) the inferred grant
//! MUST admit the program. If `gate::admit` ever returns `Deny`, that is a bug
//! or an adversarial program (e.g. a forced skew where the program declares more
//! than was inferred) — and we REFUSE (`NotAdmissible`), never ship.

use crate::refusal::Refusal;
use axon_os::gate::{admit as gate_admit, Admission, DeclaredEffects};
use axon_os::grant::Grant;

/// Prove the program's declared effects are admissible under `grant` (R22 §4.3),
/// delegating to R21's gate. `Ok(())` on admit; `Refusal::NotAdmissible` on a
/// gate denial (refuse-not-ship).
pub fn prove_admissible(declared: &DeclaredEffects, grant: &Grant) -> Result<(), Refusal> {
    match gate_admit(declared, grant) {
        Admission::Admit => Ok(()),
        Admission::Deny { reason, axis } => Err(Refusal::NotAdmissible { axis, reason }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_os::grant::{Budget, EffectSet, ExecPolicy, Grant, Label};

    fn grant(fs_read: bool, net: bool) -> Grant {
        Grant {
            fs_read: if fs_read {
                vec!["./data/".into()]
            } else {
                vec![]
            },
            fs_write: vec![],
            net: if net { vec!["x.com".into()] } else { vec![] },
            exec: ExecPolicy::None,
            max_label: Label::Internal,
            budget: Budget {
                calls: 1,
                tokens: 1,
                cost_micro: 1,
            },
        }
    }

    fn declares(fs_read: bool, net: bool) -> DeclaredEffects {
        DeclaredEffects {
            row: EffectSet {
                fs_read,
                fs_write: false,
                net,
                exec: false,
            },
            max_label: Label::Internal,
        }
    }

    // R22 §7 `synthesized_job_is_self_admissible` (the pure half — the pipeline
    // test wires the full inference→admit chain).
    #[test]
    fn synthesized_job_is_self_admissible() {
        // A grant inferred to cover exactly what the program declares admits it.
        assert!(prove_admissible(&declares(true, false), &grant(true, false)).is_ok());
    }

    // R22 §7 `non_admissible_refused` (adversarial: declared ⊄ grant).
    #[test]
    fn non_admissible_refused() {
        let r = prove_admissible(&declares(true, true), &grant(true, false));
        assert!(matches!(
            r,
            Err(Refusal::NotAdmissible { ref axis, .. }) if axis == "net"
        ));
        assert_eq!(r.unwrap_err().exit_code(), 8);
    }
}
