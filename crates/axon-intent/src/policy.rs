//! R24 §3.3 + §4.3 (S1) — the risk-proportional friction ladder. Pure,
//! deterministic: `Risk` in, `ApprovalPolicy` out, no I/O, no clock.
//!
//! This is the headline defense's foundation: `multisig::build_token` (a later
//! slice) will assert the collected distinct-approver count against
//! `policy.threshold` before ever emitting a token, so a High/Critical grant
//! can never be single-approved. This slice only derives the policy from risk
//! — it does not yet enforce anything (S3).

use crate::approval::Risk;

/// Risk-derived friction: how many distinct approvers a grant needs before a
/// token can be emitted, and how long/how-often that token is good for once
/// emitted. `threshold`/`ttl_ticks`/`max_uses` are independent knobs (a high
/// threshold doesn't imply a short TTL) but the §3.3 table couples them by
/// risk so operators get one friction level per risk tier, not nine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApprovalPolicy {
    /// Distinct recorded-name approvers REQUIRED before a valid token
    /// (name-distinctness, NOT independence — R24 §1.2 residual risk).
    pub threshold: u32,
    /// Default token lifetime in logical ticks. 0 = no expiry, and only Low
    /// risk may have `ttl_ticks == 0`.
    pub ttl_ticks: u64,
    /// Default max run-boundary uses. 0 = unlimited, and only Low risk may
    /// have `max_uses == 0`.
    pub max_uses: u64,
}

/// The §3.3 friction ladder. `floor`, when given, RAISES the threshold to
/// `max(default, floor)` — friction can be demanded but never waived, the
/// same "floor only raises" discipline as R22's `--risk`
/// (`crate::approval`, around its own `Risk::parse`/raise-only handling).
pub fn policy_for(risk: Risk, floor: Option<u32>) -> ApprovalPolicy {
    let default = match risk {
        Risk::Low => ApprovalPolicy {
            threshold: 1,
            ttl_ticks: 0,
            max_uses: 0,
        },
        Risk::Medium => ApprovalPolicy {
            threshold: 1,
            ttl_ticks: 10_000,
            max_uses: 100,
        },
        Risk::High => ApprovalPolicy {
            threshold: 2,
            ttl_ticks: 1_000,
            max_uses: 10,
        },
        Risk::Critical => ApprovalPolicy {
            threshold: 3,
            ttl_ticks: 100,
            max_uses: 1,
        },
    };
    match floor {
        Some(f) if f > default.threshold => ApprovalPolicy {
            threshold: f,
            ..default
        },
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_ladder_is_monotone() {
        // The §3.3 table, exactly.
        let low = policy_for(Risk::Low, None);
        assert_eq!(low.threshold, 1);
        assert_eq!(low.ttl_ticks, 0);
        assert_eq!(low.max_uses, 0);

        let medium = policy_for(Risk::Medium, None);
        assert_eq!(medium.threshold, 1);
        assert_eq!(medium.ttl_ticks, 10_000);
        assert_eq!(medium.max_uses, 100);

        let high = policy_for(Risk::High, None);
        assert_eq!(high.threshold, 2);
        assert_eq!(high.ttl_ticks, 1_000);
        assert_eq!(high.max_uses, 10);

        let critical = policy_for(Risk::Critical, None);
        assert_eq!(critical.threshold, 3);
        assert_eq!(critical.ttl_ticks, 100);
        assert_eq!(critical.max_uses, 1);

        // threshold is non-decreasing in risk.
        assert!(low.threshold <= medium.threshold);
        assert!(medium.threshold <= high.threshold);
        assert!(high.threshold <= critical.threshold);

        // Only Low may have unlimited ttl/uses — every friction-bearing risk
        // (Medium/High/Critical) gets a finite ttl AND a finite max_uses.
        for risk in [Risk::Medium, Risk::High, Risk::Critical] {
            let p = policy_for(risk, None);
            assert!(p.ttl_ticks > 0, "{risk:?} must have a finite ttl_ticks");
            assert!(p.max_uses > 0, "{risk:?} must have a finite max_uses");
        }

        // The floor RAISES the threshold but never lowers it.
        assert_eq!(policy_for(Risk::Low, Some(1)).threshold, 1);
        assert_eq!(policy_for(Risk::Low, Some(5)).threshold, 5);
        assert_eq!(
            policy_for(Risk::Critical, Some(1)).threshold,
            3,
            "a floor below the default must not lower the threshold"
        );
        assert_eq!(
            policy_for(Risk::Critical, Some(10)).threshold,
            10,
            "a floor above the default must raise the threshold"
        );

        // A floor never changes ttl_ticks/max_uses — only threshold.
        let raised = policy_for(Risk::High, Some(9));
        assert_eq!(raised.ttl_ticks, high.ttl_ticks);
        assert_eq!(raised.max_uses, high.max_uses);
    }
}
