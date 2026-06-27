//! R23 §3.1 — the `Obligation` IR: the quantifier-free linear-integer + boolean
//! fragment. Pure (std + sha2 + serde only — NO solver). This is the checker's
//! input #1: the prover lowers an R20/R21 obligation into this canonical tree;
//! the checker reads only this, not the original Axon AST.
//!
//! **Linearity is a validation invariant** (§3.1): `MulConst` is the only
//! multiplication. A `Mul(Term, Term)` of two non-constant terms is not even
//! representable here, and a parse that would require it is rejected as
//! `Unsupported` — this keeps the checker's decision procedure decidable and
//! small (the whole point: an auditor can read all of it).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The sort of a free variable (§3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    Int,
    Bool,
}

/// A free variable (a universally-quantified parameter) of the obligation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Var {
    pub name: String,
    pub sort: Sort,
}

/// A comparison operator (§3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

/// A linear-integer term (§3.1). The ONLY multiplication is `MulConst` (a term
/// times an integer constant) — nonlinearity is unrepresentable by construction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "node", rename_all = "snake_case")]
pub enum Term {
    /// An integer literal.
    Int { v: i64 },
    /// An integer-sorted variable, by name.
    IVar { name: String },
    /// Addition.
    Add { l: Box<Term>, r: Box<Term> },
    /// Subtraction.
    Sub { l: Box<Term>, r: Box<Term> },
    /// Multiplication by an integer constant (the only legal `*`).
    MulConst { k: i64, t: Box<Term> },
    /// The shipped bound builtin `min`.
    Min { l: Box<Term>, r: Box<Term> },
    /// The shipped bound builtin `max`.
    Max { l: Box<Term>, r: Box<Term> },
    /// The shipped bound builtin `abs`.
    Abs { t: Box<Term> },
    /// A conditional term (for clamp / remaining-style expressions).
    Ite {
        cond: Box<Formula>,
        then: Box<Term>,
        els: Box<Term>,
    },
}

/// A quantifier-free boolean formula over the term language (§3.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "node", rename_all = "snake_case")]
pub enum Formula {
    /// A boolean constant.
    Bool { v: bool },
    /// A boolean variable, by name.
    BVar { name: String },
    /// Negation.
    Not { f: Box<Formula> },
    /// Conjunction.
    And { fs: Vec<Formula> },
    /// Disjunction.
    Or { fs: Vec<Formula> },
    /// Implication.
    Implies { a: Box<Formula>, b: Box<Formula> },
    /// A linear comparison `l OP r`.
    Cmp { op: Op, l: Term, r: Term },
}

/// An obligation: the ∀-claim asserted true for ALL assignments of `vars`
/// (§3.1). The defended claim is "∀-inputs validity" — i.e. `Not(claim)` is
/// unsatisfiable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Obligation {
    pub id: String,
    pub vars: Vec<Var>,
    pub claim: Formula,
}

/// Why a `.obl` / `.cert` string could not be parsed (→ exit 2, malformed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseErr {
    pub detail: String,
}

impl std::fmt::Display for ParseErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.detail)
    }
}

/// Serialized obligation envelope (§3.1, schema `axon-obl/1`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OblFile {
    schema: String,
    id: String,
    vars: Vec<Var>,
    claim: Formula,
}

const OBL_SCHEMA: &str = "axon-obl/1";

impl Obligation {
    /// Parse + VALIDATE an obligation from its JSON form (§3.1). Validation
    /// enforces linearity (no nonlinear products — unrepresentable here) and
    /// well-formedness (every referenced var is declared, with the right sort).
    pub fn parse(s: &str) -> Result<Obligation, ParseErr> {
        let file: OblFile = serde_json::from_str(s).map_err(|e| ParseErr {
            detail: format!("malformed obligation JSON: {e}"),
        })?;
        if file.schema != OBL_SCHEMA {
            return Err(ParseErr {
                detail: format!("unexpected schema {:?} (want {OBL_SCHEMA})", file.schema),
            });
        }
        let obl = Obligation {
            id: file.id,
            vars: file.vars,
            claim: file.claim,
        };
        obl.validate()?;
        Ok(obl)
    }

    /// Serialize to canonical JSON (§4.5: deterministic — serde emits struct
    /// fields in declaration order with no whitespace; integers are exact).
    pub fn to_json(&self) -> String {
        let file = OblFile {
            schema: OBL_SCHEMA.to_string(),
            id: self.id.clone(),
            vars: self.vars.clone(),
            claim: self.claim.clone(),
        };
        serde_json::to_string(&file).expect("Obligation serializes")
    }

