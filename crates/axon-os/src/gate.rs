//! R21 §3.3 + §4.1 — the static admission gate. Pure, fail-closed.
//!
//! Before any program runs, the gate proves its *declared* effect row is a
//! subset of the (effective) grant, and that the data it handles does not exceed
//! the grant's confidentiality ceiling. The FIRST failure denies — and a denial
//! means **no execution happens** (the supervisor builds a 1-event denial record
//! and returns). Unknown/absent declarations are treated as the FULL effect set
//! (deny-by-default): a program that doesn't say what it does must be granted
//! everything or be refused.

use crate::grant::{EffectSet, Grant, Label};

/// What a program declares it may do (R21 §3.3). Sourced from the program's
/// effect-row / `@[contained]` annotations via the runtime; absent ⇒ full set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeclaredEffects {
    pub row: EffectSet,
    pub max_label: Label,
}

impl DeclaredEffects {
    /// Deny-by-default: a program with no declaration is treated as declaring
    /// EVERY effect at the highest confidentiality — so it only admits under a
    /// maximal grant, never by omission.
    pub fn unknown() -> Self {
        DeclaredEffects {
            row: EffectSet {
                fs_read: true,
                fs_write: true,
                net: true,
                exec: true,
            },
            max_label: Label::Secret,
        }
    }
}

/// The gate verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    Admit,
    Deny { reason: String, axis: String },
}

/// Static admission (R21 §4.1). Checks, in order, denying on the FIRST failure:
/// (1) every declared effect axis is permitted by the grant; (2) the declared
/// confidentiality does not exceed the grant's ceiling.
///
/// `grant` is the EFFECTIVE grant (already `J ∩ S`); the caller computes the
/// intersection so the supervisor can never admit beyond its own authority.
pub fn admit(declared: &DeclaredEffects, grant: &Grant) -> Admission {
    let permitted = grant.effect_set();
    // 1. capability subset, axis by axis (deterministic order).
    for (needs, has, name) in [
        (declared.row.fs_read, permitted.fs_read, "fs_read"),
        (declared.row.fs_write, permitted.fs_write, "fs_write"),
        (declared.row.net, permitted.net, "net"),
        (declared.row.exec, permitted.exec, "exec"),
    ] {
        if needs && !has {
            return Admission::Deny {
                reason: format!("program may perform {name} but the grant withholds it"),
                axis: name.to_string(),
            };
        }
    }
    // 2. confidentiality ceiling.
    if declared.max_label > grant.max_label {
        return Admission::Deny {
            reason: format!(
                "program handles {} data above the grant ceiling {}",
                declared.max_label.as_str(),
                grant.max_label.as_str()
            ),
            axis: "label".to_string(),
        };
    }
    Admission::Admit
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grant::{Budget, ExecPolicy};

    fn grant(fs_read: bool, net: bool, label: Label) -> Grant {
        Grant {
            fs_read: if fs_read {
                vec!["./data/".into()]
            } else {
                vec![]
            },
            fs_write: vec![],
            net: if net { vec!["x.com".into()] } else { vec![] },
            exec: ExecPolicy::None,
            max_label: label,
            budget: Budget {
                calls: 1,
                tokens: 1,
                cost_micro: 1,
            },
        }
    }

    fn declares(fs_read: bool, net: bool, label: Label) -> DeclaredEffects {
        DeclaredEffects {
            row: EffectSet {
                fs_read,
                fs_write: false,
                net,
                exec: false,
            },
            max_label: label,
        }
    }

    #[test]
    fn admit_when_declared_is_a_subset() {
        let a = admit(
            &declares(true, false, Label::Internal),
            &grant(true, true, Label::Internal),
        );
        assert_eq!(a, Admission::Admit);
    }

    #[test]
    fn gate_denies_effect_outside_grant() {
        // declares net; grant withholds net → Deny{axis: net}, no run.
        let a = admit(
            &declares(false, true, Label::Public),
            &grant(true, false, Label::Internal),
        );
        match a {
            Admission::Deny { axis, .. } => assert_eq!(axis, "net"),
            other => panic!("expected Deny, got {other:?}"),
        }
    }

    #[test]
    fn gate_denies_label_above_ceiling() {
        let a = admit(
            &declares(true, false, Label::Secret),
            &grant(true, false, Label::Internal),
        );
        match a {
            Admission::Deny { axis, .. } => assert_eq!(axis, "label"),
            other => panic!("expected label Deny, got {other:?}"),
        }
    }

    #[test]
    fn deny_by_default_unknown_declaration() {
        // An undeclared program (full set) under a narrow grant → Deny.
        let a = admit(
            &DeclaredEffects::unknown(),
            &grant(true, false, Label::Public),
        );
        assert!(matches!(a, Admission::Deny { .. }));
    }
}
