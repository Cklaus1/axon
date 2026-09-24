//! The executable admission-rule grammar. A plan can only be FROZEN when every
//! rule field parses here; there is no "unknown rule ⇒ guess".
//!
//! | field | accepted value | meaning |
//! |---|---|---|
//! | `quality_margin` | `pass_rate_margin_ppm=<n>`, 0 ≤ n < 1 000 000 | noninferiority margin on verified-pass rate |
//! | `economic_threshold` | `min_cost_reduction_ppm=<n>`, 0 ≤ n ≤ 1 000 000 | cost per task must fall by at least n/1e6 |
//! | `budget_rule` | `max_unresolved_liability_micro=<n>` | tolerance for unresolved liability across both arms |
//! | `missing_data_rule` | `unknown_bounds` | an unknown or missing outcome counts as fail for the candidate and pass for the incumbent |
//! | `uncertainty_rule` | `exact_bounds` | decide on worst/best-case bounds, no sampling model |
//! | `multiplicity_rule` | `single_candidate` | one candidate per incumbent, one experiment per candidate |
//! | `independent_unit` | `task` | the cluster unit is `task_id`; repeated trials of a task are one unit |
//! | `order_rule` | `paired_tasks` | both arms are assigned exactly the same task set |
//! | `cache_rule` | `not_enforced_here` | this admitter cannot observe cache state; the plan must say so explicitly |
//!
//! `<n>` is a CANONICAL decimal: `0`, or a nonzero digit followed by digits,
//! at most 2^53−1. `+5`, `05`, ` 5`, `5 ` are refused, so one rule has one
//! spelling and one plan digest.

use crate::plan::PilotPlan;
use axon_loop_contracts::MAX_INTEGER;

pub const PPM: u64 = 1_000_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rules {
    pub margin_ppm: u64,
    pub min_cost_reduction_ppm: u64,
    pub max_liability_micro: u64,
    pub independent_units: u64,
    pub candidate_budget: u64,
}

fn canonical_u64(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.is_empty() || !b.iter().all(u8::is_ascii_digit) || (b.len() > 1 && b[0] == b'0') {
        return None;
    }
    s.parse::<u64>().ok().filter(|n| *n <= MAX_INTEGER)
}

fn keyed(field: &str, v: &Option<String>, key: &str) -> Result<u64, String> {
    let v = v.as_deref().ok_or(format!("{field} unset"))?;
    v.strip_prefix(key)
        .and_then(|r| r.strip_prefix('='))
        .and_then(canonical_u64)
        .ok_or(format!(
            "{field} {v:?} is not executable (expected `{key}=<canonical integer>`)"
        ))
}

fn word(field: &str, v: &Option<String>, w: &str) -> Result<(), String> {
    match v.as_deref() {
        Some(x) if x == w => Ok(()),
        Some(x) => Err(format!("{field} {x:?} is not executable (expected `{w}`)")),
        None => Err(format!("{field} unset")),
    }
}

impl Rules {
    /// Every rule, or the first reason one is not executable.
    pub fn parse(p: &PilotPlan) -> Result<Rules, String> {
        let margin_ppm = keyed("quality_margin", &p.quality_margin, "pass_rate_margin_ppm")?;
        if margin_ppm >= PPM {
            return Err(format!(
                "quality_margin {margin_ppm} ppm must be < 1000000 (a 100% margin accepts anything)"
            ));
        }
        let min_cost_reduction_ppm = keyed(
            "economic_threshold",
            &p.economic_threshold,
            "min_cost_reduction_ppm",
        )?;
        if min_cost_reduction_ppm > PPM {
            return Err("economic_threshold above 100%".into());
        }
        let max_liability_micro = keyed(
            "budget_rule",
            &p.budget_rule,
            "max_unresolved_liability_micro",
        )?;
        word("missing_data_rule", &p.missing_data_rule, "unknown_bounds")?;
        word("uncertainty_rule", &p.uncertainty_rule, "exact_bounds")?;
        word(
            "multiplicity_rule",
            &p.multiplicity_rule,
            "single_candidate",
        )?;
        word("independent_unit", &p.independent_unit, "task")?;
        word("order_rule", &p.order_rule, "paired_tasks")?;
        word("cache_rule", &p.cache_rule, "not_enforced_here")?;
        Ok(Rules {
            margin_ppm,
            min_cost_reduction_ppm,
            max_liability_micro,
            independent_units: p.independent_units.ok_or("independent_units unset")?,
            candidate_budget: p.candidate_budget.ok_or("candidate_budget unset")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::canonical_u64;
    #[test]
    fn canonical_integers_only() {
        for ok in [
            ("0", 0),
            ("5", 5),
            ("9007199254740991", 9_007_199_254_740_991),
        ] {
            assert_eq!(canonical_u64(ok.0), Some(ok.1));
        }
        for bad in [
            "",
            "+5",
            "05",
            "00",
            "-5",
            " 5",
            "5 ",
            "5\n",
            "9007199254740992",
            "٥",
            "５",
            "0x5",
        ] {
            assert_eq!(canonical_u64(bad), None, "{bad:?}");
        }
    }
}
