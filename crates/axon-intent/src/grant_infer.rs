//! R22 §4.2 — least-privilege `Grant` inference. Pure, fail-closed.
//!
//! The inferred grant is the **intersection of what the program needs and what
//! the intent permits** — never more. An axis the program never uses is granted
//! NOTHING even if the ceiling would allow it (least privilege). A program that
//! needs more than the ceiling permits is REFUSED (`GrantExceedsCeiling`); the
//! human must widen the intent explicitly.

use crate::intent::CapCeiling;
use crate::refusal::Refusal;
use axon_os::gate::DeclaredEffects;
use axon_os::grant::{Budget, ExecPolicy, Grant};

/// Infer the smallest grant that admits the program (R22 §4.2). `declared` is
/// the program's effect row; `ceiling` is the intent's explicit upper bound;
/// `budget` is the human's stated cap (never inflated).
///
/// Least-privilege invariant: an axis is granted iff the program DECLARES it AND
/// the ceiling permits it. The concrete targets the grant carries are the
/// ceiling prefixes/hosts (the human-stated scope) for each axis the program
/// uses — narrower or equal to the ceiling, never broader.
pub fn infer(
    declared: &DeclaredEffects,
    ceiling: &CapCeiling,
    budget: &Budget,
) -> Result<Grant, Refusal> {
    // fs_read: only if the program reads; then the ceiling's read prefixes (the
    // human-permitted scope). If the program declares the axis but the ceiling
    // grants NO prefix, the intent forbids it → GrantExceedsCeiling.
    let fs_read = axis(declared.row.fs_read, &ceiling.fs_read, "fs_read")?;
    let fs_write = axis(declared.row.fs_write, &ceiling.fs_write, "fs_write")?;
    let net = axis(declared.row.net, &ceiling.net, "net")?;

    // exec: granted only if the program declares exec AND the ceiling allows it.
    let exec = if declared.row.exec {
        if matches!(ceiling.exec, ExecPolicy::Any) {
            ExecPolicy::Any
        } else {
            return Err(Refusal::GrantExceedsCeiling {
                axis: "exec".into(),
            });
        }
    } else {
        ExecPolicy::None
    };

    // label = min(program's, ceiling's); if the program needs MORE than the
    // ceiling → GrantExceedsCeiling.
    if declared.max_label > ceiling.max_label {
        return Err(Refusal::GrantExceedsCeiling {
            axis: "label".into(),
        });
    }
    let max_label = declared.max_label.min(ceiling.max_label);

    Ok(Grant {
        fs_read,
        fs_write,
        net,
        exec,
        max_label,
        // The human's stated budget — never inflated (least privilege also
        // applies to resources).
        budget: *budget,
    })
}

/// One filesystem/network axis: grant the ceiling's permitted scope iff the
/// program declares the axis; refuse if it declares the axis but the ceiling is
/// empty (the intent forbids it). An undeclared axis grants nothing.
fn needs(used: bool) -> bool {
    used
}

fn axis(used: bool, ceiling_scope: &[String], name: &str) -> Result<Vec<String>, Refusal> {
    if !needs(used) {
        return Ok(Vec::new()); // least-privilege: unused ⇒ empty
    }
    if ceiling_scope.is_empty() {
        return Err(Refusal::GrantExceedsCeiling { axis: name.into() });
    }
    Ok(ceiling_scope.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_os::grant::{EffectSet, ExecPolicy, Label};

    fn ceiling() -> CapCeiling {
        CapCeiling {
            fs_read: vec!["./data/".into()],
            fs_write: vec!["./out/".into()],
            net: vec!["api.example.com".into()],
            exec: ExecPolicy::Any,
            max_label: Label::Secret,
        }
    }

    fn budget() -> Budget {
        Budget {
            calls: 100,
            tokens: 50000,
            cost_micro: 1000000,
        }
    }

    fn declares(
        fs_read: bool,
        fs_write: bool,
        net: bool,
        exec: bool,
        label: Label,
    ) -> DeclaredEffects {
        DeclaredEffects {
            row: EffectSet {
                fs_read,
                fs_write,
                net,
                exec,
            },
            max_label: label,
        }
    }

    // R22 §7 `grant_is_least_privilege`.
    #[test]
    fn grant_is_least_privilege() {
        // The ceiling permits net + exec + secret, but the program only reads.
        let g = infer(
            &declares(true, false, false, false, Label::Internal),
            &ceiling(),
            &budget(),
        )
        .expect("admissible");
        assert_eq!(g.fs_read, vec!["./data/".to_string()]);
        assert!(g.fs_write.is_empty(), "fs_write unused ⇒ empty");
        assert!(
            g.net.is_empty(),
            "net unused ⇒ empty even though ceiling permits it"
        );
        assert_eq!(g.exec, ExecPolicy::None, "exec unused ⇒ none");
        // label clamps to min(program, ceiling).
        assert_eq!(g.max_label, Label::Internal);
        // budget is the human's stated cap, not inflated.
        assert_eq!(g.budget, budget());
    }

    #[test]
    fn read_and_write_grants_both_axes() {
        let g = infer(
            &declares(true, true, false, false, Label::Internal),
            &ceiling(),
            &budget(),
        )
        .unwrap();
        assert_eq!(g.fs_read, vec!["./data/".to_string()]);
        assert_eq!(g.fs_write, vec!["./out/".to_string()]);
        assert!(g.net.is_empty());
    }

    // R22 §7 `grant_exceeds_ceiling_refused`.
    #[test]
    fn grant_exceeds_ceiling_refused() {
        // Program needs net, but the ceiling permits NO net hosts.
        let narrow = CapCeiling {
            net: vec![],
            ..ceiling()
        };
        let r = infer(
            &declares(true, false, true, false, Label::Internal),
            &narrow,
            &budget(),
        );
        assert!(matches!(
            r,
            Err(Refusal::GrantExceedsCeiling { ref axis }) if axis == "net"
        ));
    }

    #[test]
    fn exec_needs_ceiling_any() {
        let no_exec = CapCeiling {
            exec: ExecPolicy::None,
            ..ceiling()
        };
        let r = infer(
            &declares(true, false, false, true, Label::Internal),
            &no_exec,
            &budget(),
        );
        assert!(matches!(
            r,
            Err(Refusal::GrantExceedsCeiling { ref axis }) if axis == "exec"
        ));
    }

    #[test]
    fn label_above_ceiling_refused() {
        let internal_ceiling = CapCeiling {
            max_label: Label::Internal,
            ..ceiling()
        };
        let r = infer(
            &declares(true, false, false, false, Label::Secret),
            &internal_ceiling,
            &budget(),
        );
        assert!(matches!(
            r,
            Err(Refusal::GrantExceedsCeiling { ref axis }) if axis == "label"
        ));
    }
}
