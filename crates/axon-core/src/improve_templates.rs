//! R10 AI-driven discovery — the **closed template registry** (the TCB boundary).
//!
//! This module is the single, compile-time, immutable source of truth for which
//! optimization passes exist. The AI-driven discoverer (`improve::discover_with_ai`)
//! may only *select template names from this registry* — it never authors code.
//! Every safety property the red-team review demanded rests on this being a
//! `const` array of `(name, fn-pointer)` pairs that cannot be mutated, loaded
//! from a file, or extended at runtime:
//!
//! - **No code injection (must-fix #1):** a pass is a `&'static Pass` checked
//!   into the repo and reviewed; the AI supplies only a *name* that indexes here.
//! - **Validate-before-instantiate, fail-closed (#2):** `get_pass_for_template`
//!   returns `None` for an unknown name; callers reject with **E1407** *before*
//!   writing a proposal or running `verify` — an unknown name never reaches the
//!   verifier.
//! - **No runtime mutability (#4):** the registry is `const`. Adding a template
//!   is a commit (new Rust fn + a row here + a test), never a runtime discovery.
//!   A manifest naming a pass absent from this registry is **E1408**
//!   (tampering / version skew) — a graduated name must always resolve here.
//!
//! The AI's value is *selection and ranking* over this fixed menu, not authoring
//! — exactly the I-12 posture (self-modification cannot grant a capability the
//! system did not already hold: the system already holds every template here).

use crate::ast::Program;
use crate::improve::{
    bool_simplify_pass, compare_fold_pass, constant_fold_pass, count_arith_identity_sites,
    count_bool_simplify_sites, count_compare_fold_sites, count_constant_fold_sites,
    count_redundant_branch_sites, fold_arith_identities_pass, redundant_branch_fold_pass, Pass,
};

/// One entry in the closed optimization-template menu: a stable name, a
/// one-line description (shown to the AI and the user), the deterministic Rust
/// rewrite, and a detector that counts applicable sites in a program (so a
/// proposal can report real opportunity counts, and so the AI's selection can
/// be cross-checked against a static scan).
pub struct Template {
    /// Stable, kebab-case name. The ONLY thing the AI may emit to select this.
    pub name: &'static str,
    /// One-line human/AI description of the optimization.
    pub description: &'static str,
    /// The deterministic rewrite. `&'static` fn pointer — reviewed Rust, never
    /// AI-authored. Run through the four-gate firewall by `verify`.
    pub pass: &'static Pass,
    /// Count applicable sites in one program (for opportunity reporting +
    /// static cross-check of an AI selection). Pure function of the AST.
    pub detector: fn(&Program) -> usize,
}

/// The closed menu. **Compile-time, immutable.** Adding a row requires a
/// reviewed commit (Rust fn + detector + a test) — the code-review boundary the
/// red-team review mandates. Starts at the one pass already verified correct +
/// safe through the four gates (`fold-arith-identities`); more templates slot in
/// here as they are each independently verified.
pub const TEMPLATES: &[Template] = &[
    Template {
        name: "fold-arith-identities",
        description: "simplify arithmetic identities (x+0, 0+x, x-0, x*1, 1*x → x)",
        pass: &(fold_arith_identities_pass as fn(&Program) -> Program),
        detector: program_arith_identity_sites,
    },
    Template {
        name: "constant-fold",
        description: "fold integer-literal arithmetic (2+3 → 5), skipping overflow/div-by-zero",
        pass: &(constant_fold_pass as fn(&Program) -> Program),
        detector: program_constant_fold_sites,
    },
    Template {
        name: "bool-simplify",
        description: "simplify boolean negation (!true → false, !false → true, !(!x) → x)",
        pass: &(bool_simplify_pass as fn(&Program) -> Program),
        detector: program_bool_simplify_sites,
    },
    Template {
        name: "redundant-branch-fold",
        description: "fold constant-condition if/else (if true {a} else {b} → a)",
        pass: &(redundant_branch_fold_pass as fn(&Program) -> Program),
        detector: program_redundant_branch_sites,
    },
    Template {
        name: "compare-fold",
        description: "fold integer-literal comparisons (3 < 5 → true, 2 == 2 → true)",
        pass: &(compare_fold_pass as fn(&Program) -> Program),
        detector: program_compare_fold_sites,
    },
];

/// Count `fold-arith-identities` sites across one program (sum over its fns).
fn program_arith_identity_sites(program: &Program) -> usize {
    use crate::ast::Item;
    program
        .items
        .iter()
        .map(|it| match it {
            Item::FnDef(f) => count_arith_identity_sites(&f.body),
            Item::ImplBlock(b) => b
                .methods
                .iter()
                .map(|m| count_arith_identity_sites(&m.body))
                .sum(),
            _ => 0,
        })
        .sum()
}