    /// The canonical, content-addressed digest used to BIND a certificate to its
    /// obligation (§3.2 / §4.1.4): `"axsha256:" + sha256(canonical JSON)`.
    pub fn digest(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.to_json().as_bytes());
        format!("axsha256:{:x}", h.finalize())
    }

    /// Look up a declared variable's sort by name.
    pub fn sort_of(&self, name: &str) -> Option<Sort> {
        self.vars.iter().find(|v| v.name == name).map(|v| v.sort)
    }

    /// Validate well-formedness + linearity. A violation that puts the
    /// obligation OUTSIDE the certified fragment is surfaced by the caller as
    /// `Unsupported`; a structurally malformed one is a `ParseErr` (exit 2).
    fn validate(&self) -> Result<(), ParseErr> {
        // Unique var names.
        for (i, v) in self.vars.iter().enumerate() {
            if self.vars[..i].iter().any(|w| w.name == v.name) {
                return Err(ParseErr {
                    detail: format!("duplicate variable {:?}", v.name),
                });
            }
        }
        self.check_formula(&self.claim)
    }

    fn check_formula(&self, f: &Formula) -> Result<(), ParseErr> {
        match f {
            Formula::Bool { .. } => Ok(()),
            Formula::BVar { name } => match self.sort_of(name) {
                Some(Sort::Bool) => Ok(()),
                Some(Sort::Int) => Err(ParseErr {
                    detail: format!("variable {name:?} used as Bool but declared Int"),
                }),
                None => Err(ParseErr {
                    detail: format!("undeclared boolean variable {name:?}"),
                }),
            },
            Formula::Not { f } => self.check_formula(f),
            Formula::And { fs } | Formula::Or { fs } => {
                for g in fs {
                    self.check_formula(g)?;
                }
                Ok(())
            }
            Formula::Implies { a, b } => {
                self.check_formula(a)?;
                self.check_formula(b)
            }
            Formula::Cmp { l, r, .. } => {
                self.check_term(l)?;
                self.check_term(r)
            }
        }
    }

    fn check_term(&self, t: &Term) -> Result<(), ParseErr> {
        match t {
            Term::Int { .. } => Ok(()),
            Term::IVar { name } => match self.sort_of(name) {
                Some(Sort::Int) => Ok(()),
                Some(Sort::Bool) => Err(ParseErr {
                    detail: format!("variable {name:?} used as Int but declared Bool"),
                }),
                None => Err(ParseErr {
                    detail: format!("undeclared integer variable {name:?}"),
                }),
            },
            Term::Add { l, r } | Term::Sub { l, r } | Term::Min { l, r } | Term::Max { l, r } => {
                self.check_term(l)?;
                self.check_term(r)
            }
            Term::MulConst { t, .. } => self.check_term(t),
            Term::Abs { t } => self.check_term(t),
            Term::Ite { cond, then, els } => {
                self.check_formula(cond)?;
                self.check_term(then)?;
                self.check_term(els)
            }
        }
    }
}

/// Convenience constructors (keep call-sites and tests readable).
pub mod build {
    use super::*;

    pub fn ivar(n: &str) -> Term {
        Term::IVar { name: n.into() }
    }
    pub fn int(v: i64) -> Term {
        Term::Int { v }
    }
    pub fn add(l: Term, r: Term) -> Term {
        Term::Add {
            l: Box::new(l),
            r: Box::new(r),
        }
    }
    pub fn sub(l: Term, r: Term) -> Term {
        Term::Sub {
            l: Box::new(l),
            r: Box::new(r),
        }
    }
    pub fn mul(k: i64, t: Term) -> Term {
        Term::MulConst { k, t: Box::new(t) }
    }
    pub fn min(l: Term, r: Term) -> Term {
        Term::Min {
            l: Box::new(l),
            r: Box::new(r),
        }
    }
    pub fn max(l: Term, r: Term) -> Term {
        Term::Max {
            l: Box::new(l),
            r: Box::new(r),
        }
    }
    pub fn cmp(op: Op, l: Term, r: Term) -> Formula {
        Formula::Cmp { op, l, r }
    }
    pub fn and(fs: Vec<Formula>) -> Formula {
        Formula::And { fs }
    }
    pub fn implies(a: Formula, b: Formula) -> Formula {
        Formula::Implies {
            a: Box::new(a),
            b: Box::new(b),
        }
    }
    pub fn bvar(n: &str) -> Formula {
        Formula::BVar { name: n.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::build::*;
    use super::*;

    fn carve_obligation() -> Obligation {
        // ∀ g, avail. min(g, avail) ≤ avail   (the R22/R20 relational shape).
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

    #[test]
    fn parse_roundtrips_and_digests_stably() {
        let o = carve_obligation();
        let js = o.to_json();
        let back = Obligation::parse(&js).expect("valid obligation parses");
        assert_eq!(back, o);
        // Digest is a pure function of the canonical bytes (A5 building block).
        assert_eq!(o.digest(), back.digest());
        assert!(o.digest().starts_with("axsha256:"));
    }

    #[test]
    fn rejects_undeclared_variable() {
        let bad = Obligation {
            id: "x".into(),
            vars: vec![Var {
                name: "g".into(),
                sort: Sort::Int,
            }],
            claim: cmp(Op::Le, ivar("g"), ivar("avail")), // avail undeclared
        };
        let err = Obligation::parse(&bad.to_json()).unwrap_err();
        assert!(err.detail.contains("undeclared"), "{}", err.detail);
    }

    #[test]
    fn rejects_sort_mismatch() {
        // `g` declared Bool but used as an Int term.
        let bad = Obligation {
            id: "x".into(),
            vars: vec![Var {
                name: "g".into(),
                sort: Sort::Bool,
            }],
            claim: cmp(Op::Le, ivar("g"), int(0)),
        };
        let err = Obligation::parse(&bad.to_json()).unwrap_err();
        assert!(err.detail.contains("declared Bool"), "{}", err.detail);
    }

    #[test]
    fn malformed_json_is_a_parse_error() {
        let err = Obligation::parse("{ not json").unwrap_err();
        assert!(err.detail.contains("malformed"));
    }

    #[test]
    fn wrong_schema_rejected() {
        let err = Obligation::parse(
            r#"{"schema":"nope","id":"x","vars":[],"claim":{"node":"bool","v":true}}"#,
        )
        .unwrap_err();
        assert!(err.detail.contains("schema"));
    }
}