/// Count `constant-fold` sites across one program (sum over its fns).
fn program_constant_fold_sites(program: &Program) -> usize {
    use crate::ast::Item;
    program
        .items
        .iter()
        .map(|it| match it {
            Item::FnDef(f) => count_constant_fold_sites(&f.body),
            Item::ImplBlock(b) => b
                .methods
                .iter()
                .map(|m| count_constant_fold_sites(&m.body))
                .sum(),
            _ => 0,
        })
        .sum()
}

/// Count `bool-simplify` sites across one program (sum over its fns).
fn program_bool_simplify_sites(program: &Program) -> usize {
    use crate::ast::Item;
    program
        .items
        .iter()
        .map(|it| match it {
            Item::FnDef(f) => count_bool_simplify_sites(&f.body),
            Item::ImplBlock(b) => b
                .methods
                .iter()
                .map(|m| count_bool_simplify_sites(&m.body))
                .sum(),
            _ => 0,
        })
        .sum()
}

/// Count `redundant-branch-fold` sites across one program (sum over its fns).
fn program_redundant_branch_sites(program: &Program) -> usize {
    use crate::ast::Item;
    program
        .items
        .iter()
        .map(|it| match it {
            Item::FnDef(f) => count_redundant_branch_sites(&f.body),
            Item::ImplBlock(b) => b
                .methods
                .iter()
                .map(|m| count_redundant_branch_sites(&m.body))
                .sum(),
            _ => 0,
        })
        .sum()
}

/// Count `compare-fold` sites across one program (sum over its fns).
fn program_compare_fold_sites(program: &Program) -> usize {
    use crate::ast::Item;
    program
        .items
        .iter()
        .map(|it| match it {
            Item::FnDef(f) => count_compare_fold_sites(&f.body),
            Item::ImplBlock(b) => b
                .methods
                .iter()
                .map(|m| count_compare_fold_sites(&m.body))
                .sum(),
            _ => 0,
        })
        .sum()
}

/// Look up a template by name in the closed registry. `None` for any name not
/// in [`TEMPLATES`] — the single validation point every caller routes through
/// (discover, verify, graduate). This is the fail-closed gate: an unknown name
/// can never be instantiated into a runnable pass.
pub fn get_template(name: &str) -> Option<&'static Template> {
    TEMPLATES.iter().find(|t| t.name == name)
}

/// The deterministic rewrite for a template name, or `None` if the name is not
/// in the registry. Callers that get `None` must reject (E1407 at discover/verify
/// time, E1408 if a *graduated* name is missing — manifest tampering).
pub fn get_pass_for_template(name: &str) -> Option<&'static Pass> {
    get_template(name).map(|t| t.pass)
}

/// Is `name` a known template? The cheap validation predicate.
pub fn is_known_template(name: &str) -> bool {
    get_template(name).is_some()
}

/// All template names, for the AI prompt menu and `--help`/listing.
pub fn template_names() -> Vec<&'static str> {
    TEMPLATES.iter().map(|t| t.name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_is_stable_and_closed() {
        // The menu is identical on every call (a const, no runtime mutation).
        let a = template_names();
        let b = template_names();
        assert_eq!(a, b);
        assert!(a.contains(&"fold-arith-identities"));
        assert!(a.contains(&"constant-fold"));
        assert!(a.contains(&"bool-simplify"));
        assert!(a.contains(&"redundant-branch-fold"));
    }

    #[test]
    fn get_pass_resolves_known_and_rejects_unknown() {
        // Known → Some (a runnable pass); unknown → None (the fail-closed gate
        // that becomes E1407/E1408 at the call sites).
        assert!(get_pass_for_template("fold-arith-identities").is_some());
        assert!(get_pass_for_template("evil-template").is_none());
        assert!(get_pass_for_template("").is_none());
        assert!(!is_known_template("constant-fold-no-bounds-check"));
    }

    #[test]
    fn every_template_has_a_detector_and_pass() {
        // Exhaustiveness: each menu entry is fully wired (no half-registered
        // template that would resolve to a name with no runnable pass).
        for t in TEMPLATES {
            assert!(!t.name.is_empty());
            assert!(!t.description.is_empty());
            // The detector is callable on an empty program (returns 0, no panic).
            let empty = Program { items: vec![] };
            assert_eq!((t.detector)(&empty), 0);
        }
    }
}
